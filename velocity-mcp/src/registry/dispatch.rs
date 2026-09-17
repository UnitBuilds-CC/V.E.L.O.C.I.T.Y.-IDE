use super::browser_tools::handle_browser_tool;
use super::custom_tools;
use super::event_store;
use super::system_tools::handle_system_tool;
use super::team_tools::handle_team_tool;
use super::wa_tools::handle_wa_tool;
use crate::agent::drone_bridge::handle_drone_tool;
use crate::errors::ToolError;
use crate::security::audit::{self, ToolAuditOutcome};
use serde_json::Value;
use std::error::Error;
use std::path::Path;
use std::time::Instant;

pub fn call_tool_in_workspace(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    let root = root.canonicalize().map_err(|e| {
        log::error!(
            "tool dispatch: failed to canonicalize workspace root: {}",
            e
        );
        e
    })?;

    // Governance gate: deny or park-for-approval per the workspace policy. With
    // no policy configured this allows everything (no behavior change).
    crate::editor::governance::gate_tool_call(&root, name, arguments).map_err(|e| {
        log::warn!("tool dispatch: governance denied '{}': {}", name, e);
        e
    })?;

    let start = Instant::now();

    // Capture the site map Merkle root *before* the tool call so we can
    // detect state changes and record codebase events.
    let merkle_before = read_sitemap_merkle_root(&root);

    // Try built-in tool handlers first (in priority order).
    let builtin = try_builtin_tools(&root, name, arguments);

    match &builtin {
        Ok(Some(output)) => {
            // Capture the site map Merkle root after the tool call.
            let merkle_after = read_sitemap_merkle_root(&root);
            audit::record_tool_call_full(
                "stdio",
                name,
                start,
                ToolAuditOutcome::Success,
                merkle_after.map(|r| format!("{:016x}", r)),
            );
            // ── Universal codebase event recording ─────────────────────
            // Every successful tool call is recorded in the event store.
            // Context (the *why*) is read generically from the tool call
            // arguments so agents cannot forget to declare their changes.
            // Useless events (grep, list_dir) can be pruned later; the
            // important ones (write, delete, index, edit) are always kept.
            {
                let ctx = arguments["context"].as_str();
                let affected = extract_affected_files(name, arguments);
                let description = build_event_description(name, arguments);
                let store = event_store::EventStore::open(&root);
                let _ = store.record(
                    name,
                    &description,
                    merkle_before.map(|r| format!("{:016x}", r)),
                    merkle_after.map(|r| format!("{:016x}", r)),
                    ctx.map(|s| s.to_string()),
                    affected,
                );
            }
            // Flush audit log after site-map-mutating tools.
            if is_sitemap_tool(name) {
                let _ = audit::flush_all_sessions_to_dir(&root.join(".velocity"));
            }
            // Enrich read_file responses with the decision trail so the
            // agent sees *why* code exists, not just *what* it contains.
            if name == "read_file" {
                if let Some(rel_path) = arguments["relativeFilePath"].as_str() {
                    let enriched =
                        event_store::enrich_read_response(&root, rel_path, output);
                    return Ok(enriched);
                }
            }
            Ok(output.clone())
        }
        Ok(None) => {
            // No built-in handler — try custom tools.
            let custom_result = try_custom_tool(&root, name, arguments);
            let outcome = match &custom_result {
                Ok(_) => ToolAuditOutcome::Success,
                Err(e) => ToolAuditOutcome::Error(e.to_string()),
            };
            audit::record_tool_call("stdio", name, start, outcome);
            custom_result
        }
        Err(e) => {
            audit::record_tool_call("stdio", name, start, ToolAuditOutcome::Error(e.to_string()));
            Err(e.to_string().into())
        }
    }
}

/// Try all built-in tool handlers. Returns `Ok(None)` if no handler recognizes
/// the tool name.
fn try_builtin_tools(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    if let Some(result) = handle_system_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_team_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_browser_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_wa_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_drone_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    Ok(None)
}

/// Try to execute a dynamically registered custom tool.
fn try_custom_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    if let Some(tool) = custom_tools::get_tool(root, name) {
        custom_tools::execute_tool(root, &tool, arguments)
    } else {
        log::warn!("tool dispatch: unknown tool '{}'", name);
        // Return the structured variant rather than an opaque string: callers
        // (the JSON-RPC layer, the parity tests, plugin bridges) branch on the
        // failure mode instead of matching message text.
        Err(Box::new(ToolError::ToolNotFound(name.to_string())))
    }
}

/// Read the current site map Merkle root from the workspace's persisted metadata.
/// Returns None if the site map doesn't exist or can't be read.
fn read_sitemap_merkle_root(root: &Path) -> Option<u64> {
    let sitemap_dir = root.join(".velocity").join("site_map");
    let index_path = sitemap_dir.join("index.json");
    if !index_path.exists() {
        return None;
    }
    velocity_ide::site_map::SiteMap::read_persisted_root(&sitemap_dir)
}

/// Returns true if the tool is known to mutate the site map index.
fn is_sitemap_tool(name: &str) -> bool {
    matches!(name, "index_workspace")
}

/// Extract affected file paths from tool arguments (best-effort).
fn extract_affected_files(name: &str, arguments: &Value) -> Vec<String> {
    // Tools that carry a file path in a known argument.
    if let Some(path) = arguments["relativeFilePath"].as_str() {
        return vec![path.to_string()];
    }
    if let Some(path) = arguments["filePath"].as_str() {
        return vec![path.to_string()];
    }
    if let Some(path) = arguments["path"].as_str() {
        // index_workspace, knowledge_ingest, etc.
        if matches!(name, "index_workspace" | "knowledge_ingest") {
            return vec![path.to_string()];
        }
    }
    Vec::new()
}

/// Build a human-readable event description from the tool name and arguments.
fn build_event_description(name: &str, arguments: &Value) -> String {
    match name {
        "write_file" => {
            let path = arguments["relativeFilePath"].as_str().unwrap_or("unknown");
            format!("Wrote {}", path)
        }
        "delete_file" => {
            let path = arguments["relativeFilePath"].as_str().unwrap_or("unknown");
            format!("Deleted {}", path)
        }
        "read_file" => {
            let path = arguments["relativeFilePath"].as_str().unwrap_or("unknown");
            format!("Read {}", path)
        }
        "index_workspace" => {
            let path = arguments["path"].as_str().unwrap_or("entire workspace");
            format!("Indexed {}", path)
        }
        "grep_search" => {
            let query = arguments["query"].as_str().unwrap_or("?");
            format!("Searched for '{}'", query)
        }
        "run_command" => {
            let cmd = arguments["command"].as_str().unwrap_or("?");
            let preview = if cmd.len() > 60 { &cmd[..60] } else { cmd };
            format!("Ran command: {}", preview)
        }
        "knowledge_ingest" => {
            let source = arguments["source"]
                .as_str()
                .or_else(|| arguments["path"].as_str())
                .unwrap_or("unknown");
            format!("Ingested knowledge from {}", source)
        }
        _ => format!("Called {}", name),
    }
}
