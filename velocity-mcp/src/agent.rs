use std::path::PathBuf;
use std::io::BufRead;
use crossbeam_channel::{Sender, Receiver};
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use crate::registry;

#[derive(Debug, Clone)]
pub enum UiToAgentMessage {
    UserPrompt(String),
    ApproveTool { id: String, tool_name: String, arguments: Value },
    #[allow(dead_code)]
    RejectTool { id: String, tool_name: String },
}

#[derive(Debug, Clone)]
pub enum AgentToUiMessage {
    #[allow(dead_code)]
    ThoughtToken(String),
    OutputToken(String),
    RequestToolApproval { id: String, tool_name: String, arguments: Value },
    ToolExecutionStarted { tool_name: String },
    ToolExecutionFinished { tool_name: String, result: String },
    StatusUpdate(String),
    AgentFinished,
    UpdateFileBuffer { path: PathBuf, content: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
}

struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

struct CloudflareAccount {
    id: String,
    token: String,
}

fn load_accounts() -> Vec<CloudflareAccount> {
    dotenvy::dotenv().ok();
    let mut accounts = Vec::new();
    for i in 1..=30 {
        let id_key = format!("CF_ACCOUNT_{}_ID", i);
        let token_key = format!("CF_ACCOUNT_{}_TOKEN", i);
        if let (Ok(id), Ok(token)) = (std::env::var(&id_key), std::env::var(&token_key)) {
            accounts.push(CloudflareAccount { id, token });
        }
    }
    // Fallback
    if accounts.is_empty() {
        if let (Ok(id), Ok(token)) = (std::env::var("CF_ACCOUNT_ID"), std::env::var("CF_API_TOKEN")) {
            accounts.push(CloudflareAccount { id, token });
        }
    }
    accounts
}

fn compress_history(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let max_intact_tool_calls = 4;
    let tool_indices: Vec<usize> = messages.iter().enumerate()
        .filter(|(_, m)| m.role == "tool")
        .map(|(idx, _)| idx)
        .collect();

    let mut compressed = Vec::new();
    for (idx, m) in messages.iter().enumerate() {
        let mut m_copy = m.clone();
        if m.role == "tool" {
            let remaining_tools = tool_indices.iter().filter(|&&i| i > idx).count();
            if remaining_tools >= max_intact_tool_calls && m_copy.content.len() > 300 {
                let tool_name = m_copy.name.clone().unwrap_or_else(|| "unknown_tool".to_string());
                m_copy.content = format!(
                    "[Tool output of \"{}\" compressed to optimize context & cost. Original size: {} characters. Output successfully processed in a previous turn.]",
                    tool_name, m_copy.content.len()
                );
            }
        }
        compressed.push(m_copy);
    }
    compressed
}

fn pack_ndav(filename: &str, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"NDAV");
    let size = payload.len() as u32;
    buf.extend_from_slice(&size.to_le_bytes());
    buf.extend_from_slice(filename.as_bytes());
    buf.push(0);
    buf.extend_from_slice(payload);
    buf
}

fn unpack_ndav(data: &[u8]) -> Option<(String, Vec<u8>)> {
    if data.len() < 9 || &data[0..4] != b"NDAV" {
        return None;
    }
    let size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let mut name_end = 8;
    while name_end < data.len() && data[name_end] != 0 {
        name_end += 1;
    }
    if name_end >= data.len() {
        return None;
    }
    let filename = String::from_utf8_lossy(&data[8..name_end]).to_string();
    let payload_start = name_end + 1;
    if payload_start + size > data.len() {
        return None;
    }
    let payload = data[payload_start..payload_start + size].to_vec();
    Some((filename, payload))
}

fn generate_sitemap_text(workspace_root: &std::path::Path) -> String {
    let mut text = String::new();
    text.push_str("V.E.L.O.C.I.T.Y. Codebase Sitemap Registry\n");
    text.push_str("=========================================\n");
    
    fn scan_sitemap(dir: &std::path::Path, base: &std::path::Path, text: &mut String) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if file_name == ".git" || file_name == "target" || file_name == "node_modules" || file_name == ".velocity" {
                    continue;
                }
                
                if let Ok(meta) = entry.metadata() {
                    let rel_path = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().to_string();
                    if meta.is_dir() {
                        text.push_str(&format!("dir\t{}\n", rel_path));
                        scan_sitemap(&path, base, text);
                    } else {
                        text.push_str(&format!("file\t{}\t{}\n", rel_path, meta.len()));
                    }
                }
            }
        }
    }
    
    scan_sitemap(workspace_root, workspace_root, &mut text);
    text
}

fn write_sitemap_nda(workspace_root: &std::path::Path) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let sitemap_text = generate_sitemap_text(workspace_root);
    let nda_data = pack_ndav("sitemap.txt", sitemap_text.as_bytes());
    let _ = std::fs::write(sitemap_dir.join("sitemap.nda"), nda_data);
}

fn load_chatlogs_nda(workspace_root: &std::path::Path) -> Option<Vec<ChatMessage>> {
    let nda_path = workspace_root.join(".velocity").join("chatlogs.nda");
    if !nda_path.exists() {
        return None;
    }
    let data = std::fs::read(&nda_path).ok()?;
    let (_filename, payload) = unpack_ndav(&data)?;
    let text = String::from_utf8_lossy(&payload);
    let mut messages = Vec::new();
    for msg_block in text.split("\n---\n") {
        if msg_block.trim().is_empty() {
            continue;
        }
        let lines: Vec<&str> = msg_block.lines().collect();
        if lines.len() >= 2 {
            let role = lines[0].to_string();
            let mut content = lines[1..].join("\n");
            let mut name = None;
            let mut tool_call_id = None;
            if role == "tool" {
                if let Some(first_line) = lines.get(1) {
                    let parts: Vec<&str> = first_line.split('\t').collect();
                    if parts.len() == 2 {
                        name = Some(parts[0].to_string());
                        tool_call_id = Some(parts[1].to_string());
                        content = lines[2..].join("\n");
                    }
                }
            }
            messages.push(ChatMessage {
                role,
                content,
                name,
                tool_call_id,
                tool_calls: None,
            });
        }
    }
    if messages.is_empty() { None } else { Some(messages) }
}

fn save_chatlogs_nda(workspace_root: &std::path::Path, messages: &[ChatMessage]) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let mut text = String::new();
    for (i, msg) in messages.iter().enumerate() {
        if i > 0 {
            text.push_str("\n---\n");
        }
        text.push_str(&msg.role);
        text.push('\n');
        if msg.role == "tool" {
            let name = msg.name.as_deref().unwrap_or("unknown");
            let call_id = msg.tool_call_id.as_deref().unwrap_or("unknown");
            text.push_str(&format!("{}\t{}\n", name, call_id));
        }
        text.push_str(&msg.content);
    }
    let nda_data = pack_ndav("chatlogs.txt", text.as_bytes());
    let _ = std::fs::write(sitemap_dir.join("chatlogs.nda"), nda_data);
}

fn append_changelog_nda(workspace_root: &std::path::Path, file_path: &str, action: &str) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let changelog_path = sitemap_dir.join("changelog.nda");
    
    let mut current_changelog = String::new();
    if changelog_path.exists() {
        if let Ok(data) = std::fs::read(&changelog_path) {
            if let Some((_filename, payload)) = unpack_ndav(&data) {
                current_changelog = String::from_utf8_lossy(&payload).to_string();
            }
        }
    }
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = format!("{}\t{}\t{}\n", now, file_path, action);
    current_changelog.push_str(&entry);
    
    let nda_data = pack_ndav("changelog.txt", current_changelog.as_bytes());
    let _ = std::fs::write(changelog_path, nda_data);
}

fn write_handover_nda(workspace_root: &std::path::Path, task_state: &str, last_active_turn: usize, build_status: &str, interrupted: bool) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let handover_path = sitemap_dir.join("handover.nda");
    
    let payload = format!(
        "state: {}\nturn: {}\nbuild: {}\ninterrupted: {}\n",
        task_state, last_active_turn, build_status, interrupted
    );
    let nda_data = pack_ndav("handover.txt", payload.as_bytes());
    let _ = std::fs::write(handover_path, nda_data);
}

pub fn run_agent_thread(
    workspace_root: PathBuf,
    ui_rx: Receiver<UiToAgentMessage>,
    ui_tx: Sender<AgentToUiMessage>,
) {
    let accounts = load_accounts();
    let model = std::env::var("CF_MODEL")
        .unwrap_or_else(|_| "@cf/moonshotai/kimi-k2.7-code".to_string());

    let mut message_history = match load_chatlogs_nda(&workspace_root) {
        Some(history) => {
            ui_tx.send(AgentToUiMessage::StatusUpdate("Loaded previous chat session context.".to_string())).ok();
            history
        }
        None => vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE. You have access to local workspace files and execution sandboxes via tools. Help the user program the workspace. Always output concise, correct, and high-quality responses.".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            }
        ]
    };

    // Build the initial sitemap
    write_sitemap_nda(&workspace_root);

    ui_tx.send(AgentToUiMessage::StatusUpdate("Agent thread initialized and idling.".to_string())).ok();

    while let Ok(msg) = ui_rx.recv() {
        match msg {
            UiToAgentMessage::UserPrompt(prompt) => {
                message_history.push(ChatMessage {
                    role: "user".to_string(),
                    content: prompt,
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                });
                
                run_agent_reasoning_loop(
                    &workspace_root,
                    &accounts,
                    &model,
                    &mut message_history,
                    &ui_rx,
                    &ui_tx,
                );
            }
            _ => {}
        }
    }
}

fn convert_jsonl_to_nda(workspace_root: &std::path::Path) {
    let conv_id = "17bd30f6-be7a-4829-b5b9-023fa4dd8c59";
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\visse".to_string());
    let transcript_path = std::path::Path::new(&home)
        .join(".gemini")
        .join("antigravity")
        .join("brain")
        .join(conv_id)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl");
        
    if let Ok(content) = std::fs::read(&transcript_path) {
        let nda_payload = pack_ndav("transcript.txt", &content);
        let nda_path = transcript_path.with_extension("nda");
        let _ = std::fs::write(nda_path, &nda_payload);
        
        let workspace_nda = workspace_root.join(".velocity").join("transcript.nda");
        let _ = std::fs::write(workspace_nda, &nda_payload);
    }
}

fn run_compilation_check(workspace_root: &std::path::Path) -> Result<(), String> {
    let output = std::process::Command::new("cargo")
        .arg("check")
        .current_dir(workspace_root)
        .output();
        
    match output {
        Ok(out) => {
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let mut errors = Vec::new();
                for line in stderr.lines() {
                    let trimmed = line.trim();
                    if trimmed.contains("error[E") || trimmed.contains("error:") || trimmed.starts_with("--> src/") {
                        errors.push(trimmed.to_string());
                    }
                }
                if errors.is_empty() {
                    // Build failed but no parseable errors; return raw tail.
                    let lines: Vec<&str> = stderr.lines().collect();
                    let start = lines.len().saturating_sub(10);
                    return Err(lines[start..].iter().map(|s| s.to_string()).collect::<Vec<_>>().join("\n"));
                }
                return Err(errors.join("\n"));
            }
            Ok(())
        }
        Err(e) => Err(format!("Failed to execute cargo check: {:?}", e))
    }
}

fn run_agent_reasoning_loop(
    workspace_root: &PathBuf,
    accounts: &[CloudflareAccount],
    model: &str,
    message_history: &mut Vec<ChatMessage>,
    ui_rx: &Receiver<UiToAgentMessage>,
    ui_tx: &Sender<AgentToUiMessage>,
) {
    let mut loop_count = 0;
    let max_loops = 15;
    
    // Map registered tools to Workers AI schema
    let registered_tools = registry::get_tools();
    let cf_tools: Vec<Value> = registered_tools.iter().map(|t| {
        json!({
            "type": "function",
            "function": {
                "name": t.name,
                "description": t.description,
                "parameters": t.input_schema
            }
        })
    }).collect();

    while loop_count < max_loops {
        loop_count += 1;
        ui_tx.send(AgentToUiMessage::StatusUpdate(format!("Querying Workers AI (Turn {})...", loop_count))).ok();

        let compressed_history = compress_history(message_history);

        let request_body = json!({
            "model": model,
            "messages": compressed_history,
            "tools": cf_tools,
            "stream": true
        });

        let mut response_opt = None;
        for account in accounts {
            let api_url = format!(
                "https://api.cloudflare.com/client/v4/accounts/{}/ai/v1/chat/completions",
                account.id
            );

            match ureq::post(&api_url)
                .set("Authorization", &format!("Bearer {}", account.token))
                .set("Content-Type", "application/json")
                .send_json(&request_body)
            {
                Ok(res) => {
                    response_opt = Some(res);
                    break;
                }
                Err(e) => {
                    eprintln!("Account {} failed or rate-limited: {:?}", account.id, e);
                }
            }
        }

        let response = match response_opt {
            Some(res) => res,
            None => {
                let err_msg = "All Cloudflare Workers AI accounts exhausted or failed.";
                ui_tx.send(AgentToUiMessage::OutputToken(format!("\n\nError: {}", err_msg))).ok();
                break;
            }
        };

        let mut reader = std::io::BufReader::new(response.into_reader());
        let mut line_buf = String::new();
        
        let mut assistant_content = String::new();
        let mut accumulated_tools: Vec<ToolCallAccumulator> = Vec::new();
        
        loop {
            line_buf.clear();
            match reader.read_line(&mut line_buf) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let cleaned = line_buf.trim();
                    if cleaned.is_empty() {
                        continue;
                    }
                    if cleaned == "data: [DONE]" {
                        break;
                    }
                    if cleaned.starts_with("data: ") {
                        let json_part = &cleaned[6..];
                        if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                            if let Some(choices) = parsed["choices"].as_array() {
                                if let Some(first_choice) = choices.get(0) {
                                    let delta = &first_choice["delta"];
                                    
                                    // Extract content tokens
                                    if let Some(content) = delta["content"].as_str() {
                                        assistant_content.push_str(content);
                                        ui_tx.send(AgentToUiMessage::OutputToken(content.to_string())).ok();
                                    }
                                    
                                    // Extract tool calls tokens
                                    if let Some(tool_calls) = delta["tool_calls"].as_array() {
                                        for tc in tool_calls {
                                            let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                                            
                                            // Ensure space in vector
                                            while accumulated_tools.len() <= idx {
                                                accumulated_tools.push(ToolCallAccumulator {
                                                    id: String::new(),
                                                    name: String::new(),
                                                    arguments: String::new(),
                                                });
                                            }
                                            
                                            if let Some(id) = tc["id"].as_str() {
                                                accumulated_tools[idx].id.push_str(id);
                                            }
                                            
                                            if let Some(func) = tc["function"].as_object() {
                                                if let Some(name) = func.get("name").and_then(|n| n.as_str()) {
                                                    accumulated_tools[idx].name.push_str(name);
                                                }
                                                if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                                                    accumulated_tools[idx].arguments.push_str(args);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    ui_tx.send(AgentToUiMessage::OutputToken(format!("\nError reading stream: {:?}", e))).ok();
                    break;
                }
            }
        }

        // Store assistant response in history
        let final_tool_calls_value = if !accumulated_tools.is_empty() {
            let tc_json: Vec<Value> = accumulated_tools.iter().map(|t| {
                json!({
                    "id": t.id,
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "arguments": t.arguments
                    }
                })
            }).collect();
            Some(Value::Array(tc_json))
        } else {
            None
        };

        message_history.push(ChatMessage {
            role: "assistant".to_string(),
            content: assistant_content.clone(),
            name: None,
            tool_call_id: None,
            tool_calls: final_tool_calls_value.clone(),
        });

        // Handle tool calls if any
        if let Some(ref tcs) = final_tool_calls_value {
            let tool_calls_arr = tcs.as_array().unwrap();
            
            for tc in tool_calls_arr {
                let call_id = tc["id"].as_str().unwrap_or("").to_string();
                let tool_name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                
                let arguments: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                
                ui_tx.send(AgentToUiMessage::StatusUpdate(format!("Requesting approval for tool: {}", tool_name))).ok();
                ui_tx.send(AgentToUiMessage::RequestToolApproval {
                    id: call_id.clone(),
                    tool_name: tool_name.clone(),
                    arguments: arguments.clone(),
                }).ok();
                
                // Wait for approval response
                let mut tool_result = String::new();
                
                while let Ok(ui_msg) = ui_rx.recv() {
                    match ui_msg {
                        UiToAgentMessage::ApproveTool { id, tool_name: _, arguments: approved_args } => {
                            if id == call_id {
                                ui_tx.send(AgentToUiMessage::ToolExecutionStarted { tool_name: tool_name.clone() }).ok();
                                
                                let active_dir = std::env::current_dir().unwrap_or_else(|_| workspace_root.clone());
                                // Execute tool in our native registry
                                match registry::call_tool(&tool_name, &approved_args) {
                                    Ok(res) => {
                                        tool_result = res;
                                        // If tool is writing/modifying a file, send direct buffer update to UI
                                        if tool_name == "write_file" {
                                            if let Some(rel_path) = approved_args["relativeFilePath"].as_str() {
                                                let full_path = active_dir.join(rel_path);
                                                if let Some(content) = approved_args["content"].as_str() {
                                                    ui_tx.send(AgentToUiMessage::UpdateFileBuffer {
                                                        path: full_path,
                                                        content: content.to_string(),
                                                     }).ok();
                                                }
                                                // Log change to changelog
                                                append_changelog_nda(&active_dir, rel_path, "write_file");
                                                // Refresh codebase sitemap
                                                write_sitemap_nda(&active_dir);
                                            }
                                        }
                                        write_handover_nda(&active_dir, "executing", loop_count, "ok", false);
                                    }
                                    Err(e) => {
                                        tool_result = format!("Error executing tool: {:?}", e);
                                        write_handover_nda(&active_dir, "tool_error", loop_count, &format!("{:?}", e), false);
                                    }
                                }
                                ui_tx.send(AgentToUiMessage::ToolExecutionFinished {
                                    tool_name: tool_name.clone(),
                                    result: tool_result.clone(),
                                }).ok();
                                break;
                            }
                        }
                        UiToAgentMessage::RejectTool { id, tool_name: _ } => {
                            if id == call_id {
                                tool_result = "Error: Tool execution rejected by the user.".to_string();
                                let active_dir = std::env::current_dir().unwrap_or_else(|_| workspace_root.clone());
                                write_handover_nda(&active_dir, "tool_rejected", loop_count, "user_reject", false);
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                // Add tool response to history
                message_history.push(ChatMessage {
                    role: "tool".to_string(),
                    content: tool_result,
                    name: Some(tool_name.clone()),
                    tool_call_id: Some(call_id),
                    tool_calls: None,
                });
                
                // Save progress chatlogs
                let active_dir = std::env::current_dir().unwrap_or_else(|_| workspace_root.clone());
                save_chatlogs_nda(&active_dir, message_history);
            }
        } else {
            // No tool calls, we are done
            break;
        }
    }
    
    let active_dir = std::env::current_dir().unwrap_or_else(|_| workspace_root.clone());
    save_chatlogs_nda(&active_dir, message_history);
    
    // Perform compilation check
    ui_tx.send(AgentToUiMessage::StatusUpdate("Running automatic compilation validation...".to_string())).ok();
    match run_compilation_check(&active_dir) {
        Ok(()) => {
            write_handover_nda(&active_dir, "idle", loop_count, "compiler validated", false);
            ui_tx.send(AgentToUiMessage::StatusUpdate("Compiler validation succeeded!".to_string())).ok();
        }
        Err(errors) => {
            if loop_count < max_loops {
                write_handover_nda(&active_dir, "self_correcting", loop_count, "compile_failed", false);
                ui_tx.send(AgentToUiMessage::StatusUpdate("Compilation failed! Self-correcting...".to_string())).ok();
                
                let error_prompt = format!(
                    "[SYSTEM NOTIFICATION: compiler validation failed. Please fix the following build errors immediately]\n{}",
                    errors
                );
                message_history.push(ChatMessage {
                    role: "user".to_string(),
                    content: error_prompt,
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                });
                
                // Recursively self-correct
                run_agent_reasoning_loop(
                    workspace_root,
                    accounts,
                    model,
                    message_history,
                    ui_rx,
                    ui_tx,
                );
                return;
            } else {
                write_handover_nda(&active_dir, "idle", loop_count, "compile_failed", false);
                ui_tx.send(AgentToUiMessage::StatusUpdate("Compilation validation failed (Max limits reached)".to_string())).ok();
            }
        }
    }
    
    convert_jsonl_to_nda(&active_dir);
    ui_tx.send(AgentToUiMessage::StatusUpdate("Agent workflow finished. Idling.".to_string())).ok();
    ui_tx.send(AgentToUiMessage::AgentFinished).ok();
}
