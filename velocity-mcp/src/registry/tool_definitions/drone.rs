use crate::registry::types::Tool;
use serde_json::json;

pub fn get_drone_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "drone_deploy".to_string(),
            description: "Deploy the velocity-drone binary to a remote machine via SSH. Copies the drone binary and starts it as a background process. Returns the drone's connection details (host, port, id)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "Remote hostname or IP address (e.g., '192.168.1.100' or 'user@vm.example.com')." },
                    "port": { "type": "integer", "description": "SSH port on the remote machine. Defaults to 22." },
                    "drone_port": { "type": "integer", "description": "Port the drone should listen on. Defaults to 9191." },
                    "drone_name": { "type": "string", "description": "Human-readable name for the drone. Defaults to 'drone-<hostname>'." },
                    "auth_token": { "type": "string", "description": "Bearer token for drone API authentication. Auto-generated if omitted." },
                    "ssh_user": { "type": "string", "description": "SSH username. Defaults to current user." },
                    "ssh_key_path": { "type": "string", "description": "Path to SSH private key. Defaults to ~/.ssh/id_rsa." }
                },
                "required": ["host"]
            }),
        },
        Tool {
            name: "drone_command".to_string(),
            description: "Send a shell command to a deployed drone for execution. The drone runs the command asynchronously and returns a task ID. Use drone_task_status to poll for results."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "drone_url": { "type": "string", "description": "Drone base URL (e.g., 'http://192.168.1.100:9191')." },
                    "command": { "type": "string", "description": "Shell command to execute on the remote machine." },
                    "auth_token": { "type": "string", "description": "Bearer token for authentication." }
                },
                "required": ["drone_url", "command"]
            }),
        },
        Tool {
            name: "drone_task_status".to_string(),
            description: "Check the status of a previously submitted drone task. Returns exit code, stdout, stderr, and completion state."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "drone_url": { "type": "string", "description": "Drone base URL." },
                    "task_id": { "type": "string", "description": "Task ID returned by drone_command." },
                    "auth_token": { "type": "string", "description": "Bearer token for authentication." }
                },
                "required": ["drone_url", "task_id"]
            }),
        },
        Tool {
            name: "drone_status".to_string(),
            description: "Query a drone's health and identity. Returns the drone's ID, name, version, capabilities, uptime, and environment."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "drone_url": { "type": "string", "description": "Drone base URL (e.g., 'http://192.168.1.100:9191')." },
                    "auth_token": { "type": "string", "description": "Bearer token for authentication." }
                },
                "required": ["drone_url"]
            }),
        },
        Tool {
            name: "drone_screenshot".to_string(),
            description: "Capture a screenshot from the remote machine where the drone is running. Returns base64-encoded PNG image data."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "drone_url": { "type": "string", "description": "Drone base URL." },
                    "auth_token": { "type": "string", "description": "Bearer token for authentication." }
                },
                "required": ["drone_url"]
            }),
        },
        Tool {
            name: "drone_type_keys".to_string(),
            description: "Simulate keyboard input on the remote machine. Can type text or press key combinations (e.g., 'ctrl+c', 'alt+f4')."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "drone_url": { "type": "string", "description": "Drone base URL." },
                    "action": {
                        "type": "string",
                        "enum": ["type", "press_key"],
                        "description": "Either 'type' to type text, or 'press_key' to press a key combination."
                    },
                    "text": { "type": "string", "description": "Text to type (when action is 'type')." },
                    "key": { "type": "string", "description": "Key combination to press (when action is 'press_key'), e.g. 'ctrl+c'." },
                    "auth_token": { "type": "string", "description": "Bearer token for authentication." }
                },
                "required": ["drone_url", "action"]
            }),
        },
        Tool {
            name: "drone_click".to_string(),
            description: "Simulate a mouse click at specific coordinates on the remote machine."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "drone_url": { "type": "string", "description": "Drone base URL." },
                    "x": { "type": "integer", "description": "X coordinate." },
                    "y": { "type": "integer", "description": "Y coordinate." },
                    "button": { "type": "string", "enum": ["left", "right", "middle"], "description": "Mouse button. Defaults to 'left'." },
                    "auth_token": { "type": "string", "description": "Bearer token for authentication." }
                },
                "required": ["drone_url", "x", "y"]
            }),
        },
        Tool {
            name: "drone_network_stats".to_string(),
            description: "Get network statistics from the remote machine — bytes sent/received, active connections, packet counts."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "drone_url": { "type": "string", "description": "Drone base URL." },
                    "auth_token": { "type": "string", "description": "Bearer token for authentication." }
                },
                "required": ["drone_url"]
            }),
        },
        Tool {
            name: "drone_upload".to_string(),
            description: "Upload a file from the local workspace to the remote drone machine. Supports chunked transfer for large files with SHA-256 verification."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "drone_url": { "type": "string", "description": "Drone base URL." },
                    "local_path": { "type": "string", "description": "Local file path (relative to workspace root)." },
                    "remote_path": { "type": "string", "description": "Destination path on the remote machine." },
                    "deploy_instructions": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "action": { "type": "string", "enum": ["run", "copy", "notify"] },
                                "target": { "type": "string" }
                            }
                        },
                        "description": "Optional deploy instructions to execute after upload (run a script, copy to location, send notification)."
                    },
                    "auth_token": { "type": "string", "description": "Bearer token for authentication." }
                },
                "required": ["drone_url", "local_path", "remote_path"]
            }),
        },
        Tool {
            name: "drone_pair".to_string(),
            description: "Initiate pairing with a drone. After pairing, the drone appears in the IDE's peer panel and can receive messages and tasks through the peer protocol."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "drone_url": { "type": "string", "description": "Drone base URL." },
                    "drone_name": { "type": "string", "description": "Friendly name for the paired drone." },
                    "auth_token": { "type": "string", "description": "Bearer token for authentication." }
                },
                "required": ["drone_url"]
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drone_tool_count() {
        let tools = get_drone_tools();
        assert_eq!(tools.len(), 10);
    }

    #[test]
    fn test_drone_tool_names() {
        let tools = get_drone_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"drone_deploy"));
        assert!(names.contains(&"drone_command"));
        assert!(names.contains(&"drone_task_status"));
        assert!(names.contains(&"drone_status"));
        assert!(names.contains(&"drone_screenshot"));
        assert!(names.contains(&"drone_type_keys"));
        assert!(names.contains(&"drone_click"));
        assert!(names.contains(&"drone_network_stats"));
        assert!(names.contains(&"drone_upload"));
        assert!(names.contains(&"drone_pair"));
    }

    #[test]
    fn test_all_tools_have_required_fields() {
        for tool in get_drone_tools() {
            assert!(!tool.name.is_empty(), "tool name must not be empty");
            assert!(!tool.description.is_empty(), "{} has empty description", tool.name);
            assert!(
                tool.input_schema.get("type").is_some(),
                "{} missing input schema type",
                tool.name
            );
        }
    }
}
