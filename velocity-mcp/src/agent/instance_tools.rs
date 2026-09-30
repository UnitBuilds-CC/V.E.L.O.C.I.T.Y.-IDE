//! MCP tool surface for the managed-instance registry.
//!
//! `instances.rs` holds the bookkeeping and `drone_bridge.rs` speaks HTTP/SSH;
//! this module is the seam between them that the dispatch chain calls. Keeping
//! it separate means the registry stays network-free and testable while the
//! tool handlers get to be the only place that knows about argument shapes,
//! secret masking, and which mutations are allowed to touch the file.
//!
//! Registry state lives at `<workspace>/.velocity/instances.json`. Every
//! handler that changes state loads, mutates, and saves, so the GUI and any
//! agent run in the same workspace see one list rather than stale copies.
//!
//! Secrets: a node's bearer token is persisted (the ping/deploy loop is
//! useless without it) but never echoed back by a tool result - callers get
//! `has_auth_token` instead, so a transcript of tool output cannot leak the
//! thing that authorises remote code execution.

use super::drone_bridge::{
    DroneClient, DroneDeployer, DroneHealth, DEFAULT_DRONE_PORT, DEFAULT_SSH_PORT,
};
use super::instances::{
    slugify, InstancePlatform, InstanceRecord, InstanceRegistry, InstanceRole, RouteQuery,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::path::{Path, PathBuf};

/// Handle an `instance_*` tool call. `Ok(None)` means the name is not ours and
/// dispatch should keep walking the chain.
pub fn handle_instance_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    let handled = match name {
        "instance_add" => handle_add(root, arguments)?,
        "instance_list" => handle_list(root, arguments),
        "instance_remove" => handle_remove(root, arguments)?,
        "instance_ping" => handle_ping(root, arguments)?,
        "instance_deploy" => handle_deploy(root, arguments)?,
        "instance_pick" => handle_pick(root, arguments)?,
        "instance_release" => handle_release(root, arguments)?,
        _ => return Ok(None),
    };
    Ok(Some(handled.to_string()))
}

// ── File location & clock ──

/// `<root>/.velocity/instances.json` - the one place that spells out where the
/// registry lives, shared with the GUI so both read the same list.
pub fn instances_path(root: &Path) -> PathBuf {
    root.join(".velocity").join("instances.json")
}

fn load(root: &Path) -> InstanceRegistry {
    InstanceRegistry::load(&instances_path(root))
}

/// Wall clock in unix seconds; shared with the GUI so a panel's eligibility
/// judgements use the same expiry arithmetic as the tool handlers.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Wrap a build command so it runs inside the node's checkout. An empty
/// `work_dir` means "wherever the drone already lives", and the command is
/// passed through untouched rather than prefixed with a broken `cd`.
pub fn wrap_for_work_dir(work_dir: &str, command: &str) -> String {
    let dir = work_dir.trim();
    if dir.is_empty() {
        return command.to_string();
    }
    format!("cd {dir} && {command}")
}

// ── Argument reading ──

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, Box<dyn Error>> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{key} is required").into())
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn opt_role(
    args: &Value,
    key: &str,
    fallback: InstanceRole,
) -> Result<InstanceRole, Box<dyn Error>> {
    match opt_role_arg(args, key)? {
        Some(r) => Ok(r),
        None => Ok(fallback),
    }
}

/// `None` when the key is absent or blank; an unparseable value is an error so
/// a typo in `role` is reported instead of silently widening the result set.
fn opt_role_arg(args: &Value, key: &str) -> Result<Option<InstanceRole>, Box<dyn Error>> {
    match opt_str(args, key) {
        None => Ok(None),
        Some(raw) => InstanceRole::parse(&raw).map(Some).ok_or_else(|| {
            format!("unknown {key} {raw:?}; expected buildbox, agent_host, or gui_server").into()
        }),
    }
}

fn opt_labels(args: &Value, key: &str) -> Vec<String> {
    // Accept a JSON array of strings or a single comma-separated string: both
    // shapes show up in agent-generated calls and neither should need a retry.
    if let Some(items) = args[key].as_array() {
        return items
            .iter()
            .filter_map(|i| i.as_str().map(|s| s.to_string()))
            .collect();
    }
    opt_str(args, key)
        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect())
        .unwrap_or_default()
}

/// Normalise the many ways a caller writes "where the drone listens" into the
/// registry's `host:port` shape. A bare host gets the default drone port; a URL
/// gets its scheme, path and trailing slash dropped, so re-adding the same node
/// in a different spelling does not look like an endpoint move (which would
/// wipe its sighting state and clone the entry).
pub fn normalize_addr(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for scheme in ["http://", "https://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest.to_string();
            break;
        }
    }
    if let Some((authority, _path)) = s.split_once('/') {
        s = authority.to_string();
    }
    s = s.trim_end_matches(':').to_string();
    if s.is_empty() {
        return String::new();
    }
    match s.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty() && !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) =>
        {
            s
        }
        _ => format!("{s}:{DEFAULT_DRONE_PORT}"),
    }
}

/// The base URL a `DroneClient` wants for a stored `host:port` address.
pub fn drone_url_for(addr: &str) -> String {
    let addr = normalize_addr(addr);
    if addr.is_empty() {
        return String::new();
    }
    if addr.contains(':') {
        return format!("http://{addr}");
    }
    format!("http://{addr}:{DEFAULT_DRONE_PORT}")
}

/// Derive the OS family from a drone's own `environment` report (`linux-x86_64`).
pub fn platform_from_environment(env: &str) -> InstancePlatform {
    let family = env.split(['-', '_']).next().unwrap_or(env);
    InstancePlatform::parse(family)
}

fn generated_token(host: &str, port: u16) -> String {
    // Same shape `drone_deploy` uses, truncated to the 32 hex characters the
    // drone's auth path produces for its own defaults.
    let mut hasher = Sha256::new();
    hasher.update(format!("{host}-{port}-{}", std::process::id()));
    hasher.update(b"-instance-registry");
    let hash = hasher.finalize();
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    hex[..32].to_string()
}

fn default_ssh_user() -> String {
    for key in ["USER", "USERNAME", "LOGNAME"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                return v.trim().to_string();
            }
        }
    }
    "root".to_string()
}

// ── Result shaping ──

/// Public view of one record: everything routing needs, minus the secret.
pub fn show(rec: &InstanceRecord, now: u64) -> Value {
    json!({
        "id": rec.id,
        "name": rec.name,
        "role": rec.role.label(),
        "platform": rec.platform.label(),
        "drone_addr": rec.drone_addr,
        "drone_url": drone_url_for(&rec.drone_addr),
        "ssh": rec.ssh,
        "work_dir": rec.work_dir,
        "labels": rec.labels,
        "enabled": rec.enabled,
        "status": rec.status.label(),
        "effective_status": rec.effective_status(now).label(),
        "eligible": rec.eligible(now),
        "last_seen_secs": rec.last_seen_secs,
        "in_flight": rec.in_flight,
        "has_auth_token": rec.auth_token.is_some(),
    })
}

// ── Handlers ──

fn handle_add(root: &Path, args: &Value) -> Result<Value, Box<dyn Error>> {
    let name = req_str(args, "name")?;
    let role = opt_role(args, "role", InstanceRole::Buildbox)?;
    let drone_addr = normalize_addr(req_str(args, "drone_addr")?);
    if drone_addr.is_empty() {
        return Err("drone_addr must be the host or host:port of the drone endpoint".into());
    }
    let platform = match opt_str(args, "platform") {
        Some(p) => InstancePlatform::parse(&p),
        None => InstancePlatform::Unknown,
    };
    let id = opt_str(args, "id").unwrap_or_else(|| slugify(name));

    let mut registry = load(root);
    let mut rec = InstanceRecord::new(name, role, platform, &drone_addr);
    rec.id = id.clone();
    rec.ssh = opt_str(args, "ssh");
    rec.work_dir = opt_str(args, "work_dir").unwrap_or_default();
    rec.labels = opt_labels(args, "labels");
    rec.enabled = args["enabled"].as_bool().unwrap_or(true);
    rec.auth_token = opt_str(args, "auth_token");

    registry.upsert(rec)?;
    registry.save_in_place()?;
    let stored = registry
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("instance {id:?} was not stored"))?;
    Ok(json!({
        "success": true,
        "instance": show(&stored, now_secs()),
        "total_instances": registry.instances.len(),
    }))
}

fn handle_list(root: &Path, args: &Value) -> Value {
    let registry = load(root);
    let now = now_secs();
    // Only filter by role/platform when the caller asked usefully; an empty
    // call lists everything, which is what the Nodes panel needs. A
    // nonsensical role is treated as "no filter" here rather than an error,
    // so a panel query with a stale value still shows the operator something.
    let role = opt_role_arg(args, "role").ok().flatten();
    let platform = opt_str(args, "platform").map(|p| InstancePlatform::parse(&p));
    let eligible_only = args["eligible_only"].as_bool().unwrap_or(false);

    let items: Vec<Value> = registry
        .instances
        .iter()
        .filter(|i| role.is_none_or(|r| i.role == r))
        .filter(|i| platform.is_none_or(|p| i.platform == p))
        .filter(|i| !eligible_only || i.eligible(now))
        .map(|i| show(i, now))
        .collect();

    json!({
        "success": true,
        "count": items.len(),
        "instances": items,
    })
}

fn handle_remove(root: &Path, args: &Value) -> Result<Value, Box<dyn Error>> {
    let id = req_str(args, "id")?;
    let mut registry = load(root);
    let removed = registry
        .remove(id)
        .ok_or_else(|| format!("no instance with id {id:?}"))?;
    registry.save_in_place()?;
    Ok(json!({
        "success": true,
        "removed": removed.id,
        "name": removed.name,
        "remaining": registry.instances.len(),
    }))
}

fn handle_ping(root: &Path, args: &Value) -> Result<Value, Box<dyn Error>> {
    let id = req_str(args, "id")?;
    let mut registry = load(root);
    let rec = registry
        .get(id)
        .ok_or_else(|| format!("no instance with id {id:?}"))?;
    let (addr, token) = (rec.drone_addr.clone(), rec.auth_token.clone());

    let client = DroneClient::new(&drone_url_for(&addr), token.as_deref());
    let now = now_secs();
    match client.health() {
        Ok(health) => {
            let adopted = apply_ping(&mut registry, id, &health, now);
            registry.save_in_place()?;
            Ok(json!({
                "success": true,
                "id": id,
                "status": registry.get(id).map(|r| r.status.label()).unwrap_or("unknown"),
                "instance": registry.get(id).map(|r| show(r, now)),
                "drone": {
                    "drone_id": health.id,
                    "name": health.name,
                    "version": health.version,
                    "environment": health.environment,
                    "uptime_secs": health.uptime_secs,
                    "capabilities": health.capabilities,
                },
                "adopted_platform": adopted,
            }))
        }
        Err(e) => {
            apply_failed_ping(&mut registry, id, now);
            // Persist the downgrade: a node that stopped answering must not keep
            // receiving routed work after this process exits.
            registry.save_in_place()?;
            Ok(json!({
                "success": false,
                "id": id,
                "status": registry.get(id).map(|r| r.status.label()).unwrap_or("unknown"),
                "error": e.to_string(),
            }))
        }
    }
}

/// Fold a health answer into the record. Returns the platform adopted from the
/// drone's own report, if the record had none.
fn apply_ping(
    registry: &mut InstanceRegistry,
    id: &str,
    health: &DroneHealth,
    now: u64,
) -> Option<String> {
    let mut adopted = None;
    if let Some(rec) = registry.get_mut(id) {
        // `ok` here means "answered", which is what liveness tracking keys on;
        // a drone reporting a poor self-check is still reachable, and its raw
        // `status` field rides along in the tool result for the caller to judge.
        rec.observe_health(true, now);
        if rec.platform == InstancePlatform::Unknown {
            let p = platform_from_environment(&health.environment);
            if p != InstancePlatform::Unknown {
                rec.platform = p;
                adopted = Some(p.label().to_string());
            }
        }
    }
    adopted
}

fn apply_failed_ping(registry: &mut InstanceRegistry, id: &str, now: u64) {
    if let Some(rec) = registry.get_mut(id) {
        rec.observe_health(false, now);
    }
}

fn handle_deploy(root: &Path, args: &Value) -> Result<Value, Box<dyn Error>> {
    let host = req_str(args, "host")?;
    let ssh_port = args["ssh_port"].as_u64().unwrap_or(DEFAULT_SSH_PORT as u64) as u16;
    let drone_port = args["drone_port"]
        .as_u64()
        .unwrap_or(DEFAULT_DRONE_PORT as u64) as u16;
    let name = opt_str(args, "name").unwrap_or_else(|| host.replace('.', "-"));
    let role = opt_role(args, "role", InstanceRole::Buildbox)?;
    let platform = match opt_str(args, "platform") {
        Some(p) => InstancePlatform::parse(&p),
        None => InstancePlatform::Unknown,
    };
    let ssh_user = opt_str(args, "ssh_user").unwrap_or_else(default_ssh_user);
    let ssh_key = opt_str(args, "ssh_key_path").map(PathBuf::from);
    let token = opt_str(args, "auth_token").unwrap_or_else(|| generated_token(host, drone_port));
    let labels = opt_labels(args, "labels");
    let work_dir = opt_str(args, "work_dir").unwrap_or_default();
    let drone_name = opt_str(args, "drone_name").unwrap_or_else(|| name.clone());

    // Deploy is the slow part (scp plus up to 10 health retries); do it before
    // touching the registry so a failed deploy leaves the list unchanged.
    let deployer = DroneDeployer::new(&ssh_user, ssh_key.as_deref(), ssh_port);
    let health = deployer.deploy(host, drone_port, &drone_name, &token)?;

    let addr = normalize_addr(&format!("{host}:{drone_port}"));
    let id = slugify(&name);
    let mut registry = load(root);
    let mut rec = InstanceRecord::new(&name, role, platform, &addr);
    rec.id = id.clone();
    rec.ssh = Some(format!("{ssh_user}@{host}"));
    rec.work_dir = work_dir;
    rec.labels = labels;
    rec.auth_token = Some(token);
    registry.upsert(rec)?;
    let now = now_secs();
    let adopted = apply_ping(&mut registry, &id, &health, now);
    registry.save_in_place()?;

    let stored = registry
        .get(&id)
        .cloned()
        .ok_or("deployed instance is missing from the registry")?;
    Ok(json!({
        "success": true,
        "deployed": true,
        "status": stored.status.label(),
        "instance": show(&stored, now),
        "adopted_platform": adopted,
        "drone": {
            "drone_id": health.id,
            "version": health.version,
            "environment": health.environment,
        },
        "note": "a bearer token was generated and stored on the registry record; it is not returned here.",
    }))
}

fn handle_pick(root: &Path, args: &Value) -> Result<Value, Box<dyn Error>> {
    let registry = load(root);
    let now = now_secs();
    // Absent role means "any role"; an unparseable one is refused outright,
    // since routing to the wrong kind of node is worse than not routing.
    let role = opt_role_arg(args, "role")?;
    let platform = opt_str(args, "platform").map(|p| InstancePlatform::parse(&p));
    let wanted = opt_labels(args, "labels");
    let refs: Vec<&str> = wanted.iter().map(|s| s.as_str()).collect();
    let q = RouteQuery {
        role,
        platform,
        require_labels: &refs,
    };

    let candidates = registry.candidates(&q, now).len();
    let Some(chosen) = registry.pick(&q, now) else {
        return Ok(json!({
            "success": false,
            "error": "no eligible instance",
            "total_instances": registry.instances.len(),
            "hint": "nodes need status online or degraded (seen healthy recently) and enabled=true; instance_ping refreshes one.",
        }));
    };
    let id = chosen.id.clone();
    let url = drone_url_for(&chosen.drone_addr);
    drop(refs);

    let claimed = if args["claim"].as_bool().unwrap_or(false) {
        let mut reg = load(root);
        let ok = reg.claim(&id);
        if ok {
            reg.save_in_place()?;
        }
        ok
    } else {
        false
    };
    Ok(json!({
        "success": true,
        "id": id,
        "drone_url": url,
        "candidates": candidates,
        "claimed": claimed,
    }))
}

fn handle_release(root: &Path, args: &Value) -> Result<Value, Box<dyn Error>> {
    let id = req_str(args, "id")?;
    let mut registry = load(root);
    let released = registry.release(id);
    if released {
        registry.save_in_place()?;
    }
    Ok(json!({
        "success": released,
        "id": id,
        "in_flight": registry.get(id).map(|r| r.in_flight).unwrap_or(0),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn wrap_for_work_dir_prefixes_cd_only_when_set() {
        assert_eq!(
            wrap_for_work_dir("/home/ian/vel", "cargo build --release"),
            "cd /home/ian/vel && cargo build --release"
        );
        assert_eq!(wrap_for_work_dir("", "cargo run"), "cargo run");
        assert_eq!(wrap_for_work_dir("   ", "cargo run"), "cargo run");
    }

    fn scratch_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "vel_instance_tools_{}_{}_{}",
            tag,
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn arg(pairs: &[(&str, Value)]) -> Value {
        let mut map = serde_json::Map::new();
        for (k, v) in pairs {
            map.insert((*k).to_string(), v.clone());
        }
        Value::Object(map)
    }

    #[test]
    fn normalize_addr_accepts_host_url_and_scheme_forms() {
        assert_eq!(normalize_addr("192.168.0.5"), "192.168.0.5:9191");
        assert_eq!(normalize_addr("192.168.0.5:9000"), "192.168.0.5:9000");
        assert_eq!(
            normalize_addr("http://buildbox.local:9191/"),
            "buildbox.local:9191"
        );
        assert_eq!(normalize_addr(" 10.0.0.1/x/y "), "10.0.0.1:9191");
        assert_eq!(normalize_addr(""), "");
        assert_eq!(drone_url_for("10.0.0.1:9191"), "http://10.0.0.1:9191");
        assert_eq!(drone_url_for("10.0.0.1"), "http://10.0.0.1:9191");
    }

    #[test]
    fn platform_comes_from_the_drones_own_environment_report() {
        assert_eq!(
            platform_from_environment("linux-x86_64"),
            InstancePlatform::Linux
        );
        assert_eq!(
            platform_from_environment("windows-x86_64"),
            InstancePlatform::Windows
        );
        assert_eq!(
            platform_from_environment("macos-aarch64"),
            InstancePlatform::Macos
        );
        assert_eq!(
            platform_from_environment("weird"),
            InstancePlatform::Unknown
        );
    }

    #[test]
    fn add_list_remove_roundtrip_persists_without_leaking_token() {
        let root = scratch_root("roundtrip");
        let added = handle_add(
            &root,
            &arg(&[
                ("name", json!("Buildbox")),
                ("role", json!("build-box")),
                ("platform", json!("linux")),
                ("drone_addr", json!("192.168.0.5")),
                ("auth_token", json!("s3cr3t-do-not-echo")),
                ("labels", json!("rust, GPU")),
            ]),
        )
        .unwrap();
        assert_eq!(added["instance"]["id"], "buildbox");
        assert_eq!(added["instance"]["drone_addr"], "192.168.0.5:9191");
        assert_eq!(added["instance"]["role"], "buildbox");
        assert_eq!(added["instance"]["has_auth_token"], true);
        // The secret never appears in a tool result.
        assert!(!added.to_string().contains("s3cr3t-do-not-echo"));

        // Comma form split and lower-cased by the registry's label normaliser.
        let listed = handle_list(&root, &arg(&[]));
        assert_eq!(listed["count"], 1);
        let labels = listed["instances"][0]["labels"].as_array().unwrap();
        assert_eq!(labels[0], "gpu");
        assert_eq!(labels[1], "rust");

        // It really hit the file: a fresh load sees the same record, token kept.
        let reg = InstanceRegistry::load(&instances_path(&root));
        assert_eq!(reg.instances.len(), 1);
        assert_eq!(
            reg.get("buildbox").unwrap().auth_token.as_deref(),
            Some("s3cr3t-do-not-echo")
        );

        let removed = handle_remove(&root, &arg(&[("id", json!("buildbox"))])).unwrap();
        assert_eq!(removed["remaining"], 0);
        assert!(handle_remove(&root, &arg(&[("id", json!("buildbox"))])).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn add_rejects_bad_role_and_missing_fields() {
        let root = scratch_root("badargs");
        let e = handle_add(
            &root,
            &arg(&[("role", json!("toaster")), ("drone_addr", json!("1.2.3.4"))]),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("name is required"), "{e}");

        let e2 = handle_add(
            &root,
            &arg(&[
                ("name", json!("x")),
                ("role", json!("toaster")),
                ("drone_addr", json!("1.2.3.4")),
            ]),
        )
        .unwrap_err()
        .to_string();
        assert!(e2.contains("unknown role"), "{e2}");

        let e3 = handle_add(
            &root,
            &arg(&[("name", json!("x")), ("role", json!("buildbox"))]),
        )
        .unwrap_err()
        .to_string();
        assert!(e3.contains("drone_addr is required"), "{e3}");
        // Nothing partial was written by the rejected calls.
        assert_eq!(handle_list(&root, &arg(&[]))["count"], 0);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ping_of_unreachable_node_downgrades_and_persists() {
        let root = scratch_root("ping");
        handle_add(
            &root,
            &arg(&[
                ("name", json!("ghost")),
                ("drone_addr", json!("127.0.0.1:1")),
                ("platform", json!("linux")),
            ]),
        )
        .unwrap();
        let res = handle_ping(&root, &arg(&[("id", json!("ghost"))])).unwrap();
        assert_eq!(res["success"], false);
        // Never seen before, so a failed probe is Offline rather than Degraded.
        assert_eq!(res["status"], "offline");
        let reg = InstanceRegistry::load(&instances_path(&root));
        assert!(!reg.get("ghost").unwrap().eligible(now_secs()));
        assert!(handle_ping(&root, &arg(&[("id", json!("nobody"))])).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pick_requires_eligibility_and_claim_moves_load() {
        let root = scratch_root("pick");
        handle_add(
            &root,
            &arg(&[
                ("name", json!("b1")),
                ("drone_addr", json!("1.1.1.1")),
                ("platform", json!("linux")),
            ]),
        )
        .unwrap();
        // Registered but never pinged => not eligible, so nothing routes.
        let none = handle_pick(&root, &arg(&[("role", json!("buildbox"))])).unwrap();
        assert_eq!(none["success"], false);
        assert_eq!(none["error"], "no eligible instance");

        mark_online(&root, "b1");
        let got = handle_pick(
            &root,
            &arg(&[("role", json!("buildbox")), ("claim", json!(true))]),
        )
        .unwrap();
        assert_eq!(got["success"], true);
        assert_eq!(got["id"], "b1");
        assert_eq!(got["drone_url"], "http://1.1.1.1:9191");
        assert_eq!(got["claimed"], true);
        assert_eq!(
            InstanceRegistry::load(&instances_path(&root))
                .get("b1")
                .unwrap()
                .in_flight,
            1
        );

        let rel = handle_release(&root, &arg(&[("id", json!("b1"))])).unwrap();
        assert_eq!(rel["in_flight"], 0);
        assert_eq!(
            handle_release(&root, &arg(&[("id", json!("ghost"))])).unwrap()["success"],
            false
        );

        // Label filter excludes it: routing asks for something the node lacks.
        let miss = handle_pick(&root, &arg(&[("labels", json!("gpu"))])).unwrap();
        assert_eq!(miss["success"], false);
        // And a platform filter that matches finds it again.
        let hit = handle_pick(&root, &arg(&[("platform", json!("linux"))])).unwrap();
        assert_eq!(hit["success"], true);
        std::fs::remove_dir_all(&root).ok();
    }

    fn mark_online(root: &Path, id: &str) {
        let path = instances_path(root);
        let mut reg = InstanceRegistry::load(&path);
        reg.get_mut(id).unwrap().observe_health(true, now_secs());
        reg.save(&path).unwrap();
    }

    #[test]
    fn list_honors_role_and_eligible_only_filters() {
        let root = scratch_root("filters");
        handle_add(
            &root,
            &arg(&[
                ("name", json!("bb")),
                ("role", json!("buildbox")),
                ("drone_addr", json!("2.2.2.2")),
                ("platform", json!("linux")),
            ]),
        )
        .unwrap();
        handle_add(
            &root,
            &arg(&[
                ("name", json!("gui")),
                ("role", json!("gui_server")),
                ("drone_addr", json!("3.3.3.3")),
            ]),
        )
        .unwrap();
        mark_online(&root, "gui");

        assert_eq!(handle_list(&root, &arg(&[]))["count"], 2);
        assert_eq!(
            handle_list(&root, &arg(&[("role", json!("gui_server"))]))["count"],
            1
        );
        assert_eq!(
            handle_list(&root, &arg(&[("eligible_only", json!(true))]))["count"],
            1
        );
        // A nonsense role argument degrades to "no filter" rather than erroring,
        // so a typo in a panel query still shows the user something.
        assert_eq!(
            handle_list(&root, &arg(&[("role", json!("toaster"))]))["count"],
            2
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dispatch_returns_none_for_foreign_tool_names() {
        let root = std::env::temp_dir();
        assert!(handle_instance_tool(&root, "drone_status", &json!({}))
            .unwrap()
            .is_none());
        assert!(handle_instance_tool(&root, "instance_nope", &json!({}))
            .unwrap()
            .is_none());
    }

    #[test]
    fn every_advertised_instance_tool_is_dispatched() {
        // Guards the seam between the schema list and the match arms above: a
        // tool advertised without a handler answers "Unknown tool" to the agent.
        let root = scratch_root("dispatch");
        for tool in crate::registry::tool_definitions::instances::get_instance_tools() {
            let name = tool.name.as_str();
            let outcome = handle_instance_tool(&root, name, &json!({}));
            // Either it answered (list/pick/release style) or it refused the
            // empty arguments (required-field style); "not mine" is a bug.
            match outcome {
                Ok(Some(_)) | Err(_) => {}
                Ok(None) => panic!("{name} is advertised but not dispatched"),
            }
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// Live round-trip against a real node, through the shipped tool code and
    /// the real network. `#[ignore]`d so CI never needs a machine: run it with
    ///
    ///   VELOCITY_LIVE_ROOT=<workspace with .velocity/>
    ///   VELOCITY_LIVE_HOST=<ip or hostname>
    ///   VELOCITY_LIVE_TOKEN=<drone bearer token>
    ///   VELOCITY_LIVE_NAME=<display name, default "live-node">
    ///   VELOCITY_LIVE_LABELS="rust,gpu" / VELOCITY_LIVE_WORK_DIR / VELOCITY_LIVE_SSH
    ///   VELOCITY_LIVE_KEEP=1 to leave the node registered afterwards
    ///   cargo test -p velocity_mcp --lib instance_tools -- --ignored
    ///
    /// It registers the node, pings it, routes work to it by platform, runs a
    /// command on it, and cleans the claim counter up again - which is the whole
    /// point of the registry, and nothing short of a live call can show that
    /// the wire format both ends use still agrees.
    #[test]
    #[ignore = "requires VELOCITY_LIVE_* pointing at a reachable drone"]
    fn live_node_roundtrip_through_the_tools() {
        let Ok(host) = std::env::var("VELOCITY_LIVE_HOST") else {
            panic!("VELOCITY_LIVE_HOST not set");
        };
        let root =
            PathBuf::from(std::env::var("VELOCITY_LIVE_ROOT").unwrap_or_else(|_| ".".to_string()));
        let token = std::env::var("VELOCITY_LIVE_TOKEN").unwrap_or_default();
        let name = std::env::var("VELOCITY_LIVE_NAME").unwrap_or_else(|_| "live-node".into());

        // 1. Register (no platform given: the node must report its own).
        let mut add_args = vec![
            ("name", json!(name.clone())),
            ("role", json!("buildbox")),
            ("drone_addr", json!(host.clone())),
            ("auth_token", json!(token.clone())),
        ];
        // Optional enrolment detail, so the same probe can both verify a node
        // and leave it registered the way an operator would want it.
        if let Ok(labels) = std::env::var("VELOCITY_LIVE_LABELS") {
            add_args.push(("labels", json!(labels)));
        }
        if let Ok(dir) = std::env::var("VELOCITY_LIVE_WORK_DIR") {
            add_args.push(("work_dir", json!(dir)));
        }
        if let Ok(ssh) = std::env::var("VELOCITY_LIVE_SSH") {
            add_args.push(("ssh", json!(ssh)));
        }
        let added = handle_add(&root, &arg(&add_args)).unwrap();
        let id = added["instance"]["id"].as_str().unwrap().to_string();
        assert_eq!(added["instance"]["platform"], "unknown");

        // 2. Ping: online, and the platform adopted from the drone's report.
        let ping = handle_ping(&root, &arg(&[("id", json!(id.clone()))])).unwrap();
        assert_eq!(
            ping["status"], "online",
            "live ping failed: {ping} (auth must present a Bearer token)"
        );
        let env_seen = ping["drone"]["environment"]
            .as_str()
            .unwrap_or("")
            .to_string();
        assert!(!env_seen.is_empty(), "drone reported no environment");
        assert_eq!(
            ping["instance"]["platform"],
            platform_from_environment(&env_seen).label()
        );

        // 3. Route by platform, claiming load so the counters move.
        let picked = handle_pick(
            &root,
            &arg(&[
                ("role", json!("buildbox")),
                (
                    "platform",
                    json!(platform_from_environment(&env_seen).label()),
                ),
                ("claim", json!(true)),
            ]),
        )
        .unwrap();
        assert_eq!(picked["success"], true, "nothing routed: {picked}");
        assert_eq!(picked["id"], id);
        let drone_url = picked["drone_url"].as_str().unwrap().to_string();

        // 4. Run real work on it through the existing drone tool, and poll.
        let marker = format!("V63_LIVE_{}", std::process::id());
        let submitted = crate::agent::drone_bridge::handle_drone_tool(
            &root,
            "drone_command",
            &json!({
                "drone_url": drone_url,
                "auth_token": token,
                "command": format!("echo {marker}; uname -sr; nproc"),
            }),
        )
        .unwrap()
        .expect("drone_command is dispatched");
        let submitted: Value = serde_json::from_str(&submitted).unwrap();
        let task_id = submitted["task_id"].as_str().unwrap().to_string();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let stdout = loop {
            let status = crate::agent::drone_bridge::handle_drone_tool(
                &root,
                "drone_task_status",
                &json!({ "drone_url": drone_url, "auth_token": token, "task_id": task_id }),
            )
            .unwrap()
            .unwrap();
            let status: Value = serde_json::from_str(&status).unwrap();
            if status["status"].as_str().unwrap_or("") == "completed" {
                // The drone nests the shell outcome under `result`.
                break status["stdout"]
                    .as_str()
                    .or_else(|| status["result"]["stdout"].as_str())
                    .unwrap_or_default()
                    .to_string();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "task {task_id} never completed: {status}"
            );
            std::thread::sleep(std::time::Duration::from_millis(500));
        };
        assert!(stdout.contains(&marker), "stdout was {stdout:?}");
        assert!(
            stdout.contains("Linux") || stdout.contains("linux"),
            "uname missing from {stdout:?}"
        );

        // 5. Release the claim. The node is then forgotten unless the operator
        // asked to keep it, so a probe run cannot silently edit the registry the
        // IDE reads - while `KEEP=1` is how you enrol a real buildbox.
        let rel = handle_release(&root, &arg(&[("id", json!(id.clone()))])).unwrap();
        assert_eq!(rel["in_flight"], 0);
        if std::env::var("VELOCITY_LIVE_KEEP").as_deref() == Ok("1") {
            let listed = handle_list(&root, &arg(&[]));
            assert!(
                listed.to_string().contains(&format!("\"id\":\"{id}\""))
                    || listed.to_string().contains(&format!("\"id\": \"{id}\"")),
                "KEEP=1 but {id} is not in the registry: {listed}"
            );
        } else {
            handle_remove(&root, &arg(&[("id", json!(id.clone()))])).unwrap();
        }
    }
}
