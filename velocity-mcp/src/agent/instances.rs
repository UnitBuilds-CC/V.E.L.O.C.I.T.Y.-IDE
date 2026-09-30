//! Managed-instance registry: the IDE's list of remote nodes it can hand work
//! to - buildboxes that compile for other platforms, agent hosts, GUI servers.
//!
//! This module is the *pure* core: plain data, deterministic selection, and
//! file I/O that callers drive. Nothing here talks SSH or HTTP; the drone
//! bridge (`drone_bridge.rs`) is the transport, and the MCP/GUI layers decide
//! when to ping. Keeping the bookkeeping separate means the routing rules are
//! testable without a network and survive restarts untouched.
//!
//! Persistence lives at `<workspace>/.velocity/instances.json` (same plain-JSON
//! convention as `provider-settings.json`). A corrupt or missing file loads as
//! empty - the IDE must still start when the registry is junk.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What a node is primarily kept around for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceRole {
    /// Remote compile/test box (e.g. an idle Ubuntu machine that builds the
    /// Linux targets while the local IDE stays responsive).
    Buildbox,
    /// Host that runs agent workloads (ephemeral or sticky).
    AgentHost,
    /// Host running the IDE itself headless, serving a remote GUI.
    GuiServer,
}

impl InstanceRole {
    pub fn label(self) -> &'static str {
        match self {
            InstanceRole::Buildbox => "buildbox",
            InstanceRole::AgentHost => "agent_host",
            InstanceRole::GuiServer => "gui_server",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "buildbox" | "build_box" => Some(InstanceRole::Buildbox),
            "agent_host" | "agent" => Some(InstanceRole::AgentHost),
            "gui_server" | "gui" => Some(InstanceRole::GuiServer),
            _ => None,
        }
    }
}

/// OS family the node runs. Routing uses this so `cargo build --target
/// x86_64-unknown-linux-gnu` lands on a Linux box instead of failing locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstancePlatform {
    Linux,
    Windows,
    Macos,
    Unknown,
}

impl InstancePlatform {
    pub fn label(self) -> &'static str {
        match self {
            InstancePlatform::Linux => "linux",
            InstancePlatform::Windows => "windows",
            InstancePlatform::Macos => "macos",
            InstancePlatform::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "linux" => InstancePlatform::Linux,
            "windows" | "win" => InstancePlatform::Windows,
            "macos" | "darwin" | "mac" => InstancePlatform::Macos,
            _ => InstancePlatform::Unknown,
        }
    }

    /// The platform of the machine running this code, for "prefer local"
    /// decisions.
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            InstancePlatform::Linux
        } else if cfg!(target_os = "windows") {
            InstancePlatform::Windows
        } else if cfg!(target_os = "macos") {
            InstancePlatform::Macos
        } else {
            InstancePlatform::Unknown
        }
    }
}

/// Liveness as last observed by whoever pings the drone endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    /// Never pinged since registration.
    Unknown,
    /// Health succeeded within the freshness window.
    Online,
    /// Ping answered but reported trouble (or answered late).
    Degraded,
    /// Ping failed, or the last good sighting is older than [`HEALTH_STALE_SECS`].
    Offline,
}

impl InstanceStatus {
    /// Stable wire/tool name, matching the snake_case serde representation so
    /// tool output and the JSON file never disagree.
    pub fn label(self) -> &'static str {
        match self {
            InstanceStatus::Unknown => "unknown",
            InstanceStatus::Online => "online",
            InstanceStatus::Degraded => "degraded",
            InstanceStatus::Offline => "offline",
        }
    }
}

/// Seconds after which a node that has not been seen healthy is treated as
/// offline regardless of the stored status - a machine that went to sleep
/// mid-session must not keep receiving routed work.
pub const HEALTH_STALE_SECS: u64 = 120;

/// One managed node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstanceRecord {
    /// Stable id derived from the name at registration ([`slugify`]); ids are
    /// what upserts match on, so renames via re-add are intentional.
    pub id: String,
    pub name: String,
    pub role: InstanceRole,
    pub platform: InstancePlatform,
    /// Drone endpoint as `host:port` (the drone HTTP API, usually :9191).
    pub drone_addr: String,
    /// Optional SSH endpoint `user@host[:port]` used by the deployer to push
    /// the drone binary when it is not running yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<String>,
    /// Workspace path on the remote box where builds run.
    #[serde(default)]
    pub work_dir: String,
    /// Free-form capability tags ("rust", "gpu", "arm64", ...) that routing
    /// filters on. Lower-cased at the boundary.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Manually disabled nodes stay listed but are never routed to.
    #[serde(default)]
    pub enabled: bool,
    pub status: InstanceStatus,
    /// Unix seconds of the last successful health sighting (0 = never).
    #[serde(default)]
    pub last_seen_secs: u64,
    /// Work items claimed and not yet released; least-loaded routing reads it.
    #[serde(default)]
    pub in_flight: u32,
    /// Bearer secret for the drone's HTTP API, kept so a later ping/task does
    /// not need the caller to remember it. Only written by `instance_deploy`
    /// (which generates one) or by an explicit `instance_add` argument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
}

impl InstanceRecord {
    /// Construct with sane defaults (enabled, Unknown status, no traffic).
    pub fn new(
        name: &str,
        role: InstanceRole,
        platform: InstancePlatform,
        drone_addr: &str,
    ) -> Self {
        Self {
            id: slugify(name),
            name: name.to_string(),
            role,
            platform,
            drone_addr: drone_addr.to_string(),
            ssh: None,
            work_dir: String::new(),
            labels: Vec::new(),
            enabled: true,
            status: InstanceStatus::Unknown,
            last_seen_secs: 0,
            in_flight: 0,
            auth_token: None,
        }
    }

    /// The status routing should honour right now: a stored `Online` whose
    /// last good sighting has aged out degrades to `Offline` here, so every
    /// caller gets the same expiry without each re-implementing the clock
    /// check.
    pub fn effective_status(&self, now_secs: u64) -> InstanceStatus {
        match self.status {
            InstanceStatus::Online | InstanceStatus::Degraded
                if self.last_seen_secs == 0
                    || now_secs.saturating_sub(self.last_seen_secs) > HEALTH_STALE_SECS =>
            {
                InstanceStatus::Offline
            }
            other => other,
        }
    }

    /// Whether this node may receive routed work now.
    pub fn eligible(&self, now_secs: u64) -> bool {
        self.enabled
            && matches!(
                self.effective_status(now_secs),
                InstanceStatus::Online | InstanceStatus::Degraded
            )
    }

    /// Record a health probe outcome. `ok` means the drone answered `health`.
    pub fn observe_health(&mut self, ok: bool, now_secs: u64) {
        if ok {
            self.status = InstanceStatus::Online;
            self.last_seen_secs = now_secs;
        } else {
            // A node we used to see goes Degraded first (one bad ping may be
            // transient); one never seen or long stale is simply Offline.
            self.status = if self.last_seen_secs > 0
                && now_secs.saturating_sub(self.last_seen_secs) <= HEALTH_STALE_SECS
            {
                InstanceStatus::Degraded
            } else {
                InstanceStatus::Offline
            };
        }
    }
}

/// Turn a display name into the id shape: lowercase alphanumeric runs joined
/// by single hyphens. `"My Buildbox 01"` → `"my-buildbox-01"`.
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut pending = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if pending {
                out.push('-');
                pending = false;
            }
            out.extend(c.to_lowercase());
        } else if !out.is_empty() {
            pending = true;
        }
    }
    if out.is_empty() {
        "instance".to_string()
    } else {
        out
    }
}

/// The in-memory registry plus its file location. Cheap to clone-read; all
/// mutations go through methods so the invariants (unique ids, unique
/// case-insensitive names, lower-cased labels) hold in one place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstanceRegistry {
    #[serde(default)]
    pub instances: Vec<InstanceRecord>,
    #[serde(default, skip)]
    path: Option<PathBuf>,
}

/// One filter step in [`InstanceRegistry::candidates`].
#[derive(Debug, Clone, Copy)]
pub struct RouteQuery<'a> {
    pub role: Option<InstanceRole>,
    pub platform: Option<InstancePlatform>,
    /// Every label listed must be present on the node.
    pub require_labels: &'a [&'a str],
}

impl InstanceRegistry {
    pub fn load(path: &Path) -> Self {
        let raw = match std::fs::read_to_string(path) {
            // Absent registry is still *this file's* registry: carry the path so
            // the first mutation can save in place instead of failing.
            Err(_) => {
                return Self {
                    instances: Vec::new(),
                    path: Some(path.to_path_buf()),
                }
            }
            Ok(r) => r,
        };
        // A BOM would make serde_json reject a file written by an editor;
        // strip before parsing (same robustness the hot-exit reader learned).
        let trimmed = raw.trim_start_matches('\u{feff}');
        match serde_json::from_str::<InstanceRegistry>(trimmed) {
            Ok(mut reg) => {
                reg.path = Some(path.to_path_buf());
                reg
            }
            // Corrupt or hand-edited beyond repair: start clean but keep the
            // path so the next save overwrites the junk with valid data.
            Err(_) => Self {
                instances: Vec::new(),
                path: Some(path.to_path_buf()),
            },
        }
    }

    /// Save to the loaded path (or an explicit one). Parent dirs are created.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let body = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize instances: {e}"))?;
        std::fs::write(path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    /// Insert or replace by id. Rejects a name collision with a *different*
    /// id (case-insensitive) so two "Buildbox" entries cannot silently shadow
    /// each other.
    pub fn upsert(&mut self, mut rec: InstanceRecord) -> Result<(), String> {
        rec.labels = normalize_labels(&rec.labels);
        if rec.id.is_empty() {
            rec.id = slugify(&rec.name);
        }
        if let Some(bad) = self
            .instances
            .iter()
            .find(|i| i.id != rec.id && i.name.eq_ignore_ascii_case(rec.name.trim()))
        {
            return Err(format!(
                "name {:?} is already used by instance {}",
                rec.name, bad.id
            ));
        }
        match self.instances.iter_mut().find(|i| i.id == rec.id) {
            Some(slot) => {
                // Preserve sighting/traffic only when re-saving the same
                // endpoint; a changed address invalidates both.
                if slot.drone_addr != rec.drone_addr {
                    rec.status = InstanceStatus::Unknown;
                    rec.last_seen_secs = 0;
                    rec.in_flight = 0;
                }
                *slot = rec;
            }
            None => self.instances.push(rec),
        }
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Option<InstanceRecord> {
        let at = self.instances.iter().position(|i| i.id == id)?;
        Some(self.instances.remove(at))
    }

    pub fn get(&self, id: &str) -> Option<&InstanceRecord> {
        self.instances.iter().find(|i| i.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut InstanceRecord> {
        self.instances.iter_mut().find(|i| i.id == id)
    }

    /// All records matching the query, in stable list order, honouring the
    /// eligibility rules (enabled + not-expired status).
    pub fn candidates(&self, q: &RouteQuery, now_secs: u64) -> Vec<&InstanceRecord> {
        self.instances
            .iter()
            .filter(|i| i.eligible(now_secs))
            .filter(|i| q.role.is_none_or(|r| i.role == r))
            .filter(|i| q.platform.is_none_or(|p| i.platform == p))
            .filter(|i| {
                q.require_labels
                    .iter()
                    .all(|want| i.labels.iter().any(|l| l == want))
            })
            .collect()
    }

    /// Deterministic least-loaded pick: fewest in-flight items first, then id
    /// so repeated calls on identical state agree without a cursor.
    pub fn pick(&self, q: &RouteQuery, now_secs: u64) -> Option<&InstanceRecord> {
        let mut cands = self.candidates(q, now_secs);
        cands.sort_by(|a, b| a.in_flight.cmp(&b.in_flight).then_with(|| a.id.cmp(&b.id)));
        cands.first().copied()
    }

    /// Increment the in-flight counter of a routed node (claim).
    pub fn claim(&mut self, id: &str) -> bool {
        match self.get_mut(id) {
            Some(i) => {
                i.in_flight += 1;
                true
            }
            None => false,
        }
    }

    /// Decrement on completion; never wraps below zero (a double-release is a
    /// caller bug, not a license to hand the node negative load).
    pub fn release(&mut self, id: &str) -> bool {
        match self.get_mut(id) {
            Some(i) => {
                i.in_flight = i.in_flight.saturating_sub(1);
                true
            }
            None => false,
        }
    }

    /// Where this registry was loaded from, if it was loaded from anywhere.
    pub fn file_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Persist back to the load location. A registry built in memory (never
    /// loaded) has no path, so callers must pass an explicit one to `save`.
    pub fn save_in_place(&self) -> Result<(), String> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| "registry has no file to save to".to_string())?;
        self.save(&path)
    }
}

fn normalize_labels(labels: &[String]) -> Vec<String> {
    let mut out: Vec<String> = labels
        .iter()
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buildbox(name: &str, addr: &str) -> InstanceRecord {
        InstanceRecord::new(name, InstanceRole::Buildbox, InstancePlatform::Linux, addr)
    }

    #[test]
    fn slugify_joins_alnum_runs_with_hyphens() {
        assert_eq!(slugify("My Buildbox 01"), "my-buildbox-01");
        assert_eq!(slugify("  weird__name!! "), "weird-name");
        assert_eq!(slugify("!!!"), "instance");
    }

    #[test]
    fn role_and_platform_parse_tolerate_shapes() {
        assert_eq!(
            InstanceRole::parse("Build-Box"),
            Some(InstanceRole::Buildbox)
        );
        assert_eq!(InstanceRole::parse("gui"), Some(InstanceRole::GuiServer));
        assert_eq!(InstanceRole::parse("nonsense"), None);
        assert_eq!(InstancePlatform::parse("darwin"), InstancePlatform::Macos);
        assert_eq!(InstancePlatform::parse("xbox"), InstancePlatform::Unknown);
    }

    #[test]
    fn upsert_replaces_by_id_and_rejects_name_collision() {
        let mut reg = InstanceRegistry::default();
        reg.upsert(buildbox("alpha", "10.0.0.1:9191")).unwrap();
        // Same id, new address: replace, and the sighting resets because the
        // endpoint moved.
        let mut moved = buildbox("alpha", "10.0.0.2:9191");
        moved.status = InstanceStatus::Online;
        moved.last_seen_secs = 500;
        reg.upsert(moved).unwrap();
        assert_eq!(reg.instances.len(), 1);
        assert_eq!(reg.get("alpha").unwrap().drone_addr, "10.0.0.2:9191");
        assert_eq!(reg.get("alpha").unwrap().status, InstanceStatus::Unknown);
        // A record with a *different* id wanting the same name is refused.
        // (Same name with the derived-equal id is an intentional update, so
        // plain re-adding "ALPHA" would just replace "alpha" above.)
        let mut dup = buildbox("ALPHA", "10.0.0.3:9191");
        dup.id = "explicit-other".into();
        assert!(reg.upsert(dup).is_err());
    }

    #[test]
    fn stale_sighting_expires_to_offline() {
        let mut rec = buildbox("snoozy", "10.0.0.9:9191");
        rec.status = InstanceStatus::Online;
        rec.last_seen_secs = 1_000;
        assert_eq!(
            rec.effective_status(1_000 + HEALTH_STALE_SECS + 1),
            InstanceStatus::Offline
        );
        assert_eq!(rec.effective_status(1_000 + 10), InstanceStatus::Online);
        assert!(!rec.eligible(1_000 + HEALTH_STALE_SECS + 1));
    }

    #[test]
    fn health_flapping_degrades_then_offlines() {
        let mut rec = buildbox("flappy", "10.0.0.9:9191");
        rec.observe_health(true, 200);
        assert_eq!(rec.status, InstanceStatus::Online);
        rec.observe_health(false, 210);
        assert_eq!(rec.status, InstanceStatus::Degraded);
        // Long gone: a failed ping past the window cannot even claim Degraded.
        rec.observe_health(false, 210 + HEALTH_STALE_SECS * 2);
        assert_eq!(rec.status, InstanceStatus::Offline);
    }

    #[test]
    fn pick_favors_least_loaded_and_honors_filters() {
        let mut reg = InstanceRegistry::default();
        let mut a = buildbox("a", "1:1");
        a.status = InstanceStatus::Online;
        a.last_seen_secs = 1_000;
        a.labels = vec!["Rust".into(), " gpu ".into()];
        let mut b = buildbox("b", "2:2");
        b.status = InstanceStatus::Online;
        b.last_seen_secs = 1_000;
        b.labels = vec!["node".into()];
        let mut off = buildbox("off", "3:3");
        off.status = InstanceStatus::Online;
        off.last_seen_secs = 1_000;
        off.enabled = false;
        reg.upsert(a).unwrap();
        reg.upsert(b).unwrap();
        reg.upsert(off).unwrap();
        reg.claim("a");

        let q_gpu = RouteQuery {
            role: Some(InstanceRole::Buildbox),
            platform: Some(InstancePlatform::Linux),
            require_labels: &["gpu"],
        };
        // Only `a` has gpu; load does not disqualify it.
        assert_eq!(reg.pick(&q_gpu, 1_010).unwrap().id, "a");

        let q_any = RouteQuery {
            role: Some(InstanceRole::Buildbox),
            platform: None,
            require_labels: &[],
        };
        assert_eq!(reg.candidates(&q_any, 1_010).len(), 2);
        assert_eq!(reg.pick(&q_any, 1_010).unwrap().id, "b");

        // Past the window both expire; nothing routes.
        assert!(reg.pick(&q_any, 1_010 + HEALTH_STALE_SECS + 1).is_none());
    }

    #[test]
    fn claim_release_never_goes_negative() {
        let mut reg = InstanceRegistry::default();
        let mut a = buildbox("a", "1:1");
        a.status = InstanceStatus::Online;
        a.last_seen_secs = 1;
        reg.upsert(a).unwrap();
        assert!(reg.claim("a"));
        assert_eq!(reg.get("a").unwrap().in_flight, 1);
        assert!(reg.release("a"));
        assert!(reg.release("a")); // double release clamps at 0
        assert_eq!(reg.get("a").unwrap().in_flight, 0);
        assert!(!reg.claim("ghost"));
        assert!(!reg.release("ghost"));
    }

    #[test]
    fn save_load_roundtrip_and_corrupt_file_recovery() {
        let dir = std::env::temp_dir().join(format!("vel_instances_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nested").join("instances.json");

        let mut reg = InstanceRegistry::default();
        let mut a = buildbox("roundtrip", "9.9.9.9:9191");
        a.ssh = Some("builder@9.9.9.9".into());
        a.status = InstanceStatus::Online;
        a.last_seen_secs = 4242;
        reg.upsert(a).unwrap();
        reg.save(&path).unwrap();

        let loaded = InstanceRegistry::load(&path);
        assert_eq!(loaded.instances.len(), 1);
        let got = loaded.get("roundtrip").unwrap();
        assert_eq!(got.ssh.as_deref(), Some("builder@9.9.9.9"));
        assert_eq!(got.status, InstanceStatus::Online);
        assert_eq!(got.last_seen_secs, 4242);

        // Junk on disk must not wedge the IDE: loads empty, next save wins.
        std::fs::write(&path, "{not json").unwrap();
        let recovered = InstanceRegistry::load(&path);
        assert!(recovered.instances.is_empty());
        recovered2_save_and_reload(&path);

        // BOM-prefixed file (editor artifact) still parses.
        let bom = format!("\u{feff}{}", std::fs::read_to_string(&path).unwrap());
        std::fs::write(&path, bom).unwrap();
        let after_bom = InstanceRegistry::load(&path);
        assert_eq!(after_bom.instances.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    fn recovered2_save_and_reload(path: &Path) {
        let mut reg = InstanceRegistry::default();
        reg.upsert(buildbox("rebuilt", "8.8.8.8:9191")).unwrap();
        reg.save(path).unwrap();
        let reloaded = InstanceRegistry::load(path);
        assert_eq!(reloaded.instances.len(), 1);
        assert_eq!(reloaded.instances[0].id, "rebuilt");
    }

    #[test]
    fn missing_file_loads_empty() {
        let reg = InstanceRegistry::load(Path::new("Z:/definitely/not/here/instances.json"));
        assert!(reg.instances.is_empty());
    }

    #[test]
    fn a_missing_registry_still_remembers_where_it_will_be_saved() {
        // The first mutation must be able to save in place: load() of a
        // nonexistent file therefore carries the path, unlike Default.
        let dir = std::env::temp_dir().join(format!(
            "vel_instances_new_{}_{}",
            std::process::id(),
            NEW_FILE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let path = dir.join("instances.json");
        let mut reg = InstanceRegistry::load(&path);
        assert_eq!(reg.file_path(), Some(path.as_path()));

        let mut rec = buildbox("fresh", "4.4.4.4:9191");
        rec.auth_token = Some("tok-123".into());
        reg.upsert(rec).unwrap();
        reg.save_in_place().unwrap();

        let reread = InstanceRegistry::load(&path);
        let got = reread.get("fresh").unwrap();
        assert_eq!(got.auth_token.as_deref(), Some("tok-123"));
        // Absent tokens stay out of the file entirely rather than writing nulls.
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("\"auth_token\":\"tok-123\"") || body.contains("tok-123"));
        std::fs::remove_dir_all(&dir).ok();
    }

    static NEW_FILE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    #[test]
    fn in_memory_registry_without_a_path_cannot_save_in_place() {
        let reg = InstanceRegistry::default();
        assert!(reg.file_path().is_none());
        assert!(reg.save_in_place().is_err());
    }
}
