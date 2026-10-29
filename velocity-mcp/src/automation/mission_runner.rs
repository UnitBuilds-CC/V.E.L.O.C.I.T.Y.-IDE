//! The unattended executor: turns a [`SectorPlan`] and a team roster into a
//! rewrite that runs to completion without a human at the keyboard.
//!
//! Everything upstream of this module produces a *plan*. [`crate::agent::intent`]
//! decides the work is long-horizon and staffs a team; [`crate::automation::sectors`]
//! decomposes the repository into dispatchable units and dependency waves;
//! [`crate::registry::team_tools::build_rewrite_plan`] sequences the phases.
//! None of that runs anything. This module does, and its entire reason to exist
//! is the three properties a multi-hour unattended run needs and an interactive
//! agent loop does not have:
//!
//! - **Resumable.** Progress is persisted after every sector, atomically, so a
//!   run that is killed at hour six — by a budget cap, a machine reboot or an
//!   operator — picks up where it stopped instead of re-paying for finished
//!   work. Sector ids come from the *persisted* plan, never a re-derivation.
//! - **Bounded.** Model spend is the one resource this can exhaust without
//!   asking. Every axis is capped: dispatch count, wall clock, waves, sweep
//!   dispatches, retries. A cap that trips pauses the mission and says so; it
//!   never silently overruns and never reports a capped run as complete.
//! - **Verified.** Gates run against the *output* directory, not the IDE's own
//!   crate — [`crate::automation::build_runner::run_cargo_check`] resolves its
//!   manifest through `cargo_manifest_dir(workspace_root)` and is bound to the
//!   Velocity workspace by design, so it cannot check code a mission just
//!   produced. The gate runner here shells out with `current_dir` set to the
//!   output root and reports a skipped gate as skipped, never as passed.
//!
//! # Parallelism
//!
//! Waves run in parallel across OS threads; the units inside one sector run
//! sequentially. That split is deliberate. A sector's units are line-range
//! chunks of the *same* files, so two of them running at once would race on the
//! same output file, and chunk `n` legitimately needs chunk `n-1` already
//! written. Sectors within a wave are by construction independent, which is
//! what makes them safe to fan out.
//!
//! Plain `std::thread` is sufficient and correct here: the whole dispatch path
//! ([`crate::registry::team_tools::handle_team_tool`] → `team_dispatch` →
//! [`crate::agent::run_headless_subagent`]) is synchronous blocking code with
//! no `block_on`, no `Runtime` and no `Handle::current` anywhere in it, so
//! there is no async runtime context for a worker thread to be missing.

use crate::automation::sectors::{SectorPlan, SectorUnit, DEFAULT_PARALLELISM};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Bytes above which [`crate::registry::team_tools`] truncates a pre-loaded
/// file in a dispatch prompt.
///
/// The *default*: the dispatcher reads `VELOCITY_PRELOAD_MAX_BYTES` per call, so
/// what a run actually preloads comes from [`prompt_preload_cap_bytes`], and this
/// constant is kept equal to the default by a test rather than by sharing the
/// number. Passing a file larger than the live cap in `files` does not fail loudly
/// — the model gets the first part plus a note naming the path and the `read_file`
/// `startLine` that returns the rest, so the loss is recoverable only if the run
/// actually reads. [`split_units_for_prompt`] keeps even that from happening by
/// never pre-loading an over-cap unit; an operator who raises the cap only makes
/// it defer units it no longer has to, which is the safe direction.
pub const PROMPT_PRELOAD_CAP_BYTES: u64 = 12_000;

/// Default per-gate wall-clock ceiling.
///
/// `cargo test` on a large generated crate can hang on a network-bound test
/// forever, and an unattended mission that hangs is worse than one that fails:
/// it holds the spend budget open and produces nothing.
pub const DEFAULT_GATE_TIMEOUT_SECS: u64 = 900;

/// How many output sectors one sweep dispatch audits.
///
/// One dispatch per sector would triple the porting cost — 1,175 sectors on
/// LiteLLM is 3,525 more dispatches for the three sweeps. One dispatch for the
/// whole tree would exceed any context window. Six sectors is roughly 290 KB of
/// source, which fits alongside the audit instructions.
pub const DEFAULT_SWEEP_BATCH_SECTORS: usize = 6;

pub const MISSION_FILE: &str = "mission.json";
pub const PLAN_FILE: &str = "sector_plan.json";
pub const PROGRESS_FILE: &str = "progress.json";
pub const REPORT_FILE: &str = "report.json";
pub const FINDINGS_DIR: &str = "findings";

// ─────────────────────────────────────────────────────────────────────
// Manifest: the mission's identity, written once by `repo_rewrite`
// ─────────────────────────────────────────────────────────────────────

/// What a mission *is*, as opposed to how this run is allowed to behave.
///
/// Persisted by `repo_rewrite` and read back on resume. Keeping identity and
/// runtime knobs in separate files means re-running with a tighter budget does
/// not rewrite the record of what is being ported, and a resumed run cannot
/// accidentally re-target a different language because a caller forgot an
/// argument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionManifest {
    pub repo_name: String,
    pub source_url: String,
    pub target_language: String,
    /// The clone, relative to the workspace root. Read-only during a mission.
    pub source_root: String,
    /// Package root *inside* the clone (`"litellm"`), from
    /// [`SectorPlan::source_root`]. Recorded here rather than only in the plan
    /// because every ported path is derived from it, and a resumed run that
    /// guessed it differently would scatter the output across two trees.
    #[serde(default)]
    pub package_root: String,
    /// Where ported code is written, relative to the workspace root. Always
    /// outside `source_root`: the clone is the reference, and a port that edits
    /// its own reference cannot be diffed against it.
    pub output_root: String,
    pub team_id: String,
    pub parallelism: usize,
    pub include_tests: bool,
    pub created_at_ms: u64,
}

impl MissionManifest {
    pub fn path(mission_dir: &Path) -> PathBuf {
        mission_dir.join(MISSION_FILE)
    }

    pub fn load(mission_dir: &Path) -> Result<Self, String> {
        let path = Self::path(mission_dir);
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        serde_json::from_str(&raw).map_err(|e| format!("cannot parse {}: {e}", path.display()))
    }

    pub fn save(&self, mission_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(mission_dir)
            .map_err(|e| format!("cannot create {}: {e}", mission_dir.display()))?;
        atomic_write_json(
            &Self::path(mission_dir),
            &serde_json::to_value(self).map_err(|e| e.to_string())?,
        )
    }
}

// ─────────────────────────────────────────────────────────────────────
// Runtime configuration
// ─────────────────────────────────────────────────────────────────────

/// How this run is allowed to behave. Every cap is a spend decision.
#[derive(Debug, Clone, PartialEq)]
pub struct MissionConfig {
    /// Overrides the manifest's team. `None` uses the recorded one.
    pub team_id: Option<String>,
    /// Concurrent sector dispatches. `0` takes the manifest's value.
    pub parallelism: usize,
    /// Hard cap on dispatches *this run* may make. `0` means uncapped.
    pub max_dispatches: usize,
    /// Hard cap on wall-clock seconds spent dispatching. `0` means uncapped.
    pub max_wall_secs: u64,
    /// Stop after this many waves. `0` means all of them.
    pub max_waves: usize,
    /// Retries per sector across the whole mission, including prior runs.
    pub max_attempts_per_sector: u32,
    pub run_gates: bool,
    /// Halt the mission when a blocking gate fails. Off means "record it and
    /// carry on", which is what a long port wants — one sector that will not
    /// compile should not strand the other 1,174 — but it also means the final
    /// report must not read as a success.
    pub stop_on_gate_failure: bool,
    pub run_sweeps: bool,
    pub sweep_batch_sectors: usize,
    /// Per-sweep dispatch cap. Bounds the *audit* spend separately from the
    /// porting spend, because a sweep that silently covers 12% of the tree is
    /// worse than one that says it covered 12%.
    pub max_sweep_dispatches: usize,
    pub run_documentation: bool,
    pub gate_timeout_secs: u64,
    /// Plan the work, run every gate and sweep decision, dispatch nothing.
    ///
    /// Not a test-only convenience: an operator about to authorise 321
    /// model-hours should be able to see the schedule first, and the dry run
    /// exercises the same code path with the model call elided.
    pub dry_run: bool,
}

impl Default for MissionConfig {
    fn default() -> Self {
        Self {
            team_id: None,
            parallelism: 0,
            // Uncapped by default is deliberate but not careless: `run_rewrite_mission`
            // is only reachable with an explicit user authorization to run
            // unattended, and a cap nobody chose is a cap that silently
            // truncates the job. The report states the caps in force.
            max_dispatches: 0,
            max_wall_secs: 0,
            max_waves: 0,
            max_attempts_per_sector: 2,
            run_gates: true,
            stop_on_gate_failure: false,
            run_sweeps: true,
            sweep_batch_sectors: DEFAULT_SWEEP_BATCH_SECTORS,
            max_sweep_dispatches: 20,
            run_documentation: true,
            gate_timeout_secs: DEFAULT_GATE_TIMEOUT_SECS,
            dry_run: false,
        }
    }
}

impl MissionConfig {
    /// Build from tool arguments, falling back to [`Default`] per field.
    ///
    /// Parallelism is clamped to 1..=32 rather than trusted: it becomes a
    /// thread count, and a caller-supplied 10,000 would be 10,000 OS threads
    /// each holding an open model connection.
    pub fn from_args(arguments: &Value) -> Self {
        let base = Self::default();
        let num = |k: &str, d: usize| {
            arguments[k]
                .as_u64()
                .map(|v| v.clamp(0, u32::MAX as u64) as usize)
                .unwrap_or(d)
        };
        let flag = |k: &str, d: bool| arguments[k].as_bool().unwrap_or(d);
        Self {
            team_id: arguments["team_id"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            parallelism: num("parallelism", base.parallelism).clamp(0, 32),
            max_dispatches: num("max_dispatches", base.max_dispatches),
            max_wall_secs: arguments["max_wall_secs"]
                .as_u64()
                .unwrap_or(base.max_wall_secs),
            max_waves: num("max_waves", base.max_waves),
            max_attempts_per_sector: num("max_attempts", base.max_attempts_per_sector as usize)
                .max(1) as u32,
            run_gates: flag("run_gates", base.run_gates),
            stop_on_gate_failure: flag("stop_on_gate_failure", base.stop_on_gate_failure),
            run_sweeps: flag("run_sweeps", base.run_sweeps),
            sweep_batch_sectors: num("sweep_batch_sectors", base.sweep_batch_sectors).max(1),
            max_sweep_dispatches: num("max_sweep_dispatches", base.max_sweep_dispatches),
            run_documentation: flag("run_documentation", base.run_documentation),
            gate_timeout_secs: arguments["gate_timeout_secs"]
                .as_u64()
                .filter(|s| *s > 0)
                .unwrap_or(base.gate_timeout_secs),
            dry_run: flag("dry_run", base.dry_run),
        }
    }

    /// The caps in force, as sentences. Printed in every report because
    /// "completed" means something different under each of them.
    pub fn describe_caps(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(if self.max_dispatches == 0 {
            "dispatches: uncapped".to_string()
        } else {
            format!("dispatches: at most {}", self.max_dispatches)
        });
        out.push(if self.max_wall_secs == 0 {
            "wall clock: uncapped".to_string()
        } else {
            format!("wall clock: at most {}s", self.max_wall_secs)
        });
        if self.max_waves > 0 {
            out.push(format!("waves: at most {}", self.max_waves));
        }
        out.push(format!(
            "retries: at most {} attempts per sector",
            self.max_attempts_per_sector
        ));
        out
    }
}

// ─────────────────────────────────────────────────────────────────────
// Progress: the resume record
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    /// Dispatching, or between phases.
    Running,
    /// Stopped by a cap or a gate. Resumable: nothing is lost, nothing is
    /// finished.
    Paused,
    /// Every sector, gate and phase this mission was configured to run has run.
    Completed,
    /// The mission could not proceed at all — missing plan, unreadable
    /// directory, no team. Distinct from `Paused` because retrying unchanged
    /// will fail again.
    Failed,
}

impl MissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MissionStatus::Running => "running",
            MissionStatus::Paused => "paused",
            MissionStatus::Completed => "completed",
            MissionStatus::Failed => "failed",
        }
    }
}

/// Why a sector did not complete, and how many times it has been tried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorFailure {
    pub attempts: u32,
    pub last_error: String,
    pub last_at_ms: u64,
}

/// One dispatch, as recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DispatchRecord {
    pub sector: String,
    pub unit: String,
    pub ok: bool,
    pub secs: u64,
    pub provider: String,
    pub model: String,
    /// True when the winning model was sub-7B. Carried through from
    /// `team_dispatch`'s own degraded flag so the mission does not launder a
    /// small-model result into a clean one.
    pub degraded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One gate execution against the output directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateResult {
    pub gate: String,
    pub command: String,
    pub success: bool,
    /// Never ran — no manifest, no toolchain, no such language gate set. Kept
    /// separate from `success` because `success == false, skipped == false`
    /// means "it ran and found problems", which is a completely different
    /// claim.
    pub skipped: bool,
    pub timed_out: bool,
    /// Blocks the mission when it fails. `fmt` does not; `build` does.
    pub blocking: bool,
    pub duration_ms: u64,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// Last lines of combined output, so a failure is diagnosable from the
    /// report alone without re-running the gate.
    pub tail: String,
}

impl GateResult {
    fn skipped(gate: &str, command: &str, reason: String) -> Self {
        Self {
            gate: gate.to_string(),
            command: command.to_string(),
            success: false,
            skipped: true,
            timed_out: false,
            blocking: false,
            duration_ms: 0,
            errors: vec![reason],
            warnings: Vec::new(),
            tail: String::new(),
        }
    }
}

/// One of the three audit sweeps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepSummary {
    pub sweep: String,
    pub role: String,
    pub dispatches: usize,
    pub sectors_covered: usize,
    pub sectors_available: usize,
    /// True when `max_sweep_dispatches` cut the sweep short. The coverage
    /// numbers below it are what make that honest rather than decorative.
    pub truncated: bool,
    /// True when any of this sweep's dispatches was served by a sub-7B fallback
    /// model. A degraded audit does not merely miss things, it invents them
    /// (findings citing line numbers the file does not have), so the report has
    /// to say the sweep ran *and* that its findings cannot be trusted — the
    /// same honesty `truncated` brings to coverage. Without this the global
    /// `degraded_dispatches` count is the only signal, and it does not say
    /// *which* axis was degraded. `#[serde(default)]` keeps checkpoints written
    /// before this field existed loadable.
    #[serde(default)]
    pub degraded: bool,
    pub reports: Vec<String>,
    pub secs: u64,
}

impl SweepSummary {
    /// Coverage as a sentence, because `sectors_covered: 144` alone does not
    /// tell an operator whether that is the whole tree or a twentieth of it.
    pub fn coverage(&self) -> String {
        if self.sectors_available == 0 {
            return format!(
                "{} sweep: nothing to audit — no sector had been ported when the sweep ran",
                self.sweep
            );
        }
        let pct = 100.0 * self.sectors_covered as f64 / self.sectors_available as f64;
        let tail = if self.truncated {
            format!(
                " — truncated at {} dispatches, so the remaining {:.0}% was never audited",
                self.dispatches,
                100.0 - pct
            )
        } else {
            String::new()
        };
        let degraded = if self.degraded {
            " — DEGRADED: served by a small (<7B) fallback model, so these findings are \
             unreliable and are not a clean bill of health"
        } else {
            ""
        };
        format!(
            "{} sweep: audited {} of {} ported sectors ({pct:.0}%) in {} dispatches{tail}{degraded}",
            self.sweep, self.sectors_covered, self.sectors_available, self.dispatches
        )
    }
}

/// The persisted resume record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionProgress {
    /// Bumped when the shape changes incompatibly. A runner that silently
    /// mis-reads an older record would re-port finished sectors, so a mismatch
    /// is a hard error rather than a defaulted struct.
    pub version: u32,
    pub status: MissionStatus,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    pub sectors_done: Vec<String>,
    pub sectors_failed: BTreeMap<String, SectorFailure>,
    pub dispatches_run: usize,
    pub dispatch_secs_used: u64,
    /// Dispatches whose winning model was sub-7B. Counted for the mission's
    /// whole life rather than derived from `recent_dispatches`, which is bounded
    /// and would silently undercount on a 3,854-dispatch run.
    #[serde(default)]
    pub degraded_dispatches: usize,
    pub waves_completed: usize,
    pub phases_done: Vec<String>,
    pub gates: Vec<GateResult>,
    pub sweeps: Vec<SweepSummary>,
    /// Bounded tail of dispatch records. Unbounded would grow to 3,854 entries
    /// on LiteLLM and be rewritten after every single dispatch.
    pub recent_dispatches: Vec<DispatchRecord>,
}

/// Bumped for any incompatible change to [`MissionProgress`]'s shape.
pub const PROGRESS_VERSION: u32 = 1;

/// How many dispatch records to retain. Enough to diagnose the recent past,
/// small enough that saving after every dispatch stays cheap.
const RECENT_DISPATCH_CAP: usize = 200;

impl Default for MissionProgress {
    fn default() -> Self {
        Self {
            version: PROGRESS_VERSION,
            status: MissionStatus::Running,
            started_at_ms: crate::automation::build_runner::now_ms(),
            updated_at_ms: crate::automation::build_runner::now_ms(),
            stop_reason: None,
            sectors_done: Vec::new(),
            sectors_failed: BTreeMap::new(),
            dispatches_run: 0,
            dispatch_secs_used: 0,
            degraded_dispatches: 0,
            waves_completed: 0,
            phases_done: Vec::new(),
            gates: Vec::new(),
            sweeps: Vec::new(),
            recent_dispatches: Vec::new(),
        }
    }
}

impl MissionProgress {
    pub fn path(mission_dir: &Path) -> PathBuf {
        mission_dir.join(PROGRESS_FILE)
    }

    /// Load an existing record, or `None` when this is the first run.
    ///
    /// A *corrupt* record is an error, not a `None`: treating an unreadable
    /// progress file as "fresh start" would re-dispatch every sector already
    /// paid for, which is the most expensive failure mode this module has.
    pub fn load(mission_dir: &Path) -> Result<Option<Self>, String> {
        let path = Self::path(mission_dir);
        let raw = match std::fs::read_to_string(&path) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        let p: Self = serde_json::from_str(&raw)
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
        if p.version != PROGRESS_VERSION {
            return Err(format!(
                "{} was written by progress version {} but this build reads version {}; \
                 refusing to guess — an unread resume record re-ports sectors already paid for",
                path.display(),
                p.version,
                PROGRESS_VERSION
            ));
        }
        Ok(Some(p))
    }

    /// Persist atomically: write a sibling temp file, then rename over the real
    /// one.
    ///
    /// A mission is expected to be interrupted — that is the whole premise — and
    /// `fs::write` truncated in place would leave a half-written record at
    /// exactly that moment. [`load`] then errors on the malformed JSON and the
    /// mission cannot resume at all. Rename is atomic on both NTFS and POSIX.
    pub fn save(&self, mission_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(mission_dir)
            .map_err(|e| format!("cannot create {}: {e}", mission_dir.display()))?;
        atomic_write_json(
            &Self::path(mission_dir),
            &serde_json::to_value(self).map_err(|e| e.to_string())?,
        )
    }

    pub fn is_done(&self, sector: &str) -> bool {
        self.sectors_done.iter().any(|s| s == sector)
    }

    pub fn mark_done(&mut self, sector: &str) {
        if !self.is_done(sector) {
            self.sectors_done.push(sector.to_string());
        }
        self.sectors_failed.remove(sector);
    }

    pub fn attempts(&self, sector: &str) -> u32 {
        self.sectors_failed
            .get(sector)
            .map(|f| f.attempts)
            .unwrap_or(0)
    }

    pub fn mark_failed(&mut self, sector: &str, error: String) {
        let now = crate::automation::build_runner::now_ms();
        let entry = self
            .sectors_failed
            .entry(sector.to_string())
            .or_insert(SectorFailure {
                attempts: 0,
                last_error: String::new(),
                last_at_ms: now,
            });
        entry.attempts = entry.attempts.saturating_add(1);
        entry.last_error = error;
        entry.last_at_ms = now;
    }

    /// Record one dispatch and keep the retained tail bounded.
    pub fn record_dispatch(&mut self, rec: DispatchRecord) {
        self.dispatches_run = self.dispatches_run.saturating_add(1);
        self.dispatch_secs_used = self.dispatch_secs_used.saturating_add(rec.secs);
        if rec.degraded {
            self.degraded_dispatches = self.degraded_dispatches.saturating_add(1);
        }
        self.recent_dispatches.push(rec);
        let overflow = self
            .recent_dispatches
            .len()
            .saturating_sub(RECENT_DISPATCH_CAP);
        if overflow > 0 {
            self.recent_dispatches.drain(0..overflow);
        }
        self.updated_at_ms = crate::automation::build_runner::now_ms();
    }

    pub fn mark_phase(&mut self, phase: &str) {
        if !self.phases_done.iter().any(|p| p == phase) {
            self.phases_done.push(phase.to_string());
        }
        self.updated_at_ms = crate::automation::build_runner::now_ms();
    }

    pub fn phase_done(&self, phase: &str) -> bool {
        self.phases_done.iter().any(|p| p == phase)
    }
}

/// Write JSON via a temp file + rename so an interruption cannot leave a
/// truncated record behind.
fn atomic_write_json(path: &Path, value: &Value) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        // Leave the temp file in place on failure: it holds the newer state, and
        // deleting it would destroy the only copy.
        format!(
            "cannot replace {} (partial write left at {}): {e}",
            path.display(),
            tmp.display()
        )
    })
}

// ─────────────────────────────────────────────────────────────────────
// Gates
// ─────────────────────────────────────────────────────────────────────

/// One verification command to run against the output directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateSpec {
    pub name: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    /// Whether a failure halts the mission (when `stop_on_gate_failure` is set).
    pub blocking: bool,
    /// True when the tool exits 0 even on a problem and only reports it on
    /// stdout — `gofmt -l` lists unformatted files and still succeeds. Without
    /// this flag such a gate can never fail, which is a gate that looks like it
    /// ran.
    pub fails_on_output: bool,
    /// Manifest that must exist in the output directory, else the gate is
    /// skipped rather than run against nothing.
    pub requires: Option<&'static str>,
}

/// The file that must exist for `lang`'s gate set to mean anything.
pub fn manifest_for(lang: &str) -> Option<&'static str> {
    match normalize_lang(lang).as_str() {
        "rust" => Some("Cargo.toml"),
        "python" => None,
        "typescript" | "javascript" => Some("package.json"),
        "go" => Some("go.mod"),
        _ => None,
    }
}

fn normalize_lang(lang: &str) -> String {
    let l = lang.trim().to_ascii_lowercase();
    match l.as_str() {
        "rs" => "rust".to_string(),
        "py" => "python".to_string(),
        "ts" => "typescript".to_string(),
        "js" | "node" | "nodejs" => "javascript".to_string(),
        "golang" => "go".to_string(),
        "cs" => "csharp".to_string(),
        "cpp" | "c++" => "cpp".to_string(),
        "kt" | "kts" => "kotlin".to_string(),
        other => other.to_string(),
    }
}

/// The gates a port to `lang` must pass.
///
/// Empty for languages this module has no verified toolchain story for. That is
/// intentional and is reported as "nothing was machine-verified" rather than
/// papered over with a gate that always passes — an unattended run that claims
/// green gates it never ran is worse than one that admits it checked nothing.
pub fn gate_set(lang: &str) -> Vec<GateSpec> {
    let spec = |name: &'static str,
                program: &'static str,
                args: &'static [&'static str],
                blocking: bool,
                fails_on_output: bool,
                requires: Option<&'static str>| {
        GateSpec {
            name,
            program,
            args,
            blocking,
            fails_on_output,
            requires,
        }
    };
    match normalize_lang(lang).as_str() {
        "rust" => vec![
            spec(
                "fmt",
                "cargo",
                &["fmt", "--all", "--", "--check"],
                false,
                false,
                Some("Cargo.toml"),
            ),
            spec(
                "build",
                "cargo",
                &["build", "--all-targets"],
                true,
                false,
                Some("Cargo.toml"),
            ),
            spec(
                "clippy",
                "cargo",
                &["clippy", "--all-targets", "--", "-D", "warnings"],
                true,
                false,
                Some("Cargo.toml"),
            ),
            spec(
                "test",
                "cargo",
                &["test", "--all"],
                true,
                false,
                Some("Cargo.toml"),
            ),
            // Non-blocking and `--no-run`: the performance sweep is the phase
            // that stands up a benchmark harness, and a harness that does not
            // compile is a real finding, but a crate with no `[[bench]]` target
            // yet must not fail the mission for it. Running the benchmarks is
            // left to the sweep, which is what reports the numbers.
            spec(
                "bench",
                "cargo",
                &["bench", "--all-targets", "--no-run"],
                false,
                false,
                Some("Cargo.toml"),
            ),
        ],
        "python" => vec![
            spec(
                "compile",
                "python",
                &["-m", "compileall", "-q", "."],
                true,
                false,
                None,
            ),
            spec("test", "python", &["-m", "pytest", "-q"], true, false, None),
        ],
        "typescript" | "javascript" => vec![
            spec(
                "typecheck",
                "npx",
                &["--no-install", "tsc", "--noEmit"],
                true,
                false,
                Some("package.json"),
            ),
            spec(
                "test",
                "npm",
                &["test", "--silent"],
                true,
                false,
                Some("package.json"),
            ),
        ],
        "go" => vec![
            spec("fmt", "gofmt", &["-l", "."], false, true, Some("go.mod")),
            spec(
                "build",
                "go",
                &["build", "./..."],
                true,
                false,
                Some("go.mod"),
            ),
            spec("vet", "go", &["vet", "./..."], true, false, Some("go.mod")),
            spec(
                "test",
                "go",
                &["test", "./..."],
                true,
                false,
                Some("go.mod"),
            ),
        ],
        _ => Vec::new(),
    }
}

/// One command run in a directory under a real deadline, with both pipes drained.
///
/// The shape is here because getting it wrong is silent and fatal, and the agent
/// loop's post-run test gate needs the same guarantees the mission gates do:
///
/// - `Command::output()` offers no deadline, so a hung `cargo test` hangs the
///   caller forever. This spawns and polls `try_wait`, killing the tree when the
///   deadline passes.
/// - Polling `try_wait` while the child writes more than the OS pipe buffer
///   (~64 KB) deadlocks: the child blocks on write, the parent blocks on wait.
///   `cargo test` on a crate with generated coverage output exceeds that easily,
///   so both streams are read on their own threads from the start.
/// - Killing the child does not close its pipes when it spawned grandchildren —
///   they inherit the write handle — so the drain gets a bounded grace period
///   rather than a join.
///
/// The raw output is the point: a caller that has to judge *which* tests failed
/// cannot read that out of an error-line tally.
pub struct BoundedRun {
    pub timed_out: bool,
    /// The child reported an exit status (so `code` is meaningful).
    pub exited: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

impl BoundedRun {
    /// Exit code 0, no timeout, no signal — the only run that may be reported
    /// as having passed.
    pub fn succeeded(&self) -> bool {
        self.exited && !self.timed_out && self.code == Some(0)
    }

    pub fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

/// Spawn `program args` in `dir` and wait for it no longer than `timeout`.
///
/// `Err` carries the OS message for a program that never started, which a caller
/// reports as "this gate did not run" — never as a pass, and never as a failure
/// of the code under test.
pub fn run_bounded(
    dir: &Path,
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<BoundedRun, String> {
    let started = Instant::now();
    let mut child = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    let (out_tx, out_rx) = std::sync::mpsc::channel();
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    // The handles are deliberately not kept: nothing may join them, because a
    // join is exactly the unbounded wait this function exists to avoid.
    let _ = child
        .stdout
        .take()
        .map(|s| std::thread::spawn(move || drain_pipe(s, out_tx)));
    let _ = child
        .stderr
        .take()
        .map(|s| std::thread::spawn(move || drain_pipe(s, err_tx)));

    let deadline = started + timeout;
    let mut timed_out = false;
    // No initialiser: every path out of the loop assigns it, and a `None` here
    // would be a value the compiler can see is never read.
    let exit: Option<std::process::ExitStatus>;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit = Some(status);
                break;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_tree(&mut child);
                    exit = child.wait().ok();
                    timed_out = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                kill_tree(&mut child);
                let _ = child.wait();
                // Dropping the receivers makes each reader's next send fail, so
                // the threads finish on their own instead of parking on a pipe
                // that nothing will ever read again.
                drop(out_rx);
                drop(err_rx);
                return Err(format!("could not wait on `{program}`: {e}"));
            }
        }
    }

    // Capped at 10s rather than tied to the command's own timeout: the grace
    // period is for a pipe a dying grandchild still holds, and a 900s deadline
    // must not turn into a 900s wait for output that has stopped.
    let grace = timeout
        .min(Duration::from_secs(10))
        .max(Duration::from_secs(1));
    let stdout = collect_pipe(out_rx, grace);
    let stderr = collect_pipe(err_rx, grace);
    Ok(BoundedRun {
        timed_out,
        exited: exit.is_some(),
        code: exit.and_then(|s| s.code()),
        stdout,
        stderr,
        duration: started.elapsed(),
    })
}

/// Run one gate in `dir` with a real deadline.
///
/// The deadline, the drained pipes and the grandchild-safe kill all live in
/// [`run_bounded`]; this adds the mission's own reporting: a gate with no
/// manifest beside it is *skipped*, never silently passed, and a failure is
/// diagnosable from the report alone.
pub fn run_gate(dir: &Path, spec: &GateSpec, timeout: Duration) -> GateResult {
    let command = format!("{} {}", spec.program, spec.args.join(" "));
    let started = Instant::now();

    if let Some(manifest) = spec.requires {
        if !dir.join(manifest).is_file() {
            return GateResult::skipped(
                spec.name,
                &command,
                format!("no {manifest} in {} — nothing to check yet", dir.display()),
            );
        }
    }
    if !dir.is_dir() {
        return GateResult::skipped(
            spec.name,
            &command,
            format!("output directory {} does not exist", dir.display()),
        );
    }

    let run = match run_bounded(dir, spec.program, spec.args, timeout) {
        Ok(run) => run,
        Err(e) => {
            return GateResult::skipped(
                spec.name,
                &command,
                format!("`{}` is not runnable here: {e}", spec.program),
            )
        }
    };
    // `succeeded()` already folds the timeout in.
    let exit_ok = run.succeeded();
    let BoundedRun {
        timed_out,
        stdout,
        stderr,
        ..
    } = run;
    let combined = format!("{stdout}\n{stderr}");

    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for line in combined.lines() {
        let t = line.trim();
        if t.starts_with("error[") || t.starts_with("error:") || t.starts_with("Error:") {
            errors.push(t.to_string());
        } else if t.starts_with("warning:") || t.starts_with("warning[") {
            warnings.push(t.to_string());
        }
    }
    if timed_out {
        errors.push(format!(
            "gate `{}` exceeded its {}s deadline and was killed",
            spec.name,
            timeout.as_secs()
        ));
    }
    // `gofmt -l` and friends exit 0 while listing problems on stdout, so a
    // non-empty listing is a failure the exit status will not report.
    let output_is_failure = spec.fails_on_output && !stdout.trim().is_empty();
    if output_is_failure {
        errors.push(format!(
            "`{}` exited 0 but reported problems on stdout",
            spec.name
        ));
    }

    let success = exit_ok && !timed_out && errors.is_empty() && !output_is_failure;
    GateResult {
        gate: spec.name.to_string(),
        command,
        success,
        skipped: false,
        timed_out,
        blocking: spec.blocking,
        duration_ms: started.elapsed().as_millis() as u64,
        errors,
        warnings,
        tail: tail_lines(&combined, 20),
    }
}

/// Read a pipe to EOF, forwarding each chunk as it arrives.
///
/// Chunked rather than `read_to_end` so that a gate killed at its deadline
/// still yields what it printed before hanging, which is usually the line
/// naming the test that hung — the one thing worth having from a timeout.
fn drain_pipe<R: std::io::Read>(mut r: R, tx: std::sync::mpsc::Sender<Option<String>>) {
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf) {
            // EOF, or a handle closed underneath us; either way it is over.
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                // A send error means the receiver was dropped because the gate
                // gave up waiting, so there is no point reading any further.
                if tx.send(Some(chunk)).is_err() {
                    break;
                }
            }
        }
    }
    let _ = tx.send(None);
}

/// Gather what a drained pipe produced, giving up after `grace`.
///
/// Killing a gate does not necessarily close its pipes. Gates are shells and
/// test harnesses that spawn children of their own, and a child inherits the
/// write handle: `cmd /C ping -n 30 ...` killed after two seconds keeps the pipe
/// open for another twenty-eight. Joining the reader thread unconditionally
/// would therefore make `gate_timeout_secs` a lie and stall an unattended
/// mission inside a single gate, holding the whole spend budget open while
/// producing nothing.
fn collect_pipe(rx: std::sync::mpsc::Receiver<Option<String>>, grace: Duration) -> String {
    let deadline = Instant::now() + grace;
    let mut out = String::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match rx.recv_timeout(left) {
            Ok(Some(chunk)) => out.push_str(&chunk),
            // `None` is the reader's end-of-stream marker; an error means the
            // thread panicked. Neither leaves anything worth waiting for.
            Ok(None) | Err(_) => break,
        }
    }
    out
}

/// Kill the gate and everything it spawned.
///
/// `Child::kill` reaches exactly one process, which is not enough for the
/// reason given in [`collect_pipe`]. `taskkill /T` walks the tree; it is
/// best-effort, and where it is unavailable the plain kill still covers the
/// common case of a gate that spawns nothing.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

/// Last `n` non-empty lines, oldest first.
fn tail_lines(text: &str, n: usize) -> String {
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .take(n)
        .collect();
    kept.into_iter().rev().collect::<Vec<_>>().join("\n")
}

/// Run every gate for `lang` in `dir`, in order.
///
/// Stops early on a blocking failure only when `stop_on_failure` is set; the
/// default is to run the whole set, because "build failed" plus "clippy found
/// 40 lints" is more actionable than "build failed" alone and costs nothing but
/// the time the gates were going to take anyway.
pub fn run_gates(
    dir: &Path,
    lang: &str,
    timeout: Duration,
    stop_on_failure: bool,
) -> Vec<GateResult> {
    let specs = gate_set(lang);
    if specs.is_empty() {
        return vec![GateResult::skipped(
            "none",
            "(no command)",
            format!(
                "no gate set is defined for target language '{lang}', so nothing in the output \
                 was machine-verified — a green report here would be a claim, not a measurement"
            ),
        )];
    }
    let mut out = Vec::with_capacity(specs.len());
    for spec in &specs {
        let r = run_gate(dir, spec, timeout);
        let halt = stop_on_failure && spec.blocking && !r.success && !r.skipped;
        out.push(r);
        if halt {
            break;
        }
    }
    out
}

/// One-line verdict over a gate run.
///
/// Skipped gates are counted and named. A summary that said only "3 passed,
/// 1 failed" would let a mission with no toolchain at all read as three
/// successes.
pub fn gate_summary(gates: &[GateResult]) -> String {
    if gates.is_empty() {
        return "no gates ran".to_string();
    }
    let passed: Vec<&str> = gates
        .iter()
        .filter(|g| g.success)
        .map(|g| g.gate.as_str())
        .collect();
    let failed: Vec<&str> = gates
        .iter()
        .filter(|g| !g.success && !g.skipped)
        .map(|g| g.gate.as_str())
        .collect();
    let skipped: Vec<&str> = gates
        .iter()
        .filter(|g| g.skipped)
        .map(|g| g.gate.as_str())
        .collect();
    let mut parts = Vec::new();
    parts.push(format!("{} passed", passed.len()));
    if !failed.is_empty() {
        parts.push(format!("{} FAILED ({})", failed.len(), failed.join(", ")));
    }
    if !skipped.is_empty() {
        parts.push(format!(
            "{} skipped ({})",
            skipped.len(),
            skipped.join(", ")
        ));
    }
    format!(
        "gates: {}{}",
        parts.join(", "),
        if failed.is_empty() && !skipped.is_empty() {
            " — a skipped gate verified nothing"
        } else {
            ""
        }
    )
}

// ─────────────────────────────────────────────────────────────────────
// Target paths and prompt assembly
// ─────────────────────────────────────────────────────────────────────

/// File extension the ported code should use, per target language.
const TARGET_EXT: &[(&str, &str)] = &[
    ("rust", "rs"),
    ("python", "py"),
    ("typescript", "ts"),
    ("javascript", "js"),
    ("go", "go"),
    ("java", "java"),
    ("kotlin", "kt"),
    ("csharp", "cs"),
    ("cpp", "cpp"),
    ("c", "c"),
    ("zig", "zig"),
    ("ruby", "rb"),
    ("php", "php"),
    ("swift", "swift"),
    ("haskell", "hs"),
    ("scala", "scala"),
    ("dart", "dart"),
];

/// Conventions every porter is pointed at, written by the design phase.
///
/// Without it, 1,175 independently dispatched sectors produce 1,175 opinions
/// about error handling, module layout and naming. The file is a prompt-level
/// contract: cheap to write once, and the only thing that makes the output read
/// as one codebase rather than a directory of strangers.
pub const CONVENTIONS_FILE: &str = "conventions.md";

pub fn target_ext(lang: &str) -> Option<&'static str> {
    let l = normalize_lang(lang);
    TARGET_EXT.iter().find(|(k, _)| *k == l).map(|(_, v)| *v)
}

/// Where a source unit's ported code goes, relative to the output root.
///
/// Strips the detected source root so `litellm/proxy/router.py` lands at
/// `proxy/router.rs` rather than `litellm/proxy/router.rs`: the output root is
/// already the package, and nesting it inside a second copy of the repo name
/// makes every import path in the port wrong.
///
/// For Rust the result is additionally placed under `src/`, because Rust is the
/// one target language here that compiles only what is reachable from the crate
/// root through `mod` declarations. A ported file anywhere else is invisible to
/// `cargo build`, so the gates compile an empty crate and report green — which
/// is exactly what happened on the first live LiteLLM run, where every porter
/// wrote to `output/enterprise/...` and the build, clippy and test gates all
/// passed without reading a line of it. [`wire_rust_modules`] declares whatever
/// lands under `src/`.
pub fn target_rel(unit: &SectorUnit, source_root: &str, lang: &str) -> String {
    let rel = unit.rel.replace('\\', "/");
    let prefix = source_root.trim_matches('/');
    let stripped = if prefix.is_empty() {
        rel.as_str()
    } else {
        rel.strip_prefix(&format!("{prefix}/")).unwrap_or(&rel)
    };
    let swapped = match (stripped.rsplit_once('.'), target_ext(lang)) {
        // A known target language always wins over the source extension, which
        // is the whole point of a rewrite. An extensionless source file whose
        // language has no mapping keeps its name unchanged.
        (Some((stem, _)), Some(new)) => format!("{stem}.{new}"),
        (None, Some(new)) => format!("{stripped}.{new}"),
        _ => stripped.to_string(),
    };
    if normalize_lang(lang) != "rust" {
        return swapped;
    }
    let (dir, file) = match swapped.rsplit_once('/') {
        Some((d, f)) => (format!("{d}/"), f.to_string()),
        None => (String::new(), swapped),
    };
    // `src/lib.rs` is the crate root, and `<dir>/mod.rs` is the file the wiring
    // pass generates to declare that directory's children. A porter writing
    // either would be overwritten by the next pass, or would overwrite it and
    // take the whole module tree with it, so both stems are shifted aside.
    // `main.rs` is deliberately left alone: cargo builds it as a binary target
    // alongside the library, which is what a ported `main.py` should become.
    let file = match (dir.is_empty(), file.as_str()) {
        (true, "lib.rs") => "lib_.rs".to_string(),
        (_, "mod.rs") => "mod_.rs".to_string(),
        _ => file,
    };
    format!("src/{dir}{file}")
}

/// The byte ceiling a dispatch actually preloads to *right now*.
///
/// [`PROMPT_PRELOAD_CAP_BYTES`] is that cap's default, but the dispatcher reads
/// `VELOCITY_PRELOAD_MAX_BYTES` on every call, so this is the only number the
/// "pre-loaded below in full" claim can honestly be tested against. Reading it
/// rather than the constant is what keeps an operator who *lowers* the cap from
/// getting a task that swears the source arrived whole when the brief cut it.
pub fn prompt_preload_cap_bytes() -> u64 {
    crate::registry::team_tools::preload_max_bytes() as u64
}

/// The note a plan earns when a unit is one unbreakable line longer than the
/// ceiling the chunker was told to fit.
///
/// Extracted because the sentence names a number, and the number it used to name
/// was `PROMPT_PRELOAD_CAP_BYTES` — the preload cap — while `units_over_ceiling`
/// counts units over `SectorOptions::max_unit_bytes`. Two different limits: the
/// note told an operator to raise a cap that had nothing to do with the units
/// being reported. Reading the ceiling out of the plan is what keeps the count
/// and the number it is a count against from ever parting company.
fn over_ceiling_note(totals: &crate::automation::sectors::SectorTotals) -> Option<String> {
    if totals.units_over_ceiling == 0 {
        return None;
    }
    Some(format!(
        "{} units are a single line longer than the {}-byte per-unit ceiling this plan was chunked against and cannot be handed to a model whole",
        totals.units_over_ceiling, totals.unit_ceiling_bytes
    ))
}

/// Split a sector's units into what is safe to pre-load into the dispatch
/// prompt and what the agent must read itself.
///
/// The split exists because `team_dispatch` pastes at most
/// [`prompt_preload_cap_bytes`] of each pre-loaded file and marks the cut with a
/// note naming the path and the `read_file` `startLine` that returns the rest.
/// That note is what makes the loss recoverable for an agent that goes and
/// reads, but a porting task whose contract is "port this unit whole" still gets
/// silent data loss if it does not: the model ports the first third of a file
/// and nobody finds out until the parity review, if then. Partial units are
/// never pre-loaded either — a line span is not a file, and pre-loading the
/// whole file hands over exactly the context the chunker was built to avoid.
pub fn split_units_for_prompt(units: &[SectorUnit]) -> (Vec<&SectorUnit>, Vec<&SectorUnit>) {
    split_units_for_prompt_at_cap(units, prompt_preload_cap_bytes())
}

/// The split with its ceiling passed in, so a test can pin a size instead of
/// inheriting whatever the ambient environment happens to set.
fn split_units_for_prompt_at_cap(
    units: &[SectorUnit],
    cap_bytes: u64,
) -> (Vec<&SectorUnit>, Vec<&SectorUnit>) {
    let mut preload = Vec::new();
    let mut deferred = Vec::new();
    for u in units {
        if !u.is_partial() && u.bytes <= cap_bytes {
            preload.push(u);
        } else {
            deferred.push(u);
        }
    }
    (preload, deferred)
}

/// Hard ceiling on the conventions contract inlined into every porting task.
///
/// The contract rides in the pinned mission anchor, so it is sent on every turn
/// of every dispatch. A normal contract is a few KB; this cap only bites if the
/// design phase runs away, and an unattended mission cannot let one oversized
/// document silently bloat thousands of dispatches past their context window.
pub const CONVENTIONS_INLINE_CAP_BYTES: usize = 24_000;

/// Trim the conventions contract to [`CONVENTIONS_INLINE_CAP_BYTES`] on a char
/// boundary, with a note saying it was capped. Under the cap it is verbatim.
fn cap_conventions(body: &str) -> String {
    if body.len() <= CONVENTIONS_INLINE_CAP_BYTES {
        return body.to_string();
    }
    let mut end = CONVENTIONS_INLINE_CAP_BYTES;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n\n[conventions contract capped at {} bytes for the context budget; the rules above are the binding ones]",
        &body[..end],
        CONVENTIONS_INLINE_CAP_BYTES
    )
}

/// The task text for porting one unit.
///
/// Says where the source is, where the output goes, and — for a chunk — that
/// earlier chunks of the same file are already written and must be extended
/// rather than replaced. That last instruction is the reason units inside a
/// sector run sequentially: chunk 3 dispatched alongside chunk 2 would either
/// clobber it or duplicate it.
pub fn porting_task(
    unit: &SectorUnit,
    manifest: &MissionManifest,
    source_rel: &str,
    output_rel: &str,
    preloaded: bool,
    conventions: Option<&str>,
) -> String {
    let target = target_rel(unit, &manifest.package_root, &manifest.target_language);
    let mut t = format!(
        "Port one unit of the {lang} rewrite of {repo}.\n\nSOURCE: {src}/{rel}\n",
        lang = manifest.target_language,
        repo = manifest.repo_name,
        src = source_rel.trim_end_matches('/'),
        rel = unit.rel
    );
    if unit.is_partial() {
        t.push_str(&format!(
            "  lines {a}-{b} only — chunk {c} of {n}. The file is too large for one context window so it arrives in pieces; do NOT read outside this span.\n",
            a = unit.line_start,
            b = unit.line_end,
            c = unit.chunk,
            n = unit.chunks
        ));
    }
    t.push_str(&format!(
        "TARGET: {out}/{target}\nSIZE: {bytes} bytes\n\n",
        out = output_rel.trim_end_matches('/'),
        bytes = unit.bytes
    ));
    match conventions.map(str::trim).filter(|c| !c.is_empty()) {
        // Inlined whole into the task, which `team_dispatch` does not truncate
        // and the loop pins as the mission anchor, so the contract stays visible
        // on every turn. The old text told the porter to `read_file` it: a 19 KB
        // conventions.md read as a tool result is compressed away on the next
        // turn, and the stub used to say "call read_file again", so the porter
        // looped re-reading it (~18x per dispatch) and never wrote its target —
        // zero files, zero sectors, gates green over an empty crate.
        Some(body) => {
            let body = cap_conventions(body);
            t.push_str(&format!(
                "The shared conventions contract for the {lang} rewrite is below in full. Follow it exactly: it fixes the module layout, error-handling style and naming every other sector is using, and a locally sensible choice that disagrees with it is a defect. Do NOT read_file it again — this is the whole contract, and re-reading only burns the turns you need to port.\n\n<conventions>\n{body}\n</conventions>\n\n",
                lang = manifest.target_language,
                body = body.trim_end(),
            ));
        }
        None => t.push_str(&format!(
            "No conventions contract was produced for this rewrite. Do not wait for one, do not retry any read and do not fail the unit: port with idiomatic {lang} conventions and note in your one-line report that the conventions file was absent.\n\n",
            lang = manifest.target_language
        )),
    }
    if unit.is_partial() && unit.chunk > 1 {
        t.push_str(&format!(
            "The target file already exists and holds chunks 1-{prev} of this source file. EXTEND it: read it, then add the code for this span in the right place. Do not rewrite or delete what is already there, and do not re-emit chunks you were not given.\n\n",
            prev = unit.chunk - 1
        ));
    }
    t.push_str(if preloaded {
        "The source file is pre-loaded below in full. Port all of it.\n\n"
    } else {
        "The source is NOT pre-loaded — read it yourself with the read tool, using the line span above if one is given.\n\n"
    });
    t.push_str(
        "Requirements:\n\
         - Write idiomatic target-language code, not a transliteration. Match the source's observable behaviour, not its syntax.\n\
         - Write the result to the TARGET path above, creating parent directories as needed.\n\
         - Port only what this unit covers. Other units and other sectors own the rest; leave their files alone.\n\
         - Where the source depends on something not yet ported, declare the minimal interface you need and note it in your report rather than inventing an implementation.\n\
         - Do not modify anything under the SOURCE tree; it is the reference and must stay diffable.\n",
    );
    t
}

/// The task text for one batch of a sweep.
///
/// Names the axis and demands `file:line` findings. It deliberately does not ask
/// the auditor to fix anything: remediation is its own phase with its own seat,
/// and an auditor that edits the code it is auditing cannot report on the result.
pub fn sweep_task(sweep: &str, targets: &[String], output_rel: &str, source_rel: &str) -> String {
    let axis = match sweep {
        "security" => "SECURITY: injection and unvalidated input crossing a trust boundary; secrets, tokens or credentials reachable from logs, error text or debug output; unsafe blocks, raw pointer casts and disabled checks; dependency-manifest entries that are known-vulnerable or unmaintained.",
        "durability" => "DURABILITY: swallowed errors and `let _ =` on a fallible call; every unwrap/expect reachable from outside; resource lifetime — files, sockets, locks, spawned tasks — leaked on the error path as well as the happy path; partial-failure state that cannot be retried or resumed; integer overflow, empty-collection and boundary cases assumed away.",
        "performance" => "PERFORMANCE: accidental quadratic behaviour introduced by the port — repeated linear scans, cloning where a borrow would do, allocation inside a loop; blocking calls on an async path; hot paths with no benchmark coverage. Report numbers where you can measure them and say plainly where you cannot.",
        other => other,
    };
    let list = targets
        .iter()
        .map(|t| format!("- {t}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{sweep_upper} SWEEP over a batch of freshly ported code.\n\n\
Audit these files:\n{list}\n\n\
They live under `{out}/`; the original source they were ported from is under `{src}/` and is available for comparison.\n\n\
Look for, in this order: {axis}\n\n\
Report ONE finding per issue as `path:line — what is wrong — the concrete fix`. Do not restate the code, do not summarise what a file does, and do not edit anything: remediation is a separate phase with a separate owner. If you find nothing in a file, say so explicitly rather than omitting it — a silent pass is indistinguishable from a skipped file.\n",
        sweep_upper = sweep.to_uppercase(),
        out = output_rel.trim_end_matches('/'),
        src = source_rel.trim_end_matches('/')
    )
}

// ─────────────────────────────────────────────────────────────────────
// Budget
// ─────────────────────────────────────────────────────────────────────

/// This run's spend ceiling, shared across worker threads.
///
/// Caps apply per *run*, not per mission lifetime. A mission paused because it
/// hit its dispatch cap must be resumable with a fresh cap — otherwise raising
/// the limit and re-invoking would immediately re-trip the old one, and the only
/// way forward would be editing the progress file by hand. Lifetime totals live
/// in [`MissionProgress`] and are reported separately.
pub struct Budget {
    max_dispatches: usize,
    max_wall_secs: u64,
    started: Instant,
    dispatches: AtomicUsize,
    secs: AtomicU64,
}

impl Budget {
    pub fn new(max_dispatches: usize, max_wall_secs: u64) -> Self {
        Self {
            max_dispatches,
            max_wall_secs,
            started: Instant::now(),
            dispatches: AtomicUsize::new(0),
            secs: AtomicU64::new(0),
        }
    }

    /// `Some(reason)` once a cap is reached. Checked *before* each dispatch,
    /// which bounds the overshoot to the workers already past this check when
    /// the cap tripped — at most `parallelism - 1`. Checking after would let
    /// every worker start one more on top of that.
    pub fn exhausted(&self) -> Option<String> {
        if self.max_dispatches > 0 && self.dispatches.load(Ordering::SeqCst) >= self.max_dispatches
        {
            // Names both numbers. This string becomes the report's
            // `stop_reason`, where it sits next to `dispatches_this_run`, and
            // "(8 this run)" beside a count of 11 reads as a contradiction
            // rather than as the documented overshoot it is.
            return Some(format!(
                "dispatch cap reached (limit {}, {} dispatched; workers already past this check finish, so the count can exceed the cap by up to parallelism-1)",
                self.max_dispatches,
                self.dispatches()
            ));
        }
        if self.max_wall_secs > 0 && self.started.elapsed().as_secs() >= self.max_wall_secs {
            return Some(format!(
                "wall-clock cap reached ({}s this run)",
                self.max_wall_secs
            ));
        }
        None
    }

    pub fn record(&self, secs: u64) {
        self.dispatches.fetch_add(1, Ordering::SeqCst);
        self.secs.fetch_add(secs, Ordering::SeqCst);
    }

    pub fn dispatches(&self) -> usize {
        self.dispatches.load(Ordering::SeqCst)
    }

    pub fn secs(&self) -> u64 {
        self.secs.load(Ordering::SeqCst)
    }

    /// Dispatches still permitted, or `None` when uncapped.
    pub fn remaining(&self) -> Option<usize> {
        (self.max_dispatches > 0).then(|| self.max_dispatches.saturating_sub(self.dispatches()))
    }
}

// ─────────────────────────────────────────────────────────────────────
// Shared state
// ─────────────────────────────────────────────────────────────────────

/// State shared by every worker thread in a wave.
struct Shared {
    progress: Mutex<MissionProgress>,
    mission_dir: PathBuf,
    budget: Budget,
    /// Set once, by whoever notices a cap first. Workers check it between
    /// sectors so an in-flight dispatch is allowed to finish rather than being
    /// abandoned with half a file written.
    stop: AtomicBool,
    stop_reason: Mutex<Option<String>>,
    dry_run: AtomicBool,
}

impl Shared {
    fn new(mission_dir: PathBuf, progress: MissionProgress, cfg: &MissionConfig) -> Self {
        Self {
            progress: Mutex::new(progress),
            mission_dir,
            budget: Budget::new(cfg.max_dispatches, cfg.max_wall_secs),
            stop: AtomicBool::new(false),
            stop_reason: Mutex::new(None),
            dry_run: AtomicBool::new(cfg.dry_run),
        }
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn halt(&self, reason: String) {
        // A poisoned lock here means a worker panicked while holding it. Taking
        // the value anyway is right: losing the stop reason would make the
        // mission report "paused" with no explanation, which is the one thing
        // an operator cannot act on.
        let mut slot = self.stop_reason.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(reason);
        }
        self.stop.store(true, Ordering::SeqCst);
    }

    fn reason(&self) -> Option<String> {
        self.stop_reason
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Persist progress. Called after every sector, under the progress lock, so
    /// an interruption loses at most the sectors still in flight.
    fn checkpoint(&self) -> Result<(), String> {
        if self.dry_run.load(Ordering::SeqCst) {
            return Ok(());
        }
        let p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        p.save(&self.mission_dir)
    }
}

// ─────────────────────────────────────────────────────────────────────
// Dispatch
// ─────────────────────────────────────────────────────────────────────

/// What one `team_dispatch` call actually returned.
struct DispatchOutcome {
    ok: bool,
    secs: u64,
    provider: String,
    model: String,
    degraded: bool,
    error: Option<String>,
    transcript: String,
}

/// Call `team_dispatch` and interpret its JSON.
///
/// Success is the handler's own `success` field. It is never inferred from the
/// transcript: a *successful* porting run mentions compiler errors whenever it
/// quotes a diagnostic it already fixed, and reading that as failure is how an
/// earlier version of this system discarded a completed LiteLLM port and re-ran
/// it on two models that 404.
///
/// `deliverable` tells the agent what it owes: `"artifact"` for a task that
/// writes files (porting, design, docs, parity, remediation) and `"report"` for
/// an audit (a sweep). It selects the closing output-format block in
/// `team_dispatch`; sending the wrong one makes the agent produce prose when it
/// should have written a file, and report success while doing it.
fn dispatch(
    root: &Path,
    team_id: &str,
    task: &str,
    member: Option<&str>,
    files: &[String],
    deliverable: &str,
) -> DispatchOutcome {
    let started = Instant::now();
    let mut args = json!({ "team_id": team_id, "task": task, "deliverable": deliverable });
    if !files.is_empty() {
        args["files"] = json!(files);
    }
    if let Some(m) = member {
        args["member"] = json!(m);
    }
    let fail = |error: String, transcript: String| DispatchOutcome {
        ok: false,
        secs: started.elapsed().as_secs(),
        provider: String::new(),
        model: String::new(),
        degraded: false,
        error: Some(error),
        transcript,
    };

    let raw = match crate::registry::team_tools::handle_team_tool(root, "team_dispatch", &args) {
        Ok(Some(text)) => text,
        // `handle_team_tool` returns `Ok(None)` for a name it does not own. That
        // is a build-time wiring error, not a model failure, and saying so is
        // what stops somebody debugging the provider chain for an hour.
        Ok(None) => {
            return fail(
                "team_dispatch is not a registered tool in this build".into(),
                String::new(),
            )
        }
        Err(e) => return fail(e.to_string(), String::new()),
    };

    let v: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return fail(
                format!("team_dispatch returned unparseable JSON: {e}"),
                raw.chars().take(4000).collect(),
            )
        }
    };

    let ok = v["success"].as_bool().unwrap_or(false);
    let error = if ok {
        None
    } else {
        Some(
            v["fallback_attempts"]
                .as_array()
                .and_then(|a| a.last())
                .and_then(|e| e["blocked_reason"].as_str())
                .map(str::to_string)
                .unwrap_or_else(|| "no model in the fallback chain completed the task".to_string()),
        )
    };
    DispatchOutcome {
        ok,
        secs: started.elapsed().as_secs(),
        provider: v["provider"].as_str().unwrap_or("").to_string(),
        model: v["model"].as_str().unwrap_or("").to_string(),
        degraded: v["degraded"].as_bool().unwrap_or(false),
        error,
        transcript: v["transcript"].as_str().unwrap_or("").to_string(),
    }
}

/// Find the role title to pin a phase to.
///
/// Each candidate is tried in order: exact role-title match first, then
/// substring. Returns `None` when the team has no such seat, and callers must
/// then skip the phase and say so. Substituting a different specialist would be
/// worse than not running: an unattended security sweep that quietly ran on the
/// Documentation Engineer returns a clean report and closes the phase.
fn resolve_seat(root: &Path, team_id: &str, candidates: &[&str]) -> Option<String> {
    let team = load_team(root, team_id)?;
    for want in candidates {
        let lower = want.to_lowercase();
        if let Some(m) = team
            .members
            .iter()
            .find(|m| m.role.to_lowercase() == lower)
            .or_else(|| {
                team.members
                    .iter()
                    .find(|m| m.role.to_lowercase().contains(&lower))
            })
        {
            return Some(m.role.clone());
        }
    }
    None
}

/// The team's first member's role — the lead, by construction order.
fn first_role(root: &Path, team_id: &str) -> Option<String> {
    load_team(root, team_id).and_then(|t| t.members.first().map(|m| m.role.clone()))
}

fn load_team(root: &Path, team_id: &str) -> Option<crate::editor::expert_team::ExpertTeam> {
    let teams = crate::editor::expert_team::load_expert_teams(root);
    let lower = team_id.to_lowercase();
    let slug = crate::editor::expert_team::slugify(team_id);
    teams.into_iter().find(|t| {
        t.id.to_lowercase() == lower || t.slug() == slug || t.name.to_lowercase() == lower
    })
}

/// The seat each phase is pinned to, resolved once against the real roster.
///
/// Held in a struct rather than resolved per phase so a missing seat is reported
/// once, up front, instead of surfacing as a mid-run dispatch error at hour
/// four.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Seats {
    lead: Option<String>,
    porter: Option<String>,
    qa: Option<String>,
    reviewer: Option<String>,
    security: Option<String>,
    durability: Option<String>,
    performance: Option<String>,
    docs: Option<String>,
}

impl Seats {
    fn resolve(root: &Path, team_id: &str, lang: &str) -> Self {
        Self {
            lead: resolve_seat(root, team_id, &["Lead Architect", "Architect", "Lead"])
                .or_else(|| first_role(root, team_id)),
            // "port" before the language name: a roster may carry "Python Port
            // Engineer" or "Port Engineer" and both must resolve.
            porter: resolve_seat(root, team_id, &["Port Engineer", "port", lang]),
            qa: resolve_seat(root, team_id, &["Test & QA Engineer", "QA", "Test"]),
            reviewer: resolve_seat(root, team_id, &["Reviewer", "Review"]),
            security: resolve_seat(root, team_id, &["Security Sweep Auditor", "Security"]),
            durability: resolve_seat(root, team_id, &["Durability Engineer", "Durability"]),
            performance: resolve_seat(root, team_id, &["Performance Engineer", "Performance"]),
            docs: resolve_seat(root, team_id, &["Documentation Engineer", "Documentation"]),
        }
    }

    /// Every phase that has no seat, as one honest sentence each.
    fn missing(&self) -> Vec<String> {
        let mut out = Vec::new();
        let check = |seat: &Option<String>, phase: &str, out: &mut Vec<String>| {
            if seat.is_none() {
                out.push(format!(
                    "{phase} will be SKIPPED: no seat for it on the team. Skipping is deliberate — \
                     an unattended phase that runs on the wrong specialist reports success while \
                     doing a different job."
                ));
            }
        };
        check(&self.porter, "porting", &mut out);
        check(&self.lead, "design", &mut out);
        check(&self.qa, "test parity and remediation", &mut out);
        check(&self.security, "the security sweep", &mut out);
        check(&self.durability, "the durability sweep", &mut out);
        check(&self.performance, "the performance sweep", &mut out);
        check(&self.reviewer, "the parity review", &mut out);
        check(&self.docs, "documentation", &mut out);
        out
    }
}

/// Make the output directory a buildable project so the gates have something to
/// check.
///
/// Without this every gate skips forever — `cargo build` in a directory with no
/// manifest is not a failure the runner can act on — and the mission reports
/// "completed" having machine-verified nothing.
fn ensure_skeleton(output_dir: &Path, manifest: &MissionManifest) -> Option<String> {
    let lang = normalize_lang(&manifest.target_language);
    if lang != "rust" {
        return manifest_for(&lang).map(|m| {
            format!(
                "no project skeleton is generated for '{}' — the gates need a {m} at {} before they can run",
                manifest.target_language,
                output_dir.display()
            )
        });
    }
    if output_dir.join("Cargo.toml").is_file() {
        return None;
    }
    let name: String = manifest
        .repo_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let name = name.trim_matches('_');
    let name = if name.is_empty() { "ported" } else { name };
    let toml = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n"
    );
    if std::fs::create_dir_all(output_dir.join("src")).is_err() {
        return Some(format!(
            "could not create {}: the Rust gates will skip",
            output_dir.display()
        ));
    }
    let _ = std::fs::write(output_dir.join("Cargo.toml"), toml);
    // Written through the same renderer [`wire_rust_modules`] uses, so there is
    // one definition of what a crate root looks like and the skeleton cannot
    // drift into a shape the wiring pass will later refuse to overwrite.
    //
    // It also has to be rustfmt-clean exactly as it stands. `String::new()` was
    // not: rustfmt rejects a zero-byte `.rs` file and wants a single newline,
    // which left the fmt gate red on every Rust mission before a line had been
    // ported. A gate that always fails for a reason unrelated to the work is
    // worse than no gate, because it teaches the reader to skip the line.
    let _ = std::fs::write(
        output_dir.join("src").join("lib.rs"),
        render_module_file("the crate root", &[]),
    );
    Some(format!(
        "created a Rust crate skeleton at {} so the build, clippy and test gates have a manifest to run against",
        output_dir.display()
    ))
}

/// Marker on every module file the runner generates.
///
/// Regeneration has to distinguish its own files from ones a porter wrote.
/// Overwriting a porter's file deletes ported code; refusing to overwrite the
/// runner's own leaves the module tree stale and the new files unreachable. The
/// marker settles it, and a file without one is left alone and reported rather
/// than silently replaced.
pub const WIRING_MARKER: &str = "// velocity-mission: generated module wiring";

/// Words that cannot be a Rust module name but are ordinary file and directory
/// names in the languages being ported *from* — `type.py`, `match/`, `impl.rs`.
const RUST_RESERVED_MOD_NAMES: &[&str] = &[
    "as",
    "async",
    "await",
    "box",
    "break",
    "const",
    "continue",
    "crate",
    "dyn",
    "else",
    "enum",
    "extern",
    "false",
    "fn",
    "for",
    "if",
    "impl",
    "in",
    "let",
    "loop",
    "macro_rules",
    "match",
    "mod",
    "move",
    "mut",
    "pub",
    "ref",
    "return",
    "self",
    "static",
    "struct",
    "super",
    "trait",
    "true",
    "type",
    "unsafe",
    "use",
    "where",
    "while",
    "yield",
];

/// A module name Rust will accept, derived from a file stem or directory name.
///
/// The caller must still emit `#[path = ...]` whenever this differs from the on
/// disk name, because `pub mod foo;` makes rustc look for `foo.rs` and nothing
/// else. Real repositories need it: LiteLLM's dashboard lives under
/// `litellm-dashboard`, and a hyphen is not legal in an identifier.
fn rust_mod_name(raw: &str) -> String {
    let mut s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s.push('_');
    }
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    if RUST_RESERVED_MOD_NAMES.contains(&s.as_str()) {
        s.push('_');
    }
    s
}

/// Render one generated module file: the marker, a line saying what it is, and
/// the declarations. Deterministic and idempotent, so re-running the pass after
/// every wave produces byte-identical output when nothing changed.
fn render_module_file(what: &str, entries: &[String]) -> String {
    let mut s = format!(
        "{WIRING_MARKER}\n\
         //\n\
         // Declarations for {what}. Regenerated from what exists on disk after each\n\
         // wave, because a file nothing declares is a file `cargo build` never\n\
         // compiles and the gates never check. Do not edit by hand.\n"
    );
    for e in entries {
        s.push_str(e);
        s.push('\n');
    }
    s
}

/// One `pub mod` declaration, with the `#[path]` attribute it needs when the
/// module name had to be sanitised away from the name on disk.
fn mod_decl(mod_name: &str, on_disk: &str) -> String {
    if mod_name == on_disk {
        format!("pub mod {mod_name};")
    } else {
        format!("#[path = \"{on_disk}\"]\npub mod {mod_name};")
    }
}

/// Every `.rs` file under `dir`, sorted, skipping build output.
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                // `target/` is where cargo puts gigabytes of build output. Every
                // `.rs` under it is generated, and walking it would make this
                // audit slower than the porting it is auditing.
                if name == "target" || name.starts_with('.') {
                    continue;
                }
                stack.push(p);
                continue;
            }
            if name.ends_with(".rs") {
                out.push(p);
            }
        }
    }
    out.sort();
}

fn rel_to(root: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// What the wiring pass found and what it did.
///
/// Reported in full rather than reduced to a count: the interesting outcome is
/// not "wired 40 modules" but "3 ported files are outside `src/` and no gate
/// compiled them", which is the difference between a verified port and a green
/// report over an empty crate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RustWiring {
    /// Ported `.rs` files under `src/`, i.e. ones the crate root can now reach.
    /// Excludes the generated `lib.rs` and `mod.rs` files themselves.
    pub modules: usize,
    /// Module files written or rewritten, relative to the output root.
    pub wired: Vec<String>,
    /// `.rs` files under the output root but outside `src/`. Cargo never sees
    /// these, so they are ported code that no gate compiled.
    pub orphaned: Vec<String>,
    /// Existing files without [`WIRING_MARKER`], left untouched.
    pub left_alone: Vec<String>,
    /// Directories that have both `d.rs` and `d/` holding sources. Rust rejects
    /// that pair outright, so `d/`'s contents stay unreachable and are named
    /// here instead of being quietly left out of the build.
    pub conflicts: Vec<String>,
}

impl RustWiring {
    /// One line for the report. `None` when nothing was ported and nothing went
    /// wrong, which is the state of a mission that has not started yet.
    pub fn summary(&self) -> Option<String> {
        if self.modules == 0
            && self.orphaned.is_empty()
            && self.conflicts.is_empty()
            && self.left_alone.is_empty()
        {
            return None;
        }
        let mut parts = vec![format!(
            "{} ported module(s) reachable from src/lib.rs, {} module file(s) written",
            self.modules,
            self.wired.len()
        )];
        if !self.orphaned.is_empty() {
            parts.push(format!(
                "{} .rs file(s) OUTSIDE src/ that cargo will never compile: {}",
                self.orphaned.len(),
                cap_list(&self.orphaned, 5)
            ));
        }
        if !self.conflicts.is_empty() {
            parts.push(format!(
                "{} directory/directory.rs conflict(s) leaving children unreachable: {}",
                self.conflicts.len(),
                cap_list(&self.conflicts, 5)
            ));
        }
        if !self.left_alone.is_empty() {
            parts.push(format!(
                "{} existing module file(s) not carrying the wiring marker were left alone, so their declarations may be stale: {}",
                self.left_alone.len(),
                cap_list(&self.left_alone, 5)
            ));
        }
        Some(parts.join("; "))
    }
}

/// `a, b, c (+N more)` — the counts stay exact even when the examples are cut.
fn cap_list(items: &[String], max: usize) -> String {
    let shown: Vec<&str> = items.iter().take(max).map(|s| s.as_str()).collect();
    let extra = items.len().saturating_sub(shown.len());
    if extra == 0 {
        shown.join(", ")
    } else {
        format!("{} (+{extra} more)", shown.join(", "))
    }
}

/// Declare every ported file under `src/` so the gates actually compile it.
///
/// Driven by what is on disk rather than by the plan, because the porter is a
/// model: it may write extra files, skip a target, or invent a layout nobody
/// asked for. Reading the tree means the wiring always matches reality, and
/// anything the model wrote that does not compile then fails the build gate —
/// which is the point. On the first live LiteLLM run it wrote a syntactically
/// invalid `pub type Result<T, std::result::Result<T, error::Error>>;` and the
/// build gate passed, because the file was never part of the crate.
///
/// Idempotent, so it is safe to call after every wave and before every gate run.
pub fn wire_rust_modules(output_dir: &Path) -> RustWiring {
    let mut w = RustWiring::default();
    let src = output_dir.join("src");
    if !src.is_dir() {
        return w;
    }

    let mut all: Vec<PathBuf> = Vec::new();
    collect_rs(output_dir, &mut all);
    for p in &all {
        if let Some(rel) = rel_to(output_dir, p) {
            if !rel.starts_with("src/") {
                w.orphaned.push(rel);
            }
        }
    }

    // dir (relative to `src`, "" for `src` itself) -> file stems in it, and the
    // child directories that hold sources.
    let mut files_by_dir: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut dirs_by_parent: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for p in &all {
        let Ok(rel) = p.strip_prefix(&src) else {
            continue;
        };
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        let Some(file) = parts.last() else { continue };
        let Some(stem) = file.strip_suffix(".rs") else {
            continue;
        };
        let dir = parts[..parts.len() - 1].join("/");
        files_by_dir
            .entry(dir.clone())
            .or_default()
            .push(stem.to_string());
        let mut cur = dir;
        while let Some((parent, name)) = cur.rsplit_once('/') {
            dirs_by_parent
                .entry(parent.to_string())
                .or_default()
                .insert(name.to_string());
            cur = parent.to_string();
        }
        if !cur.is_empty() {
            dirs_by_parent
                .entry(String::new())
                .or_default()
                .insert(cur.clone());
        }
    }

    let mut dirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    dirs.extend(files_by_dir.keys().cloned());
    dirs.extend(dirs_by_parent.keys().cloned());

    for d in &dirs {
        let is_root = d.is_empty();
        // The generated file for this directory: `src/lib.rs` at the root,
        // `src/<d>/mod.rs` below it. It must not declare itself.
        let own_stem = if is_root { "lib" } else { "mod" };
        let child_dirs = dirs_by_parent.get(d).cloned().unwrap_or_default();
        let stems = files_by_dir.get(d).cloned().unwrap_or_default();

        let mut entries: Vec<String> = Vec::new();
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for stem in stems.iter().filter(|s| s.as_str() != own_stem) {
            // `d/<stem>.rs` and `d/<stem>/` cannot both exist in Rust. The file
            // wins — it may hold ported code — and the directory's children are
            // reported as unreachable rather than silently dropped from the
            // build, which is what would happen if this pass just skipped them.
            if child_dirs.contains(stem) {
                let prefix = if is_root {
                    "src/".to_string()
                } else {
                    format!("src/{d}/")
                };
                w.conflicts
                    .push(format!("{prefix}{stem}.rs and {prefix}{stem}/ both exist"));
                continue;
            }
            if !seen.insert(stem.clone()) {
                continue;
            }
            let m = rust_mod_name(stem);
            entries.push(mod_decl(&m, &format!("{stem}.rs")));
        }
        for c in &child_dirs {
            // A directory whose `d.rs` twin won above is not declared; its
            // children were recorded as a conflict already.
            if stems.iter().any(|s| s == c) {
                continue;
            }
            let m = rust_mod_name(c);
            entries.push(mod_decl(&m, &format!("{c}/mod.rs")));
        }
        entries.sort();
        entries.dedup();

        let rel_dir = if is_root {
            "src".to_string()
        } else {
            format!("src/{d}")
        };
        let file_name = if is_root { "lib.rs" } else { "mod.rs" };
        let path = output_dir.join(&rel_dir).join(file_name);
        let rel_path = format!("{rel_dir}/{file_name}");
        let what = if is_root {
            "the crate root".to_string()
        } else {
            format!("module `{}`", d.replace('/', "::"))
        };
        let body = render_module_file(&what, &entries);
        match std::fs::read_to_string(&path) {
            // Absent, unreadable, or ours: write it. An unreadable file is
            // treated as ours rather than left alone, because leaving it means
            // the modules beside it stay unreachable and the gates go green on
            // a crate that compiles nothing.
            Err(_) => {
                if std::fs::write(&path, &body).is_ok() {
                    w.wired.push(rel_path);
                }
            }
            Ok(existing) if existing.starts_with(WIRING_MARKER) => {
                if existing != body && std::fs::write(&path, &body).is_ok() {
                    w.wired.push(rel_path);
                }
            }
            Ok(_) => w.left_alone.push(rel_path),
        }
    }

    w.modules = all
        .iter()
        .filter(|p| {
            p.strip_prefix(&src).is_ok()
                && p.file_name()
                    .map(|n| {
                        let n = n.to_string_lossy();
                        let is_root_lib = p.parent() == Some(src.as_path()) && n == "lib.rs";
                        !is_root_lib && n != "mod.rs"
                    })
                    .unwrap_or(false)
        })
        .count();
    w
}

/// Run the wiring pass and turn what it found into notes.
///
/// Called before every gate run, because the gates are only meaningful if the
/// crate they build contains the port. The alarming case is a mission that
/// records completed sectors while nothing is reachable from the crate root:
/// every gate then compiles an empty crate and passes. That is not a
/// hypothetical, it is what the first live LiteLLM run did — `build`, `clippy`,
/// `test` and `bench` all reported success over a zero-byte `lib.rs` while a
/// syntactically invalid file sat unreferenced a few directories away — so it
/// gets its own note in terms that cannot be read as progress.
fn wire_output_tree(
    output_dir: &Path,
    target_language: &str,
    ported_sectors: usize,
    notes: &mut Vec<String>,
) -> RustWiring {
    if normalize_lang(target_language) != "rust" {
        return RustWiring::default();
    }
    let w = wire_rust_modules(output_dir);
    if let Some(s) = w.summary() {
        notes.push(format!("rust module wiring: {s}"));
    }
    if w.modules == 0 && ported_sectors > 0 {
        notes.push(format!(
            "GATES VERIFIED NOTHING: {ported_sectors} sector(s) are recorded as ported but no \
             ported .rs file is reachable from src/lib.rs, so the build, clippy and test gates \
             compiled an empty crate and their green result says nothing about the port. {}",
            if w.orphaned.is_empty() {
                "The porters wrote no files at all."
            } else {
                "Files were written outside src/ and are listed in the wiring note above."
            }
        ));
    }
    w
}

/// How a sector's run ended.
///
/// `Interrupted` is not `Failed`, and collapsing the two was a real defect. A
/// sector stopped because *this run* hit a dispatch or wall-clock cap has done
/// nothing wrong, but recording it via `mark_failed` consumed one of its
/// `max_attempts_per_sector` retries. Running under caps is the documented
/// operating mode for a mission this size, so a sector interrupted twice was
/// filtered out of every later run and never ported at all — while the report
/// counted it as a failure, which reads as a quality problem rather than a
/// budget boundary. That contradicts the promise on [`Budget`] that a capped run
/// is resumable with a fresh cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorOutcome {
    /// Every unit was dispatched and succeeded.
    Done,
    /// A dispatch failed. The only outcome that consumes a retry.
    Failed,
    /// The run stopped before the sector finished — a cap tripped here, or
    /// another worker halted the run. Stays pending and resumes at no cost.
    Interrupted,
}

/// Port every unit of one sector, sequentially.
///
/// Sequential within a sector, parallel across sectors — see the module docs. The
/// first failing unit stops the sector: later units of the same file would be
/// extending something that is not there.
fn run_sector(
    root: &Path,
    manifest: &MissionManifest,
    plan: &SectorPlan,
    sector_id: &str,
    porter: Option<&str>,
    source_rel: &str,
    output_rel: &str,
    conventions_rel: &str,
    shared: &Shared,
) -> (SectorOutcome, Vec<String>) {
    let Some(sector) = plan.sector(sector_id) else {
        return (
            SectorOutcome::Failed,
            vec![format!("sector '{sector_id}' is not in the plan")],
        );
    };
    let mut notes = Vec::new();
    let mut state = SectorOutcome::Done;

    // Read the conventions contract once per sector and inline it into every
    // porting task (see `porting_task`). Per unit would re-read the same file
    // for each chunk of a large source; per mission would have to thread it
    // through `run_wave`. Absent is fine — `porting_task` has a missing-case.
    let conventions = std::fs::read_to_string(root.join(conventions_rel)).ok();

    for unit in &sector.units {
        if shared.stopping() {
            state = SectorOutcome::Interrupted;
            notes.push(format!(
                "stopped before {}: {}",
                unit.span_label(),
                shared.reason().unwrap_or_default()
            ));
            break;
        }
        if let Some(reason) = shared.budget.exhausted() {
            shared.halt(reason.clone());
            state = SectorOutcome::Interrupted;
            notes.push(format!("stopped before {}: {reason}", unit.span_label()));
            break;
        }

        let preloaded = !unit.is_partial() && unit.bytes <= prompt_preload_cap_bytes();
        let task = porting_task(
            unit,
            manifest,
            source_rel,
            output_rel,
            preloaded,
            conventions.as_deref(),
        );
        let files: Vec<String> = if preloaded {
            vec![format!("{}/{}", source_rel.trim_end_matches('/'), unit.rel)]
        } else {
            Vec::new()
        };

        let outcome = if shared.dry_run.load(Ordering::SeqCst) {
            DispatchOutcome {
                ok: true,
                secs: 0,
                provider: "dry-run".to_string(),
                model: "none".to_string(),
                degraded: false,
                error: None,
                transcript: String::new(),
            }
        } else {
            dispatch(root, &manifest.team_id, &task, porter, &files, "artifact")
        };

        shared.budget.record(outcome.secs);
        {
            let mut p = shared.progress.lock().unwrap_or_else(|e| e.into_inner());
            p.record_dispatch(DispatchRecord {
                sector: sector_id.to_string(),
                unit: unit.span_label(),
                ok: outcome.ok,
                secs: outcome.secs,
                provider: outcome.provider.clone(),
                model: outcome.model.clone(),
                degraded: outcome.degraded,
                error: outcome.error.clone(),
            });
        }

        if !outcome.ok {
            state = SectorOutcome::Failed;
            notes.push(format!(
                "{} failed: {}",
                unit.span_label(),
                outcome.error.unwrap_or_else(|| "unknown".to_string())
            ));
            break;
        }
        if outcome.degraded {
            notes.push(format!(
                "{} was produced by small model '{}' — treat the output as unreliable",
                unit.span_label(),
                outcome.model
            ));
        }
    }

    if let Err(e) = shared.checkpoint() {
        // Not fatal, but it must be visible: a checkpoint that fails silently
        // means an interruption re-does work that had already completed and been
        // paid for.
        notes.push(format!("could not checkpoint progress: {e}"));
    }
    (state, notes)
}

/// Run one wave: every pending sector in it, `parallelism` at a time.
///
/// Workers pull from a shared queue rather than being handed a fixed slice, so a
/// wave of 193 sectors where one takes ten minutes does not leave seven threads
/// idle waiting for it.
///
/// Returns `(completed, failed, interrupted, notes)`. The three counts are kept
/// apart because they mean different things to whoever reads the report and
/// different things to the next run: only `failed` consumes a retry, and only
/// `completed` marks a sector done. An interrupted sector stays pending.
fn run_wave(
    root: &Path,
    manifest: &MissionManifest,
    plan: &SectorPlan,
    pending: Vec<String>,
    porter: Option<&str>,
    source_rel: &str,
    output_rel: &str,
    conventions_rel: &str,
    parallelism: usize,
    shared: &Shared,
) -> (usize, usize, usize, Vec<String>) {
    use std::collections::VecDeque;
    let queue: Mutex<VecDeque<String>> = Mutex::new(pending.into_iter().collect());
    let done = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let interrupted = AtomicUsize::new(0);
    let notes: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let workers = parallelism.max(1);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                if shared.stopping() {
                    break;
                }
                let next = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                let Some(id) = next else { break };
                let (state, mut sector_notes) = run_sector(
                    root,
                    manifest,
                    plan,
                    &id,
                    porter,
                    source_rel,
                    output_rel,
                    conventions_rel,
                    shared,
                );
                {
                    // A dry run must not touch the progress record. Marking
                    // sectors done here would make the real run skip every one
                    // of them and report a completed mission that ported
                    // nothing — the most expensive possible silent failure.
                    if shared.dry_run.load(Ordering::SeqCst) {
                        match state {
                            SectorOutcome::Done => done.fetch_add(1, Ordering::SeqCst),
                            SectorOutcome::Failed => failed.fetch_add(1, Ordering::SeqCst),
                            SectorOutcome::Interrupted => {
                                interrupted.fetch_add(1, Ordering::SeqCst)
                            }
                        };
                    } else {
                        let mut p = shared.progress.lock().unwrap_or_else(|e| e.into_inner());
                        match state {
                            SectorOutcome::Done => {
                                p.mark_done(&id);
                                done.fetch_add(1, Ordering::SeqCst);
                            }
                            SectorOutcome::Failed => {
                                p.mark_failed(
                                    &id,
                                    sector_notes.last().cloned().unwrap_or_default(),
                                );
                                failed.fetch_add(1, Ordering::SeqCst);
                            }
                            // Deliberately not `mark_failed`: the sector is
                            // unfinished, not broken, and burning a retry here
                            // would strand it once `max_attempts_per_sector` is
                            // reached. It stays pending and the next run picks
                            // it up, which is the whole point of a cap.
                            SectorOutcome::Interrupted => {
                                interrupted.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                    }
                }
                if !sector_notes.is_empty() {
                    let head = format!("[{id}]");
                    for n in &mut sector_notes {
                        n.insert_str(0, &head);
                    }
                    notes
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .extend(sector_notes);
                }
            });
        }
    });

    // Bound before the tuple: a `MutexGuard` created in the tail expression
    // would be dropped at the end of the statement, and the borrow checker
    // rejects returning a clone taken through it.
    let collected = notes.lock().unwrap_or_else(|e| e.into_inner()).clone();
    (
        done.load(Ordering::SeqCst),
        failed.load(Ordering::SeqCst),
        interrupted.load(Ordering::SeqCst),
        collected,
    )
}

/// The ported-file paths a sweep should audit, for a batch of sectors.
///
/// Deduplicated because the chunked units of one source file all target the same
/// output file, and auditing `router.rs` five times in one batch wastes the
/// dispatch without finding anything new.
fn sweep_targets(
    plan: &SectorPlan,
    batch: &[String],
    manifest: &MissionManifest,
    output_rel: &str,
) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for id in batch {
        let Some(sector) = plan.sector(id) else {
            continue;
        };
        for unit in &sector.units {
            let t = target_rel(unit, &manifest.package_root, &manifest.target_language);
            seen.insert(format!("{}/{}", output_rel.trim_end_matches('/'), t));
        }
    }
    seen.into_iter().collect()
}

/// Group ported sectors into sweep batches, in plan order.
fn sweep_batches(done: &[String], plan: &SectorPlan, batch_size: usize) -> Vec<Vec<String>> {
    let order: BTreeMap<usize, &String> = plan
        .sectors
        .iter()
        .enumerate()
        .filter(|(_, s)| done.iter().any(|d| d == &s.id))
        .map(|(i, s)| (i, &s.id))
        .collect();
    let size = batch_size.max(1);
    order
        .values()
        .map(|s| (*s).clone())
        .collect::<Vec<_>>()
        .chunks(size)
        .map(|c| c.to_vec())
        .collect()
}

/// Run one of the three audit sweeps over batches of ported output.
fn run_sweep(
    root: &Path,
    manifest: &MissionManifest,
    plan: &SectorPlan,
    sweep: &str,
    role: &str,
    batches: &[Vec<String>],
    sectors_available: usize,
    output_rel: &str,
    source_rel: &str,
    mission_dir: &Path,
    mission_dir_rel: &str,
    max_dispatches: usize,
    shared: &Shared,
) -> SweepSummary {
    let mut summary = SweepSummary {
        sweep: sweep.to_string(),
        role: role.to_string(),
        dispatches: 0,
        sectors_covered: 0,
        sectors_available,
        truncated: false,
        degraded: false,
        reports: Vec::new(),
        secs: 0,
    };
    let findings_dir = mission_dir.join(FINDINGS_DIR);
    let _ = std::fs::create_dir_all(&findings_dir);

    for (i, batch) in batches.iter().enumerate() {
        if max_dispatches > 0 && summary.dispatches >= max_dispatches {
            summary.truncated = true;
            break;
        }
        if shared.stopping() {
            summary.truncated = true;
            break;
        }
        if let Some(reason) = shared.budget.exhausted() {
            shared.halt(reason);
            summary.truncated = true;
            break;
        }

        let targets = sweep_targets(plan, batch, manifest, output_rel);
        if targets.is_empty() {
            continue;
        }
        let task = sweep_task(sweep, &targets, output_rel, source_rel);
        let outcome = if shared.dry_run.load(Ordering::SeqCst) {
            DispatchOutcome {
                ok: true,
                secs: 0,
                provider: "dry-run".into(),
                model: "none".into(),
                degraded: false,
                error: None,
                transcript: String::new(),
            }
        } else {
            dispatch(root, &manifest.team_id, &task, Some(role), &[], "report")
        };
        shared.budget.record(outcome.secs);
        summary.dispatches += 1;
        summary.secs += outcome.secs;
        summary.sectors_covered += batch.len();
        // A sweep served by a sub-7B fallback model produces confident nonsense
        // (it will cite line numbers the file does not have), so the degradation
        // is carried into the summary even though the findings file is written
        // either way. The per-dispatch record below already logs it; this is
        // what lifts it out of the raw records and into the report a human reads.
        summary.degraded |= outcome.degraded;

        // The report is written to disk whether or not the dispatch succeeded: a
        // failed sweep that leaves no trace is indistinguishable from a sweep
        // that never ran, and both look like a clean tree in the summary.
        let name = format!("{sweep}-{:03}.md", i + 1);
        let body = if outcome.ok {
            outcome.transcript.clone()
        } else {
            format!(
                "# {sweep} sweep batch {} FAILED\n\nSectors: {}\n\nReason: {}\n\nThis batch was NOT audited.\n",
                i + 1,
                batch.join(", "),
                outcome.error.clone().unwrap_or_else(|| "unknown".into())
            )
        };
        let _ = std::fs::write(findings_dir.join(&name), body);
        summary.reports.push(format!(
            "{md}/{FINDINGS_DIR}/{name}",
            md = mission_dir_rel.trim_end_matches('/')
        ));

        {
            let mut p = shared.progress.lock().unwrap_or_else(|e| e.into_inner());
            p.record_dispatch(DispatchRecord {
                sector: format!("{sweep}-sweep-batch-{}", i + 1),
                unit: format!("{} files", targets.len()),
                ok: outcome.ok,
                secs: outcome.secs,
                provider: outcome.provider.clone(),
                model: outcome.model.clone(),
                degraded: outcome.degraded,
                error: outcome.error.clone(),
            });
        }
    }

    if batches.len() > summary.dispatches && !summary.truncated {
        summary.truncated = true;
    }
    summary
}

/// The design phase's task: write the conventions contract every porter follows.
fn design_task(
    manifest: &MissionManifest,
    plan: &SectorPlan,
    source_rel: &str,
    output_rel: &str,
    conventions_rel: &str,
) -> String {
    format!(
        "Write the conventions contract for the {lang} rewrite of {repo}.\n\n\
The tree being ported is at `{src}/`. {sectors} sectors ({dispatches} dispatch units, {lines} lines) will be written to `{out}/` by independently dispatched engineers who cannot see each other's work and cannot ask each other questions.\n\n\
Write a single Markdown file to `{conv}` covering, concretely and with short examples:\n\
- the module and directory layout under `{out}/`, and exactly how a source path maps to a target path\n\
- the error-handling strategy: which error type, when to propagate, when to recover, and the rule on unwrap/expect\n\
- naming conventions for modules, types, functions and constants\n\
- how to express the source language's dynamic behaviour (duck typing, monkey-patching, reflection, runtime code generation) idiomatically in {lang}\n\
- what a porter must do when the source depends on something not yet ported\n\
- the dependency-manifest policy: what may be added, and what must be implemented instead\n\n\
Keep the contract tight: it is inlined whole into every one of the {dispatches} porting dispatches, so favour rules and one-line examples over prose. A few hundred lines is plenty, and a short contract that is followed beats a long one that is skimmed.\n\
Be specific enough that two engineers who never speak produce code that compiles together. Write only `{conv}`; do not modify anything under `{src}/` or `{out}/`.\n",
        lang = manifest.target_language,
        repo = manifest.repo_name,
        src = source_rel.trim_end_matches('/'),
        out = output_rel.trim_end_matches('/'),
        sectors = plan.totals.sectors,
        dispatches = plan.totals.dispatches,
        lines = plan.totals.lines,
        conv = conventions_rel,
    )
}

/// Dispatch one bounded phase (design, remediation, parity, documentation).
///
/// Returns the log line and whether the phase actually completed. "Completed" is
/// the dispatch's own structured result, never a word searched for in the log
/// line — callers branch on the bool.
fn run_phase(
    root: &Path,
    manifest: &MissionManifest,
    phase: &str,
    seat: Option<&str>,
    task: &str,
    shared: &Shared,
) -> (String, bool) {
    let Some(role) = seat else {
        return (format!("{phase}: skipped, no seat on the team"), false);
    };
    if shared
        .progress
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .phase_done(phase)
    {
        return (
            format!("{phase}: already complete on a previous run"),
            false,
        );
    }
    if shared.stopping() {
        return (
            format!("{phase}: skipped, the mission had already halted"),
            false,
        );
    }
    if shared.dry_run.load(Ordering::SeqCst) {
        return (format!("{phase}: would dispatch to {role}"), false);
    }
    let outcome = dispatch(root, &manifest.team_id, task, Some(role), &[], "artifact");
    shared.budget.record(outcome.secs);
    {
        let mut p = shared.progress.lock().unwrap_or_else(|e| e.into_inner());
        p.record_dispatch(DispatchRecord {
            sector: phase.to_string(),
            unit: "phase".to_string(),
            ok: outcome.ok,
            secs: outcome.secs,
            provider: outcome.provider.clone(),
            model: outcome.model.clone(),
            degraded: outcome.degraded,
            error: outcome.error.clone(),
        });
        if outcome.ok {
            p.mark_phase(phase);
        }
    }
    if outcome.ok {
        (
            format!(
                "{phase}: completed on {}/{} in {}s{}",
                outcome.provider,
                outcome.model,
                outcome.secs,
                if outcome.degraded {
                    " (SMALL MODEL — treat the output as unreliable)"
                } else {
                    ""
                }
            ),
            true,
        )
    } else {
        (
            format!(
                "{phase}: FAILED on {}/{} — {}",
                outcome.provider,
                outcome.model,
                outcome.error.unwrap_or_else(|| "unknown".into())
            ),
            false,
        )
    }
}

/// The remediation task for one batch of sweep reports.
fn remediation_task(
    manifest: &MissionManifest,
    reports: &[String],
    source_rel: &str,
    output_rel: &str,
    mission_dir_rel: &str,
) -> String {
    let list = reports
        .iter()
        .map(|r| format!("- {r}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Remediate the findings from the security, durability and performance sweeps of the {lang} rewrite of {repo}.\n\n\
Read these sweep reports:\n{list}\n\n\
The code they refer to is under `{out}/`; the source it was ported from is under `{src}/` and is the authority on intended behaviour.\n\n\
For every finding, either apply the concrete fix or append a line to `{md}/REJECTED.md` saying why it was rejected. Do not dismiss a finding because the code compiles — these three axes are exactly the ones that compile, pass their tests, and fail in production.\n\n\
When you are done, re-check that the code still builds and its tests still pass. A fix that regresses a gate is not a fix; revert it and record that instead.\n",
        lang = manifest.target_language,
        repo = manifest.repo_name,
        list = list,
        out = output_rel.trim_end_matches('/'),
        src = source_rel.trim_end_matches('/'),
        md = mission_dir_rel.trim_end_matches('/')
    )
}

/// The behavioural-parity review task.
fn parity_task(manifest: &MissionManifest, source_rel: &str, output_rel: &str) -> String {
    format!(
        "Review the {lang} rewrite of {repo} for behavioural parity, not for style.\n\n\
Source: `{src}/`\nPort:   `{out}/`\n\n\
Check, in this order:\n\
1. The public API surface of the source — every exported entrypoint — has a counterpart in the port, with equivalent arguments, return values and failure behaviour.\n\
2. Where the port deliberately diverges, the divergence is documented rather than silent.\n\
3. The code is idiomatic {lang}, not a transliteration: no source-language idioms carried across literally where {lang} has a better one.\n\
4. Nothing in `{out}/` still contains placeholder bodies, `todo!()`, or functions that return a default value instead of doing the work.\n\n\
Write your findings to `{out}/PARITY.md` as `path:line — divergence — is it acceptable, and why`. Do not fix anything here; report it. Do not modify `{src}/`.\n",
        lang = manifest.target_language,
        repo = manifest.repo_name,
        src = source_rel.trim_end_matches('/'),
        out = output_rel.trim_end_matches('/')
    )
}

/// The documentation task, which also publishes the benchmark numbers.
fn docs_task(
    manifest: &MissionManifest,
    source_rel: &str,
    output_rel: &str,
    mission_dir_rel: &str,
    totals: &crate::automation::sectors::SectorTotals,
) -> String {
    format!(
        "Document the {lang} rewrite of {repo}.\n\n\
Source: `{src}/`
Port:   `{out}/`
Sweep reports (including the benchmark numbers): `{md}/{findings}/`

\
Write three files under `{out}/`:\n\
- `README.md` — what the crate is, how to build and test it, and the module map.\n\
- `PORTING_NOTES.md` — every place the port deliberately diverges from the source, and anything that could not be ported 1:1 (FFI, platform-specific code, runtime code generation, dynamic dispatch).\n\
- `BENCHMARKS.md` — the performance sweep's measured numbers, reproduced faithfully. Where the port is slower than the source, say so and give the ratio; where nothing was measured, say that instead of estimating.\n\n\
Scope, for honesty's sake: {sectors} sectors, {dispatches} dispatch units, {lines} source lines were scheduled for porting. Document what actually exists in `{out}/`, not what was planned. Do not modify `{src}/`.\n",
        lang = manifest.target_language,
        repo = manifest.repo_name,
        src = source_rel.trim_end_matches('/'),
        out = output_rel.trim_end_matches('/'),
        md = mission_dir_rel.trim_end_matches('/'),
        findings = FINDINGS_DIR,
        sectors = totals.sectors,
        dispatches = totals.dispatches,
        lines = totals.lines,
    )
}

/// What the operator should do next, given how the run ended.
///
/// The cap that bit outranks the dry-run flag. Telling somebody who just watched
/// a run stop on its dispatch cap to "re-run without dry_run" sends them straight
/// back into the same wall, having spent everything up to it — and the dry run
/// wrote no checkpoint, so "resume" would be a lie too.
fn next_step(
    status: MissionStatus,
    remaining: usize,
    dry_run: bool,
    stop_reason: Option<&str>,
) -> String {
    match status {
        MissionStatus::Completed => format!(
            "read {REPORT_FILE} and the sweep reports under {FINDINGS_DIR}/; the gates passed against the output tree"
        ),
        MissionStatus::Paused if dry_run && stop_reason.is_some() => format!(
            "a live run would have stopped here: {}. Raise that cap before re-running without \
             dry_run, or the real run stops in the same place after spending up to it",
            stop_reason.unwrap_or_default()
        ),
        MissionStatus::Paused if dry_run => {
            "re-run without dry_run to execute it; the caps that will be in force are listed above".to_string()
        }
        MissionStatus::Paused => format!(
            "re-run run_rewrite_mission with the same mission_dir to resume: {remaining} sectors remain and the checkpoint is intact{why}. Raise max_dispatches or max_wall_secs if a cap is what stopped it",
            why = stop_reason
                .map(|r| format!(" (this run stopped because {r})"))
                .unwrap_or_default()
        ),
        MissionStatus::Failed => {
            "add the missing role to the team (add_team_member) and re-run; nothing dispatches until the seat exists, and re-running unchanged will fail the same way".to_string()
        }
        MissionStatus::Running => {
            "the record still says running, which means the process died mid-run; re-running resumes from the last checkpoint rather than restarting".to_string()
        }
    }
}

/// A workspace-relative path with forward slashes and no surrounding separators.
///
/// Every path this module puts into a prompt goes through here, because the
/// agent's tools resolve against the workspace root and a `\` or a leading `/`
/// turns a valid relative reference into one that silently points nowhere.
fn normalize_rel(p: &str) -> String {
    p.trim().replace('\\', "/").trim_matches('/').to_string()
}

// ─────────────────────────────────────────────────────────────────────
// Report
// ─────────────────────────────────────────────────────────────────────

/// What a mission run did, including everything it did not do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionReport {
    pub mission_dir: String,
    pub repo_name: String,
    pub target_language: String,
    pub source_root: String,
    pub output_root: String,
    pub team_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    pub resumed: bool,
    pub dry_run: bool,
    pub parallelism: usize,
    pub sectors_total: usize,
    pub sectors_done_total: usize,
    pub sectors_done_this_run: usize,
    pub sectors_failed_total: usize,
    pub sectors_remaining: usize,
    pub dispatches_total: usize,
    pub dispatches_this_run: usize,
    pub dispatch_secs_this_run: u64,
    pub degraded_dispatches: usize,
    pub waves_total: usize,
    pub waves_processed: usize,
    pub gates: Vec<GateResult>,
    pub gate_summary: String,
    /// What the Rust module wiring pass found: how much of the port is actually
    /// reachable from the crate root, and what is not. `None` for a non-Rust
    /// target or a dry run, where no output tree was touched.
    ///
    /// Reported alongside `gate_summary` because the gates are only evidence
    /// when the crate they built contains the ported code. Four green gates over
    /// an empty crate and four green gates over 1,800 wired modules look
    /// identical in `gate_summary` alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_wiring: Option<String>,
    pub sweeps: Vec<SweepSummary>,
    pub sweep_coverage: Vec<String>,
    pub phases_done: Vec<String>,
    pub phase_log: Vec<String>,
    /// [`crate::automation::sectors::SectorTotals::feasibility`], carried into
    /// the report so the arithmetic is in front of whoever reads the outcome.
    pub feasibility: String,
    pub caps: Vec<String>,
    /// Everything the run could not verify, refused to do, or did badly.
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
}

impl MissionReport {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({ "error": "report not serialisable" }))
    }
}

// ─────────────────────────────────────────────────────────────────────
// The mission
// ─────────────────────────────────────────────────────────────────────

/// Execute (or resume) the rewrite mission recorded in `mission_dir_rel`.
///
/// Reads the manifest and sector plan that `repo_rewrite` persisted, dispatches
/// the porting waves in parallel, runs the gates against the output tree, then
/// the three sweeps, remediation, parity review and documentation. Every step is
/// checkpointed, so an interruption — cap, crash, reboot — resumes rather than
/// restarts.
///
/// Returns `Err` only for conditions that make the mission impossible to even
/// describe: no manifest, unreadable plan, version mismatch. Everything else,
/// including total failure of every dispatch, is a *report*, because a run that
/// spent money and produced nothing must still say so in detail.
pub fn run_mission(
    root: &Path,
    mission_dir_rel: &str,
    cfg: &MissionConfig,
) -> Result<MissionReport, String> {
    let rel = normalize_rel(mission_dir_rel);
    let mission_dir = if Path::new(mission_dir_rel).is_absolute() {
        PathBuf::from(mission_dir_rel)
    } else {
        root.join(&rel)
    };
    if !mission_dir.is_dir() {
        return Err(format!(
            "mission directory {} does not exist — run repo_rewrite first, which writes {} and {PLAN_FILE}",
            mission_dir.display(),
            MISSION_FILE
        ));
    }

    let mut manifest = MissionManifest::load(&mission_dir)?;
    if let Some(team) = &cfg.team_id {
        manifest.team_id = team.clone();
    }
    if manifest.team_id.trim().is_empty() {
        return Err(format!(
            "{} records no team_id and none was supplied; a mission with no team cannot dispatch",
            MissionManifest::path(&mission_dir).display()
        ));
    }

    // System C chokepoint (spec §6.2): a governed workspace may not dispatch a
    // mission's headless sub-agents until the org RBAC engine clears the run.
    // `Governor::require_dispatch` is the *same* seam `team_dispatch` consults,
    // so the two paths cannot drift; a denial is appended to the durable
    // op-log before the `Err` surfaces (INV-6). Dry runs stay open: they
    // dispatch nothing, and refusing the preview would remove the very tool an
    // operator uses to decide the approval (unattended safety, §6).
    if !cfg.dry_run {
        let governor = crate::automation::Governor::open(root);
        if governor.governed() {
            governor.require_dispatch(&manifest.team_id)?;
        }
    }

    let plan_raw = std::fs::read_to_string(mission_dir.join(PLAN_FILE))
        .map_err(|e| format!("cannot read {}: {e}", mission_dir.join(PLAN_FILE).display()))?;
    let plan_v: Value =
        serde_json::from_str(&plan_raw).map_err(|e| format!("cannot parse {PLAN_FILE}: {e}"))?;
    let plan = SectorPlan::from_json(&plan_v)?;
    // The plan's own source root wins: it is what the sector `rel` paths are
    // relative to, and a manifest written by an older build may not carry it.
    if manifest.package_root.trim().is_empty() {
        manifest.package_root = plan.source_root.clone();
    }

    let existing = MissionProgress::load(&mission_dir)?;
    let resumed = existing.is_some();
    let mut progress = existing.unwrap_or_default();
    let prior_done = progress.sectors_done.len();
    let prior_dispatches = progress.dispatches_run;
    progress.status = MissionStatus::Running;
    progress.stop_reason = None;

    let parallelism = if cfg.parallelism > 0 {
        cfg.parallelism
    } else if manifest.parallelism > 0 {
        manifest.parallelism
    } else {
        DEFAULT_PARALLELISM
    };

    let source_rel = normalize_rel(&manifest.source_root);
    let output_rel = normalize_rel(&manifest.output_root);
    let conventions_rel = format!("{}/{}", rel.trim_end_matches('/'), CONVENTIONS_FILE);
    let output_dir = root.join(&output_rel);

    let shared = Shared::new(mission_dir.clone(), progress, cfg);
    let mut notes: Vec<String> = Vec::new();
    let mut phase_log: Vec<String> = Vec::new();

    // Neither of these happens in a dry run. An output root and a crate skeleton
    // are real artifacts, and the dry run's contract is that an operator can
    // read the schedule, the seats and the caps without the run leaving state
    // behind that the first live run would mistake for work already done —
    // `run_gate` skips a missing directory but runs against a skeleton, so a
    // dry run that created one would change what the live run's gates report.
    if !cfg.dry_run {
        std::fs::create_dir_all(&output_dir)
            .map_err(|e| format!("cannot create output root {}: {e}", output_dir.display()))?;
        if let Some(n) = ensure_skeleton(&output_dir, &manifest) {
            notes.push(n);
        }
    }

    let seats = Seats::resolve(root, &manifest.team_id, &manifest.target_language);
    notes.extend(seats.missing());
    if plan.totals.cycles_broken > 0 {
        notes.push(format!(
            "{} dependency edges were broken to make the graph acyclic, so wave order is advisory: sectors in one wave may need stubs for each other",
            plan.totals.cycles_broken
        ));
    }
    if let Some(note) = over_ceiling_note(&plan.totals) {
        notes.push(note);
    }

    // ── Porting ────────────────────────────────────────────────────
    let mut waves_processed = 0usize;
    let porting_blocked = seats.porter.is_none();
    // Design runs before the first porting dispatch, not after it. Porters are
    // pointed at the conventions file whether or not it exists, and its absence
    // is reported rather than left to surface as a tree that will not link.
    if !mission_dir.join(CONVENTIONS_FILE).is_file() {
        let task = design_task(&manifest, &plan, &source_rel, &output_rel, &conventions_rel);
        let (msg, _) = run_phase(
            root,
            &manifest,
            "design",
            seats.lead.as_deref(),
            &task,
            &shared,
        );
        phase_log.push(msg);
        if !cfg.dry_run && !mission_dir.join(CONVENTIONS_FILE).is_file() {
            notes.push(format!(
                "{conventions_rel} was not written, yet every porter is instructed to follow it: \
                 the ported sectors will diverge from each other, and the divergence surfaces only \
                 as build failures in the gate phase"
            ));
        }
    }
    if porting_blocked {
        notes.push("no porting seat on the team: no sector was dispatched".to_string());
    }
    if !porting_blocked {
        for wave in &plan.waves {
            if cfg.max_waves > 0 && waves_processed >= cfg.max_waves {
                shared.halt(format!("wave cap reached ({} this run)", cfg.max_waves));
                break;
            }
            if shared.stopping() {
                break;
            }
            let pending: Vec<String> = {
                let p = shared.progress.lock().unwrap_or_else(|e| e.into_inner());
                wave.iter()
                    .filter(|id| !p.is_done(id) && p.attempts(id) < cfg.max_attempts_per_sector)
                    .cloned()
                    .collect()
            };
            if pending.is_empty() {
                waves_processed += 1;
                continue;
            }
            let (d, f, interrupted, wave_notes) = run_wave(
                root,
                &manifest,
                &plan,
                pending.clone(),
                seats.porter.as_deref(),
                &source_rel,
                &output_rel,
                &conventions_rel,
                parallelism,
                &shared,
            );
            waves_processed += 1;
            // Interrupted is reported separately from failed for the same reason
            // it is recorded separately: "5 failed" sends the reader looking for
            // a bug in the port when the truth is that the budget ran out.
            phase_log.push(format!(
                "wave {waves_processed}: {} sectors dispatched, {d} completed, {f} failed, {interrupted} interrupted by a cap",
                pending.len()
            ));
            notes.extend(wave_notes);
            {
                let mut p = shared.progress.lock().unwrap_or_else(|e| e.into_inner());
                p.waves_completed = p.waves_completed.max(waves_processed);
            }
            if let Err(e) = shared.checkpoint() {
                notes.push(format!(
                    "could not checkpoint after wave {waves_processed}: {e}"
                ));
            }
        }
    }

    // ── Verification, sweeps and documentation ─────────────────────────────────────────

    // ── Gates ──────────────────────────────────────────────────────
    // Wiring runs first and is deliberately not part of the gate set: it is
    // what makes the gates mean something. Declaring the modules after checking
    // them would report on a crate that does not contain the port.
    let mut output_wiring = RustWiring::default();
    let mut gates: Vec<GateResult> = Vec::new();
    if cfg.run_gates && !cfg.dry_run {
        let ported = shared
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sectors_done
            .len();
        output_wiring =
            wire_output_tree(&output_dir, &manifest.target_language, ported, &mut notes);
        gates = run_gates(
            &output_dir,
            &manifest.target_language,
            Duration::from_secs(cfg.gate_timeout_secs),
            cfg.stop_on_gate_failure,
        );
        if let Some(n) = gates
            .iter()
            .find(|g| g.skipped)
            .and_then(|g| g.errors.first().cloned())
        {
            notes.push(n);
        }
    } else if cfg.run_gates {
        notes.push("dry run: gates were not executed".to_string());
    }
    let mut gate_line = gate_summary(&gates);

    // ── Sweeps ─────────────────────────────────────────────────────
    let mut sweeps: Vec<SweepSummary> = Vec::new();
    if cfg.run_sweeps && !porting_blocked && cfg.dry_run {
        notes.push("dry run: the sweeps were not dispatched".to_string());
    } else if cfg.run_sweeps && porting_blocked {
        notes.push(
            "sweeps were skipped: with no porting seat there was no output to audit".to_string(),
        );
    } else if cfg.run_sweeps {
        let done_snapshot: Vec<String> = shared
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sectors_done
            .clone();
        let batches_all = sweep_batches(&done_snapshot, &plan, cfg.sweep_batch_sectors);
        if done_snapshot.is_empty() {
            notes.push(
                "sweeps were skipped: no sector had been ported, so there was nothing to audit"
                    .to_string(),
            );
        } else {
            let specs: [(&str, Option<&String>); 3] = [
                ("security", seats.security.as_ref()),
                ("durability", seats.durability.as_ref()),
                ("performance", seats.performance.as_ref()),
            ];
            let results: Mutex<Vec<SweepSummary>> = Mutex::new(Vec::new());
            // Every one of these locals is bound as a reference first so the
            // worker closures below borrow them; `thread::scope` guarantees they
            // outlive every thread it spawns, and joins them all when the block
            // ends, so `results` is complete by the time it is read below.
            let sink = &results;
            let batches = batches_all.as_slice();
            let available = done_snapshot.len();
            let mf = &manifest;
            let pl = &plan;
            let sh = &shared;
            let mdir = mission_dir.as_path();
            let mrel = rel.as_str();
            let src = source_rel.as_str();
            let out = output_rel.as_str();
            let sweep_cap = cfg.max_sweep_dispatches;
            // The three sweeps read the same artifact and ask different
            // questions, so they *may* run concurrently — but the concurrency is
            // bounded by `parallelism`, exactly like `run_wave` bounds sector
            // dispatches, instead of always firing all three axes at once. An
            // earlier draft spawned one thread per axis unconditionally, so even
            // a mission configured for serial execution hit the provider with
            // three simultaneous requests. Against the single cloud provider a
            // rewrite team is usually given, that trips its concurrency or rate
            // cap and every sweep falls back to the sub-7B local model, which
            // audits nothing and invents findings at line numbers the file does
            // not have. Each worker pulls axes off a shared queue, so
            // `parallelism == 1` runs them one at a time (each with the provider
            // to itself) and `parallelism >= 3` keeps the old concurrency.
            use std::collections::VecDeque;
            let specs_len = specs.len();
            let queue: Mutex<VecDeque<_>> = Mutex::new(specs.into_iter().collect());
            let workers = parallelism.max(1).min(specs_len);
            std::thread::scope(|scope| {
                for _ in 0..workers {
                    scope.spawn(|| loop {
                        let next = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                        let Some((sweep, seat)) = next else { break };
                        let Some(role) = seat.cloned() else {
                            sink.lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .push(SweepSummary {
                                    sweep: sweep.to_string(),
                                    role: String::new(),
                                    dispatches: 0,
                                    sectors_covered: 0,
                                    sectors_available: available,
                                    truncated: true,
                                    degraded: false,
                                    reports: Vec::new(),
                                    secs: 0,
                                });
                            continue;
                        };
                        let summary = run_sweep(
                            root, mf, pl, sweep, &role, batches, available,
                            // `run_sweep` takes output before source. Passing
                            // them the other way round compiles — both are
                            // `&str` — and quietly sends the auditor to read the
                            // Python tree as though it were the Rust port.
                            out, src, mdir, mrel, sweep_cap, sh,
                        );
                        sink.lock().unwrap_or_else(|e| e.into_inner()).push(summary);
                    });
                }
            });
            sweeps = results.lock().unwrap_or_else(|e| e.into_inner()).clone();
        }
    }
    sweeps.sort_by(|a, b| a.sweep.cmp(&b.sweep));
    let sweep_coverage: Vec<String> = sweeps.iter().map(|s| s.coverage()).collect();

    // A degraded sweep is worse than a missing one: it files findings that read
    // as an audit but were produced by a sub-7B fallback model, which invents
    // line numbers the file does not have and leaks its own reasoning instead of
    // reviewing the port. Each `coverage()` line already ends in DEGRADED, but
    // that is easy to miss among the percentages, and the global
    // `degraded_dispatches` count does not say *which axis* was affected. This
    // lifts it into one prominent note naming the axes, so a reader cannot
    // mistake a run whose audits were served by a toy model for a clean bill of
    // health — the exact failure the bounded-concurrency fix above prevents.
    let degraded_axes: Vec<&str> = sweeps
        .iter()
        .filter(|s| s.degraded)
        .map(|s| s.sweep.as_str())
        .collect();
    let degraded_dispatch_count = shared
        .progress
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .degraded_dispatches;
    if !degraded_axes.is_empty() {
        notes.push(format!(
            "{} sweep(s) ran on a small (<7B) fallback model [{}], so their findings are \
             unreliable and must NOT be read as a clean audit — re-run with lower parallelism \
             or a larger provider before trusting them ({} degraded dispatch(es) this run)",
            degraded_axes.len(),
            degraded_axes.join(", "),
            degraded_dispatch_count,
        ));
    } else if degraded_dispatch_count > 0 {
        notes.push(format!(
            "{} dispatch(es) fell back to a small (<7B) model, so their output — including any \
             code they ported — is lower-confidence than the rest of the run",
            degraded_dispatch_count,
        ));
    }

    // ── Remediation (plan phase 10) ────────────────────────────────
    // Runs only when a sweep actually filed a report. Remediating an empty
    // findings list would spend a dispatch to fix nothing and then mark the
    // phase done, which the report would render as "findings addressed".
    let findings: Vec<String> = sweeps
        .iter()
        .flat_map(|s| s.reports.iter().cloned())
        .collect();
    if !cfg.dry_run && !shared.stopping() {
        if findings.is_empty() {
            if cfg.run_sweeps && !porting_blocked {
                notes.push(
                    "remediation was skipped: the sweeps filed no findings. On a tree of this size \
                     that is more likely to mean the sweep dispatches did not produce a report than \
                     that the code was clean — check the sweep coverage lines above"
                        .to_string(),
                );
            }
        } else {
            let task = remediation_task(&manifest, &findings, &source_rel, &output_rel, &rel);
            let (msg, ok) = run_phase(
                root,
                &manifest,
                "remediation",
                seats.qa.as_deref(),
                &task,
                &shared,
            );
            phase_log.push(msg);
            // A fix that regresses a gate is not a fix, so the gates run again
            // and the report carries the post-remediation result. The earlier
            // line is kept in the log rather than silently overwritten.
            if ok && cfg.run_gates {
                phase_log.push(format!("gates before remediation: {gate_line}"));
                // Re-wire as well as re-check: remediation can add files, and a
                // new module nothing declares is invisible to the re-run gates,
                // which would then report green over a crate missing the fix.
                let ported = shared
                    .progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .sectors_done
                    .len();
                output_wiring =
                    wire_output_tree(&output_dir, &manifest.target_language, ported, &mut notes);
                gates = run_gates(
                    &output_dir,
                    &manifest.target_language,
                    Duration::from_secs(cfg.gate_timeout_secs),
                    cfg.stop_on_gate_failure,
                );
                gate_line = gate_summary(&gates);
                phase_log.push(format!("gates after remediation: {gate_line}"));
            }
        }
    }

    // ── Parity review (phase 11) and documentation (phase 12) ──────
    let ported_anything = !shared
        .progress
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .sectors_done
        .is_empty();
    if !ported_anything {
        notes.push(
            "the parity review and documentation were skipped: no sector was ported, so there is \
             no tree to review and nothing to document beyond this report"
                .to_string(),
        );
    } else if !cfg.dry_run && !shared.stopping() {
        let task = parity_task(&manifest, &source_rel, &output_rel);
        let (msg, _) = run_phase(
            root,
            &manifest,
            "parity",
            seats.reviewer.as_deref(),
            &task,
            &shared,
        );
        phase_log.push(msg);
        if cfg.run_documentation {
            let task = docs_task(&manifest, &source_rel, &output_rel, &rel, &plan.totals);
            let (msg, _) = run_phase(
                root,
                &manifest,
                "documentation",
                seats.docs.as_deref(),
                &task,
                &shared,
            );
            phase_log.push(msg);
        }
    }

    // ── Final state ────────────────────────────────────────────────
    // A blocking gate that failed means the tree does not build. That is not a
    // completed mission whatever the sector counters say, and reporting it as
    // one is the failure mode that makes an unattended run dangerous.
    let gates_blocked = gates.iter().any(|g| g.blocking && !g.success && !g.skipped);
    if gates_blocked {
        shared.halt(format!(
            "blocking gate failed against the output tree: {gate_line}"
        ));
    }

    let sectors_total = plan.sectors.len();
    let (
        sectors_done_total,
        sectors_failed_total,
        dispatches_total,
        degraded_dispatches,
        phases_done,
    ) = {
        let p = shared.progress.lock().unwrap_or_else(|e| e.into_inner());
        (
            p.sectors_done.len(),
            p.sectors_failed.len(),
            p.dispatches_run,
            p.degraded_dispatches,
            p.phases_done.clone(),
        )
    };
    let sectors_remaining = sectors_total.saturating_sub(sectors_done_total);

    // Ordered from least to most actionable: `Failed` means re-running unchanged
    // fails the same way; `Paused` means a re-run resumes from the checkpoint.
    let status = if porting_blocked {
        MissionStatus::Failed
    } else if cfg.dry_run || shared.stopping() || sectors_remaining > 0 {
        MissionStatus::Paused
    } else {
        MissionStatus::Completed
    };
    let stop_reason = shared.reason();
    {
        let mut p = shared.progress.lock().unwrap_or_else(|e| e.into_inner());
        p.status = status;
        p.stop_reason = stop_reason.clone();
        p.gates = gates.clone();
        p.sweeps = sweeps.clone();
        p.updated_at_ms = crate::automation::build_runner::now_ms();
    }
    if let Err(e) = shared.checkpoint() {
        notes.push(format!(
            "the final checkpoint could not be written, so a resume would repeat work this run \
             already finished: {e}"
        ));
    }

    if cfg.dry_run {
        // A dry run still counts every dispatch it walked past, because the
        // forecast is the most useful thing it can produce. Left unlabelled
        // that number reads as spend in a JSON report, so it is labelled.
        notes.push(format!(
            "dry run: {} dispatches were counted and none were made — read that number as the \
             forecast for a live run, not as spend",
            dispatches_total.saturating_sub(prior_dispatches)
        ));
    }
    // Bound before the report: `stop_reason` is moved into it, and the advice
    // depends on knowing whether a cap is what stopped this run.
    let advice = next_step(
        status,
        sectors_remaining,
        cfg.dry_run,
        stop_reason.as_deref(),
    );

    let report = MissionReport {
        mission_dir: rel.clone(),
        repo_name: manifest.repo_name.clone(),
        target_language: manifest.target_language.clone(),
        source_root: source_rel.clone(),
        output_root: output_rel.clone(),
        team_id: manifest.team_id.clone(),
        status: status.as_str().to_string(),
        stop_reason,
        resumed,
        dry_run: cfg.dry_run,
        parallelism,
        sectors_total,
        sectors_done_total,
        sectors_done_this_run: sectors_done_total.saturating_sub(prior_done),
        sectors_failed_total,
        sectors_remaining,
        dispatches_total,
        dispatches_this_run: dispatches_total.saturating_sub(prior_dispatches),
        dispatch_secs_this_run: shared.budget.secs(),
        degraded_dispatches,
        waves_total: plan.waves.len(),
        waves_processed,
        gates,
        gate_summary: gate_line,
        output_wiring: output_wiring.summary(),
        sweeps,
        sweep_coverage,
        phases_done,
        phase_log,
        feasibility: plan.totals.feasibility(),
        caps: cfg.describe_caps(),
        notes,
        next_step: Some(advice),
    };
    if !cfg.dry_run {
        atomic_write_json(&mission_dir.join(REPORT_FILE), &report.to_json())?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::sectors::{
        plan_sectors_with_parallelism, Sector, SectorOptions, SectorTotals,
    };
    use crate::editor::expert_team::{save_expert_teams, ExpertTeam};
    use serde_json::json;

    fn unit(rel: &str, bytes: u64, chunk: u32, chunks: u32, a: u32, b: u32) -> SectorUnit {
        SectorUnit {
            rel: rel.to_string(),
            bytes,
            line_start: a,
            line_end: b,
            chunk,
            chunks,
        }
    }

    fn whole(rel: &str, bytes: u64) -> SectorUnit {
        unit(rel, bytes, 1, 1, 1, 100)
    }

    /// A real decomposition over a two-file tree, for the tests that need a
    /// [`SectorPlan`] without waiting on a repository the size of LiteLLM.
    fn tiny_plan() -> (tempfile::TempDir, SectorPlan) {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["a.py", "b.py"] {
            std::fs::create_dir_all(tmp.path().join("pkg")).unwrap();
            std::fs::write(
                tmp.path().join("pkg").join(name),
                format!(
                    "def {name}():\n    return 1\n",
                    name = name.trim_end_matches(".py")
                ),
            )
            .unwrap();
        }
        let plan = plan_sectors_with_parallelism(
            tmp.path(),
            &SectorOptions {
                max_sector_bytes: 64,
                ..Default::default()
            },
            2,
        );
        assert!(!plan.sectors.is_empty(), "the fixture must decompose");
        (tmp, plan)
    }

    fn manifest(lang: &str) -> MissionManifest {
        MissionManifest {
            repo_name: "litellm".to_string(),
            source_url: "https://example.invalid/litellm.git".to_string(),
            target_language: lang.to_string(),
            source_root: ".velocity/clones/litellm".to_string(),
            package_root: "litellm".to_string(),
            output_root: ".velocity/missions/litellm-rust/output".to_string(),
            team_id: "rewrite-team".to_string(),
            parallelism: 4,
            include_tests: false,
            created_at_ms: 1,
        }
    }

    // ── Prompt shaping ────────────────────────────────────────────

    /// The output root *is* the package, so nesting the source package name
    /// inside it again would make every import path in the port wrong.
    #[test]
    fn target_rel_strips_the_package_root_and_swaps_the_extension() {
        let u = whole("litellm/proxy/router.py", 100);
        assert_eq!(target_rel(&u, "litellm", "rust"), "src/proxy/router.rs");
        assert_eq!(target_rel(&u, "litellm", "go"), "proxy/router.go");
        assert_eq!(target_rel(&u, "litellm", "typescript"), "proxy/router.ts");
    }

    #[test]
    fn target_rel_without_a_package_root_keeps_the_whole_path() {
        let u = whole("proxy/router.py", 100);
        assert_eq!(target_rel(&u, "", "rust"), "src/proxy/router.rs");
    }

    /// Rust alone gets the `src/` prefix, because Rust alone compiles only what
    /// is reachable from the crate root. A ported file anywhere else is dead
    /// weight that `cargo build` never opens, so the gates go green over an
    /// empty crate — which is what the first live LiteLLM run reported.
    #[test]
    fn only_rust_is_placed_under_src_where_cargo_will_find_it() {
        let u = whole("proxy/router.py", 100);
        assert_eq!(target_rel(&u, "", "rust"), "src/proxy/router.rs");
        for lang in ["go", "python", "typescript", "java", "zig"] {
            let t = target_rel(&u, "", lang);
            assert!(
                !t.starts_with("src/"),
                "'{lang}' has no crate root and must not gain one: {t}"
            );
        }
    }

    /// `src/lib.rs` is the crate root the wiring pass owns, and `<dir>/mod.rs`
    /// is the file it generates to declare that directory. A porter writing
    /// either would destroy the module tree, so both stems are shifted aside.
    /// `main.rs` is not: cargo builds it as a binary target next to the library,
    /// which is what a ported `main.py` should become.
    #[test]
    fn rust_stems_that_would_clobber_the_crate_root_are_shifted_aside() {
        assert_eq!(target_rel(&whole("lib.py", 10), "", "rust"), "src/lib_.rs");
        assert_eq!(
            target_rel(&whole("pkg/mod.py", 10), "", "rust"),
            "src/pkg/mod_.rs"
        );
        assert_eq!(target_rel(&whole("main.py", 10), "", "rust"), "src/main.rs");
        // Nested `lib` is a perfectly good module name; only the root collides.
        assert_eq!(
            target_rel(&whole("pkg/lib.py", 10), "", "rust"),
            "src/pkg/lib.rs"
        );
    }

    /// A source language with no target mapping keeps its name rather than
    /// inventing an extension, and an extensionless file gains one.
    #[test]
    fn target_rel_falls_back_when_the_target_language_is_unknown() {
        let u = whole("litellm/Makefile", 100);
        assert_eq!(target_rel(&u, "litellm", "brainfuck"), "Makefile");
        let u2 = whole("litellm/pyproject.toml", 100);
        assert_eq!(target_rel(&u2, "litellm", "brainfuck"), "pyproject.toml");
        let u3 = whole("litellm/Dockerfile", 100);
        assert_eq!(target_rel(&u3, "litellm", "rust"), "src/Dockerfile.rs");
    }

    /// The load-bearing one. A pre-loaded file the dispatcher cuts short becomes a
    /// port of a file nobody read, so a chunk is never pre-loaded and neither is
    /// anything over the ceiling: the agent gets a whole file or nothing.
    #[test]
    fn split_units_for_prompt_never_preloads_a_chunk_or_an_oversized_file() {
        let units = vec![
            whole("small.py", 500),
            whole("big.py", PROMPT_PRELOAD_CAP_BYTES + 1),
            unit("huge.py", 9_000, 2, 3, 100, 200),
        ];
        let (preload, deferred) = split_units_for_prompt_at_cap(&units, PROMPT_PRELOAD_CAP_BYTES);
        assert_eq!(preload.len(), 1, "{preload:?}");
        assert_eq!(preload[0].rel, "small.py");
        assert_eq!(deferred.len(), 2);
        assert!(deferred.iter().any(|u| u.rel == "big.py"));
        assert!(deferred.iter().any(|u| u.rel == "huge.py"));
    }

    #[test]
    fn a_file_exactly_at_the_cap_is_still_preloaded() {
        let units = vec![whole("edge.py", PROMPT_PRELOAD_CAP_BYTES)];
        let (preload, deferred) = split_units_for_prompt_at_cap(&units, PROMPT_PRELOAD_CAP_BYTES);
        assert_eq!(preload.len(), 1);
        assert!(deferred.is_empty());
    }

    /// The ceiling is adjustable, and the task text swears the source arrived
    /// whole, so the split has to follow the number the dispatcher will actually
    /// use. Pinning the smaller cap here is what an operator's
    /// `VELOCITY_PRELOAD_MAX_BYTES=4000` does to a 9 KB unit: deferred and read
    /// by the agent, never pre-loaded under a claim that it came in full.
    #[test]
    fn a_lowered_ceiling_defers_units_the_default_would_have_preloaded() {
        let units = vec![
            whole("mid.py", 9_000),
            whole("tiny.py", 100),
            whole("big.py", PROMPT_PRELOAD_CAP_BYTES + 1),
        ];
        let (preload, deferred) = split_units_for_prompt_at_cap(&units, 4_000);
        assert_eq!(preload.len(), 1, "{preload:?}");
        assert_eq!(preload[0].rel, "tiny.py");
        assert!(deferred.iter().any(|u| u.rel == "mid.py"));
        assert!(deferred.iter().any(|u| u.rel == "big.py"));
    }

    /// `PROMPT_PRELOAD_CAP_BYTES` is a mirror of the dispatcher's default, not a
    /// second source of truth. This is the check that keeps the mirror from
    /// drifting; the live number comes from [`prompt_preload_cap_bytes`].
    #[test]
    fn the_mirrored_default_equals_the_dispatchers_own() {
        assert_eq!(
            PROMPT_PRELOAD_CAP_BYTES as usize,
            crate::registry::team_tools::DEFAULT_PRELOAD_MAX_BYTES
        );
    }

    /// The over-ceiling note sends an operator to a limit they can raise, so the
    /// number it names has to be the ceiling those units were actually measured
    /// against. `units_over_ceiling` counts against the chunker's per-unit
    /// ceiling; the prompt preload cap is a different, smaller number, and
    /// quoting it here made the note a dead end.
    #[test]
    fn the_over_ceiling_note_names_the_ceiling_the_plan_was_chunked_at() {
        let totals = SectorTotals {
            units_over_ceiling: 2,
            unit_ceiling_bytes: 7_777,
            ..Default::default()
        };
        let note = over_ceiling_note(&totals).expect("two over-ceiling units must be reported");
        assert!(note.contains("2 units"), "{note}");
        assert!(note.contains("7777"), "{note}");
        assert!(
            !note.contains(&PROMPT_PRELOAD_CAP_BYTES.to_string()),
            "the preload cap is not the ceiling these units were measured against: {note}"
        );
        assert!(over_ceiling_note(&SectorTotals::default()).is_none());
    }

    #[test]
    fn a_chunk_is_told_to_extend_the_file_rather_than_replace_it() {
        let m = manifest("rust");
        let u = unit("litellm/proxy/router.py", 9_000, 3, 5, 400, 600);
        let t = porting_task(&u, &m, ".velocity/clones/litellm", "out", false, None);
        assert!(t.contains("lines 400-600 only"), "{t}");
        assert!(t.contains("chunk 3 of 5"), "{t}");
        assert!(t.contains("chunks 1-2"), "{t}");
        assert!(t.contains("EXTEND it"), "{t}");
        assert!(t.contains("do NOT read outside this span"), "{t}");
        assert!(t.contains("NOT pre-loaded"), "{t}");
        assert!(t.contains("out/src/proxy/router.rs"), "{t}");
    }

    #[test]
    fn a_whole_file_is_preloaded_and_says_so() {
        let m = manifest("rust");
        let u = whole("litellm/utils.py", 500);
        let t = porting_task(&u, &m, "clone", "out", true, None);
        assert!(t.contains("pre-loaded below in full"), "{t}");
        assert!(!t.contains("chunk"), "{t}");
    }

    /// Two rules without which an unattended port quietly destroys its own
    /// evidence: follow the shared conventions, and never touch the clone.
    #[test]
    fn every_porting_task_inlines_the_conventions_and_protects_the_source() {
        let m = manifest("rust");
        let contract = "# Conventions\n\nUse snake_case modules; propagate errors with Result.\n";
        for preloaded in [true, false] {
            let t = porting_task(
                &whole("litellm/a.py", 10),
                &m,
                "clone",
                "out",
                preloaded,
                Some(contract),
            );
            // The contract is inlined whole, in a tagged block, and the porter is
            // told not to read_file it again — the re-read loop that wrote zero
            // files on the first live LiteLLM run.
            assert!(t.contains("<conventions>"), "{t}");
            assert!(t.contains("Use snake_case modules"), "{t}");
            assert!(t.contains("Do NOT read_file it again"), "{t}");
            assert!(
                t.contains("Do not modify anything under the SOURCE tree"),
                "{t}"
            );
            assert!(t.contains("leave their files alone"), "{t}");
        }
    }

    /// A mission whose design phase produced no contract must still port: the
    /// task says so explicitly rather than sending the porter to read a file
    /// that is not there (the prior run's ~150 failed conventions.md reads).
    #[test]
    fn a_porting_task_with_no_conventions_says_so_and_does_not_send_a_read() {
        let m = manifest("rust");
        let t = porting_task(&whole("litellm/a.py", 10), &m, "clone", "out", true, None);
        assert!(t.contains("No conventions contract was produced"), "{t}");
        assert!(!t.contains("<conventions>"), "{t}");
        assert!(t.contains("do not retry any read"), "{t}");
    }

    /// The contract rides in the pinned anchor of every dispatch, so a runaway
    /// design phase must not be able to inline an unbounded document thousands
    /// of times. Over the cap it is trimmed with a note; under it, verbatim.
    #[test]
    fn an_oversized_conventions_contract_is_capped_not_inlined_whole() {
        let small = "# Conventions\nuse snake_case\n";
        assert_eq!(cap_conventions(small), small);

        let big = "x".repeat(CONVENTIONS_INLINE_CAP_BYTES + 5_000);
        let capped = cap_conventions(&big);
        assert!(capped.len() < big.len(), "must trim");
        assert!(capped.contains("capped at"), "{capped}");
        assert!(
            capped.starts_with("xxxx"),
            "the binding head of the contract survives: {}",
            &capped[..20]
        );
    }

    /// Regression: `run_sweep` takes output *before* source. Both are `&str`, so
    /// passing them the other way round compiles and sends the auditor to read
    /// the Python clone as though it were the Rust port.
    #[test]
    fn the_sweep_task_put_the_port_where_the_port_is() {
        let t = sweep_task("security", &["out/a.rs".to_string()], "OUTROOT", "SRCROOT");
        let they_live = t
            .lines()
            .find(|l| l.contains("They live under"))
            .unwrap_or_default();
        assert!(they_live.contains("OUTROOT"), "{they_live}");
        // Asserted as one whole sentence rather than as two independent
        // substrings: swapping the two arguments leaves both roots present in
        // the line and only breaks their order, which is exactly the failure
        // this test exists to catch.
        assert!(
            they_live.contains(
                "They live under `OUTROOT/`; the original source they were ported from is under `SRCROOT/`"
            ),
            "{they_live}"
        );
    }

    /// The three sweeps must ask genuinely different questions. If they collapse
    /// into one generic "review this", the mission pays three times for one pass.
    #[test]
    fn the_three_sweeps_audit_different_axes() {
        let targets = vec!["out/a.rs".to_string()];
        let sec = sweep_task("security", &targets, "o", "s");
        let dur = sweep_task("durability", &targets, "o", "s");
        let per = sweep_task("performance", &targets, "o", "s");
        assert!(sec.contains("injection") && sec.contains("unsafe"), "{sec}");
        assert!(
            dur.contains("swallowed errors") && dur.contains("resource lifetime"),
            "{dur}"
        );
        assert!(
            per.contains("quadratic") && per.contains("blocking"),
            "{per}"
        );
        assert!(!dur.contains("injection"), "axes must not blur");
        assert!(!per.contains("swallowed errors"), "axes must not blur");
        for t in [&sec, &dur, &per] {
            // An auditor that edits the code it is auditing cannot report on the
            // result, and remediation is a separate phase with a separate owner.
            assert!(t.contains("do not edit anything"), "{t}");
            assert!(t.contains("path:line"), "{t}");
            assert!(
                t.contains("say so explicitly rather than omitting it"),
                "a silent pass must be distinguishable from a skipped file: {t}"
            );
        }
    }

    #[test]
    fn the_design_task_names_the_file_it_must_write() {
        let m = manifest("rust");
        let (_tmp, plan) = tiny_plan();
        let t = design_task(&m, &plan, "src", "out", "mission/conventions.md");
        assert!(
            t.contains("Write a single Markdown file to `mission/conventions.md`"),
            "{t}"
        );
        assert!(t.contains("cannot see each other's work"), "{t}");
        assert!(t.contains("error-handling strategy"), "{t}");
        // The contract is inlined into every dispatch, so the design phase is
        // told to keep it tight rather than write an essay nobody can afford.
        assert!(t.contains("Keep the contract tight"), "{t}");
        // The arithmetic has to be in the prompt: "make it consistent" is not an
        // instruction, "1,175 sectors written by engineers who cannot talk to
        // each other" is.
        assert!(t.contains(&plan.totals.sectors.to_string()), "{t}");
        assert!(
            t.contains("Write only `mission/conventions.md`"),
            "the design phase must not start porting: {t}"
        );
    }

    // ── Gates ──────────────────────────────────────────────────────

    #[test]
    fn the_rust_gate_set_blocks_on_what_must_compile() {
        let gates = gate_set("rust");
        let names: Vec<&str> = gates.iter().map(|g| g.name).collect();
        assert_eq!(
            names,
            vec!["fmt", "build", "clippy", "test", "bench"],
            "{names:?}"
        );
        let blocking = |n: &str| gates.iter().find(|g| g.name == n).unwrap().blocking;
        assert!(!blocking("fmt"), "style must not strand a 40-hour port");
        assert!(blocking("build"));
        assert!(blocking("clippy"));
        assert!(blocking("test"));
        assert!(
            !blocking("bench"),
            "a crate with no bench target yet must not fail"
        );
    }

    #[test]
    fn clippy_is_held_to_deny_warnings() {
        let gates = gate_set("rust");
        let clippy = gates.iter().find(|g| g.name == "clippy").unwrap();
        assert!(clippy.args.contains(&"-D"), "{:?}", clippy.args);
        assert!(clippy.args.contains(&"warnings"), "{:?}", clippy.args);
    }

    /// A language with no verified toolchain story gets *no* gates and the
    /// report says nothing was machine-verified. A gate that always passes
    /// would be worse than none.
    #[test]
    fn an_unknown_target_language_is_reported_as_unverified_not_green() {
        assert!(gate_set("cobol").is_empty());
        let tmp = tempfile::tempdir().unwrap();
        let gates = run_gates(tmp.path(), "cobol", Duration::from_secs(5), false);
        assert_eq!(gates.len(), 1);
        assert!(gates[0].skipped, "{gates:?}");
        assert!(!gates[0].success);
        let s = gate_summary(&gates);
        assert!(s.contains("0 passed"), "{s}");
        assert!(s.contains("skipped"), "{s}");
        assert!(s.contains("verified nothing"), "{s}");
        assert!(
            gates[0].errors[0].contains("machine-verified"),
            "{:?}",
            gates[0].errors
        );
    }

    #[test]
    fn a_gate_whose_manifest_is_missing_is_skipped_not_failed() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = &gate_set("rust")[1];
        assert_eq!(spec.name, "build");
        let r = run_gate(tmp.path(), spec, Duration::from_secs(5));
        assert!(r.skipped, "{r:?}");
        assert!(!r.success);
        assert!(r.errors[0].contains("Cargo.toml"), "{:?}", r.errors);
    }

    /// A real subprocess that really fails, so the parser and the
    /// skipped/failed distinction are exercised against something other than a
    /// stub.
    #[test]
    fn a_real_failing_gate_is_reported_with_its_output() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = GateSpec {
            name: "locate",
            program: "cargo",
            args: &["locate-project"],
            blocking: true,
            fails_on_output: false,
            requires: None,
        };
        let r = run_gate(tmp.path(), &spec, Duration::from_secs(60));
        assert!(!r.skipped, "it ran, so it must not claim it skipped: {r:?}");
        assert!(!r.success, "{r:?}");
        assert!(r.blocking);
        assert!(
            !r.tail.is_empty(),
            "a failure must be diagnosable from the report"
        );
    }

    /// Asserted as the exact line, not as substrings: the point of the summary
    /// is that a *skip* cannot hide inside a pass count, and a substring check
    /// would still be satisfied by a summary that merely mentioned the word.
    /// Passed gates are counted but deliberately not named — the names that
    /// matter are the ones that did not verify anything.
    #[test]
    fn gate_summary_separates_passed_failed_and_skipped() {
        let passed = GateResult {
            gate: "build".into(),
            command: "cargo build".into(),
            success: true,
            skipped: false,
            timed_out: false,
            blocking: true,
            duration_ms: 10,
            errors: vec![],
            warnings: vec!["w".into()],
            tail: String::new(),
        };
        let mut failed = passed.clone();
        failed.gate = "test".into();
        failed.success = false;
        failed.errors = vec!["boom".into()];
        let skipped = GateResult::skipped("fmt", "cargo fmt", "no manifest".into());
        let s = gate_summary(&[passed, failed, skipped]);
        assert_eq!(s, "gates: 1 passed, 1 FAILED (test), 1 skipped (fmt)");
        // A named failure already says the run was not clean, and appending the
        // skip warning would bury the thing worth reading.
        assert!(!s.contains("verified nothing"), "{s}");
    }

    /// The clause exists so a mission with no toolchain at all cannot read as a
    /// success: "0 passed, 1 skipped" on its own looks like a tally rather than
    /// a confession.
    #[test]
    fn gate_summary_flags_a_run_that_verified_nothing() {
        let skipped = GateResult::skipped("build", "cargo build", "no manifest".into());
        assert_eq!(
            gate_summary(&[skipped]),
            "gates: 0 passed, 1 skipped (build) — a skipped gate verified nothing"
        );
        assert_eq!(gate_summary(&[]), "no gates ran");
    }

    /// A hung `cargo test` would otherwise hang the mission forever while holding
    /// the spend budget open. The bound is what is being tested, not the kill:
    /// a deadline that is enforced only against the gate process and not against
    /// the children it spawned is not a deadline, and `ping` outliving `cmd`
    /// here is exactly that case.
    #[cfg(windows)]
    #[test]
    fn a_gate_that_hangs_is_killed_at_its_deadline() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = GateSpec {
            name: "hang",
            program: "cmd",
            args: &["/C", "ping -n 30 127.0.0.1 > NUL"],
            blocking: true,
            fails_on_output: false,
            requires: None,
        };
        let started = Instant::now();
        let r = run_gate(tmp.path(), &spec, Duration::from_secs(2));
        let elapsed = started.elapsed();
        assert!(r.timed_out, "{r:?}");
        assert!(!r.success);
        assert!(
            r.errors.iter().any(|e| e.contains("deadline")),
            "{:?}",
            r.errors
        );
        // 30s is what an unbounded drain costs. 15s leaves room for a loaded
        // machine while still failing if the pipes are ever joined again.
        assert!(
            elapsed < Duration::from_secs(15),
            "the gate outlived its deadline by a wide margin: {elapsed:?}"
        );
    }

    /// The reason the drain is chunked rather than `read_to_end`: a gate that
    /// prints and *then* hangs is the common shape of a real timeout, and the
    /// line naming the stuck test is the only diagnosis on offer.
    #[cfg(windows)]
    #[test]
    fn a_gate_killed_at_its_deadline_still_hands_over_what_it_printed() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = GateSpec {
            name: "noisy_hang",
            program: "cmd",
            args: &[
                "/C",
                "echo running hanging_test ...& ping -n 30 127.0.0.1 > NUL",
            ],
            blocking: true,
            fails_on_output: false,
            requires: None,
        };
        let r = run_gate(tmp.path(), &spec, Duration::from_secs(2));
        assert!(r.timed_out, "{r:?}");
        assert!(
            r.tail.contains("hanging_test"),
            "output produced before the hang must survive the kill: {r:?}"
        );
    }

    #[test]
    fn each_language_names_the_manifest_its_gates_need() {
        assert_eq!(manifest_for("rust"), Some("Cargo.toml"));
        assert_eq!(manifest_for("rs"), Some("Cargo.toml"));
        assert_eq!(manifest_for("TypeScript"), Some("package.json"));
        assert_eq!(manifest_for("golang"), Some("go.mod"));
        assert_eq!(manifest_for("python"), None);
        assert_eq!(manifest_for("cobol"), None);
        assert_eq!(target_ext("rust"), Some("rs"));
        assert_eq!(target_ext("node"), Some("js"));
        assert_eq!(target_ext("cobol"), None);
    }

    /// The default is to run the whole set: "build failed" plus "clippy found 40
    /// lints" is more actionable than "build failed" alone. But `stop_on_failure`
    /// must halt on a *failure*, and a skip is not one.
    #[test]
    fn a_skip_is_not_a_failure_and_does_not_halt_the_set() {
        let tmp = tempfile::tempdir().unwrap();
        let all = run_gates(tmp.path(), "rust", Duration::from_secs(5), true);
        assert_eq!(
            all.len(),
            gate_set("rust").len(),
            "every gate must still have run: {all:?}"
        );
        assert!(all.iter().all(|g| g.skipped), "{all:?}");
    }

    // ── The later phases' prompts ─────────────────────────────────

    /// A finding may be rejected, but not silently: the report has to say why,
    /// because "the sweeps found nothing" and "the sweeps were ignored" look
    /// identical in a summary line.
    #[test]
    fn remediation_demands_a_fix_or_a_written_refusal_for_every_finding() {
        let m = manifest("rust");
        let findings = vec![
            "findings/security-1.md".to_string(),
            "findings/durability-2.md".to_string(),
        ];
        let t = remediation_task(&m, &findings, "clone", "out", "mission");
        for f in &findings {
            assert!(t.contains(f), "every report must be named: {t}");
        }
        assert!(t.contains("mission/REJECTED.md"), "{t}");
        assert!(
            t.contains("Do not dismiss a finding because the code compiles"),
            "{t}"
        );
        assert!(
            t.contains("A fix that regresses a gate is not a fix"),
            "{t}"
        );
        assert!(t.contains("`out/`"), "{t}");
    }

    /// The parity review exists to catch a port that compiles, passes its own
    /// tests and quietly does less than the source. Its prompt has to forbid the
    /// two ways that review gets fudged: fixing instead of reporting, and
    /// ignoring stubs.
    #[test]
    fn the_parity_review_hunts_stubs_and_reports_rather_than_fixes() {
        let t = parity_task(&manifest("rust"), "clone", "out");
        assert!(t.contains("out/PARITY.md"), "{t}");
        assert!(
            t.contains("todo!()"),
            "stubs are the point of the phase: {t}"
        );
        assert!(t.contains("behavioural parity, not for style"), "{t}");
        assert!(t.contains("Do not fix anything here"), "{t}");
        assert!(t.contains("Do not modify `clone/`"), "{t}");
        assert!(
            t.contains("the divergence is documented rather than silent"),
            "{t}"
        );
    }

    #[test]
    fn the_docs_task_publishes_measured_numbers_or_admits_there_are_none() {
        let mut totals = SectorTotals::default();
        totals.sectors = 1_175;
        totals.dispatches = 3_854;
        totals.lines = 951_182;
        let t = docs_task(&manifest("rust"), "clone", "out", "mission", &totals);
        for f in ["README.md", "PORTING_NOTES.md", "BENCHMARKS.md"] {
            assert!(t.contains(f), "{t}");
        }
        assert!(
            t.contains("out/README.md") || t.contains("`README.md`"),
            "{t}"
        );
        assert!(
            t.contains("say that instead of estimating"),
            "a fabricated benchmark is worse than an absent one: {t}"
        );
        assert!(t.contains("reproduced faithfully"), "{t}");
        // The scope arithmetic has to be in the prompt, or the docs describe the
        // plan rather than the tree.
        assert!(
            t.contains("1,175 sectors") || t.contains("1175 sectors"),
            "{t}"
        );
        assert!(t.contains("951,182") || t.contains("951182"), "{t}");
        assert!(t.contains("Document what actually exists"), "{t}");
        assert!(t.contains("Do not modify `clone/`"), "{t}");
    }

    // ── Persistence: the resume record ────────────────────────────

    /// A mission is *expected* to be interrupted, so the resume record is the
    /// load-bearing artifact in the whole module. Everything in it has to come
    /// back exactly.
    #[test]
    fn a_progress_record_survives_the_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut p = MissionProgress::default();
        p.mark_done("core-types");
        p.mark_failed("proxy-router", "model 404".into());
        p.mark_phase("design");
        p.record_dispatch(DispatchRecord {
            sector: "proxy-router".into(),
            unit: "proxy/router.py:1-200".into(),
            ok: false,
            secs: 41,
            provider: "openrouter".into(),
            model: "small".into(),
            degraded: true,
            error: Some("model 404".into()),
        });
        p.gates = vec![GateResult::skipped(
            "build",
            "cargo build",
            "no manifest".into(),
        )];
        p.sweeps = vec![SweepSummary {
            sweep: "security".into(),
            role: "Security Sweep Auditor".into(),
            dispatches: 3,
            sectors_covered: 12,
            sectors_available: 40,
            truncated: true,
            // The dispatch record above is degraded, so the sweep that consumed
            // it is too; round-tripping a non-default value proves the field
            // survives save/load rather than silently resetting to false.
            degraded: true,
            reports: vec!["findings/security-1.md".into()],
            secs: 90,
        }];
        p.save(tmp.path()).unwrap();
        let back = MissionProgress::load(tmp.path())
            .unwrap()
            .expect("just written");
        assert_eq!(back, p);
        assert_eq!(back.version, PROGRESS_VERSION);
        assert_eq!(back.dispatches_run, 1);
        assert_eq!(back.dispatch_secs_used, 41);
        assert_eq!(back.degraded_dispatches, 1);
        assert_eq!(back.attempts("proxy-router"), 1);
        assert!(back.phase_done("design"));
        assert_eq!(back.sweeps[0].coverage(), p.sweeps[0].coverage());
    }

    /// The most expensive failure mode this module has: treating an unreadable
    /// resume record as "fresh start" re-dispatches every sector already paid
    /// for, which on LiteLLM is 3,854 model calls.
    #[test]
    fn a_corrupt_progress_record_is_an_error_not_a_fresh_start() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            MissionProgress::path(tmp.path()),
            "{\"version\":1,\"status\":\"running\",\"sectors_do",
        )
        .unwrap();
        let err = MissionProgress::load(tmp.path()).unwrap_err();
        assert!(err.contains("cannot parse"), "{err}");
        assert!(err.contains(PROGRESS_FILE), "{err}");
    }

    /// Silently defaulting an unfamiliar record would re-port finished sectors,
    /// so a version mismatch is a hard error naming both versions.
    #[test]
    fn a_record_from_another_progress_version_is_refused_rather_than_guessed() {
        let tmp = tempfile::tempdir().unwrap();
        let mut p = MissionProgress::default();
        p.mark_done("core-types");
        p.version = PROGRESS_VERSION + 1;
        p.save(tmp.path()).unwrap();
        let err = MissionProgress::load(tmp.path()).unwrap_err();
        assert!(err.contains("refusing to guess"), "{err}");
        assert!(err.contains(&PROGRESS_VERSION.to_string()), "{err}");
    }

    #[test]
    fn no_progress_file_at_all_is_a_first_run_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(MissionProgress::load(tmp.path()).unwrap().is_none());
    }

    /// 3,854 dispatch records rewritten after every dispatch is how a mission
    /// spends its last hours doing file I/O. The tail is bounded; the lifetime
    /// counters must not be derived from it, or a long run undercounts its own
    /// degraded dispatches.
    #[test]
    fn the_recent_dispatch_tail_is_bounded_but_the_totals_are_not() {
        let mut p = MissionProgress::default();
        let total = RECENT_DISPATCH_CAP * 3;
        for i in 0..total {
            p.record_dispatch(DispatchRecord {
                sector: format!("s{i}"),
                unit: "u".into(),
                ok: i % 2 == 0,
                secs: 1,
                provider: "x".into(),
                model: "y".into(),
                degraded: i % 5 == 0,
                error: None,
            });
        }
        assert_eq!(p.recent_dispatches.len(), RECENT_DISPATCH_CAP);
        assert_eq!(p.dispatches_run, total);
        assert_eq!(p.dispatch_secs_used, total as u64);
        let in_tail = p.recent_dispatches.iter().filter(|d| d.degraded).count();
        assert!(
            p.degraded_dispatches > in_tail,
            "{} lifetime vs {} in the tail — the count must not come from the tail",
            p.degraded_dispatches,
            in_tail
        );
        // The retained tail is the *recent* one.
        assert_eq!(
            p.recent_dispatches.last().unwrap().sector,
            format!("s{}", total - 1)
        );
        assert!(!p.recent_dispatches.iter().any(|d| d.sector == "s0"));
    }

    #[test]
    fn a_sector_that_keeps_failing_is_counted_so_it_can_be_given_up_on() {
        let mut p = MissionProgress::default();
        p.mark_failed("proxy-router", "first".into());
        p.mark_failed("proxy-router", "second".into());
        assert_eq!(p.attempts("proxy-router"), 2);
        assert_eq!(p.sectors_failed["proxy-router"].last_error, "second");
        assert_eq!(p.attempts("never-tried"), 0);
        // Completing a sector must clear its failure, or a resumed run would see
        // it as both done and failed and the report would not know which to
        // believe.
        p.mark_done("proxy-router");
        assert!(p.is_done("proxy-router"));
        assert_eq!(p.attempts("proxy-router"), 0);
        p.mark_done("proxy-router");
        assert_eq!(
            p.sectors_done
                .iter()
                .filter(|s| *s == "proxy-router")
                .count(),
            1,
            "a sector must not be counted twice in the done list"
        );
        p.mark_phase("design");
        p.mark_phase("design");
        assert_eq!(p.phases_done.iter().filter(|f| *f == "design").count(), 1);
    }

    #[test]
    fn an_atomic_write_leaves_no_temp_sibling_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("report.json");
        atomic_write_json(&path, &json!({ "status": "completed" })).unwrap();
        assert!(path.is_file());
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the temp file must be renamed, not left beside the real one"
        );
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["status"], "completed");
    }

    #[test]
    fn an_atomic_write_into_a_missing_directory_says_where_it_failed() {
        let tmp = tempfile::tempdir().unwrap();
        let err = atomic_write_json(&tmp.path().join("nope").join("report.json"), &json!({}))
            .unwrap_err();
        assert!(err.contains("cannot write"), "{err}");
        assert!(err.contains("nope"), "{err}");
    }

    /// The invariant the parity review depends on: the port must not be able to
    /// write into the clone it is diffed against.
    #[test]
    fn the_manifest_round_trips_and_keeps_the_output_outside_the_clone() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest("rust");
        let src = normalize_rel(&m.source_root);
        let out = normalize_rel(&m.output_root);
        assert!(
            !out.starts_with(&format!("{src}/")),
            "{out} is inside {src}"
        );
        assert!(
            !src.starts_with(&format!("{out}/")),
            "{src} is inside {out}"
        );
        m.save(tmp.path()).unwrap();
        assert_eq!(MissionManifest::load(tmp.path()).unwrap(), m);
        let err = MissionManifest::load(&tmp.path().join("elsewhere")).unwrap_err();
        assert!(err.contains(MISSION_FILE), "{err}");
    }

    // ── Configuration and budget ─────────────────────────────────

    #[test]
    fn the_default_config_is_the_one_an_operator_would_choose() {
        let c = MissionConfig::default();
        assert!(
            c.run_gates && c.run_sweeps && c.run_documentation,
            "verification, all three audits and the docs are the point of the mission"
        );
        assert!(!c.dry_run);
        // Uncapped by default is deliberate: a cap nobody chose silently
        // truncates the job, and the tool is only reachable with an explicit
        // authorization to spend.
        assert_eq!(c.max_dispatches, 0);
        assert_eq!(c.max_wall_secs, 0);
        assert_eq!(c.max_waves, 0);
        assert!(
            !c.stop_on_gate_failure,
            "one sector that will not compile must not strand the other 1,174"
        );
        assert_eq!(c.max_attempts_per_sector, 2);
        assert_eq!(c.sweep_batch_sectors, DEFAULT_SWEEP_BATCH_SECTORS);
        assert!(
            c.max_sweep_dispatches > 0,
            "the audit spend is bounded separately from the porting spend"
        );
        assert_eq!(c.gate_timeout_secs, DEFAULT_GATE_TIMEOUT_SECS);
    }

    /// Parallelism becomes an OS-thread count, each holding an open model
    /// connection, so it is clamped rather than trusted.
    #[test]
    fn parallelism_is_clamped_because_it_becomes_a_thread_count() {
        assert_eq!(
            MissionConfig::from_args(&json!({"parallelism": 10_000})).parallelism,
            32
        );
        assert_eq!(
            MissionConfig::from_args(&json!({"parallelism": 0})).parallelism,
            0
        );
        assert_eq!(
            MissionConfig::from_args(&json!({"parallelism": 4})).parallelism,
            4
        );
    }

    /// Tool arguments come from a model. Every wrong type must fall back rather
    /// than panic, and the two zeros that would be actively dangerous — no
    /// retries at all, and a batch size of zero — must be raised.
    #[test]
    fn junk_arguments_fall_back_to_the_defaults_instead_of_panicking() {
        let c = MissionConfig::from_args(&json!({
            "parallelism": "lots",
            "max_dispatches": null,
            "run_gates": "yes",
            "gate_timeout_secs": 0,
            "max_attempts": 0,
            "sweep_batch_sectors": 0,
            "team_id": "   ",
        }));
        let d = MissionConfig::default();
        assert_eq!(c.parallelism, d.parallelism);
        assert_eq!(c.max_dispatches, d.max_dispatches);
        assert_eq!(c.run_gates, d.run_gates);
        assert_eq!(c.gate_timeout_secs, d.gate_timeout_secs);
        // A zero retry budget would strand every sector on its first failure.
        assert_eq!(c.max_attempts_per_sector, 1);
        // A zero batch size would chunk into nothing.
        assert_eq!(c.sweep_batch_sectors, 1);
        assert_eq!(
            c.team_id, None,
            "a blank team_id must mean 'use the recorded one', not 'no team'"
        );
    }

    /// Printed in every report because "completed" means something different
    /// under each cap.
    #[test]
    fn the_caps_in_force_are_stated_in_words() {
        let c = MissionConfig::default();
        let open = c.describe_caps().join(" | ");
        assert!(open.contains("dispatches: uncapped"), "{open}");
        assert!(open.contains("wall clock: uncapped"), "{open}");
        assert!(
            !open.contains("waves:"),
            "an unset wave cap is not a cap and must not be listed as one: {open}"
        );
        let set = MissionConfig {
            max_dispatches: 50,
            max_wall_secs: 3600,
            max_waves: 3,
            ..Default::default()
        }
        .describe_caps()
        .join(" | ");
        assert!(set.contains("dispatches: at most 50"), "{set}");
        assert!(set.contains("wall clock: at most 3600s"), "{set}");
        assert!(set.contains("waves: at most 3"), "{set}");
        assert!(set.contains("at most 2 attempts per sector"), "{set}");
    }

    #[test]
    fn the_budget_names_the_cap_that_bit() {
        let b = Budget::new(2, 0);
        assert!(b.exhausted().is_none());
        assert_eq!(b.remaining(), Some(2));
        b.record(10);
        assert_eq!(b.remaining(), Some(1));
        assert!(
            b.exhausted().is_none(),
            "one dispatch left is not exhausted"
        );
        b.record(10);
        let why = b.exhausted().expect("at the cap");
        assert!(why.contains("dispatch cap"), "{why}");
        // Both numbers, because this string becomes `stop_reason` and sits next
        // to `dispatches_this_run` in the same report. Naming only the cap read
        // as a claim about how many dispatches happened: the live LiteLLM run
        // reported "(8 this run)" beside a count of 11.
        assert!(why.contains("limit 2"), "{why}");
        assert!(why.contains("2 dispatched"), "{why}");
        assert!(
            why.contains("parallelism-1"),
            "the overshoot is by design and must say so: {why}"
        );
        assert_eq!(b.remaining(), Some(0));
        assert_eq!(b.dispatches(), 2);
        assert_eq!(b.secs(), 20);

        let open = Budget::new(0, 0);
        open.record(1);
        assert!(open.exhausted().is_none());
        assert_eq!(open.remaining(), None, "uncapped has no 'remaining'");
    }

    #[test]
    fn the_wall_clock_cap_trips_on_elapsed_time_alone() {
        let b = Budget::new(0, 1);
        assert!(b.exhausted().is_none(), "fresh");
        std::thread::sleep(Duration::from_millis(1100));
        let why = b.exhausted().expect("a second has passed");
        assert!(why.contains("wall-clock cap"), "{why}");
        assert_eq!(b.dispatches(), 0, "it tripped without a single dispatch");
    }

    /// Caps are per *run*. A mission paused on its dispatch cap must be resumable
    /// with a fresh cap, otherwise raising the limit and re-invoking would
    /// immediately re-trip the old one and the only way forward would be editing
    /// progress.json by hand.
    #[test]
    fn a_fresh_run_gets_a_fresh_budget() {
        let first = Budget::new(1, 0);
        first.record(5);
        assert!(first.exhausted().is_some());
        let resumed = Budget::new(10, 0);
        assert!(resumed.exhausted().is_none());
        assert_eq!(resumed.dispatches(), 0);
    }

    /// A halt keeps the first reason it was given. The alternative — last writer
    /// wins — would report "wave cap reached" over the gate failure that
    /// actually stopped the mission, depending on which thread noticed first.
    #[test]
    fn the_first_halt_reason_is_the_one_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let shared = Shared::new(
            tmp.path().to_path_buf(),
            MissionProgress::default(),
            &MissionConfig::default(),
        );
        assert!(!shared.stopping());
        assert_eq!(shared.reason(), None);
        shared.halt("blocking gate failed".into());
        shared.halt("dispatch cap reached".into());
        assert!(shared.stopping());
        assert_eq!(shared.reason().unwrap(), "blocking gate failed");
        // A dry run never checkpoints, so the record it would have written is
        // not written.
        assert!(shared.checkpoint().is_ok());
    }

    #[test]
    fn a_dry_run_shared_state_does_not_checkpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let shared = Shared::new(
            tmp.path().to_path_buf(),
            MissionProgress::default(),
            &MissionConfig {
                dry_run: true,
                ..Default::default()
            },
        );
        shared.checkpoint().unwrap();
        assert!(
            !MissionProgress::path(tmp.path()).exists(),
            "a dry run must not create a resume record"
        );
        let live = Shared::new(
            tmp.path().to_path_buf(),
            MissionProgress::default(),
            &MissionConfig::default(),
        );
        live.checkpoint().unwrap();
        assert!(MissionProgress::path(tmp.path()).is_file());
    }

    // ── Sweep batching ────────────────────────────────────────

    /// A synthetic plan with exactly these sector ids, for the tests that care
    /// about ordering and batching arithmetic rather than decomposition.
    fn plan_with(ids: &[&str]) -> SectorPlan {
        SectorPlan {
            repo_name: "demo".into(),
            source_root: "pkg".into(),
            sectors: ids
                .iter()
                .map(|id| Sector {
                    id: (*id).to_string(),
                    path: "pkg".into(),
                    files: vec![format!("pkg/{id}.py")],
                    bytes: 10,
                    lines: 2,
                    units: vec![whole(&format!("pkg/{id}.py"), 10)],
                    depends_on: Vec::new(),
                    oversized: false,
                })
                .collect(),
            waves: vec![ids.iter().map(|s| (*s).to_string()).collect()],
            totals: SectorTotals::default(),
        }
    }

    /// A chain, so every sector is its own wave. A real decomposition of a
    /// four-file fixture puts everything in wave 0 and would prove nothing about
    /// wave ordering.
    fn chained_plan(ids: &[&str]) -> SectorPlan {
        let mut plan = plan_with(ids);
        for (i, s) in plan.sectors.iter_mut().enumerate().skip(1) {
            s.depends_on = vec![ids[i - 1].to_string()];
        }
        plan.waves = ids.iter().map(|id| vec![(*id).to_string()]).collect();
        plan.totals.sectors = plan.sectors.len();
        plan.totals.waves = plan.waves.len();
        plan
    }

    /// Plan order, not completion order. Sweeping in the order the work happened
    /// to finish means two runs over the same tree audit it differently, and a
    /// finding cannot be traced back to a batch.
    #[test]
    fn sweep_batches_follow_plan_order_not_completion_order() {
        let plan = plan_with(&["a", "b", "c", "d"]);
        let reversed: Vec<String> = plan.sectors.iter().rev().map(|s| s.id.clone()).collect();
        let flat: Vec<String> = sweep_batches(&reversed, &plan, 1)
            .into_iter()
            .flatten()
            .collect();
        let expected: Vec<String> = plan.sectors.iter().map(|s| s.id.clone()).collect();
        assert_eq!(
            flat, expected,
            "completion order leaked into the audit order"
        );
    }

    #[test]
    fn sweep_batches_chunk_without_dropping_any_sector() {
        let plan = plan_with(&["a", "b", "c", "d", "e"]);
        let done: Vec<String> = plan.sectors.iter().map(|s| s.id.clone()).collect();
        let batches = sweep_batches(&done, &plan, 2);
        assert_eq!(
            batches.iter().map(|b| b.len()).collect::<Vec<_>>(),
            vec![2, 2, 1]
        );
        assert_eq!(batches.into_iter().flatten().count(), 5);
        // A batch size of zero must become one, not an empty chunk list.
        assert_eq!(sweep_batches(&done, &plan, 0).len(), 5);
    }

    #[test]
    fn a_sector_that_was_not_ported_is_not_audited() {
        let plan = plan_with(&["a", "b", "c"]);
        assert_eq!(
            sweep_batches(&["b".to_string()], &plan, 6),
            vec![vec!["b".to_string()]]
        );
        assert!(sweep_batches(&[], &plan, 6).is_empty());
        // An id that is not in the plan is dropped rather than carried into a
        // dispatch that would audit a file nobody wrote.
        assert!(sweep_batches(&["ghost".to_string()], &plan, 6).is_empty());
    }

    /// Second guard on the argument-order defect: a sweep must be pointed at the
    /// ported tree, under the *target* names.
    #[test]
    fn sweep_targets_are_output_paths_in_the_target_language() {
        let m = MissionManifest {
            package_root: "pkg".into(),
            ..manifest("rust")
        };
        let plan = plan_with(&["a", "b"]);
        let ids: Vec<String> = plan.sectors.iter().map(|s| s.id.clone()).collect();
        let targets = sweep_targets(&plan, &ids, &m, "out");
        assert_eq!(targets.len(), 2, "{targets:?}");
        for t in &targets {
            assert!(t.starts_with("out/"), "{t}");
            assert!(t.ends_with(".rs"), "{t}");
            assert!(
                !t.contains(".py"),
                "a sweep pointed at the clone audits nothing new: {t}"
            );
            assert!(
                !t.contains("pkg/"),
                "the package root is the output root, not a directory inside it: {t}"
            );
        }
        assert!(sweep_targets(&plan, &["ghost".to_string()], &m, "out").is_empty());
        // The chunked units of one source file all target one output file, and
        // auditing it three times in a batch wastes the dispatch.
        let mut chunked = plan_with(&["c"]);
        chunked.sectors[0].units = vec![
            unit("pkg/c.py", 9_000, 1, 3, 1, 100),
            unit("pkg/c.py", 9_000, 2, 3, 101, 200),
            unit("pkg/c.py", 9_000, 3, 3, 201, 300),
        ];
        assert_eq!(
            sweep_targets(&chunked, &["c".to_string()], &m, "out"),
            vec!["out/src/c.rs".to_string()]
        );
    }

    /// `sectors_covered: 144` alone does not tell an operator whether that is
    /// the whole tree or a twentieth of it.
    #[test]
    fn coverage_says_how_much_of_the_tree_a_sweep_actually_reached() {
        let full = SweepSummary {
            sweep: "security".into(),
            role: "Security Sweep Auditor".into(),
            dispatches: 7,
            sectors_covered: 40,
            sectors_available: 40,
            truncated: false,
            degraded: false,
            reports: vec![],
            secs: 100,
        };
        let c = full.coverage();
        assert!(c.contains("40 of 40"), "{c}");
        assert!(c.contains("100%"), "{c}");
        assert!(!c.contains("truncated"), "{c}");

        let part = SweepSummary {
            sectors_covered: 12,
            dispatches: 2,
            truncated: true,
            ..full.clone()
        };
        let c = part.coverage();
        assert!(c.contains("12 of 40"), "{c}");
        assert!(c.contains("30%"), "{c}");
        assert!(c.contains("truncated at 2 dispatches"), "{c}");
        assert!(c.contains("70% was never audited"), "{c}");

        let none = SweepSummary {
            sectors_covered: 0,
            sectors_available: 0,
            ..full.clone()
        };
        let c = none.coverage();
        assert!(c.contains("nothing to audit"), "{c}");
        assert!(
            !c.contains('%'),
            "a percentage of nothing is not a number to print: {c}"
        );
    }

    /// A degraded sweep files findings that *read* as an audit, so the coverage
    /// line has to say out loud that they came from a sub-7B fallback and cannot
    /// be trusted. Without this the run looks clean and the hallucinated line
    /// numbers get acted on.
    #[test]
    fn a_degraded_sweep_coverage_warns_its_findings_are_unreliable() {
        let clean = SweepSummary {
            sweep: "security".into(),
            role: "Security Sweep Auditor".into(),
            dispatches: 3,
            sectors_covered: 40,
            sectors_available: 40,
            truncated: false,
            degraded: false,
            reports: vec![],
            secs: 90,
        };
        let c = clean.coverage();
        assert!(
            !c.contains("DEGRADED"),
            "a sweep served by a real model must not carry the warning: {c}"
        );

        let degraded = SweepSummary {
            degraded: true,
            ..clean.clone()
        };
        let c = degraded.coverage();
        assert!(c.contains("DEGRADED"), "{c}");
        assert!(c.contains("unreliable"), "{c}");
        assert!(
            c.contains("not a clean bill of health"),
            "the warning has to say what a degraded sweep is not: {c}"
        );
        // The coverage numbers are still printed: the warning qualifies them, it
        // does not replace them.
        assert!(c.contains("40 of 40"), "{c}");
    }

    /// `degraded` was added after checkpoints were already being written, so a
    /// progress file from an older build has no such key. `#[serde(default)]`
    /// must load it as `false` rather than fail the resume — and defaulting to
    /// `true` would smear a spurious warning over every pre-existing run.
    #[test]
    fn a_sweep_summary_written_before_the_degraded_field_loads_as_not_degraded() {
        let legacy = json!({
            "sweep": "security",
            "role": "Security Sweep Auditor",
            "dispatches": 3,
            "sectors_covered": 12,
            "sectors_available": 40,
            "truncated": true,
            "reports": ["findings/security-1.md"],
            "secs": 90
        });
        let s: SweepSummary = serde_json::from_value(legacy)
            .expect("a checkpoint without `degraded` must still deserialise");
        assert!(
            !s.degraded,
            "the absent field defaults to false, not to a spurious warning"
        );
        assert!(!s.coverage().contains("DEGRADED"), "{}", s.coverage());
    }

    // ── Seats ────────────────────────────────────────────────

    fn team_with(id: &str, name: &str, roles: &[&str]) -> ExpertTeam {
        serde_json::from_value(json!({
            "id": id,
            "name": name,
            "members": roles.iter().map(|r| json!({ "role": r })).collect::<Vec<_>>(),
        }))
        .expect("a roster of role-only members must deserialise")
    }

    /// Exact match before substring, so "Test & QA Engineer" wins over a roster
    /// that also carries "Performance Test Engineer".
    #[test]
    fn a_seat_resolves_by_exact_role_before_substring() {
        let tmp = tempfile::tempdir().unwrap();
        save_expert_teams(
            tmp.path(),
            &[team_with(
                "rewrite-team",
                "Rewrite Team",
                &[
                    "Lead Architect",
                    "Performance Test Engineer",
                    "Test & QA Engineer",
                ],
            )],
        );
        assert_eq!(
            resolve_seat(
                tmp.path(),
                "rewrite-team",
                &["Test & QA Engineer", "QA", "Test"]
            ),
            Some("Test & QA Engineer".to_string())
        );
        assert_eq!(
            resolve_seat(tmp.path(), "rewrite-team", &["Performance"]),
            Some("Performance Test Engineer".to_string())
        );
        // No porter on this roster, and inventing one is the failure this guards.
        assert_eq!(
            resolve_seat(
                tmp.path(),
                "rewrite-team",
                &["Port Engineer", "port", "rust"]
            ),
            None
        );
    }

    /// `repo_rewrite` records a slug; an operator may type the display name. Both
    /// have to find the same team, or a resumed mission silently loses its seats
    /// and skips every phase.
    #[test]
    fn a_team_is_found_by_id_by_slug_or_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        save_expert_teams(
            tmp.path(),
            &[team_with(
                "litellm-rust-rewrite",
                "LiteLLM Rust Rewrite",
                &["Lead Architect"],
            )],
        );
        for id in [
            "litellm-rust-rewrite",
            "LiteLLM Rust Rewrite",
            "LITELLM-RUST-REWRITE",
        ] {
            assert_eq!(
                first_role(tmp.path(), id).as_deref(),
                Some("Lead Architect"),
                "not found when asked for as {id:?}"
            );
        }
        assert_eq!(first_role(tmp.path(), "unrelated"), None);
    }

    /// Substituting a different specialist would be worse than not running: an
    /// unattended security sweep that quietly ran on the Documentation Engineer
    /// returns a clean report and closes the phase.
    #[test]
    fn a_missing_seat_skips_its_phase_and_is_never_substituted() {
        let tmp = tempfile::tempdir().unwrap();
        save_expert_teams(
            tmp.path(),
            &[team_with("thin", "Thin", &["Documentation Engineer"])],
        );
        let seats = Seats::resolve(tmp.path(), "thin", "rust");
        assert_eq!(seats.docs.as_deref(), Some("Documentation Engineer"));
        for s in [
            &seats.porter,
            &seats.security,
            &seats.durability,
            &seats.performance,
            &seats.qa,
            &seats.reviewer,
        ] {
            assert!(
                s.is_none(),
                "a seat must not be filled by whoever was free: {seats:?}"
            );
        }
        // `lead` falls back to the first member by design: the design phase
        // writes a conventions document, and no conventions at all is worse than
        // conventions from the docs engineer. It is the only seat that falls back.
        assert_eq!(seats.lead.as_deref(), Some("Documentation Engineer"));
        let missing = seats.missing();
        assert_eq!(missing.len(), 6, "{missing:?}");
        for m in &missing {
            assert!(m.contains("SKIPPED"), "{m}");
            assert!(m.contains("wrong specialist"), "{m}");
        }
        assert!(
            missing.iter().any(|m| m.contains("the security sweep")),
            "{missing:?}"
        );
        assert!(missing.iter().any(|m| m.contains("porting")), "{missing:?}");
    }

    #[test]
    fn a_team_that_does_not_exist_resolves_no_seats_at_all() {
        let tmp = tempfile::tempdir().unwrap();
        let seats = Seats::resolve(tmp.path(), "no-such-team", "rust");
        assert_eq!(seats, Seats::default(), "{seats:?}");
        assert_eq!(seats.missing().len(), 8, "{:?}", seats.missing());
    }

    // ── Skeleton and path hygiene ─────────────────────────────

    /// Without a skeleton every gate skips forever and the mission reports
    /// "completed" having machine-verified nothing.
    #[test]
    fn the_skeleton_gives_the_rust_gates_a_manifest_to_run_against() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("output");
        let m = manifest("rust");
        let note = ensure_skeleton(&out, &m).expect("a Rust mission gets a skeleton");
        assert!(out.join("Cargo.toml").is_file(), "{note}");
        assert!(out.join("src").join("lib.rs").is_file());
        assert!(note.contains("build, clippy and test gates"), "{note}");
        let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(toml.contains("name = \"litellm\""), "{toml}");
        // Idempotent, and must not clobber a manifest the porters have since
        // filled in with real dependencies.
        let edited = "[package]\nname = \"litellm\"\n\n[dependencies]\nserde = \"1\"\n";
        std::fs::write(out.join("Cargo.toml"), edited).unwrap();
        assert_eq!(ensure_skeleton(&out, &m), None);
        assert_eq!(
            std::fs::read_to_string(out.join("Cargo.toml")).unwrap(),
            edited
        );
    }

    /// The integration claim behind Fix A, and the exact thing the first live
    /// LiteLLM run got wrong: it reported the build, clippy and test gates all
    /// green over a crate that held nothing but an empty `lib.rs`, because every
    /// ported file had landed outside `src/` and no `mod` wiring made it
    /// reachable. Green gates over an empty crate are worse than red ones — they
    /// say "ported and verified" about code cargo never opened.
    ///
    /// This drives the *real* machinery end to end — [`ensure_skeleton`],
    /// [`target_rel`], [`wire_rust_modules`], [`run_gates`] — over a synthetic
    /// ported module carrying a `#[test]`, plus a stray file left outside `src/`
    /// the way the buggy runner used to leave everything. Two things are then
    /// asserted that the old run could not: `cargo test` reports `1 passed`
    /// rather than `running 0 tests` (so the ported module really is reachable
    /// from the crate root and was compiled), and the stray file is named as
    /// orphaned (so uncompiled ported code cannot hide behind a green build).
    #[test]
    fn the_gates_compile_a_real_ported_module_not_an_empty_crate() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("output");
        let m = manifest("rust");
        ensure_skeleton(&out, &m).expect("a Rust mission gets a skeleton");

        // A ported module, placed exactly where `target_rel` tells a porter to
        // put it — under `src/`, which is the whole of Fix A.
        let unit = whole("litellm/proxy/router.py", 100);
        let rel = target_rel(&unit, &m.package_root, &m.target_language);
        assert_eq!(rel, "src/proxy/router.rs", "Rust must land under src/");
        let path = out.join(&rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "pub fn answer() -> usize {\n    42\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {\n        assert_eq!(super::answer(), 42);\n    }\n}\n",
        )
        .unwrap();

        // And a stray, syntactically *invalid* file outside `src/`, reproducing
        // the pre-fix placement. Cargo never compiles it, so the build stays
        // green — which is precisely why it has to be reported rather than
        // left to pass silently.
        std::fs::create_dir_all(out.join("orphan")).unwrap();
        std::fs::write(
            out.join("orphan").join("stray.rs"),
            "this is not valid rust and nothing compiles it\n",
        )
        .unwrap();

        // Wire the tree, as the live pass does after every wave.
        let wiring = wire_rust_modules(&out);
        assert_eq!(
            wiring.modules, 1,
            "the ported module is reachable: {wiring:?}"
        );
        assert_eq!(
            wiring.orphaned,
            vec!["orphan/stray.rs".to_string()],
            "Fix B: the file outside src/ must be named, not silently ignored: {wiring:?}"
        );
        let summary = wiring.summary().expect("something was ported");
        assert!(summary.contains("OUTSIDE src/"), "{summary}");
        let lib = std::fs::read_to_string(out.join("src").join("lib.rs")).unwrap();
        assert!(
            lib.contains("mod proxy"),
            "the crate root must declare the ported module: {lib}"
        );

        // The real gates over the real tree. `stop_on_failure = false` so every
        // gate reports, and a skip cannot masquerade as a pass.
        let gates = run_gates(&out, "rust", Duration::from_secs(180), false);
        let by = |name: &str| {
            gates
                .iter()
                .find(|g| g.gate == name)
                .unwrap_or_else(|| panic!("no {name} gate ran: {gates:?}"))
        };

        let build = by("build");
        assert!(
            build.success && !build.skipped,
            "the crate must actually compile: {build:?}"
        );
        let fmt = by("fmt");
        assert!(
            fmt.success && !fmt.skipped,
            "Fix C: skeleton + ported module must be rustfmt-clean: {fmt:?}"
        );

        // The crux. The old run's test gate read `running 0 tests`; a ported
        // module that is genuinely part of the crate makes it run one.
        let test = by("test");
        assert!(test.success && !test.skipped, "test gate: {test:?}");
        assert!(
            test.tail.contains("1 passed"),
            "the ported module's test must be discovered and run, proving the crate is not empty: {:?}",
            test.tail
        );
    }

    #[test]
    fn a_language_with_no_skeleton_says_what_its_gates_need() {
        let tmp = tempfile::tempdir().unwrap();
        for (lang, needed) in [("go", "go.mod"), ("typescript", "package.json")] {
            let mut m = manifest(lang);
            m.repo_name = "demo".into();
            let note = ensure_skeleton(tmp.path(), &m).expect("these languages need a manifest");
            assert!(note.contains(needed), "{note}");
            assert!(note.contains("no project skeleton is generated"), "{note}");
        }
        // Python needs no manifest, so there is nothing to warn about, and
        // neither does a language this module has never heard of.
        assert_eq!(ensure_skeleton(tmp.path(), &manifest("python")), None);
        assert_eq!(ensure_skeleton(tmp.path(), &manifest("cobol")), None);
    }

    /// Every path that reaches a prompt goes through here, because the agent's
    /// tools resolve against the workspace root and a `\` or a leading `/` turns
    /// a valid reference into one that silently points nowhere.
    #[test]
    fn paths_that_reach_a_prompt_are_normalized() {
        assert_eq!(
            normalize_rel(r".velocity\missions\demo-rust"),
            ".velocity/missions/demo-rust"
        );
        assert_eq!(
            normalize_rel("/.velocity/missions/demo-rust/"),
            ".velocity/missions/demo-rust"
        );
        assert_eq!(normalize_rel("  clone  "), "clone");
        assert_eq!(normalize_rel(""), "");
        assert_eq!(normalize_rel("/"), "");
    }

    /// Each status gets different advice, because "re-run it" is wrong for a
    /// mission that failed for want of a seat and right for one that hit a cap.
    #[test]
    fn next_step_tells_the_operator_what_to_do_about_this_outcome() {
        use MissionStatus::*;
        let paused = next_step(Paused, 41, false, None);
        assert!(paused.contains("41 sectors remain"), "{paused}");
        assert!(paused.contains("resume"), "{paused}");
        assert!(
            paused.contains("max_dispatches"),
            "a cap is the usual cause and the fix must be named: {paused}"
        );
        assert!(
            !paused.contains("stopped because"),
            "no reason was recorded, so none may be quoted: {paused}"
        );

        // A live run that knows why it stopped has to say so, or the operator
        // re-runs it blind and lands in the same place an hour later.
        let capped = next_step(
            Paused,
            41,
            false,
            Some("dispatch cap reached (500 this run)"),
        );
        assert!(capped.contains("resume"), "{capped}");
        assert!(
            capped.contains("dispatch cap reached (500 this run)"),
            "{capped}"
        );

        let dry = next_step(Paused, 41, true, None);
        assert!(dry.contains("without dry_run"), "{dry}");
        assert!(
            !dry.contains("41"),
            "nothing is left over by a run that did nothing: {dry}"
        );

        // The precedence this signature exists for. A dry run that stopped on a
        // cap must not be told to "re-run without dry_run": the real run then
        // stops in the same place, having spent everything up to it. And it
        // must not say "resume" either, because a dry run writes no checkpoint.
        let dry_capped = next_step(Paused, 41, true, Some("wave cap reached (2 this run)"));
        assert!(
            dry_capped.contains("would have stopped here"),
            "{dry_capped}"
        );
        assert!(
            dry_capped.contains("wave cap reached (2 this run)"),
            "{dry_capped}"
        );
        assert!(dry_capped.contains("Raise that cap"), "{dry_capped}");
        assert!(
            !dry_capped.contains("resume"),
            "a dry run left no checkpoint, so resuming is not on offer: {dry_capped}"
        );

        let failed = next_step(Failed, 41, false, None);
        assert!(failed.contains("add_team_member"), "{failed}");
        assert!(failed.contains("will fail the same way"), "{failed}");

        let done = next_step(Completed, 0, false, None);
        assert!(done.contains(REPORT_FILE), "{done}");
        assert!(done.contains(FINDINGS_DIR), "{done}");
        assert!(next_step(Running, 0, false, None).contains("process died"));
    }

    // ── `run_mission`, driven end to end at zero model spend ───────────────────

    const MISSION_REL: &str = ".velocity/missions/demo-rust";

    /// The full roster, so nothing is skipped for want of a seat.
    const FULL_ROSTER: &[&str] = &[
        "Lead Architect",
        "Rust Port Engineer",
        "Test & QA Engineer",
        "Code Parity Reviewer",
        "Security Sweep Auditor",
        "Durability Engineer",
        "Performance Engineer",
        "Documentation Engineer",
    ];

    /// Write a mission directory: manifest, plan and roster.
    fn write_mission(ws: &Path, plan: &SectorPlan, roles: &[&str]) {
        let mission = ws.join(MISSION_REL);
        std::fs::create_dir_all(&mission).unwrap();
        let m = MissionManifest {
            repo_name: "demo".into(),
            source_root: "clone".into(),
            package_root: plan.source_root.clone(),
            output_root: format!("{MISSION_REL}/output"),
            team_id: "rewrite-team".into(),
            parallelism: 2,
            ..manifest("rust")
        };
        m.save(&mission).unwrap();
        std::fs::write(
            mission.join(PLAN_FILE),
            serde_json::to_string_pretty(&plan.to_json()).unwrap(),
        )
        .unwrap();
        assert!(
            save_expert_teams(ws, &[team_with("rewrite-team", "Rewrite Team", roles)]),
            "the roster must persist, or every seat test below is testing the presets"
        );
        // Confirmed rather than assumed: `load_expert_teams` falls back to the
        // built-in presets when the store cannot be read, which would quietly
        // turn a test of this roster into a test of somebody else's.
        assert!(
            load_team(ws, "rewrite-team").is_some(),
            "the fixture roster did not come back off disk"
        );
    }

    /// A real decomposition over a real four-file tree, for the tests that need
    /// `run_mission` driven by something other than a hand-built plan.
    fn mission_fixture(roles: &[&str]) -> (tempfile::TempDir, SectorPlan) {
        let ws = tempfile::tempdir().unwrap();
        let dir = ws.path().join("clone").join("pkg");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a", "b", "c", "d"] {
            std::fs::write(
                dir.join(format!("{name}.py")),
                format!("def {name}():\n    return 1\n"),
            )
            .unwrap();
        }
        let plan = plan_sectors_with_parallelism(
            &ws.path().join("clone"),
            &SectorOptions {
                max_sector_bytes: 64,
                ..Default::default()
            },
            2,
        );
        assert!(
            plan.sectors.len() >= 2,
            "the fixture must decompose into several sectors: {plan:?}"
        );
        write_mission(ws.path(), &plan, roles);
        (ws, plan)
    }

    /// Parallelism 1 so the cap tests are deterministic: with several workers,
    /// the ones already past the budget check when the cap trips are allowed to
    /// finish, which is correct but not countable.
    fn dry() -> MissionConfig {
        MissionConfig {
            dry_run: true,
            parallelism: 1,
            ..Default::default()
        }
    }

    fn units_of(plan: &SectorPlan, skip: &[String]) -> usize {
        plan.sectors
            .iter()
            .filter(|s| !skip.iter().any(|d| d == &s.id))
            .map(|s| s.units.len())
            .sum()
    }

    #[test]
    fn a_missing_mission_directory_names_the_step_that_creates_it() {
        let ws = tempfile::tempdir().unwrap();
        let err = run_mission(ws.path(), ".velocity/missions/nope", &dry()).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
        assert!(err.contains("repo_rewrite"), "{err}");
        assert!(err.contains(MISSION_FILE), "{err}");
        assert!(err.contains(PLAN_FILE), "{err}");
    }

    #[test]
    fn a_manifest_with_no_team_refuses_to_start() {
        let (ws, _plan) = mission_fixture(FULL_ROSTER);
        let mission = ws.path().join(MISSION_REL);
        let mut m = MissionManifest::load(&mission).unwrap();
        m.team_id = "   ".into();
        m.save(&mission).unwrap();
        let err = run_mission(ws.path(), MISSION_REL, &dry()).unwrap_err();
        assert!(err.contains("no team_id"), "{err}");
        assert!(err.contains("cannot dispatch"), "{err}");
        // A team named in the call replaces the recorded one rather than being
        // ignored — that is how a mission is rescued after its team was deleted.
        let cfg = MissionConfig {
            team_id: Some("rewrite-team".into()),
            ..dry()
        };
        let report = run_mission(ws.path(), MISSION_REL, &cfg).unwrap();
        assert_eq!(report.team_id, "rewrite-team");
    }

    /// A live config that dispatches nothing extra: no gates, no sweeps, no
    /// documentation phase. Used by the RBAC-chokepoint tests so a cleared run
    /// still costs zero model calls.
    fn live_lean() -> MissionConfig {
        MissionConfig {
            run_gates: false,
            run_sweeps: false,
            run_documentation: false,
            parallelism: 1,
            ..Default::default()
        }
    }

    /// System C chokepoint (spec §6.2): a governed workspace denies dispatch
    /// without a grant — and INV-6 makes the denial itself an audit op.
    #[test]
    fn a_governed_workspace_denies_mission_dispatch_without_a_grant() {
        let (ws, _plan) = mission_fixture(FULL_ROSTER);
        let mut org = crate::automation::governor::test_governed_org();
        org.grants.clear(); // the role exists but holds no authority anymore
        assert!(crate::editor::company_org::save_company_org(
            ws.path(),
            &org
        ));
        let err = run_mission(ws.path(), MISSION_REL, &live_lean()).unwrap_err();
        assert!(err.contains("rbac") && err.contains("denied"), "{err}");
        let log = crate::automation::OpLogStore::open(ws.path()).load();
        assert!(
            log.ops()
                .iter()
                .any(|o| o.intent.contains("rbac-deny: dispatch_to")),
            "the denial must be retrievable from the durable trail: {:?}",
            log.ops().iter().map(|o| &o.intent).collect::<Vec<_>>()
        );
    }

    /// The gate admits exactly what the grant covers: with `DispatchTo` on the
    /// team resource, the run proceeds past the chokepoint into the normal
    /// path (which an empty roster then fails *as a report*, not as an Err —
    /// reaching that failure is the proof of passage).
    #[test]
    fn a_granted_staffing_role_clears_the_dispatch_gate() {
        let ws = tempfile::tempdir().unwrap();
        write_mission(ws.path(), &chained_plan(&["a"]), &[]);
        assert!(crate::editor::company_org::save_company_org(
            ws.path(),
            &crate::automation::governor::test_governed_org()
        ));
        let report = run_mission(ws.path(), MISSION_REL, &live_lean()).unwrap();
        assert_eq!(report.dispatches_this_run, 0, "empty roster spends nothing");
    }

    /// Unattended safety cuts both ways: the gate blocks live spend, but the
    /// dry-run preview stays open under a denying org — and records no denial,
    /// because a preview denies nothing; it just spends nothing.
    #[test]
    fn a_dry_run_previews_even_in_a_denying_workspace() {
        let (ws, _plan) = mission_fixture(FULL_ROSTER);
        let mut org = crate::automation::governor::test_governed_org();
        org.grants.clear();
        assert!(crate::editor::company_org::save_company_org(
            ws.path(),
            &org
        ));
        let report = run_mission(ws.path(), MISSION_REL, &dry()).unwrap();
        assert!(report.dry_run);
        assert!(
            crate::automation::OpLogStore::open(ws.path())
                .load()
                .ops()
                .is_empty(),
            "a skipped gate must not mint audit ops"
        );
    }

    /// An approval-scoped dispatch cannot be satisfied by an unattended flag:
    /// the refusal says so and points back at the preview, per §6.
    #[test]
    fn a_dispatch_approval_policy_refuses_the_unattended_run() {
        let (ws, _plan) = mission_fixture(FULL_ROSTER);
        let mut org = crate::automation::governor::test_governed_org();
        org.policies
            .push(crate::editor::company_org::ApprovalPolicy {
                id: "dispatch_gate".into(),
                resource_id: crate::automation::team_resource("rewrite-team"),
                capability: crate::editor::company_org::Capability::DispatchTo,
                threshold: None,
                required_approvers: vec!["ai_lead".into()],
                quorum: 1,
                description: String::new(),
            });
        assert!(crate::editor::company_org::save_company_org(
            ws.path(),
            &org
        ));
        let err = run_mission(ws.path(), MISSION_REL, &live_lean()).unwrap_err();
        assert!(err.contains("approval"), "{err}");
        assert!(err.contains("dry_run=true"), "{err}");
    }

    /// An unreadable checkpoint must stop the mission rather than restart it.
    #[test]
    fn an_unreadable_checkpoint_stops_the_mission_instead_of_restarting_it() {
        let (ws, _plan) = mission_fixture(FULL_ROSTER);
        std::fs::write(
            ws.path().join(MISSION_REL).join(PROGRESS_FILE),
            "{ not json",
        )
        .unwrap();
        let err = run_mission(ws.path(), MISSION_REL, &dry()).unwrap_err();
        assert!(err.contains("cannot parse"), "{err}");
        assert!(err.contains(PROGRESS_FILE), "{err}");
    }

    #[test]
    fn a_mission_with_no_plan_reports_the_file_it_could_not_read() {
        let (ws, _plan) = mission_fixture(FULL_ROSTER);
        std::fs::remove_file(ws.path().join(MISSION_REL).join(PLAN_FILE)).unwrap();
        let err = run_mission(ws.path(), MISSION_REL, &dry()).unwrap_err();
        assert!(err.contains(PLAN_FILE), "{err}");
        assert!(err.contains("cannot read"), "{err}");
    }

    /// The dry run is not a stub: it walks the same schedule, resolves the same
    /// seats and reports the same caps, and dispatches nothing. An operator
    /// about to authorise 321 model-hours should be able to read it first.
    #[test]
    fn a_dry_run_schedules_every_sector_and_writes_nothing() {
        let (ws, plan) = mission_fixture(FULL_ROSTER);
        let mission = ws.path().join(MISSION_REL);
        let report = run_mission(ws.path(), MISSION_REL, &dry()).unwrap();

        assert_eq!(report.status, MissionStatus::Paused.as_str());
        assert!(report.dry_run);
        assert!(!report.resumed);
        assert_eq!(report.sectors_total, plan.sectors.len());
        assert_eq!(report.sectors_done_total, 0);
        assert_eq!(
            report.sectors_remaining,
            plan.sectors.len(),
            "a dry run completes nothing"
        );
        assert_eq!(
            report.waves_processed, report.waves_total,
            "every wave must be scheduled: {:?}",
            report.phase_log
        );
        assert_eq!(
            report.dispatches_this_run,
            units_of(&plan, &[]),
            "the dry run must forecast the real dispatch count"
        );
        assert_eq!(report.dispatch_secs_this_run, 0, "nothing was spent");
        // A full roster means no phase is skipped for want of a seat.
        assert!(
            !report.notes.iter().any(|n| n.contains("SKIPPED")),
            "{:?}",
            report.notes
        );
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("gates were not executed")),
            "{:?}",
            report.notes
        );
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("sweeps were not dispatched")),
            "{:?}",
            report.notes
        );
        assert!(
            report.notes.iter().any(|n| n.contains("forecast")),
            "a counted dispatch in a dry run must not read as spend: {:?}",
            report.notes
        );
        assert!(report.gates.is_empty(), "{:?}", report.gates);
        assert_eq!(report.gate_summary, "no gates ran");
        assert!(report.sweeps.is_empty());
        assert!(report.phases_done.is_empty());
        assert!(
            report
                .phase_log
                .iter()
                .any(|l| l.contains("design: would dispatch to Lead Architect")),
            "{:?}",
            report.phase_log
        );
        assert!(report.next_step.unwrap().contains("without dry_run"));
        assert!(
            report.feasibility.contains("dispatches"),
            "the arithmetic belongs in front of whoever reads the outcome: {}",
            report.feasibility
        );

        // And nothing at all was left on disk.
        assert!(
            !mission.join(PROGRESS_FILE).exists(),
            "a dry run must not create a resume record: the live run would skip every sector"
        );
        assert!(!mission.join(REPORT_FILE).exists());
        assert!(!mission.join(CONVENTIONS_FILE).exists());
        assert!(
            !ws.path().join(&report.output_root).exists(),
            "a dry run must not create the output tree"
        );
    }

    /// The requirement the whole module exists for: an interruption resumes
    /// rather than restarts. Proven at zero spend by writing the checkpoint a
    /// previous run would have left, then dry-running and counting what gets
    /// scheduled.
    #[test]
    fn a_resumed_run_skips_the_sectors_the_previous_run_finished() {
        let (ws, plan) = mission_fixture(FULL_ROSTER);
        let mission = ws.path().join(MISSION_REL);
        let finished = vec![plan.sectors[0].id.clone()];

        // What a real first run would have left behind: one sector ported and
        // checkpointed, then the process died mid-flight.
        let mut p = MissionProgress::default();
        p.mark_done(&finished[0]);
        p.mark_phase("design");
        p.dispatches_run = 7;
        p.status = MissionStatus::Running;
        p.save(&mission).unwrap();

        let report = run_mission(ws.path(), MISSION_REL, &dry()).unwrap();
        assert!(
            report.resumed,
            "the checkpoint must be recognised as a resume"
        );
        assert_eq!(report.sectors_done_total, 1);
        assert_eq!(
            report.sectors_done_this_run, 0,
            "a dry run completes nothing"
        );
        assert_eq!(report.sectors_remaining, plan.sectors.len() - 1);
        assert_eq!(
            report.dispatches_this_run,
            units_of(&plan, &finished),
            "the finished sector must not be dispatched again"
        );
        assert!(
            report
                .phase_log
                .iter()
                .any(|l| l.contains("design: already complete on a previous run")),
            "a completed phase is not re-dispatched either: {:?}",
            report.phase_log
        );

        // The wave log is the evidence: summing what each wave scheduled must
        // come to the sectors that are still outstanding.
        let scheduled: usize = report
            .phase_log
            .iter()
            .filter_map(|l| {
                l.strip_prefix("wave ")
                    .and_then(|rest| rest.split(": ").nth(1))
                    .and_then(|n| n.split(' ').next())
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .sum();
        assert_eq!(scheduled, plan.sectors.len() - 1, "{:?}", report.phase_log);
        assert!(
            !report.phase_log.iter().any(|l| l.contains(&finished[0])),
            "the finished sector must not appear in any wave: {:?}",
            report.phase_log
        );
    }

    /// A cap that stops the run is the difference between "unattended" and
    /// "unbounded". It has to halt cleanly and say which cap bit.
    #[test]
    fn a_dispatch_cap_halts_the_mission_and_says_which_cap_bit() {
        let (ws, plan) = mission_fixture(FULL_ROSTER);
        assert!(
            units_of(&plan, &[]) >= 2,
            "the fixture needs more than one dispatch for a cap to bite"
        );
        let cfg = MissionConfig {
            max_dispatches: 1,
            ..dry()
        };
        let report = run_mission(ws.path(), MISSION_REL, &cfg).unwrap();
        let reason = report.stop_reason.expect("the cap must be reported");
        assert!(reason.contains("dispatch cap"), "{reason}");
        assert_eq!(
            report.dispatches_this_run, 1,
            "one worker means no overshoot: {:?}",
            report.notes
        );
        assert_eq!(report.status, MissionStatus::Paused.as_str());
        assert!(report.sectors_remaining > 0);
        // The advice must name the cap. "Resume" would be a lie — this was a
        // dry run, so there is no checkpoint to resume from.
        let advice = report.next_step.unwrap();
        assert!(advice.contains("dispatch cap"), "{advice}");
        assert!(advice.contains("Raise that cap"), "{advice}");
        assert!(!advice.contains("resume"), "{advice}");
    }

    #[test]
    fn a_wave_cap_stops_between_waves_rather_than_mid_sector() {
        let ws = tempfile::tempdir().unwrap();
        let plan = chained_plan(&["a", "b", "c"]);
        write_mission(ws.path(), &plan, FULL_ROSTER);
        let cfg = MissionConfig {
            max_waves: 2,
            ..dry()
        };
        let report = run_mission(ws.path(), MISSION_REL, &cfg).unwrap();
        assert_eq!(report.waves_total, 3, "{:?}", report.phase_log);
        assert_eq!(report.waves_processed, 2, "{:?}", report.phase_log);
        let reason = report.stop_reason.clone().expect("a cap must be reported");
        assert!(reason.contains("wave cap"), "{reason}");
        assert_eq!(report.status, MissionStatus::Paused.as_str());
        let advice = report.next_step.unwrap();
        assert!(advice.contains("wave cap"), "{advice}");
        assert!(advice.contains("Raise that cap"), "{advice}");
    }

    /// Fix D. An interruption is not a failure, and the two must not share a
    /// code path: only `Failed` may consume one of a sector's
    /// `max_attempts_per_sector` retries. Running under a cap is the documented
    /// operating mode for a mission this size, so a sector merely interrupted by
    /// one has to stay pending with `attempts == 0` and be picked up by the next
    /// run. Burning a retry here — what `mark_failed` did before the
    /// `Interrupted` variant existed — would strand the sector once the cap bit
    /// twice, and the mission would report it given-up-on when nothing was ever
    /// wrong with it.
    ///
    /// Driven through `run_wave` with the dispatch budget pre-tripped rather
    /// than through a live `run_mission`: `run_sector` checks
    /// `budget.exhausted()` at the top of its unit loop, *before* it dispatches
    /// anything, so the first sector takes the `Interrupted` branch without ever
    /// calling `dispatch` — which is what keeps this off the network and
    /// deterministic. `record` deliberately does not set the stop flag, so the
    /// worker still enters the sector instead of breaking at the top of its own
    /// loop; the flag is set by the interrupt itself, which is why the later
    /// sectors are never popped.
    #[test]
    fn an_interrupted_sector_stays_pending_and_does_not_burn_a_retry() {
        let ws = tempfile::tempdir().unwrap();
        let mission = ws.path().join(MISSION_REL);
        std::fs::create_dir_all(&mission).unwrap();
        let plan = plan_with(&["a", "b", "c"]);
        let manifest = manifest("rust");
        let cfg = MissionConfig {
            dry_run: false,
            max_dispatches: 1,
            max_attempts_per_sector: 2,
            ..Default::default()
        };
        let shared = Shared::new(mission.clone(), MissionProgress::default(), &cfg);
        // Pre-trip the dispatch cap. `exhausted()` is checked before any
        // dispatch, so the first sector becomes `Interrupted` without a network
        // call; the other two are never popped because the interrupt also sets
        // the stop flag the worker loop checks first.
        shared.budget.record(0);
        assert!(
            shared.budget.exhausted().is_some(),
            "the budget must read as spent before the wave runs"
        );
        assert!(
            !shared.stopping(),
            "record() must not set the stop flag, or no sector is entered at all"
        );

        let (done, failed, interrupted, _notes) = run_wave(
            ws.path(),
            &manifest,
            &plan,
            plan.sectors.iter().map(|s| s.id.clone()).collect(),
            Some("Rust Port Engineer"),
            "clone",
            "out",
            "conventions.md",
            1,
            &shared,
        );

        assert_eq!(interrupted, 1, "the first sector hit the spent budget");
        assert_eq!(
            failed, 0,
            "an interruption must not be counted as a failure"
        );
        assert_eq!(done, 0, "nothing was ported");

        // The heart of Fix D: no retry was consumed and nothing is marked done,
        // so the resume filter `!is_done && attempts < max_attempts` still keeps
        // every sector for the next run.
        let p = shared.progress.lock().unwrap_or_else(|e| e.into_inner());
        for s in &plan.sectors {
            assert_eq!(p.attempts(&s.id), 0, "{} must not burn a retry", s.id);
            assert!(!p.is_done(&s.id), "{} must stay pending", s.id);
            assert!(
                p.attempts(&s.id) < cfg.max_attempts_per_sector,
                "{} would be skipped on resume",
                s.id
            );
        }
    }

    /// The failure mode that makes an unattended run dangerous is reporting
    /// success while having done nothing.
    #[test]
    fn a_team_with_no_porter_fails_loudly_rather_than_reporting_a_clean_run() {
        let (ws, plan) = mission_fixture(&["Documentation Engineer"]);
        let report = run_mission(ws.path(), MISSION_REL, &dry()).unwrap();
        assert_eq!(report.status, MissionStatus::Failed.as_str());
        assert_eq!(report.sectors_done_total, 0);
        assert_eq!(report.sectors_remaining, plan.sectors.len());
        assert_eq!(report.dispatches_this_run, 0, "nothing was spent");
        assert!(
            report.notes.iter().any(|n| n.contains("no porting seat")),
            "{:?}",
            report.notes
        );
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("sweeps were skipped")),
            "{:?}",
            report.notes
        );
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("parity review") && n.contains("skipped")),
            "{:?}",
            report.notes
        );
        assert!(report.next_step.unwrap().contains("add_team_member"));
    }

    /// A team with no members at all: every phase is skipped, nothing dispatches,
    /// and the report says so eight times over.
    #[test]
    fn an_empty_roster_leaves_every_phase_unowned() {
        let ws = tempfile::tempdir().unwrap();
        write_mission(ws.path(), &chained_plan(&["a"]), &[]);
        let report = run_mission(ws.path(), MISSION_REL, &dry()).unwrap();
        assert_eq!(report.status, MissionStatus::Failed.as_str());
        assert_eq!(
            report
                .notes
                .iter()
                .filter(|n| n.contains("SKIPPED"))
                .count(),
            8
        );
        assert_eq!(report.dispatches_this_run, 0);
    }

    /// The live path is the one that has to leave a resumable record behind.
    /// Driven with an empty roster, no gates and no sweeps so that nothing is
    /// dispatched and nothing is compiled: what is under test is the persistence.
    #[test]
    fn a_live_run_writes_the_checkpoint_and_the_report_it_promises() {
        let ws = tempfile::tempdir().unwrap();
        let plan = chained_plan(&["a", "b"]);
        write_mission(ws.path(), &plan, &[]);
        let cfg = MissionConfig {
            run_gates: false,
            run_sweeps: false,
            run_documentation: false,
            parallelism: 1,
            ..Default::default()
        };
        let report = run_mission(ws.path(), MISSION_REL, &cfg).unwrap();
        let mission = ws.path().join(MISSION_REL);

        assert!(
            mission.join(PROGRESS_FILE).is_file(),
            "a live run must checkpoint"
        );
        assert!(
            mission.join(REPORT_FILE).is_file(),
            "a live run must leave its report on disk"
        );
        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(mission.join(REPORT_FILE)).unwrap())
                .unwrap();
        assert_eq!(on_disk["status"], report.status);
        assert_eq!(
            report.status,
            MissionStatus::Failed.as_str(),
            "no porter, no port"
        );
        let p = MissionProgress::load(&mission)
            .unwrap()
            .expect("just written");
        assert_eq!(p.status, MissionStatus::Failed);
        assert_eq!(p.version, PROGRESS_VERSION);
        assert_eq!(p.dispatches_run, 0, "an empty roster dispatches nothing");
        // No temp sibling left behind by the atomic writes.
        assert!(!mission.join("progress.json.tmp").exists());
        assert!(!mission.join("report.json.tmp").exists());
        // The output root exists and is buildable, which is what makes the next
        // run's gates meaningful rather than skipped.
        let out = ws.path().join(&report.output_root);
        assert!(out.join("Cargo.toml").is_file(), "{out:?}");
        assert!(out.join("src").join("lib.rs").is_file());
        assert!(
            report.notes.iter().any(|n| n.contains("crate skeleton")),
            "{:?}",
            report.notes
        );
        // And it is resumable: a second invocation says so.
        let again = run_mission(ws.path(), MISSION_REL, &cfg).unwrap();
        assert!(again.resumed);
        assert_eq!(again.sectors_done_total, 0);
    }

    /// The report is the only artifact an operator sees from an unattended run,
    /// so it has to be readable back and carry the things that make it honest.
    #[test]
    fn the_report_round_trips_and_carries_what_an_operator_needs() {
        let (ws, plan) = mission_fixture(FULL_ROSTER);
        let report = run_mission(ws.path(), MISSION_REL, &dry()).unwrap();
        let v = report.to_json();
        assert_eq!(v["status"], "paused");
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["sectors_total"], plan.sectors.len());
        assert_eq!(v["feasibility"], report.feasibility);
        assert_eq!(v["mission_dir"], MISSION_REL);
        assert!(v["caps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.as_str().unwrap().contains("uncapped")));
        // `stop_reason` is skipped when absent, so a reader of the JSON cannot
        // mistake "no reason" for an empty explanation.
        assert!(v.get("stop_reason").is_none(), "{v}");
        assert!(v.get("next_step").is_some());
        // A report that cannot be read back cannot be audited later.
        let back: MissionReport = serde_json::from_value(v).unwrap();
        assert_eq!(back, report);
        assert_eq!(back.status, MissionStatus::Paused.as_str());
    }
}
