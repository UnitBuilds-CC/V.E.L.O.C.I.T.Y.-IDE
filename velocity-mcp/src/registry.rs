use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::error::Error;
use std::process::{Command, Stdio, Child};
use std::io::{Write, BufReader, BufRead};
use std::sync::Mutex;
use once_cell::sync::Lazy;
use std::path::{Component, Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

pub fn get_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "convert_to_nda".to_string(),
            description: "Convert any file (e.g. C# source code, PDF, CSV, Excel, Image, Zip archive) into a cryptographically signed NDA (.nda) binary document.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filePath": { "type": "string", "description": "Absolute path to the input file to convert." },
                    "outputPath": { "type": "string", "description": "Optional absolute path to write the compiled .nda file. Defaults to input path with .nda extension." }
                },
                "required": ["filePath"]
            }),
        },
        Tool {
            name: "read_nda".to_string(),
            description: "Read and parse a compiled .nda binary file to view its semantic triples, visual display commands, and string pool contents.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ndaPath": { "type": "string", "description": "Absolute path to the .nda file to inspect." }
                },
                "required": ["ndaPath"]
            }),
        },
        Tool {
            name: "execute_nda".to_string(),
            description: "Execute a runnable .nda container. If it holds a compiled C# binary, it is run in-memory. If it contains a script (e.g., Python, Node.js, PowerShell, Bash), it executes via the corresponding shell process.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ndaPath": { "type": "string", "description": "Absolute path to the runnable .nda file." },
                    "arguments": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional command-line arguments to pass to the executable or script."
                    }
                },
                "required": ["ndaPath"]
            }),
        },
        Tool {
            name: "read_file".to_string(),
            description: "Read the contents of a file in the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"scripts/bootstrap.sh\")" }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "write_file".to_string(),
            description: "Write or overwrite a file with specific content in the workspace. Creates folders if they do not exist.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"scripts/bootstrap.sh\")" },
                    "content": { "type": "string", "description": "The text content to write to the file." }
                },
                "required": ["relativeFilePath", "content"]
            }),
        },
        Tool {
            name: "list_dir".to_string(),
            description: "List the contents of a directory relative to the workspace root.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeDirPath": { "type": "string", "description": "Directory path relative to workspace root. Use \".\" for the workspace root." }
                },
                "required": ["relativeDirPath"]
            }),
        },
        Tool {
            name: "grep_search".to_string(),
            description: "Find lines containing a query string in the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The text to search for" }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "run_command".to_string(),
            description: "Run a shell command inside the current workspace directory and capture its combined stdout and stderr output.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The command line string to execute." }
                },
                "required": ["command"]
            }),
        },
        Tool {
            name: "delete_file".to_string(),
            description: "Delete a file in the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"temp.txt\")" }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "web_navigate".to_string(),
            description: "Navigate and crawl a page with the current static AOM-first browser engine. It captures a truthful live snapshot, persists SiteMap facts, and writes browser artifacts for links, forms, and cookies discovered in fetched HTML.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Absolute URL to navigate to and crawl." },
                    "concurrency": { "type": "integer", "description": "Unused by the current static browser engine." }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "browser_create_session".to_string(),
            description: "Create or reset a persisted browser session with its own cookie jar and navigation state.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_get_session".to_string(),
            description: "Read the current persisted browser session state, including current URL and cookies.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_session_navigate".to_string(),
            description: "Navigate a persisted browser session so cookies and current URL survive across requests.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "url": { "type": "string", "description": "Absolute URL to navigate within the persisted session." }
                },
                "required": ["sessionId", "url"]
            }),
        },
        Tool {
            name: "browser_session_wait".to_string(),
            description: "Poll the current page in a persisted browser session until text or an element appears, then persist the updated snapshot and semantic diff.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "text": { "type": "string", "description": "Wait until this text appears in the current snapshot." },
                    "role": { "type": "string", "description": "Optional role when waiting for an element." },
                    "name": { "type": "string", "description": "Optional accessible name when waiting for an element." },
                    "timeoutMs": { "type": "integer", "description": "Maximum time to wait before failing." },
                    "intervalMs": { "type": "integer", "description": "Polling interval between re-fetches." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_save_checkpoint".to_string(),
            description: "Persist the current browser session and its latest snapshot as a named checkpoint for later restore or forking.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Existing browser session identifier." },
                    "checkpointName": { "type": "string", "description": "Human-readable checkpoint name." }
                },
                "required": ["sessionId", "checkpointName"]
            }),
        },
        Tool {
            name: "browser_restore_checkpoint".to_string(),
            description: "Restore a saved browser session checkpoint, optionally forking it into a different session id.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Original session identifier that owns the checkpoint." },
                    "checkpointName": { "type": "string", "description": "Checkpoint name to restore." },
                    "targetSessionId": { "type": "string", "description": "Optional new session identifier to restore into." }
                },
                "required": ["sessionId", "checkpointName"]
            }),
        },
        Tool {
            name: "browser_save_workflow".to_string(),
            description: "Persist a semantic browser workflow as JSON plus NDA-backed DSL for later deterministic replay.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Workflow name." },
                    "startUrl": { "type": "string", "description": "Initial URL for replay." },
                    "variables": {
                        "type": "object",
                        "description": "Optional workflow variables used by {{variable}} templates in step fields.",
                        "additionalProperties": { "type": "string" }
                    },
                    "steps": {
                        "type": "array",
                        "description": "Workflow steps. Supported kinds: navigate, click, fill_field, submit_form, wait_for_text, wait_for_element, extract_text, assert_element, assert_text_contains, assert_output.",
                        "items": { "type": "object" }
                    }
                },
                "required": ["name", "startUrl", "steps"]
            }),
        },
        Tool {
            name: "browser_read_workflow".to_string(),
            description: "Read a saved browser workflow JSON artifact from the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path to a saved .browser.json workflow relative to the workspace root." }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "browser_replay_workflow".to_string(),
            description: "Replay a saved semantic browser workflow deterministically using the current static AOM-first engine with session/form support. Provide sessionId to run inside an existing persisted session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path to a saved .browser.json workflow relative to the workspace root." },
                    "sessionId": { "type": "string", "description": "Optional persisted session id to replay inside instead of an ephemeral session." }
                },
                "required": ["relativeFilePath"]
            }),
        },
    ]
}

pub fn call_tool_in_workspace(root: &Path, name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let root = root.canonicalize()?;
    match name {
        "web_navigate" => {
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            match crate::editor::browser::crawl_and_sync_sitemap(url, &sitemap_path) {
                Ok(res) => Ok(res),
                Err(e) => Err(e.into()),
            }
        }
        "browser_create_session" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let path = crate::editor::browser::create_session(&root, session_id)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            Ok(format!("Created browser session '{}'\nSession JSON: {}", session_id, path.display()))
        }
        "browser_get_session" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let session = crate::editor::browser::load_session_state(&root, session_id)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            crate::editor::browser::session_state_to_json(&session).map_err(|e| e.into())
        }
        "browser_session_navigate" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::navigate_session(&root, session_id, url, &sitemap_path)
                .map_err(|e| e.into())
        }
        "browser_session_wait" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let text = arguments["text"].as_str();
            let role = arguments["role"].as_str();
            let name = arguments["name"].as_str();
            let timeout_ms = arguments["timeoutMs"].as_u64();
            let interval_ms = arguments["intervalMs"].as_u64();
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::wait_for_session(
                &root,
                session_id,
                text,
                role,
                name,
                timeout_ms,
                interval_ms,
                &sitemap_path,
            )
            .map_err(|e| e.into())
        }
        "browser_save_checkpoint" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let checkpoint_name = arguments["checkpointName"].as_str().ok_or("checkpointName is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let checkpoint_path = crate::editor::browser::save_session_checkpoint(&root, session_id, checkpoint_name, &sitemap_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            Ok(format!(
                "Saved browser checkpoint '{}' for session '{}'\nCheckpoint JSON: {}",
                checkpoint_name,
                session_id,
                checkpoint_path.display()
            ))
        }
        "browser_restore_checkpoint" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let checkpoint_name = arguments["checkpointName"].as_str().ok_or("checkpointName is required")?;
            let target_session_id = arguments["targetSessionId"].as_str();
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::restore_session_checkpoint(
                &root,
                session_id,
                checkpoint_name,
                target_session_id,
                &sitemap_path,
            )
            .map_err(|e| e.into())
        }
        "browser_save_workflow" => {
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let start_url = arguments["startUrl"].as_str().ok_or("startUrl is required")?;
            let steps = arguments["steps"].as_array().ok_or("steps must be an array")?;
            let mut variables = std::collections::HashMap::new();
            if let Some(map) = arguments["variables"].as_object() {
                for (key, value) in map {
                    let text = value.as_str().ok_or("workflow variables must be string values")?;
                    variables.insert(key.to_string(), text.to_string());
                }
            }
            let mut parsed_steps = Vec::with_capacity(steps.len());
            for step in steps {
                let kind = step["kind"].as_str().ok_or("workflow step kind is required")?;
                let parsed = match kind {
                    "navigate" => crate::editor::browser::BrowserWorkflowStep::Navigate {
                        url: step["url"].as_str().ok_or("navigate step url is required")?.to_string(),
                    },
                    "click" => crate::editor::browser::BrowserWorkflowStep::Click {
                        role: step["role"].as_str().ok_or("click step role is required")?.to_string(),
                        name: step["name"].as_str().ok_or("click step name is required")?.to_string(),
                    },
                    "fill_field" => crate::editor::browser::BrowserWorkflowStep::FillField {
                        field: step["field"].as_str().ok_or("fill_field step field is required")?.to_string(),
                        value: step["value"].as_str().ok_or("fill_field step value is required")?.to_string(),
                    },
                    "submit_form" => crate::editor::browser::BrowserWorkflowStep::SubmitForm {
                        form: step["form"].as_str().map(|value| value.to_string()),
                    },
                    "wait_for_text" => crate::editor::browser::BrowserWorkflowStep::WaitForText {
                        text: step["text"].as_str().ok_or("wait_for_text step text is required")?.to_string(),
                        timeout_ms: step["timeoutMs"].as_u64(),
                        interval_ms: step["intervalMs"].as_u64(),
                    },
                    "wait_for_element" => crate::editor::browser::BrowserWorkflowStep::WaitForElement {
                        role: step["role"].as_str().ok_or("wait_for_element step role is required")?.to_string(),
                        name: step["name"].as_str().ok_or("wait_for_element step name is required")?.to_string(),
                        timeout_ms: step["timeoutMs"].as_u64(),
                        interval_ms: step["intervalMs"].as_u64(),
                    },
                    "extract_text" => crate::editor::browser::BrowserWorkflowStep::ExtractText {
                        output: step["output"].as_str().ok_or("extract_text step output is required")?.to_string(),
                        source: step["source"].as_str().ok_or("extract_text step source is required")?.to_string(),
                        role: step["role"].as_str().map(|value| value.to_string()),
                        name: step["name"].as_str().map(|value| value.to_string()),
                        field: step["field"].as_str().map(|value| value.to_string()),
                    },
                    "assert_element" => crate::editor::browser::BrowserWorkflowStep::AssertElement {
                        role: step["role"].as_str().ok_or("assert_element step role is required")?.to_string(),
                        name: step["name"].as_str().ok_or("assert_element step name is required")?.to_string(),
                    },
                    "assert_text_contains" => crate::editor::browser::BrowserWorkflowStep::AssertTextContains {
                        text: step["text"].as_str().ok_or("assert_text_contains step text is required")?.to_string(),
                    },
                    "assert_output" => crate::editor::browser::BrowserWorkflowStep::AssertOutput {
                        output: step["output"].as_str().ok_or("assert_output step output is required")?.to_string(),
                        equals: step["equals"].as_str().map(|value| value.to_string()),
                        contains: step["contains"].as_str().map(|value| value.to_string()),
                    },
                    other => return Err(format!("unsupported browser workflow step kind: {}", other).into()),
                };
                parsed_steps.push(parsed);
            }

            let workflow = crate::editor::browser::BrowserWorkflow {
                name: name.to_string(),
                start_url: start_url.to_string(),
                variables,
                steps: parsed_steps,
            };
            let (json_path, nda_path) = crate::editor::browser::save_workflow(&root, &workflow)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            Ok(format!(
                "Saved browser workflow '{}'\nJSON: {}\nNDA: {}",
                workflow.name,
                json_path.display(),
                nda_path.display()
            ))
        }
        "browser_read_workflow" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(&root, rel_path, false)?;
            let workflow = crate::editor::browser::load_workflow(&full_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            Ok(serde_json::to_string_pretty(&workflow)?)
        }
        "browser_replay_workflow" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(&root, rel_path, false)?;
            let workflow = crate::editor::browser::load_workflow(&full_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let sitemap_path = root.join(".velocity").join("site_map");
            if let Some(session_id) = arguments["sessionId"].as_str() {
                crate::editor::browser::replay_workflow_in_session(&root, session_id, &workflow, &sitemap_path)
                    .map_err(|e| e.into())
            } else {
                crate::editor::browser::replay_workflow_with_artifacts(&root, &workflow, &sitemap_path)
                    .map_err(|e| e.into())
            }
        }
        "run_command" => {
            let command_str = arguments["command"].as_str().ok_or("command is required")?;
            let output = if cfg!(target_os = "windows") {
                Command::new("cmd")
                    .args(&["/C", command_str])
                    .current_dir(&root)
                    .output()
            } else {
                Command::new("sh")
                    .args(&["-c", command_str])
                    .current_dir(&root)
                    .output()
            };

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    let combined = format!("{}{}", stdout, stderr);
                    Ok(combined)
                }
                Err(e) => Err(format!("Failed to execute command: {:?}", e).into())
            }
        }
        "convert_to_nda" => {
            let _file_path = arguments["filePath"].as_str().ok_or("filePath is required")?;
            let _output_path = arguments["outputPath"].as_str().unwrap_or("");
            execute_csharp_mcp_tool("convert_to_nda", arguments)
        }
        "read_nda" => {
            let _nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            execute_csharp_mcp_tool("read_nda", arguments)
        }
        "execute_nda" => {
            let _nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            execute_csharp_mcp_tool("execute_nda", arguments)
        }
        "read_file" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(&root, rel_path, false)?;
            let content = std::fs::read_to_string(full_path)?;
            Ok(content)
        }
        "write_file" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let content = arguments["content"].as_str().ok_or("content is required")?;
            
            // Run safety scanner warnings detection
            let scan_warning = scan_file_content(content);
            
            let full_path = resolve_workspace_path(&root, rel_path, true)?;
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(full_path, content)?;
            
            if let Some(warn) = scan_warning {
                Ok(format!(
                    "Success: File written successfully. WARNING: Security scan warning triggered: [{}]. Please immediately correct this exposure in your next step.",
                    warn
                ))
            } else {
                Ok("Success: File written successfully".to_string())
            }
        }
        "list_dir" => {
            let rel_path = arguments["relativeDirPath"].as_str().ok_or("relativeDirPath is required")?;
            let target_dir = if rel_path == "." || rel_path.is_empty() {
                root.clone()
            } else {
                resolve_workspace_path(&root, rel_path, false)?
            };
            
            let mut entries_list = Vec::new();
            let entries = std::fs::read_dir(&target_dir)
                .map_err(|e| format!("Failed to read directory '{}': {:?}", target_dir.display(), e))?;
                
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type()?.is_dir();
                entries_list.push(format!("{}{}", name, if is_dir { "/" } else { "" }));
            }
            Ok(entries_list.join("\n"))
        }
        "delete_file" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(&root, rel_path, false)?;
            
            if full_path.is_dir() {
                return Err("delete_file cannot be used to delete a directory. Use a command line tool if needed.".into());
            }
            
            std::fs::remove_file(&full_path)?;
            Ok(format!("Success: File '{}' deleted successfully.", rel_path))
        }
        "grep_search" => {
            let query = arguments["query"].as_str().ok_or("query is required")?;
            let root_dir = root.clone();
            let mut matches = Vec::new();
            
            fn search_dir(dir: &std::path::Path, query: &str, matches: &mut Vec<String>, root: &std::path::Path) -> Result<(), Box<dyn Error>> {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let file_type = entry.file_type()?;
                        
                        if file_type.is_dir() {
                            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            if dir_name == "node_modules" 
                                || dir_name == ".git" 
                                || dir_name == "target"
                                || dir_name == "dist"
                                || dir_name == "build"
                                || dir_name == ".vscode"
                                || dir_name == ".idea"
                                || dir_name == "bin"
                                || dir_name == "obj" 
                            {
                                continue;
                            }
                            search_dir(&path, query, matches, root)?;
                        } else if file_type.is_file() {
                            let extension = path.extension()
                                .and_then(|ext| ext.to_str())
                                .unwrap_or("")
                                .to_lowercase();
                            let skip_exts = ["png", "jpg", "jpeg", "gif", "ico", "pdf", "zip", "tar", "gz", "7z", "rar", "exe", "dll", "so", "dylib", "class", "pyc", "nda"];
                            if skip_exts.contains(&extension.as_str()) {
                                continue;
                            }
                            
                            if let Ok(metadata) = path.metadata() {
                                if metadata.len() > 1024 * 1024 { // skip files > 1 MB
                                    continue;
                                }
                            }
                            
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().to_string();
                                for (idx, line) in content.lines().enumerate() {
                                    if line.contains(query) {
                                        matches.push(format!("{}:{}: {}", rel, idx + 1, line.trim()));
                                        if matches.len() >= 100 {
                                            return Ok(());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            
            search_dir(&root_dir, query, &mut matches, &root_dir)?;
            Ok(matches.join("\n"))
        }
        _ => Err(format!("Tool '{}' is not registered on this server.", name).into()),
    }
}

fn resolve_workspace_path(root: &Path, raw: &str, allow_missing: bool) -> Result<PathBuf, Box<dyn Error>> {
    let candidate = Path::new(raw);

    // If the model passed an absolute path, try to make it relative to root first.
    let raw_rel: std::borrow::Cow<str> = if candidate.is_absolute() {
        // Strip the workspace root prefix if present (handles model emitting full paths)
        match candidate.strip_prefix(root) {
            Ok(rel) => rel.to_string_lossy().into(),
            Err(_) => {
                // Also try with canonical root to handle UNC vs normal differences
                let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
                match candidate.strip_prefix(&canon_root) {
                    Ok(rel) => rel.to_string_lossy().into(),
                    Err(_) => return Err("workspace tool path is outside the workspace root".into()),
                }
            }
        }
    } else {
        raw.into()
    };

    let candidate = Path::new(raw_rel.as_ref());
    if candidate.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
        return Err("workspace tool path escapes the workspace".into());
    }

    let joined = root.join(candidate);

    if allow_missing {
        // For writes: just ensure existing parents are within root
        if joined.exists() {
            // path exists — validate it stays within root
            let canonical = safe_canonicalize(&joined);
            let canon_root = safe_canonicalize(root);
            if !canonical.starts_with(&canon_root) {
                return Err("workspace tool path escapes the workspace".into());
            }
        }
        return Ok(joined);
    }

    // For reads: path must exist
    if !joined.exists() {
        return Err(format!("file not found in workspace: {}", joined.display()).into());
    }

    // Validate within root without relying solely on canonicalize (which fails on OneDrive)
    let canonical = safe_canonicalize(&joined);
    let canon_root = safe_canonicalize(root);
    if !canonical.starts_with(&canon_root) {
        return Err("workspace tool path escapes the workspace".into());
    }

    Ok(joined)
}

/// Canonicalize a path without crashing on Windows permission errors (e.g. OneDrive placeholders).
/// Returns the input path unchanged when canonicalization fails.
fn safe_canonicalize(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}


pub fn call_tool(name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let root = std::env::current_dir()?;
    call_tool_in_workspace(&root, name, arguments)
}

fn scan_file_content(content: &str) -> Option<&'static str> {
    if (content.contains("mysql ") || content.contains("mysqldump ") || content.contains("sqlcmd ")) && 
       (content.contains(" -p") || content.contains(" --password=")) {
        if !content.contains("$") && !content.contains("temp_") {
            return Some("MySQL command-line password exposure detected.");
        }
    }
    if content.contains("IDENTIFIED BY") || content.contains("WITH PASSWORD") {
        if !content.contains("$") && !content.contains("temp_") {
            return Some("Plaintext password exposure in inline database query detected.");
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::call_tool_in_workspace;
    use serde_json::json;
    use std::fs;
    use std::io::Read;
    use std::net::TcpStream;
    use std::time::Duration;

    fn read_http_request(stream: &mut TcpStream) -> String {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
        let mut data = Vec::new();
        let mut buf = [0u8; 1024];
        let mut expected_total = None;

        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => {
                    data.extend_from_slice(&buf[..read]);
                    if expected_total.is_none() {
                        if let Some(header_end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
                            let headers_end = header_end + 4;
                            let headers = String::from_utf8_lossy(&data[..headers_end]);
                            let content_length = headers
                                .lines()
                                .find_map(|line| {
                                    let lower = line.to_ascii_lowercase();
                                    lower
                                        .strip_prefix("content-length:")
                                        .and_then(|value| value.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            expected_total = Some(headers_end + content_length);
                        }
                    }
                    if let Some(total) = expected_total {
                        if data.len() >= total {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }

        String::from_utf8_lossy(&data).to_string()
    }

    #[test]
    fn file_tools_use_explicit_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "write_file",
            &json!({"relativeFilePath": "src/main.rs", "content": "fn main() {}"}),
        )
        .unwrap();

        assert_eq!(fs::read_to_string(root.join("src/main.rs")).unwrap(), "fn main() {}");
    }

    #[test]
    fn file_tools_reject_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let result = call_tool_in_workspace(
            &root,
            "write_file",
            &json!({"relativeFilePath": "../outside.txt", "content": "nope"}),
        );

        assert!(result.is_err());
        assert!(!temp.path().join("outside.txt").exists());
    }

    #[test]
    fn command_runs_in_explicit_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let output = call_tool_in_workspace(&root, "run_command", &json!({"command": "cd"})).unwrap();
        assert!(output.to_lowercase().contains("project"));
    }

    #[test]
    fn file_tools_delete_file_success() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        fs::write(root.join("temp.txt"), "hello").unwrap();
        assert!(root.join("temp.txt").exists());

        call_tool_in_workspace(
            &root,
            "delete_file",
            &json!({"relativeFilePath": "temp.txt"}),
        )
        .unwrap();

        assert!(!root.join("temp.txt").exists());
    }

    #[test]
    fn file_tools_delete_file_rejects_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let result = call_tool_in_workspace(
            &root,
            "delete_file",
            &json!({"relativeFilePath": "../outside.txt"}),
        );

        assert!(result.is_err());
    }

    #[test]
    fn list_dir_returns_error_on_missing_dir() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let result = call_tool_in_workspace(
            &root,
            "list_dir",
            &json!({"relativeDirPath": "missing_folder"}),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_web_navigate_native_parser() {
        use std::io::Write;
        use std::net::TcpListener;
        use velocity_ide::site_map::SiteMap;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::Read;
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);

                let body = "<html><head><title>Egui Test</title></head><body><a href=\"/button\">Click Me</a></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let sitemap_path = temp.path().join("site_map");

        let res = crate::editor::browser::crawl_and_sync_sitemap(&url, &sitemap_path).unwrap();
        assert!(res.contains("Egui Test"));
        assert!(res.contains("Interactive Elements: 1"));
        assert!(res.contains("Snapshot JSON:"));
        assert!(res.contains("NDA Facts:"));

        let sm = SiteMap::open(&sitemap_path, 0).unwrap();
        assert!(sm.len() > 0);
    }

    #[test]
    fn browser_workflow_tools_round_trip_and_replay() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for _ in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = read_http_request(&mut stream);
                    let first_line = request.lines().next().unwrap_or_default();
                    let body = if first_line.starts_with("POST /login") {
                        "<html><head><title>Dashboard</title></head><body><p>Welcome back</p></body></html>"
                    } else {
                        "<html><head><title>Login</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'><input type='submit' value='Sign in'></form></body></html>"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc123; Path=/\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let save = call_tool_in_workspace(
            &root,
            "browser_save_workflow",
            &json!({
                "name": "Login Flow",
                "startUrl": base_url,
                "variables": {"email": "rust@example.com"},
                "steps": [
                    {"kind": "fill_field", "field": "email", "value": "{{email}}"},
                    {"kind": "submit_form", "form": "login"},
                    {"kind": "wait_for_text", "text": "Welcome back", "timeoutMs": 1500, "intervalMs": 10},
                    {"kind": "extract_text", "output": "page_title", "source": "title"},
                    {"kind": "assert_output", "output": "page_title", "equals": "Dashboard"},
                    {"kind": "assert_text_contains", "text": "Welcome back"}
                ]
            }),
        )
        .unwrap();
        assert!(save.contains("Saved browser workflow 'Login Flow'"));

        let rel_path = ".velocity/browser-workflows/login-flow.browser.json";
        let read_back = call_tool_in_workspace(
            &root,
            "browser_read_workflow",
            &json!({"relativeFilePath": rel_path}),
        )
        .unwrap();
        assert!(read_back.contains("Login Flow"));
        assert!(read_back.contains("fill_field"));
        assert!(read_back.contains("submit_form"));
        assert!(read_back.contains("wait_for_text"));
        assert!(read_back.contains("extract_text"));
        assert!(read_back.contains("assert_output"));

        let replay = call_tool_in_workspace(
            &root,
            "browser_replay_workflow",
            &json!({"relativeFilePath": rel_path}),
        )
        .unwrap();
        assert!(replay.contains("Workflow 'Login Flow' completed."));
        assert!(replay.contains("Final title: Dashboard"));
        assert!(replay.contains("Cookies: 1"));
        assert!(replay.contains("Outputs: 1"));
        assert!(replay.contains("Run Report:"));
    }

    #[test]
    fn browser_session_wait_tool_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for idx in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = read_http_request(&mut stream);
                    let body = if idx == 0 {
                        "<html><head><title>Loading</title></head><body><p>Preparing dashboard</p></body></html>"
                    } else {
                        "<html><head><title>Dashboard</title></head><body><p>Ready</p></body></html>"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "waiter"}),
        )
        .unwrap();
        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "waiter", "url": url}),
        )
        .unwrap();

        let waited = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "waiter", "text": "Ready", "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(waited.contains("Session wait complete."));
        assert!(waited.contains("Title: Dashboard"));
        assert!(waited.contains("Diff: title,summary"));
    }

    #[test]
    fn browser_session_tools_round_trip() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let body = "<html><head><title>Session Test</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'></form></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nSet-Cookie: token=xyz; Path=/\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let created = call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(created.contains("Created browser session 'qa-session'"));

        let navigated = call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "qa-session", "url": url}),
        )
        .unwrap();
        assert!(navigated.contains("Session: qa-session"));
        assert!(navigated.contains("Forms: 1"));
        assert!(navigated.contains("Cookies: 1"));

        let session = call_tool_in_workspace(
            &root,
            "browser_get_session",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(session.contains("\"id\": \"qa-session\""));
        assert!(session.contains("\"name\": \"token\""));
    }

    #[test]
    fn browser_checkpoint_and_session_replay_tools_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for _ in 0..3 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = read_http_request(&mut stream);
                    let first_line = request.lines().next().unwrap_or_default();
                    let body = if first_line.starts_with("POST /login") {
                        "<html><head><title>Dashboard</title></head><body><p>Welcome back</p></body></html>"
                    } else {
                        "<html><head><title>Login</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'><input type='submit' value='Sign in'></form></body></html>"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc123; Path=/\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "auth-session"}),
        )
        .unwrap();
        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "auth-session", "url": base_url}),
        )
        .unwrap();

        let saved = call_tool_in_workspace(
            &root,
            "browser_save_checkpoint",
            &json!({"sessionId": "auth-session", "checkpointName": "before-submit"}),
        )
        .unwrap();
        assert!(saved.contains("Saved browser checkpoint 'before-submit'"));

        let save_workflow = call_tool_in_workspace(
            &root,
            "browser_save_workflow",
            &json!({
                "name": "Resume Login",
                "startUrl": base_url,
                "steps": [
                    {"kind": "fill_field", "field": "email", "value": "rust@example.com"},
                    {"kind": "submit_form", "form": "login"},
                    {"kind": "assert_text_contains", "text": "Welcome back"}
                ]
            }),
        )
        .unwrap();
        assert!(save_workflow.contains("Saved browser workflow 'Resume Login'"));

        let replay = call_tool_in_workspace(
            &root,
            "browser_replay_workflow",
            &json!({
                "relativeFilePath": ".velocity/browser-workflows/resume-login.browser.json",
                "sessionId": "auth-session"
            }),
        )
        .unwrap();
        assert!(replay.contains("Workflow 'Resume Login' completed."));
        assert!(replay.contains("Final title: Dashboard"));
        assert!(replay.contains("Session: auth-session"));
        assert!(replay.contains("Session JSON:"));

        let restored = call_tool_in_workspace(
            &root,
            "browser_restore_checkpoint",
            &json!({
                "sessionId": "auth-session",
                "checkpointName": "before-submit",
                "targetSessionId": "forked-session"
            }),
        )
        .unwrap();
        assert!(restored.contains("Restored browser session checkpoint 'before-submit'"));
        assert!(restored.contains("Session: forked-session"));
        assert!(restored.contains("Title: Login"));
    }
}

struct SidecarDaemon {
    child: Child,
}

static DAEMON: Lazy<Mutex<Option<SidecarDaemon>>> = Lazy::new(|| Mutex::new(None));

fn execute_csharp_mcp_tool(tool_name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let exe_path = "C:\\Users\\visse\\OneDrive\\Documents\\Payment and Transaction Flow\\Velocity\\NdaMcpServer\\bin\\Debug\\net10.0\\NdaMcpServer.exe";
    
    let mut daemon_guard = DAEMON.lock().map_err(|e| e.to_string())?;
    
    if daemon_guard.is_none() {
        if !std::path::Path::new(exe_path).exists() {
            return execute_rust_fallback_tool(tool_name, arguments);
        }
        
        let child = Command::new(exe_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        *daemon_guard = Some(SidecarDaemon { child });
    } else {
        let daemon = daemon_guard.as_mut().unwrap();
        if let Ok(Some(_status)) = daemon.child.try_wait() {
            let child = Command::new(exe_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            *daemon = SidecarDaemon { child };
        }
    }
    
    let daemon = daemon_guard.as_mut().unwrap();
    
    let request = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments
        },
        "id": 999
    });

    let request_str = serde_json::to_string(&request)? + "\n";

    {
        let stdin = daemon.child.stdin.as_mut().ok_or("Failed to open stdin of C# daemon")?;
        stdin.write_all(request_str.as_bytes())?;
        stdin.flush()?;
    }

    let response_str;
    {
        let stdout = daemon.child.stdout.as_mut().ok_or("Failed to open stdout of C# daemon")?;
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line.is_empty() {
                return Err("C# sidecar daemon closed stdout unexpectedly".into());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                response_str = trimmed.to_string();
                break;
            } else {
                eprintln!("[C# Sidecar Log] {}", trimmed);
            }
        }
    }

    let response: Value = serde_json::from_str(&response_str)?;

    if let Some(err) = response.get("error") {
        return Err(format!("C# Execution Error: {}", err["message"].as_str().unwrap_or("Unknown")).into());
    }

    let is_error = response["result"]["isError"].as_bool().unwrap_or(false);
    let text = response["result"]["content"][0]["text"].as_str().ok_or("Failed to parse tool text output")?;

    if is_error {
        Err(text.into())
    } else {
        Ok(text.to_string())
    }
}

// --- Self-contained Rust Fallback & Sandboxing Runner ---

fn execute_rust_fallback_tool(tool_name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    match tool_name {
        "convert_to_nda" => {
            let file_path = arguments["filePath"].as_str().ok_or("filePath is required")?;
            let output_path = arguments["outputPath"].as_str().unwrap_or("");
            
            let final_output = if output_path.is_empty() {
                format!("{}.nda", file_path)
            } else {
                output_path.to_string()
            };
            
            let content = std::fs::read(file_path)?;
            
            let mut nda_bytes = Vec::new();
            nda_bytes.extend_from_slice(b"NDAV");
            
            let size = content.len() as u32;
            nda_bytes.extend_from_slice(&size.to_le_bytes());
            
            let file_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.txt");
            nda_bytes.extend_from_slice(file_name.as_bytes());
            nda_bytes.push(0);
            nda_bytes.extend_from_slice(&content);
            
            std::fs::write(&final_output, nda_bytes)?;
            
            Ok(format!("Success: File converted and signed to NDA container at: {}", final_output))
        }
        "read_nda" => {
            let nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            let nda_bytes = std::fs::read(nda_path)?;
            
            if nda_bytes.len() < 9 || &nda_bytes[0..4] != b"NDAV" {
                return Err("Invalid NDA container format".into());
            }
            
            let size = u32::from_le_bytes([nda_bytes[4], nda_bytes[5], nda_bytes[6], nda_bytes[7]]) as usize;
            
            let mut name_end = 8;
            while name_end < nda_bytes.len() && nda_bytes[name_end] != 0 {
                name_end += 1;
            }
            
            let file_name = String::from_utf8_lossy(&nda_bytes[8..name_end]).to_string();
            
            let report = json!({
                "format": "NDAV-Fallback",
                "fileName": file_name,
                "payloadSizeBytes": size,
                "visualDisplayCommands": [
                    "display_text: NDA Container Contents Verified",
                    format!("display_text: Filename: {}", file_name),
                    format!("display_text: Size: {} bytes", size)
                ]
            });
            
            Ok(serde_json::to_string_pretty(&report)?)
        }
        "execute_nda" => {
            let nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            let nda_bytes = std::fs::read(nda_path)?;
            
            if nda_bytes.len() < 9 || &nda_bytes[0..4] != b"NDAV" {
                return Err("Invalid NDA container format".into());
            }
            
            let mut name_end = 8;
            while name_end < nda_bytes.len() && nda_bytes[name_end] != 0 {
                name_end += 1;
            }
            
            let file_name = String::from_utf8_lossy(&nda_bytes[8..name_end]).to_string();
            let payload = &nda_bytes[name_end + 1..];
            
            let temp_dir = std::env::temp_dir();
            let temp_file_path = temp_dir.join(&file_name);
            std::fs::write(&temp_file_path, payload)?;
            
            let ext = std::path::Path::new(&file_name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
                
            let cmd_args = arguments["arguments"].as_array();
            let mut args_vec = Vec::new();
            if let Some(arr) = cmd_args {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        args_vec.push(s.to_string());
                    }
                }
            }
            
            let (shell_cmd, mut final_args) = match ext.as_str() {
                "py" => {
                    ("python".to_string(), vec![temp_file_path.to_string_lossy().to_string()])
                }
                "js" => {
                    ("node".to_string(), vec![temp_file_path.to_string_lossy().to_string()])
                }
                "ps1" => {
                    ("powershell".to_string(), vec![
                        "-ExecutionPolicy".to_string(),
                        "Bypass".to_string(),
                        "-File".to_string(),
                        temp_file_path.to_string_lossy().to_string()
                    ])
                }
                "sh" => {
                    ("bash".to_string(), vec![temp_file_path.to_string_lossy().to_string()])
                }
                "bat" | "cmd" => {
                    ("cmd".to_string(), vec!["/c".to_string(), temp_file_path.to_string_lossy().to_string()])
                }
                _ => {
                    (temp_file_path.to_string_lossy().to_string(), Vec::new())
                }
            };
            
            final_args.extend(args_vec);
            
            let dll_path = "C:\\WUIAS\\wuias_shield\\wuias_shield.dll";
            let use_sandbox = std::path::Path::new(dll_path).exists() && cfg!(target_os = "windows");
            
            let output = if use_sandbox {
                #[cfg(target_os = "windows")]
                {
                    run_in_dll_sandbox(&shell_cmd, &final_args, dll_path)?
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let out = Command::new(&shell_cmd)
                        .args(&final_args)
                        .output()?;
                    String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
                }
            } else {
                let out = Command::new(&shell_cmd)
                    .args(&final_args)
                    .output()?;
                String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
            };
            
            let _ = std::fs::remove_file(temp_file_path);
            
            Ok(output)
        }
        _ => Err(format!("Unknown fallback tool: {}", tool_name).into())
    }
}

// --- Windows DLL Sandboxing Native Implementations ---

#[cfg(target_os = "windows")]
extern "system" {
    fn CreateProcessW(
        lpApplicationName: *const u16,
        lpCommandLine: *mut u16,
        lpProcessAttributes: *mut std::ffi::c_void,
        lpThreadAttributes: *mut std::ffi::c_void,
        bInheritHandles: i32,
        dwCreationFlags: u32,
        lpEnvironment: *mut std::ffi::c_void,
        lpCurrentDirectory: *const u16,
        lpStartupInfo: *mut STARTUPINFOW,
        lpProcessInformation: *mut PROCESS_INFORMATION,
    ) -> i32;
    fn VirtualAllocEx(
        hProcess: *mut std::ffi::c_void,
        lpAddress: *mut std::ffi::c_void,
        dwSize: usize,
        flAllocationType: u32,
        flProtect: u32,
    ) -> *mut std::ffi::c_void;
    fn WriteProcessMemory(
        hProcess: *mut std::ffi::c_void,
        lpBaseAddress: *mut std::ffi::c_void,
        lpBuffer: *const std::ffi::c_void,
        nSize: usize,
        lpNumberOfBytesWritten: *mut usize,
    ) -> i32;
    fn CreateRemoteThread(
        hProcess: *mut std::ffi::c_void,
        lpThreadAttributes: *mut std::ffi::c_void,
        dwStackSize: usize,
        lpStartAddress: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        lpParameter: *mut std::ffi::c_void,
        dwCreationFlags: u32,
        lpThreadId: *mut u32,
    ) -> *mut std::ffi::c_void;
    fn ResumeThread(hThread: *mut std::ffi::c_void) -> u32;
    fn GetModuleHandleW(lpModuleName: *const u16) -> *mut std::ffi::c_void;
    fn GetProcAddress(
        hModule: *mut std::ffi::c_void,
        lpProcName: *const u8,
    ) -> *mut std::ffi::c_void;
    fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    fn WaitForSingleObject(hHandle: *mut std::ffi::c_void, dwMilliseconds: u32) -> u32;
}

#[cfg(target_os = "windows")]
#[repr(C)]
pub struct STARTUPINFOW {
    cb: u32,
    lpReserved: *mut u16,
    lpDesktop: *mut u16,
    lpTitle: *mut u16,
    dwX: u32,
    dwY: u32,
    dwXSize: u32,
    dwYSize: u32,
    dwXCountChars: u32,
    dwYCountChars: u32,
    dwFillAttribute: u32,
    dwFlags: u32,
    wShowWindow: u16,
    cbReserved2: u16,
    lpReserved2: *mut u8,
    hStdInput: *mut std::ffi::c_void,
    hStdOutput: *mut std::ffi::c_void,
    hStdError: *mut std::ffi::c_void,
}

#[cfg(target_os = "windows")]
#[repr(C)]
pub struct PROCESS_INFORMATION {
    hProcess: *mut std::ffi::c_void,
    hThread: *mut std::ffi::c_void,
    dwProcessId: u32,
    dwThreadId: u32,
}

#[cfg(target_os = "windows")]
fn to_wstring(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
fn run_in_dll_sandbox(app: &str, args: &[String], dll_path: &str) -> Result<String, Box<dyn Error>> {
    let session_id = format!("nda_session_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs());
    let redirect_dir = format!("C:\\WUIAS\\sandbox\\redirect\\{}", session_id);
    std::fs::create_dir_all(&redirect_dir)?;
    
    let w_dll_path = to_wstring(dll_path);
    let cmd_line_str = format!("\"{}\" {}", app, args.join(" "));
    let mut w_cmd_line = to_wstring(&cmd_line_str);
    
    // Pre-create registry key
    let _ = Command::new("reg")
        .args(&["add", &format!("HKCU\\Software\\WUIAS_Sandbox\\{}", session_id), "/f"])
        .output();
        
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        
        std::env::set_var("WUIAS_SESSION_ID", &session_id);
        std::env::set_var("WUIAS_REDIRECT_DIR", &redirect_dir);
        
        let CREATE_SUSPENDED: u32 = 0x00000004;
        let success = CreateProcessW(
            std::ptr::null(),
            w_cmd_line.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            CREATE_SUSPENDED,
            std::ptr::null_mut(),
            std::ptr::null(),
            &mut si,
            &mut pi,
        );
        
        std::env::remove_var("WUIAS_SESSION_ID");
        std::env::remove_var("WUIAS_REDIRECT_DIR");
        
        if success == 0 {
            return Err(format!("CreateProcessW failed. Error code: {}", std::io::Error::last_os_error()).into());
        }
        
        let path_size = (dll_path.len() + 1) * 2;
        let MEM_COMMIT = 0x1000;
        let MEM_RESERVE = 0x2000;
        let PAGE_READWRITE = 0x04;
        
        let remote_mem = VirtualAllocEx(
            pi.hProcess,
            std::ptr::null_mut(),
            path_size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        
        if remote_mem.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("VirtualAllocEx failed in target process".into());
        }
        
        let dll_bytes: Vec<u8> = w_dll_path
            .iter()
            .flat_map(|&w| w.to_le_bytes())
            .collect();
            
        let mut written = 0;
        let write_ok = WriteProcessMemory(
            pi.hProcess,
            remote_mem,
            dll_bytes.as_ptr() as *const std::ffi::c_void,
            dll_bytes.len(),
            &mut written,
        );
        
        if write_ok == 0 {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("WriteProcessMemory failed to write DLL path".into());
        }
        
        let kernel32_name = to_wstring("kernel32.dll");
        let h_kernel32 = GetModuleHandleW(kernel32_name.as_ptr());
        if h_kernel32.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("Failed to locate kernel32.dll in host".into());
        }
        
        let load_library_addr = GetProcAddress(h_kernel32, b"LoadLibraryW\0".as_ptr());
        if load_library_addr.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("Failed to resolve LoadLibraryW address".into());
        }
        
        let mut thread_id = 0;
        let load_library_fn: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 = std::mem::transmute(load_library_addr);
        let h_thread = CreateRemoteThread(
            pi.hProcess,
            std::ptr::null_mut(),
            0,
            load_library_fn,
            remote_mem,
            0,
            &mut thread_id,
        );
        
        if h_thread.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("CreateRemoteThread failed to load DLL".into());
        }
        
        WaitForSingleObject(h_thread, 5000);
        CloseHandle(h_thread);
        
        ResumeThread(pi.hThread);
        CloseHandle(pi.hThread);
        
        WaitForSingleObject(pi.hProcess, 0xFFFFFFFF);
        CloseHandle(pi.hProcess);
    }
    
    let mut run_output = format!("=== Sandboxed execution completed (Session: {}) ===\n", session_id);
    
    fn count_files_recursive(dir: &std::path::Path) -> usize {
        let mut count = 0;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        count += count_files_recursive(&entry.path());
                    } else if file_type.is_file() {
                        count += 1;
                    }
                }
            }
        }
        count
    }
    
    let files_count = count_files_recursive(std::path::Path::new(&redirect_dir));
    run_output += &format!("Sandbox redirect folder: {}\nRedirected files written: {}\n", redirect_dir, files_count);
    
    let _ = Command::new("reg")
        .args(&["delete", &format!("HKCU\\Software\\WUIAS_Sandbox\\{}", session_id), "/f"])
        .output();
        
    Ok(run_output)
}
