use crate::registry::types::Tool;
use serde_json::json;

/// Managed-instance tools: the IDE's list of remote nodes (buildboxes, agent
/// hosts, GUI servers) it can allocate work across. The registry is bookkeeping
/// only - `instance_ping` and `instance_deploy` are what talk to the drone on
/// the other end.
pub fn get_instance_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "instance_add".to_string(),
            description: "Register a managed instance (remote node) in the workspace registry at .velocity/instances.json. Requires a display name, a role (buildbox | agent_host | gui_server) and the drone endpoint address. Does not contact the node: use instance_ping to establish liveness."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Display name; the record id is its slug (lowercase alphanumerics joined by hyphens)." },
                    "role": { "type": "string", "enum": ["buildbox", "agent_host", "gui_server"], "description": "What the node is kept for. Defaults to 'buildbox'." },
                    "drone_addr": { "type": "string", "description": "Drone endpoint as host or host:port (e.g. '192.168.0.5' or 'buildbox.local:9191'). A bare host gets the default drone port 9191; an http:// URL is accepted and stored as host:port." },
                    "platform": { "type": "string", "description": "OS family: linux, windows or macos. Defaults to 'unknown'; instance_ping adopts it from the drone's own report." },
                    "ssh": { "type": "string", "description": "Optional SSH endpoint 'user@host[:port]' used to (re)deploy the drone binary." },
                    "work_dir": { "type": "string", "description": "Workspace path on the remote box where builds run." },
                    "labels": { "type": ["array", "string"], "items": { "type": "string" }, "description": "Capability tags routing filters on ('rust', 'gpu', 'arm64'). Pass an array or a comma-separated string; lower-cased on save." },
                    "enabled": { "type": "boolean", "description": "Disable a node without deleting it. Defaults to true." },
                    "auth_token": { "type": "string", "description": "Bearer token the drone expects. Stored on the record; never returned by any tool." },
                    "id": { "type": "string", "description": "Override the derived id (rarely needed; ids must be unique)." }
                },
                "required": ["name", "drone_addr"]
            }),
        },
        Tool {
            name: "instance_list".to_string(),
            description: "List managed instances with their stored and effective status, eligibility and in-flight load. A node marked online but not seen healthy within 120s reports effective_status 'offline' - routing follows the effective status, not the stored one. Auth tokens are never included."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role": { "type": "string", "enum": ["buildbox", "agent_host", "gui_server"], "description": "Only show nodes with this role." },
                    "platform": { "type": "string", "description": "Only show nodes on this OS family (linux/windows/macos)." },
                    "eligible_only": { "type": "boolean", "description": "Drop nodes that cannot receive work right now (disabled, or not seen healthy recently). Defaults to false." }
                }
            }),
        },
        Tool {
            name: "instance_remove".to_string(),
            description: "Remove a managed instance from the registry by id. Does not stop the drone process on the remote machine - only forgets it locally."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Record id (the slug of the name, as reported by instance_list)." }
                },
                "required": ["id"]
            }),
        },
        Tool {
            name: "instance_ping".to_string(),
            description: "Query a registered instance's drone /peer/health and record the outcome: reachable nodes become 'online', an unreachable one drops to 'degraded' or 'offline' and that downgrade is persisted. If the record had no platform, it is adopted from the drone's environment report (e.g. 'linux-x86_64' -> linux)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Record id to probe." }
                },
                "required": ["id"]
            }),
        },
        Tool {
            name: "instance_deploy".to_string(),
            description: "Push the velocity-drone binary to a machine over SSH, start it, verify health, and register the node in the registry in one step - the buildbox equivalent of plugging it in. Generates a bearer token and stores it on the record (it is never returned). Fails if the drone binary is not built; build with: cargo build --release -p velocity-drone for the node's platform."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "Machine to deploy to (IP or hostname)." },
                    "name": { "type": "string", "description": "Registry display name. Defaults to the host with dots replaced by hyphens." },
                    "role": { "type": "string", "enum": ["buildbox", "agent_host", "gui_server"], "description": "Defaults to 'buildbox'." },
                    "platform": { "type": "string", "description": "OS family of the target; set it or let instance_ping adopt it from the drone." },
                    "ssh_user": { "type": "string", "description": "SSH username. Defaults to the current local user." },
                    "ssh_key_path": { "type": "string", "description": "Private key for SSH. Key auth only: the deployer cannot answer a password prompt." },
                    "ssh_port": { "type": "integer", "description": "SSH port. Defaults to 22." },
                    "drone_port": { "type": "integer", "description": "Port the drone listens on. Defaults to 9191." },
                    "drone_name": { "type": "string", "description": "Name the drone reports about itself. Defaults to the registry name." },
                    "work_dir": { "type": "string", "description": "Workspace path on the remote box for builds." },
                    "labels": { "type": ["array", "string"], "items": { "type": "string" }, "description": "Capability tags for routing." },
                    "auth_token": { "type": "string", "description": "Use a specific bearer token instead of a generated one." }
                },
                "required": ["host"]
            }),
        },
        Tool {
            name: "instance_pick".to_string(),
            description: "Choose which eligible node should take a piece of work: filters by role/platform/labels, then selects the fewest in-flight items (ties broken by id, so identical state always routes identically). Returns success false with an explanation when nothing qualifies."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role": { "type": "string", "enum": ["buildbox", "agent_host", "gui_server"], "description": "Restrict to nodes of this role." },
                    "platform": { "type": "string", "description": "Restrict to this OS family - how a Linux build lands on a Linux box instead of failing locally." },
                    "labels": { "type": ["array", "string"], "items": { "type": "string" }, "description": "Every tag listed must be present on the node." },
                    "claim": { "type": "boolean", "description": "Increment the node's in-flight counter and persist, so the next pick spreads load. Release with instance_release when the work finishes. Defaults to false." }
                }
            }),
        },
        Tool {
            name: "instance_release".to_string(),
            description: "Mark one in-flight item on a node as finished, undoing an instance_pick claim. The counter never goes below zero, so a double release is harmless."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Record id that was claimed." }
                },
                "required": ["id"]
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_tool_count() {
        assert_eq!(get_instance_tools().len(), 7);
    }

    #[test]
    fn instance_tool_names_are_the_dispatched_set() {
        let names: Vec<String> = get_instance_tools().into_iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            vec![
                "instance_add".to_string(),
                "instance_list".to_string(),
                "instance_remove".to_string(),
                "instance_ping".to_string(),
                "instance_deploy".to_string(),
                "instance_pick".to_string(),
                "instance_release".to_string(),
            ]
        );
    }

    #[test]
    fn every_instance_tool_has_schema_and_description() {
        for tool in get_instance_tools() {
            assert!(
                !tool.description.is_empty(),
                "{}: empty description",
                tool.name
            );
            assert_eq!(
                tool.input_schema.get("type").and_then(|t| t.as_str()),
                Some("object"),
                "{}: schema must be an object type",
                tool.name
            );
            // Every `required` key must actually be described, or the agent has
            // no way to learn the field it is being refused for.
            if let Some(req) = tool.input_schema.get("required").and_then(|r| r.as_array()) {
                let props = &tool.input_schema["properties"];
                for key in req {
                    let key = key.as_str().unwrap_or_default();
                    assert!(
                        props.get(key).is_some(),
                        "{}: required field {key:?} is not in properties",
                        tool.name
                    );
                }
            }
        }
    }
}
