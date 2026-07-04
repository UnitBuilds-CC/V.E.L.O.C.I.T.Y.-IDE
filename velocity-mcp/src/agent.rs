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
    RejectTool { id: String, tool_name: String },
}

#[derive(Debug, Clone)]
pub enum AgentToUiMessage {
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

pub fn run_agent_thread(
    workspace_root: PathBuf,
    ui_rx: Receiver<UiToAgentMessage>,
    ui_tx: Sender<AgentToUiMessage>,
) {
    let accounts = load_accounts();
    let model = std::env::var("CF_MODEL")
        .unwrap_or_else(|_| "@cf/moonshotai/kimi-k2.7-code".to_string());

    let mut message_history: Vec<ChatMessage> = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE. You have access to local workspace files and execution sandboxes via tools. Help the user program the workspace. Always output concise, correct, and high-quality responses.".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    ];

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
                                
                                // Execute tool in our native registry
                                match registry::call_tool(&tool_name, &approved_args) {
                                    Ok(res) => {
                                        tool_result = res;
                                        // If tool is writing/modifying a file, send direct buffer update to UI
                                        if tool_name == "write_file" {
                                            if let Some(rel_path) = approved_args["relativeFilePath"].as_str() {
                                                let full_path = workspace_root.join(rel_path);
                                                if let Some(content) = approved_args["content"].as_str() {
                                                    ui_tx.send(AgentToUiMessage::UpdateFileBuffer {
                                                        path: full_path,
                                                        content: content.to_string(),
                                                     }).ok();
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tool_result = format!("Error executing tool: {:?}", e);
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
            }
        } else {
            // No tool calls, we are done
            break;
        }
    }
    
    ui_tx.send(AgentToUiMessage::StatusUpdate("Agent workflow finished. Idling.".to_string())).ok();
    ui_tx.send(AgentToUiMessage::AgentFinished).ok();
}
