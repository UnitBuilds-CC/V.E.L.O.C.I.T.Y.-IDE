//! Write-Ahead Log — SQLite-backed durable journal for workflow state.
//!
//! The WAL records step outcomes and state mutations. On crash recovery,
//! the engine replays the WAL to restore the last consistent state.
//!
//! Batching: Instead of writing each step individually, the engine collects
//! sync_steps outcomes and writes them in a single transaction with one fsync.

use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use velocity_workflow_core::*;

/// A write-ahead log entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalEntry {
    pub sequence: u64,
    pub run_id: RunId,
    pub step_id: StepId,
    pub outcome: StepOutcome,
    pub mutations: Vec<StateMutation>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// SQLite-backed write-ahead log.
#[derive(Clone)]
pub struct WriteAheadLog {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl WriteAheadLog {
    /// Open or create a WAL at the given path.
    pub fn open(path: &Path) -> Result<Self, WorkflowError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| WorkflowError::Persistence(format!("create WAL dir: {e}")))?;
        }
        let conn = Connection::open(path)
            .map_err(|e| WorkflowError::Persistence(format!("open WAL: {e}")))?;
        let wal = Self {
            conn: Arc::new(Mutex::new(conn)),
            path: path.to_path_buf(),
        };
        wal.init()?;
        Ok(wal)
    }

    /// Open an in-memory WAL (for testing).
    pub fn open_memory() -> Result<Self, WorkflowError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| WorkflowError::Persistence(format!("open memory WAL: {e}")))?;
        let wal = Self {
            conn: Arc::new(Mutex::new(conn)),
            path: PathBuf::from(":memory:"),
        };
        wal.init()?;
        Ok(wal)
    }

    fn init(&self) -> Result<(), WorkflowError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS wal_entries (
                sequence    INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id      TEXT NOT NULL,
                step_id     TEXT NOT NULL,
                outcome_json TEXT NOT NULL,
                mutations_json TEXT NOT NULL,
                timestamp   TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_wal_run ON wal_entries(run_id);

            CREATE TABLE IF NOT EXISTS run_state (
                run_id      TEXT PRIMARY KEY,
                workflow_id TEXT NOT NULL,
                state       TEXT NOT NULL,
                state_json  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );
        ").map_err(|e| WorkflowError::Persistence(format!("init WAL: {e}")))?;
        Ok(())
    }

    /// Append a batch of entries in a single transaction.
    /// This is the key batching operation — one transaction, one fsync.
    pub fn append_batch(&self, entries: &[WalEntry]) -> Result<u64, WorkflowError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()
            .map_err(|e| WorkflowError::Persistence(format!("begin WAL batch: {e}")))?;

        let mut last_seq = 0u64;
        for entry in entries {
            let outcome_json = serde_json::to_string(&entry.outcome)
                .map_err(|e| WorkflowError::Persistence(format!("serialize outcome: {e}")))?;
            let mutations_json = serde_json::to_string(&entry.mutations)
                .map_err(|e| WorkflowError::Persistence(format!("serialize mutations: {e}")))?;

            tx.execute(
                "INSERT INTO wal_entries (run_id, step_id, outcome_json, mutations_json, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    entry.run_id.as_str(),
                    entry.step_id.as_str(),
                    outcome_json,
                    mutations_json,
                    entry.timestamp.to_rfc3339(),
                ],
            ).map_err(|e| WorkflowError::Persistence(format!("insert WAL entry: {e}")))?;

            last_seq = tx.last_insert_rowid() as u64;
        }

        tx.commit().map_err(|e| WorkflowError::Persistence(format!("commit WAL batch: {e}")))?;
        Ok(last_seq)
    }

    /// Save run state snapshot.
    pub fn save_run_state(&self, state: &WorkflowRunState) -> Result<(), WorkflowError> {
        let conn = self.conn.lock().unwrap();
        let state_json = serde_json::to_string(state)
            .map_err(|e| WorkflowError::Persistence(format!("serialize run state: {e}")))?;
        conn.execute(
            "INSERT OR REPLACE INTO run_state (run_id, workflow_id, state, state_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                state.run_id.as_str(),
                state.workflow_id.as_str(),
                format!("{:?}", state.state),
                state_json,
                state.updated_at.to_rfc3339(),
            ],
        ).map_err(|e| WorkflowError::Persistence(format!("save run state: {e}")))?;
        Ok(())
    }

    /// Load run state by ID.
    pub fn load_run_state(&self, run_id: &RunId) -> Result<Option<WorkflowRunState>, WorkflowError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT state_json FROM run_state WHERE run_id = ?1"
        ).map_err(|e| WorkflowError::Persistence(format!("prepare load state: {e}")))?;

        let result = stmt.query_row(params![run_id.as_str()], |row| {
            let json: String = row.get(0)?;
            Ok(json)
        });

        match result {
            Ok(json) => {
                let state: WorkflowRunState = serde_json::from_str(&json)
                    .map_err(|e| WorkflowError::Persistence(format!("deserialize run state: {e}")))?;
                Ok(Some(state))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(WorkflowError::Persistence(format!("load run state: {e}"))),
        }
    }

    /// Replay WAL entries for a specific run (for crash recovery).
    pub fn replay_run(&self, run_id: &RunId) -> Result<Vec<WalEntry>, WorkflowError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT sequence, run_id, step_id, outcome_json, mutations_json, timestamp
             FROM wal_entries WHERE run_id = ?1 ORDER BY sequence"
        ).map_err(|e| WorkflowError::Persistence(format!("prepare replay: {e}")))?;

        let entries = stmt.query_map(params![run_id.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)? as u64,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        }).map_err(|e| WorkflowError::Persistence(format!("replay query: {e}")))?;

        let mut result = Vec::new();
        for entry in entries {
            let (seq, run_id_str, step_id_str, outcome_json, mutations_json, ts_str) =
                entry.map_err(|e| WorkflowError::Persistence(format!("replay row: {e}")))?;

            let outcome: StepOutcome = serde_json::from_str(&outcome_json)
                .map_err(|e| WorkflowError::Persistence(format!("deserialize outcome: {e}")))?;
            let mutations: Vec<StateMutation> = serde_json::from_str(&mutations_json)
                .map_err(|e| WorkflowError::Persistence(format!("deserialize mutations: {e}")))?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            result.push(WalEntry {
                sequence: seq,
                run_id: RunId::from_str(run_id_str),
                step_id: StepId::from_str(step_id_str),
                outcome,
                mutations,
                timestamp,
            });
        }
        Ok(result)
    }

    /// Get the current sequence number (for ordering).
    pub fn current_sequence(&self) -> Result<u64, WorkflowError> {
        let conn = self.conn.lock().unwrap();
        let seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(sequence), 0) FROM wal_entries",
            [],
            |row| row.get(0),
        ).map_err(|e| WorkflowError::Persistence(format!("get sequence: {e}")))?;
        Ok(seq as u64)
    }

    /// Truncate entries older than the given run (for cleanup).
    pub fn truncate_before_run(&self, run_id: &RunId) -> Result<u64, WorkflowError> {
        let conn = self.conn.lock().unwrap();
        let max_seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(sequence), 0) FROM wal_entries WHERE run_id = ?1",
            params![run_id.as_str()],
            |row| row.get(0),
        ).map_err(|e| WorkflowError::Persistence(format!("get max seq: {e}")))?;

        let deleted = conn.execute(
            "DELETE FROM wal_entries WHERE sequence < ?1",
            params![max_seq],
        ).map_err(|e| WorkflowError::Persistence(format!("truncate: {e}")))?;

        Ok(deleted as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wal_append_and_replay() {
        let wal = WriteAheadLog::open_memory().unwrap();
        let run_id = RunId::new();
        let step_id = StepId::new();

        let entries = vec![WalEntry {
            sequence: 0,
            run_id: run_id.clone(),
            step_id: step_id.clone(),
            outcome: StepOutcome::Ok {
                output: serde_json::json!({"result": "ok"}),
                mutations: vec![],
            },
            mutations: vec![],
            timestamp: chrono::Utc::now(),
        }];

        let seq = wal.append_batch(&entries).unwrap();
        assert!(seq > 0);

        let replayed = wal.replay_run(&run_id).unwrap();
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].step_id, step_id);
    }

    #[test]
    fn wal_batch_multiple_entries() {
        let wal = WriteAheadLog::open_memory().unwrap();
        let run_id = RunId::new();

        let entries: Vec<WalEntry> = (0..5).map(|i| WalEntry {
            sequence: 0,
            run_id: run_id.clone(),
            step_id: StepId::new(),
            outcome: StepOutcome::Ok {
                output: serde_json::json!({"i": i}),
                mutations: vec![],
            },
            mutations: vec![],
            timestamp: chrono::Utc::now(),
        }).collect();

        wal.append_batch(&entries).unwrap();
        let replayed = wal.replay_run(&run_id).unwrap();
        assert_eq!(replayed.len(), 5);
    }

    #[test]
    fn wal_save_and_load_run_state() {
        let wal = WriteAheadLog::open_memory().unwrap();
        let run_id = RunId::new();
        let wf_id = WorkflowId::new();
        let mut state = WorkflowRunState::new(run_id.clone(), wf_id, 10);
        state.transition_to(RunState::Running);
        state.steps_completed = 3;

        wal.save_run_state(&state).unwrap();
        let loaded = wal.load_run_state(&run_id).unwrap().unwrap();
        assert_eq!(loaded.steps_completed, 3);
        assert_eq!(loaded.state, RunState::Running);
    }
}
