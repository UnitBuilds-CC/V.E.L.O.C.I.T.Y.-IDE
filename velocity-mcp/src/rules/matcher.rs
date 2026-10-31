//! Matchers: which calls a rule fires on, and what the refusal should name.
//!
//! Matching is deliberately conservative. A ban that occasionally fires on text
//! that only *looks* like a target is safer than one that misses a target hidden
//! in a URL, a flag, or a connection string. Every matcher returns the matched
//! fragment so the refusal can name it.

use serde_json::Value;
use std::net::Ipv4Addr;

use super::model::{Rule, RuleKind};

/// A proposed tool call, with its arguments already parsed.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub tool: String,
    pub arguments: Value,
}

impl ToolCall {
    pub fn new(tool: impl Into<String>, arguments: Value) -> Self {
        Self {
            tool: tool.into(),
            arguments,
        }
    }
}

/// The matched fragment, phrased for a refusal: `server "10.0.0.5"`.
pub type MatchDetail = String;

/// Whether `rule` fires on `call`, and what it should name.
///
/// Budgets are stateful and return `None` here; [`super::engine`] counts them.
pub fn matches(rule: &Rule, call: &ToolCall) -> Option<MatchDetail> {
    match rule.kind {
        RuleKind::ToolBan => tool_ban(rule, call),
        RuleKind::EgressBan => egress_ban(rule, call),
        RuleKind::PathBan => path_ban(rule, call),
        RuleKind::CommandPattern => command_pattern(rule, call),
        RuleKind::ToolBudget => None,
    }
}

fn tool_ban(rule: &Rule, call: &ToolCall) -> Option<MatchDetail> {
    rule.list("tools")
        .iter()
        .find(|name| name.eq_ignore_ascii_case(&call.tool))
        .map(|name| format!("tool \"{name}\""))
}

fn egress_ban(rule: &Rule, call: &ToolCall) -> Option<MatchDetail> {
    let targets = rule.list("targets");
    if targets.is_empty() {
        return None;
    }
    let observed = egress_observations(&call.arguments);
    for target in targets {
        let Some(pattern) = TargetPattern::parse(target) else {
            continue;
        };
        if let Some(seen) = observed.iter().find(|o| pattern.matches(o)) {
            return Some(format!("server \"{}\"", seen.display));
        }
    }
    None
}

fn path_ban(rule: &Rule, call: &ToolCall) -> Option<MatchDetail> {
    let mut paths = Vec::new();
    collect_at_keys(&call.arguments, PATH_KEYS, &mut paths);
    paths
        .iter()
        .find_map(|path| {
            let normalized = normalize_path(path);
            rule.list("paths").iter().find_map(|prefix| {
                let prefix = normalize_path(prefix);
                path_is_under(&normalized, &prefix)
                    .then(|| format!("path \"{path}\""))
            })
        })
}

fn command_pattern(rule: &Rule, call: &ToolCall) -> Option<MatchDetail> {
    let mut commands = Vec::new();
    collect_at_keys(&call.arguments, COMMAND_KEYS, &mut commands);
    let patterns = rule.list("patterns");
    for command in &commands {
        let haystack = command.to_lowercase();
        for pattern in &patterns {
            if !pattern.is_empty() && haystack.contains(&pattern.to_lowercase()) {
                return Some(format!("command containing \"{pattern}\""));
            }
        }
    }
    None
}

const PATH_KEYS: &[&str] = &[
    "relativeFilePath",
    "filePath",
    "path",
    "file",
    "dir",
    "directory",
    "paths",
    "target",
    "source",
    "outputPath",
    "output",
    "destination",
];

const COMMAND_KEYS: &[&str] = &["command", "cmd", "script", "powershell", "shell", "commandLine"];

/// Paths are compared as forward-slash strings, case-insensitively: the same
/// workspace is edited on Windows and on remote Linux build hosts, and a rule
/// written on one must fire on the other.
pub fn normalize_path(path: &str) -> String {
    let mut out = path.trim().replace('\\', "/").to_lowercase();
    while out.starts_with("./") {
        out.drain(..2);
    }
    while out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

/// Boundary-aware prefix test: `src/secrets` contains `src/secrets/a.txt` but
/// not `src/secretsfile.rs`. An empty or `.` prefix covers the whole workspace.
pub fn path_is_under(path: &str, prefix: &str) -> bool {
    if prefix.is_empty() || prefix == "." || prefix == "./" {
        return true;
    }
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

// ─── Egress observations ──────────────────────────────────────────────────────

/// One network target seen in a call's arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// Normalized form used for comparison (lowercased host or dotted IPv4).
    pub key: String,
    /// What the refusal should quote.
    pub display: String,
}

impl Observation {
    fn host(host: &str) -> Option<Self> {
        let host = host
            .trim()
            .trim_matches(|c: char| {
                c == '"' || c == '\'' || c == ',' || c == ';' || c == '(' || c == ')'
            })
            .trim_end_matches('.');
        if host.is_empty() {
            return None;
        }
        Some(Self {
            key: host.to_ascii_lowercase(),
            display: host.to_string(),
        })
    }

    fn ip(ip: Ipv4Addr) -> Self {
        let text = ip.to_string();
        Self {
            key: text.clone(),
            display: text,
        }
    }
}

/// Extract every network target the arguments mention: dotted IPv4 literals,
/// URL hosts, `-h/--host/host=` values, and `user@host` connection strings.
pub fn egress_observations(arguments: &Value) -> Vec<Observation> {
    let mut strings = Vec::new();
    collect_all_strings(arguments, &mut strings);
    let mut out = Vec::new();
    for text in strings {
        scan_text(&text, &mut out);
    }
    // Host-typed keys are targets even without a scheme: `{"host": "db.prod"}`.
    let mut hosts = Vec::new();
    collect_at_keys(arguments, &["host", "hostname", "domain", "base_url", "url", "endpoint"], &mut hosts);
    for host in hosts {
        if let Some(observation) = host_token(&host) {
            out.push(observation);
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out.dedup_by(|a, b| a.key == b.key);
    out
}

fn scan_text(text: &str, out: &mut Vec<Observation>) {
    scan_ipv4(text, out);
    scan_urls(text, out);
    scan_flags(text, out);
    scan_at_hosts(text, out);
}

fn scan_ipv4(text: &str, out: &mut Vec<Observation>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() && bytes[i] != b'.' {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        let token = &text[start..i];
        if token.contains('.') {
            if let Ok(ip) = token.parse::<Ipv4Addr>() {
                out.push(Observation::ip(ip));
            }
        }
    }
}

fn scan_urls(text: &str, out: &mut Vec<Observation>) {
    let lower = text.to_ascii_lowercase();
    let mut search = 0;
    while let Some(pos) = lower[search..].find("://") {
        let start = search + pos + 3;
        let rest = &text[start..];
        let end = rest
            .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace())
            .unwrap_or(rest.len());
        let authority = &rest[..end];
        let host_part = authority.rsplit('@').next().unwrap_or(authority);
        let host = host_part
            .rsplit_once(':')
            .map(|(h, port)| {
                if port.chars().all(|c| c.is_ascii_digit()) {
                    h
                } else {
                    host_part
                }
            })
            .unwrap_or(host_part);
        if let Some(observation) = host_token(host) {
            out.push(observation);
        }
        search = start + end;
        if search >= lower.len() {
            break;
        }
    }
}

fn scan_flags(text: &str, out: &mut Vec<Observation>) {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        let lower = token.to_ascii_lowercase();
        let mut candidate: Option<&str> = None;
        if lower == "-h" || lower == "--host" {
            candidate = tokens.get(index + 1).copied();
        } else if let Some(rest) = lower.strip_prefix("--host=") {
            let _ = rest;
            candidate = token.get("--host=".len()..);
        } else if let Some(rest) = lower.strip_prefix("host=") {
            let _ = rest;
            candidate = token.get("host=".len()..);
        } else if lower.starts_with("-h") && token.len() > 2 {
            candidate = token.get(2..);
        } else if lower.starts_with("--host") && token.len() > 6 {
            candidate = token.get(6..).and_then(|s| s.strip_prefix('='));
        }
        if let Some(value) = candidate {
            if let Some(observation) = host_token(value) {
                out.push(observation);
            }
        }
    }
}

fn scan_at_hosts(text: &str, out: &mut Vec<Observation>) {
    for token in text.split_whitespace() {
        if token.starts_with('@') {
            continue;
        }
        let Some((_, rest)) = token.rsplit_once('@') else {
            continue;
        };
        let host = rest
            .split([':', '/', ',', ';'])
            .next()
            .unwrap_or(rest);
        if host.contains('.') {
            if let Some(observation) = host_token(host) {
                out.push(observation);
            }
        }
    }
}

/// A single host token with any scheme, port, or userinfo stripped.
fn host_token(raw: &str) -> Option<Observation> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let without_scheme = raw.split_once("://").map(|(_, rest)| rest).unwrap_or(raw);
    let authority = without_scheme
        .split(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace())
        .next()
        .unwrap_or(without_scheme);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host
        .rsplit_once(':')
        .filter(|(_, port)| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
        .map(|(h, _)| h)
        .unwrap_or(host);
    if host.parse::<Ipv4Addr>().is_ok() {
        return host.parse().ok().map(Observation::ip);
    }
    Observation::host(host)
}

/// A banned target as written in a rule: IPv4, CIDR, or hostname.
#[derive(Debug, Clone, PartialEq)]
enum TargetPattern {
    Ip(Ipv4Addr),
    Cidr { network: u32, mask: u32 },
    Host(String),
}

impl TargetPattern {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if let Some((ip, bits)) = text.split_once('/') {
            let ip: Ipv4Addr = ip.parse().ok()?;
            let bits: u32 = bits.parse().ok()?;
            if bits > 32 {
                return None;
            }
            let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
            return Some(TargetPattern::Cidr {
                network: u32::from(ip) & mask,
                mask,
            });
        }
        if let Ok(ip) = text.parse::<Ipv4Addr>() {
            return Some(TargetPattern::Ip(ip));
        }
        Some(TargetPattern::Host(text.to_ascii_lowercase()))
    }

    fn matches(&self, observation: &Observation) -> bool {
        match self {
            TargetPattern::Ip(ip) => observation.key == ip.to_string(),
            TargetPattern::Cidr { network, mask } => observation
                .key
                .parse::<Ipv4Addr>()
                .ok()
                .is_some_and(|ip| u32::from(ip) & mask == *network),
            TargetPattern::Host(host) => &observation.key == host,
        }
    }
}

// ─── Argument walking ─────────────────────────────────────────────────────────

fn collect_all_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                collect_all_strings(item, out);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_all_strings(item, out);
            }
        }
        _ => {}
    }
}

/// Strings stored under any of `keys`, at any depth. String arrays are expanded.
fn collect_at_keys(value: &Value, keys: &[&str], out: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_at_keys(item, keys, out);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                if keys.iter().any(|wanted| key.eq_ignore_ascii_case(wanted)) {
                    push_strings(item, out);
                }
                collect_at_keys(item, keys, out);
            }
        }
        _ => {}
    }
}

fn push_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                push_strings(item, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::model::{ParamValue, Rule, RuleKind};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn rule(kind: RuleKind, key: &str, values: &[&str]) -> Rule {
        let mut params = BTreeMap::new();
        params.insert(
            key.to_string(),
            ParamValue::List(values.iter().map(|v| v.to_string()).collect()),
        );
        Rule {
            id: "rule-test".into(),
            name: "test".into(),
            description: "test intent".into(),
            kind,
            params,
            remedy: None,
            owner: "user".into(),
            provenance: String::new(),
            version: 1,
            created_at: 0,
            updated_at: 0,
            witnesses: None,
        }
    }

    #[test]
    fn tool_ban_matches_the_named_tool_only() {
        let rule = rule(RuleKind::ToolBan, "tools", &["run_command"]);
        let hit = matches(&rule, &ToolCall::new("run_command", json!({})));
        assert_eq!(hit.as_deref(), Some("tool \"run_command\""));
        assert!(matches(&rule, &ToolCall::new("read_file", json!({}))).is_none());
    }

    #[test]
    fn egress_ban_finds_an_ip_in_a_shell_command() {
        let ban = rule(RuleKind::EgressBan, "targets", &["192.168.1.100"]);
        let call = ToolCall::new("run_command", json!({"command": "psql -h 192.168.1.100 -U app"}));
        assert_eq!(
            matches(&ban, &call).as_deref(),
            Some("server \"192.168.1.100\"")
        );
    }

    #[test]
    fn egress_ban_finds_the_host_of_a_connection_string() {
        let ban = rule(RuleKind::EgressBan, "targets", &["db.prod.internal"]);
        let call = ToolCall::new(
            "run_command",
            json!({"command": "psql postgres://app@db.prod.internal:5432/live"}),
        );
        assert_eq!(
            matches(&ban, &call).as_deref(),
            Some("server \"db.prod.internal\"")
        );
    }

    #[test]
    fn egress_ban_matches_a_cidr_range() {
        let ban = rule(RuleKind::EgressBan, "targets", &["10.0.0.0/8"]);
        assert!(matches(
            &ban,
            &ToolCall::new("connector_call", json!({"url": "http://10.4.5.6/api"}))
        )
        .is_some());
        assert!(matches(
            &ban,
            &ToolCall::new("connector_call", json!({"url": "http://11.4.5.6/api"}))
        )
        .is_none());
    }

    #[test]
    fn egress_ban_reads_a_host_argument_without_a_scheme() {
        let ban = rule(RuleKind::EgressBan, "targets", &["db.prod.internal"]);
        assert!(matches(
            &ban,
            &ToolCall::new("run_query", json!({"host": "db.prod.internal", "sql": "select 1"}))
        )
        .is_some());
    }

    #[test]
    fn egress_ban_allows_the_dev_server() {
        let ban = rule(RuleKind::EgressBan, "targets", &["192.168.1.100"]);
        let call = ToolCall::new("run_command", json!({"command": "psql -h 192.168.0.2 -U app"}));
        assert!(matches(&ban, &call).is_none());
    }

    #[test]
    fn path_ban_respects_component_boundaries() {
        let ban = rule(RuleKind::PathBan, "paths", &["src/secrets"]);
        let blocked = ToolCall::new("read_file", json!({"relativeFilePath": "src/secrets/key.pem"}));
        let allowed = ToolCall::new("read_file", json!({"relativeFilePath": "src/secretsfile.rs"}));
        assert_eq!(matches(&ban, &blocked).as_deref(), Some("path \"src/secrets/key.pem\""));
        assert!(matches(&ban, &allowed).is_none());
    }

    #[test]
    fn path_ban_is_case_and_separator_insensitive() {
        let ban = rule(RuleKind::PathBan, "paths", &["src/secrets"]);
        let call = ToolCall::new("read_file", json!({"relativeFilePath": "SRC\\Secrets\\key.pem"}));
        assert!(matches(&ban, &call).is_some());
    }

    #[test]
    fn command_pattern_is_case_insensitive() {
        let ban = rule(RuleKind::CommandPattern, "patterns", &["DROP TABLE"]);
        let call = ToolCall::new("run_command", json!({"command": "psql live -c 'drop table users'"}));
        assert_eq!(
            matches(&ban, &call).as_deref(),
            Some("command containing \"DROP TABLE\"")
        );
    }

    #[test]
    fn command_pattern_ignores_a_non_command_argument() {
        let ban = rule(RuleKind::CommandPattern, "patterns", &["rm -rf"]);
        let call = ToolCall::new("write_file", json!({"content": "never run rm -rf"}));
        assert!(matches(&ban, &call).is_none());
    }

    #[test]
    fn an_empty_target_list_never_matches() {
        let ban = rule(RuleKind::EgressBan, "targets", &[]);
        assert!(matches(&ban, &ToolCall::new("run_command", json!({"command": "ping 1.2.3.4"}))).is_none());
    }

    #[test]
    fn observations_deduplicate_and_sort() {
        let observed = egress_observations(&json!({
            "command": "curl http://db.x:8080/a && curl http://db.x/b",
            "host": "DB.x"
        }));
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].display, "db.x");
    }

    #[test]
    fn invalid_cidr_prefixes_are_not_patterns() {
        assert!(TargetPattern::parse("10.0.0.0/33").is_none());
        assert!(TargetPattern::parse("not an ip/").is_none());
    }
}
