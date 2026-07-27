//! Unattended execution: triggers.
//!
//! A [`TriggerRegistry`] holds named [`Trigger`]s that fire a [`TriggerAction`]
//! (run a workflow or dispatch an agent prompt) when their [`TriggerKind`]
//! condition is met. The registry persists to `.velocity/triggers.json` and is
//! evaluated by the headless daemon loop (`velocity_mcp --daemon`) as well as
//! the Triggers UI panel.
//!
//! Time is represented as Unix epoch seconds so evaluation is deterministic and
//! testable — `due_triggers(now)` takes an explicit timestamp. Schedule specs
//! are dependency-free: interval forms (`"30s"`, `"5m"`, `"1h"`, `"2d"`) and a
//! wall-clock daily form (`"daily@HH:MM"`, interpreted in UTC).

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const TRIGGERS_FILE: &str = "triggers.json";
const SECS_PER_DAY: u64 = 86_400;

/// What condition causes a trigger to fire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerKind {
    /// Fire on a recurring schedule (see [`parse_schedule`] for accepted specs).
    Schedule { interval: String },
    /// Fire when files under `path` matching `glob` change (evaluated by the
    /// daemon at runtime; not time-due).
    FileWatch { path: String, glob: String },
    /// Fire when an inbound webhook presents `token` (external dispatch).
    Webhook { token: String },
    /// Fire only when explicitly run.
    Manual,
}

/// What a trigger does when it fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerAction {
    /// Run a workflow by id (see the workflow composer).
    RunWorkflow { workflow_id: String },
    /// Dispatch a free-form prompt to a headless agent.
    AgentPrompt { prompt: String },
}

/// A single configured trigger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub id: String,
    pub name: String,
    pub kind: TriggerKind,
    pub action: TriggerAction,
    pub enabled: bool,
    /// Epoch seconds of the last time this trigger fired, if ever.
    pub last_run: Option<u64>,
}

impl Trigger {
    pub fn new(id: impl Into<String>, name: impl Into<String>, kind: TriggerKind, action: TriggerAction) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind,
            action,
            enabled: true,
            last_run: None,
        }
    }

    /// Whether this trigger is a schedule that is due to fire at `now`.
    /// Only enabled `Schedule` triggers are time-due; other kinds fire via
    /// external dispatch or manual run.
    pub fn is_due(&self, now: u64) -> bool {
        if !self.enabled {
            return false;
        }
        let TriggerKind::Schedule { interval } = &self.kind else {
            return false;
        };
        let Some(schedule) = parse_schedule(interval) else {
            return false;
        };
        schedule.is_due(self.last_run, now)
    }

    /// Seconds until this schedule trigger next fires (0 = due now). Returns
    /// `None` for non-schedule kinds or unparseable specs.
    pub fn seconds_until_due(&self, now: u64) -> Option<u64> {
        let TriggerKind::Schedule { interval } = &self.kind else {
            return None;
        };
        parse_schedule(interval).map(|s| s.seconds_until_due(self.last_run, now))
    }
}

/// A parsed schedule specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// Fire every N seconds.
    Every(u64),
    /// Fire once per day at N seconds after UTC midnight.
    DailyAt(u32),
}

impl Schedule {
    fn is_due(self, last_run: Option<u64>, now: u64) -> bool {
        match self {
            Schedule::Every(period) => match last_run {
                None => true,
                Some(prev) => now.saturating_sub(prev) >= period.max(1),
            },
            Schedule::DailyAt(target) => {
                let secs_of_day = (now % SECS_PER_DAY) as u32;
                if secs_of_day < target {
                    return false;
                }
                match last_run {
                    None => true,
                    // Due only if we have not already fired today.
                    Some(prev) => prev / SECS_PER_DAY < now / SECS_PER_DAY,
                }
            }
        }
    }

    fn seconds_until_due(self, last_run: Option<u64>, now: u64) -> u64 {
        if self.is_due(last_run, now) {
            return 0;
        }
        match self {
            Schedule::Every(period) => match last_run {
                None => 0,
                Some(prev) => (prev + period.max(1)).saturating_sub(now),
            },
            Schedule::DailyAt(target) => {
                let secs_of_day = now % SECS_PER_DAY;
                let target = target as u64;
                if secs_of_day < target {
                    target - secs_of_day
                } else {
                    // Next occurrence is tomorrow at target.
                    SECS_PER_DAY - secs_of_day + target
                }
            }
        }
    }
}

/// Parse a schedule spec into a [`Schedule`]. Accepts:
/// - `"<n>s"`, `"<n>m"`, `"<n>h"`, `"<n>d"` (seconds/minutes/hours/days)
/// - a bare integer (seconds)
/// - `"daily@HH:MM"` (UTC wall-clock)
pub fn parse_schedule(spec: &str) -> Option<Schedule> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    if let Some(rest) = spec.strip_prefix("daily@") {
        let (hh, mm) = rest.split_once(':')?;
        let h: u32 = hh.trim().parse().ok()?;
        let m: u32 = mm.trim().parse().ok()?;
        if h >= 24 || m >= 60 {
            return None;
        }
        return Some(Schedule::DailyAt(h * 3600 + m * 60));
    }

    let (num_part, mult) = match spec.chars().last()? {
        's' => (&spec[..spec.len() - 1], 1u64),
        'm' => (&spec[..spec.len() - 1], 60),
        'h' => (&spec[..spec.len() - 1], 3600),
        'd' => (&spec[..spec.len() - 1], SECS_PER_DAY),
        c if c.is_ascii_digit() => (spec, 1),
        _ => return None,
    };
    let n: u64 = num_part.trim().parse().ok()?;
    if n == 0 {
        return None;
    }
    Some(Schedule::Every(n * mult))
}

/// The persisted set of triggers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriggerRegistry {
    pub triggers: Vec<Trigger>,
}

impl TriggerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from `.velocity/triggers.json`; a missing/corrupt file yields empty.
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_root.join(".velocity").join(TRIGGERS_FILE);
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::new();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    /// Persist to `.velocity/triggers.json`.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create .velocity dir: {e}"))?;
        let json =
            serde_json::to_vec_pretty(self).map_err(|e| format!("triggers serialize failed: {e}"))?;
        std::fs::write(dir.join(TRIGGERS_FILE), json)
            .map_err(|e| format!("cannot write triggers: {e}"))
    }

    pub fn add(&mut self, trigger: Trigger) {
        self.triggers.push(trigger);
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.triggers.len();
        self.triggers.retain(|t| t.id != id);
        self.triggers.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&Trigger> {
        self.triggers.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Trigger> {
        self.triggers.iter_mut().find(|t| t.id == id)
    }

    /// Toggle a trigger's enabled state. Returns the new state, if found.
    pub fn toggle(&mut self, id: &str) -> Option<bool> {
        self.get_mut(id).map(|t| {
            t.enabled = !t.enabled;
            t.enabled
        })
    }

    pub fn is_empty(&self) -> bool {
        self.triggers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.triggers.len()
    }

    /// Ids of enabled schedule triggers that are due to fire at `now`.
    pub fn due_triggers(&self, now: u64) -> Vec<String> {
        self.triggers
            .iter()
            .filter(|t| t.is_due(now))
            .map(|t| t.id.clone())
            .collect()
    }

    /// Record that a trigger fired at `now`.
    pub fn mark_run(&mut self, id: &str, now: u64) {
        if let Some(t) = self.get_mut(id) {
            t.last_run = Some(now);
        }
    }
}

/// Current wall-clock time as Unix epoch seconds.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule_trigger(id: &str, interval: &str) -> Trigger {
        Trigger::new(
            id,
            id,
            TriggerKind::Schedule {
                interval: interval.to_string(),
            },
            TriggerAction::AgentPrompt {
                prompt: "do work".to_string(),
            },
        )
    }

    #[test]
    fn parse_interval_forms() {
        assert_eq!(parse_schedule("30s"), Some(Schedule::Every(30)));
        assert_eq!(parse_schedule("5m"), Some(Schedule::Every(300)));
        assert_eq!(parse_schedule("1h"), Some(Schedule::Every(3600)));
        assert_eq!(parse_schedule("2d"), Some(Schedule::Every(172_800)));
        assert_eq!(parse_schedule("45"), Some(Schedule::Every(45)));
        assert_eq!(parse_schedule("daily@09:30"), Some(Schedule::DailyAt(34_200)));
        assert_eq!(parse_schedule(""), None);
        assert_eq!(parse_schedule("0s"), None);
        assert_eq!(parse_schedule("daily@25:00"), None);
        assert_eq!(parse_schedule("abc"), None);
    }

    #[test]
    fn every_schedule_due_after_period() {
        let mut t = schedule_trigger("a", "60s");
        // Never run → due immediately.
        assert!(t.is_due(1_000));
        t.last_run = Some(1_000);
        // Not enough time elapsed.
        assert!(!t.is_due(1_030));
        // Period elapsed.
        assert!(t.is_due(1_060));
        assert!(t.is_due(2_000));
    }

    #[test]
    fn daily_schedule_fires_once_per_day() {
        let t_spec = "daily@01:00"; // 3600 secs after midnight
        let mut t = schedule_trigger("d", t_spec);
        // 00:30 on day 0 → before target, not due.
        assert!(!t.is_due(1_800));
        // 02:00 on day 0 → after target, never run → due.
        assert!(t.is_due(7_200));
        // Mark as run at 02:00 day 0.
        t.last_run = Some(7_200);
        // 03:00 same day → already ran today, not due.
        assert!(!t.is_due(10_800));
        // 01:30 next day → due again.
        assert!(t.is_due(SECS_PER_DAY + 5_400));
    }

    #[test]
    fn disabled_and_manual_never_time_due() {
        let mut t = schedule_trigger("a", "1s");
        t.enabled = false;
        assert!(!t.is_due(10_000));

        let manual = Trigger::new(
            "m",
            "manual",
            TriggerKind::Manual,
            TriggerAction::AgentPrompt {
                prompt: "x".into(),
            },
        );
        assert!(!manual.is_due(10_000));
        assert_eq!(manual.seconds_until_due(10_000), None);
    }

    #[test]
    fn due_triggers_selects_correct_ids() {
        let mut reg = TriggerRegistry::new();
        reg.add(schedule_trigger("due", "30s"));
        let mut not_yet = schedule_trigger("waiting", "1h");
        not_yet.last_run = Some(1_000);
        reg.add(not_yet);
        reg.add(Trigger::new(
            "manual",
            "manual",
            TriggerKind::Manual,
            TriggerAction::AgentPrompt { prompt: "x".into() },
        ));

        let due = reg.due_triggers(1_100);
        assert_eq!(due, vec!["due".to_string()]);

        reg.mark_run("due", 1_100);
        assert!(reg.due_triggers(1_110).is_empty());
        assert!(reg.due_triggers(1_200).contains(&"due".to_string()));
    }

    #[test]
    fn seconds_until_due_countdown() {
        let mut t = schedule_trigger("a", "100s");
        t.last_run = Some(1_000);
        assert_eq!(t.seconds_until_due(1_040), Some(60));
        assert_eq!(t.seconds_until_due(1_100), Some(0));
        assert_eq!(t.seconds_until_due(1_200), Some(0));
    }

    #[test]
    fn registry_round_trip_and_mutation() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reg = TriggerRegistry::new();
        reg.add(schedule_trigger("nightly", "daily@00:00"));
        reg.add(Trigger::new(
            "hook",
            "webhook",
            TriggerKind::Webhook { token: "secret".into() },
            TriggerAction::RunWorkflow { workflow_id: "wf1".into() },
        ));
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.toggle("nightly"), Some(false));
        reg.save(tmp.path()).expect("save");

        let loaded = TriggerRegistry::load(tmp.path());
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get("nightly").unwrap().enabled, false);
        assert!(matches!(
            loaded.get("hook").unwrap().action,
            TriggerAction::RunWorkflow { .. }
        ));

        let mut reg2 = loaded;
        assert!(reg2.remove("hook"));
        assert_eq!(reg2.len(), 1);
    }
}
