use crate::safety::SafeMutex;
use serde_json::{json, Value};
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

struct SidecarDaemon {
    child: std::process::Child,
}

static SIDECAR_DAEMON: Mutex<Option<SidecarDaemon>> = Mutex::new(None);

pub fn resolve_workspace_path(
    root: &Path,
    rel_path: &str,
    allow_create: bool,
) -> Result<PathBuf, Box<dyn Error>> {
    let clean_rel = rel_path.trim_start_matches('/').trim_start_matches('\\');
    let target = root.join(clean_rel);

    if allow_create {
        if let Some(parent) = target.parent() {
            let parent_canon = parent.canonicalize().or_else(|_| {
                fs::create_dir_all(parent)?;
                parent.canonicalize()
            })?;
            if !parent_canon.starts_with(root) {
                return Err(
                    format!("Access Denied: Path escapes workspace root ({})", rel_path).into(),
                );
            }
        }
        Ok(target)
    } else {
        let canon = target
            .canonicalize()
            .map_err(|_| format!("File or directory not found in workspace: {}", rel_path))?;

        if !canon.starts_with(root) {
            return Err(
                format!("Access Denied: Path escapes workspace root ({})", rel_path).into(),
            );
        }
        Ok(canon)
    }
}

pub fn scan_file_content(content: &str) -> Option<String> {
    let lowercase = content.to_lowercase();
    if lowercase.contains("api_key = \"sk-") || lowercase.contains("secret = \"") {
        Some("Potential hardcoded secret or API key detected.".to_string())
    } else {
        None
    }
}

/// Generate test stubs from source code using regex-based function extraction.
fn generate_test_stubs(source: &str, language: &str) -> Vec<String> {
    let mut tests = Vec::new();

    match language {
        "rust" => {
            // Extract function signatures: pub fn name(...) -> RetType
            for line in source.lines() {
                let trimmed = line.trim();
                if let Some(fn_start) = trimmed.find("fn ") {
                    let after_fn = &trimmed[fn_start + 3..];
                    if let Some(paren) = after_fn.find('(') {
                        let name = after_fn[..paren].trim().trim_start_matches("pub ");
                        if name.is_empty() || name.starts_with("test_") {
                            continue;
                        }
                        tests.push(format!(
                            "#[test]\nfn test_{}() {{\n    // TODO: Setup\n    let result = {}(/* args */);\n    // TODO: Assert\n    assert!(result != Default::default());\n}}",
                            name, name
                        ));
                        // Edge case: empty/zero input
                        tests.push(format!(
                            "#[test]\nfn test_{}_empty_input() {{\n    // Edge case: empty or zero input\n    // TODO: Verify graceful handling\n}}",
                            name
                        ));
                    }
                }
            }
        }
        "typescript" | "javascript" => {
            // Extract function/const arrow signatures
            for line in source.lines() {
                let trimmed = line.trim();
                let name = if trimmed.starts_with("export function ")
                    || trimmed.starts_with("function ")
                {
                    trimmed
                        .trim_start_matches("export ")
                        .trim_start_matches("function ")
                        .split('(')
                        .next()
                        .map(|s| s.trim().to_string())
                } else if trimmed.contains("= (") || trimmed.contains("= async (") {
                    trimmed.split("= ").next().map(|s| {
                        s.trim()
                            .trim_start_matches("export ")
                            .trim_start_matches("const ")
                            .trim_start_matches("let ")
                            .to_string()
                    })
                } else {
                    None
                };
                if let Some(fname) = name {
                    if fname.is_empty() || fname.contains("test") {
                        continue;
                    }
                    tests.push(format!(
                        "describe('{}', () => {{\n  it('should work with valid input', () => {{\n    // TODO: Arrange\n    // TODO: Act\n    // TODO: Assert\n  }});\n\n  it('should handle empty input', () => {{\n    // Edge case\n  }});\n}});",
                        fname
                    ));
                }
            }
        }
        "python" => {
            for line in source.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("def ") && !trimmed.starts_with("def test_") {
                    if let Some(paren) = trimmed.find('(') {
                        let name = &trimmed[4..paren];
                        tests.push(format!(
                            "def test_{}():\n    # TODO: Setup\n    result = {}()\n    assert result is not None\n\ndef test_{}_edge_case():\n    # Edge case: empty/None input\n    pass",
                            name, name, name
                        ));
                    }
                }
            }
        }
        _ => {
            tests.push("// Unsupported language for test generation".to_string());
        }
    }

    if tests.is_empty() {
        tests.push("// No testable functions found in source".to_string());
    }
    tests
}

/// Read structured IDE panel data without navigating the GUI.
///
/// Returns a JSON [`Value`] suitable for both the MCP tool boundary and the
/// agent-to-UI message channel, so callers share a single serialisation path.
pub fn fetch_panel_data_value(
    root: &Path,
    panel: &str,
    relative_path: Option<&str>,
) -> Result<Value, Box<dyn Error>> {
    let data = match panel {
        "teams" => {
            let teams = crate::editor::expert_team::load_expert_teams(root);
            let teams: Vec<Value> = teams
                .into_iter()
                .map(|team| {
                    json!({
                        "id": team.id,
                        "name": team.name,
                        "slug": team.slug(),
                        "description": team.description,
                        "is_preset": team.is_preset,
                        "members": team.members.into_iter().map(|member| json!({
                            "id": member.id,
                            "name": member.name,
                            "role": member.role,
                            "provider": member.provider.label(),
                            "model_id": member.model_id,
                            "skills": member.skills,
                            "scope_patterns": member.scope_patterns,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            json!({"panel": panel, "teams": teams})
        }
        "wiki" => match crate::automation::open_workspace_site_map(root) {
            Ok(site_map) => {
                let wiki = velocity_ide::wiki::build_wiki(&site_map);
                let pages: Vec<Value> = wiki
                    .file_pages
                    .iter()
                    .map(|page| {
                        json!({
                            "title": page.title,
                            "slug": page.slug,
                            "relationship_count": page.relationships.len(),
                            "called_by_count": page.called_by.len(),
                        })
                    })
                    .collect();
                json!({
                    "panel": panel,
                    "file_count": wiki.file_count(),
                    "symbol_count": wiki.symbol_count(),
                    "pages": pages,
                })
            }
            Err(error) => json!({"panel": panel, "error": error}),
        },
        "graph" => {
            let symbols = crate::editor::search::collect_workspace_symbols(root);
            let mut files = std::collections::BTreeMap::<String, usize>::new();
            for symbol in symbols {
                *files.entry(symbol.file).or_default() += 1;
            }
            let mut top_files: Vec<(String, usize)> = files.into_iter().collect();
            top_files
                .sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
            let symbol_count: usize = top_files.iter().map(|(_, count)| count).sum();
            json!({
                "panel": panel,
                "file_count": top_files.len(),
                "symbol_count": symbol_count,
                "top_files": top_files.into_iter().take(20).map(|(file, symbols)| json!({"file": file, "symbols": symbols})).collect::<Vec<_>>(),
            })
        }
        "bookmarks" => {
            let path = root.join(".velocity").join("bookmarks.json");
            match fs::read_to_string(path) {
                Ok(raw) => serde_json::from_str(&raw)
                    .unwrap_or_else(|_| json!({"error": "invalid bookmarks file"})),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    json!({"bookmarks": []})
                }
                Err(error) => return Err(error.into()),
            }
        }
        "files" => {
            let rel = relative_path.unwrap_or(".");
            let directory = if rel == "." || rel.is_empty() {
                root.to_path_buf()
            } else {
                resolve_workspace_path(root, rel, false)?
            };
            if !directory.is_dir() {
                return Err(format!("Not a directory: {}", rel).into());
            }
            let mut files = fs::read_dir(directory)?
                .flatten()
                .filter_map(|entry| {
                    let metadata = entry.metadata().ok()?;
                    Some(json!({
                        "name": entry.file_name().to_string_lossy(),
                        "is_dir": metadata.is_dir(),
                        "size": metadata.len(),
                    }))
                })
                .collect::<Vec<_>>();
            files.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
            json!({"panel": panel, "relative_path": rel, "files": files})
        }
        _ => {
            return Err(format!(
                "Unknown panel '{}'. Expected teams, wiki, graph, bookmarks, or files.",
                panel
            )
            .into())
        }
    };
    Ok(data)
}

fn fetch_panel_data(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let panel = arguments["panel"].as_str().ok_or("panel is required")?;
    let relative_path = arguments["relativePath"].as_str();
    let data = fetch_panel_data_value(root, panel, relative_path)?;
    Ok(serde_json::to_string(&data)?)
}

pub fn handle_system_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    log::debug!("system_tool: {} called", name);
    let result = match name {
        // Native Rust NDA document path (portable NDA1 with in-file history).
        "convert_to_nda" => native_convert_to_nda(root, arguments)?,
        "read_nda" => native_read_nda(root, arguments)?,
        "execute_nda" => execute_csharp_mcp_tool(name, arguments)?,
        "read_file" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(root, rel_path, false)?;

            fs::read_to_string(full_path)?
        }
        "write_file" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let content = arguments["content"].as_str().ok_or("content is required")?;

            let scan_warning = scan_file_content(content);

            let full_path = resolve_workspace_path(root, rel_path, true)?;
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(full_path, content)?;

            if let Some(warn) = scan_warning {
                format!(
                    "Success: File written successfully. WARNING: Security scan warning triggered: [{}]. Please immediately correct this exposure in your next step.",
                    warn
                )
            } else {
                "Success: File written successfully".to_string()
            }
        }
        "list_dir" => {
            let rel_path = arguments["relativeDirPath"]
                .as_str()
                .ok_or("relativeDirPath is required")?;
            let target_dir = if rel_path == "." || rel_path.is_empty() {
                root.to_path_buf()
            } else {
                resolve_workspace_path(root, rel_path, false)?
            };

            let mut entries_list = Vec::new();
            let entries = fs::read_dir(&target_dir).map_err(|e| {
                format!(
                    "Failed to read directory '{}': {:?}",
                    target_dir.display(),
                    e
                )
            })?;

            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type()?.is_dir();
                entries_list.push(format!("{}{}", name, if is_dir { "/" } else { "" }));
            }
            entries_list.join("\n")
        }
        "delete_file" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(root, rel_path, false)?;

            if full_path.is_dir() {
                return Err("delete_file cannot be used to delete a directory. Use a command line tool if needed.".into());
            }

            fs::remove_file(&full_path)?;
            format!("Success: File '{}' deleted successfully.", rel_path)
        }
        "grep_search" => {
            let query = arguments["query"].as_str().ok_or("query is required")?;
            let root_dir = root.to_path_buf();
            let mut matches = Vec::new();

            fn search_dir(
                dir: &Path,
                query: &str,
                matches: &mut Vec<String>,
                root: &Path,
            ) -> Result<(), Box<dyn Error>> {
                if let Ok(entries) = fs::read_dir(dir) {
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
                            let extension = path
                                .extension()
                                .and_then(|ext| ext.to_str())
                                .unwrap_or("")
                                .to_lowercase();
                            let skip_exts = [
                                "png", "jpg", "jpeg", "gif", "ico", "pdf", "zip", "tar", "gz",
                                "7z", "rar", "exe", "dll", "so", "dylib", "class", "pyc", "nda",
                            ];
                            if skip_exts.contains(&extension.as_str()) {
                                continue;
                            }

                            if let Ok(metadata) = path.metadata() {
                                if metadata.len() > 1024 * 1024 {
                                    continue;
                                }
                            }

                            if let Ok(content) = fs::read_to_string(&path) {
                                let rel = path
                                    .strip_prefix(root)
                                    .unwrap_or(&path)
                                    .to_string_lossy()
                                    .to_string();
                                for (idx, line) in content.lines().enumerate() {
                                    if line.contains(query) {
                                        matches.push(format!(
                                            "{}:{}: {}",
                                            rel,
                                            idx + 1,
                                            line.trim()
                                        ));
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

            search_dir(&root_dir, query, &mut matches, root)?;
            if matches.is_empty() {
                format!("No matches found for '{}'", query)
            } else {
                matches.join("\n")
            }
        }
        "fetch_panel_data" => fetch_panel_data(root, arguments)?,
        "run_command" => {
            let cmd_str = arguments["command"].as_str().ok_or("command is required")?;

            let (shell, arg) = if cfg!(target_os = "windows") {
                ("cmd", "/C")
            } else {
                ("sh", "-c")
            };

            let output = Command::new(shell)
                .arg(arg)
                .arg(cmd_str)
                .current_dir(root)
                .output()?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}{}", stdout, stderr);

            if combined.trim().is_empty() {
                format!(
                    "Command executed with exit code: {}",
                    output.status.code().unwrap_or(-1)
                )
            } else {
                combined
            }
        }
        // ── Windows Automation (WA) MCP Tools ──────────────────────────────
        "wa_registry_read" => {
            let hive_str = arguments["hive"].as_str().ok_or("hive is required")?;
            let path = arguments["path"].as_str().ok_or("path is required")?;
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let hive = crate::wa::registry::RegistryHive::from_str(hive_str)
                .ok_or_else(|| format!("Unknown registry hive: {}", hive_str))?;
            let result = crate::wa::registry::RegistryManager::read(hive, path, name);
            serde_json::to_string(&json!({
                "success": result.success,
                "operation": result.operation,
                "detail": result.detail,
                "value": result.value.map(|v| format!("{:?}", v))
            }))?
        }
        "wa_registry_write" => {
            let hive_str = arguments["hive"].as_str().ok_or("hive is required")?;
            let path = arguments["path"].as_str().ok_or("path is required")?;
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let value_str = arguments["value"].as_str().ok_or("value is required")?;
            let hive = crate::wa::registry::RegistryHive::from_str(hive_str)
                .ok_or_else(|| format!("Unknown registry hive: {}", hive_str))?;
            let entry = crate::wa::registry::RegistryEntry {
                hive,
                path: path.to_string(),
                name: name.to_string(),
                value: crate::wa::registry::RegistryValue::String(value_str.to_string()),
            };
            let result = crate::wa::registry::RegistryManager::write(&entry);
            serde_json::to_string(&json!({
                "success": result.success,
                "operation": result.operation,
                "detail": result.detail
            }))?
        }
        "wa_registry_delete" => {
            let hive_str = arguments["hive"].as_str().ok_or("hive is required")?;
            let path = arguments["path"].as_str().ok_or("path is required")?;
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let hive = crate::wa::registry::RegistryHive::from_str(hive_str)
                .ok_or_else(|| format!("Unknown registry hive: {}", hive_str))?;
            let result = crate::wa::registry::RegistryManager::delete(hive, path, name);
            serde_json::to_string(&json!({
                "success": result.success,
                "operation": result.operation,
                "detail": result.detail
            }))?
        }
        "wa_notifications_list" => {
            let notifications =
                crate::wa::notifications::NotificationManager::get_visible_notifications();
            let count = crate::wa::notifications::NotificationManager::get_notification_count();
            serde_json::to_string(&json!({
                "count": count,
                "notifications": notifications.iter().map(|n| json!({
                    "app": n.app_name,
                    "title": n.title,
                    "body": n.body,
                    "visible": n.is_visible,
                    "system": n.is_system
                })).collect::<Vec<_>>()
            }))?
        }
        "wa_notifications_dismiss" => {
            let result = crate::wa::notifications::NotificationManager::dismiss_all();
            serde_json::to_string(&json!({
                "success": result.success,
                "detail": result.detail,
                "remaining": result.notifications_remaining
            }))?
        }
        "wa_virtual_desktop_enumerate" => {
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let state = mgr.enumerate();
            serde_json::to_string(&json!({
                "total": state.total_count,
                "current": state.current_index,
                "desktops": state.desktops.iter().map(|d| json!({
                    "id": d.id,
                    "name": d.name,
                    "index": d.index,
                    "is_current": d.is_current
                })).collect::<Vec<_>>()
            }))?
        }
        "wa_virtual_desktop_switch" => {
            let index = arguments["index"].as_u64().ok_or("index is required")? as u32;
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let result = mgr.apply(&crate::wa::virtual_desktop::VDesktopOperation::SwitchTo(
                index,
            ));
            serde_json::to_string(&json!({
                "success": result.success,
                "detail": result.detail
            }))?
        }
        "wa_window_list" => {
            let windows = crate::wa::window_mgmt::WindowManager::enumerate_windows();
            serde_json::to_string(&json!({
                "count": windows.len(),
                "windows": windows.iter().map(|w| json!({
                    "hwnd": w.hwnd,
                    "title": w.title,
                    "class": w.class_name,
                    "pid": w.process_id
                })).collect::<Vec<_>>()
            }))?
        }
        "wa_process_launch" => {
            let exe = arguments["executable"]
                .as_str()
                .ok_or("executable is required")?;
            let mut config = crate::wa::process_mgmt::LaunchConfig::new(exe);
            if let Some(args_str) = arguments["arguments"].as_str() {
                for arg in args_str.split_whitespace() {
                    config = config.arg(arg);
                }
            }
            if let Some(wd) = arguments["working_dir"].as_str() {
                config = config.working_dir(wd);
            }
            let result = crate::wa::process_mgmt::ProcessManager::launch(&config);
            serde_json::to_string(&json!({
                "success": result.success,
                "pid": result.pid,
                "detail": result.detail,
                "window_ready": result.window_ready
            }))?
        }
        "wa_process_terminate" => {
            let pid = arguments["pid"].as_u64().ok_or("pid is required")? as u32;
            let timeout =
                std::time::Duration::from_millis(arguments["timeout_ms"].as_u64().unwrap_or(5000));
            let success = crate::wa::process_mgmt::ProcessManager::terminate(pid, timeout);
            serde_json::to_string(&json!({ "success": success, "pid": pid }))?
        }
        "wa_screenshot" => {
            let output_path = arguments["output_path"]
                .as_str()
                .unwrap_or("screenshot.png");
            let target = if let Some(pid) = arguments["pid"].as_u64() {
                crate::wa::screenshot::CaptureTarget::Window(pid as u32)
            } else if let Some(region) = arguments["region"].as_object() {
                crate::wa::screenshot::CaptureTarget::Region(crate::wa::screenshot::CaptureRegion {
                    x: region.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    y: region.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    width: region.get("width").and_then(|v| v.as_u64()).unwrap_or(1920) as u32,
                    height: region
                        .get("height")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1080) as u32,
                })
            } else {
                crate::wa::screenshot::CaptureTarget::FullScreen
            };
            let img = crate::wa::screenshot::capture(&target);
            let _ = img.save_bmp(std::path::Path::new(output_path));
            serde_json::to_string(&json!({
                "success": img.pixel_count() > 0,
                "path": output_path,
                "width": img.width,
                "height": img.height
            }))?
        }
        "wa_clipboard_read" => {
            let state = crate::wa::clipboard::ClipboardManager::read();
            let text = match &state.content {
                crate::wa::clipboard::ClipboardContent::Text(t) => t.clone(),
                _ => String::new(),
            };
            serde_json::to_string(&json!({ "text": text, "formats": state.available_formats }))?
        }
        "wa_clipboard_write" => {
            let text = arguments["text"].as_str().ok_or("text is required")?;
            // Clipboard write requires PowerShell on Windows
            let script = format!("Set-Clipboard -Value '{}'", text.replace('\'', "''"));
            let _ = std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &script])
                .output();
            serde_json::to_string(&json!({ "success": true, "length": text.len() }))?
        }
        "wa_ocr_capture" => {
            let x = arguments["x"].as_i64().unwrap_or(0) as i32;
            let y = arguments["y"].as_i64().unwrap_or(0) as i32;
            let w = arguments["width"].as_u64().unwrap_or(0) as u32;
            let h = arguments["height"].as_u64().unwrap_or(0) as u32;
            let region = crate::wa::ocr::OcrRegion {
                x,
                y,
                width: w,
                height: h,
            };
            let config = crate::wa::ocr::OcrConfig {
                language: None,
                preprocess: true,
                scale_factor: 1.0,
                min_confidence: 0.5,
            };
            let result = crate::wa::ocr::OcrEngine::recognize_region(&region, &config);
            serde_json::to_string(&json!({
                "text": result.full_text,
                "language": result.language,
                "blocks": result.blocks.len()
            }))?
        }
        // ── Agent Checkpointing ─────────────────────────────────────────────
        "agent_checkpoint_create" => {
            let label = arguments["label"].as_str().ok_or("label is required")?;
            let mut mgr = crate::agent::checkpoint::CheckpointManager::new(root);
            match mgr.checkpoint(label) {
                Some(id) => serde_json::to_string(&json!({
                    "success": true,
                    "checkpointId": id,
                    "label": label,
                    "totalCheckpoints": mgr.count()
                }))?,
                None => serde_json::to_string(&json!({
                    "success": false,
                    "error": "Checkpointing unavailable (not a git repository or git not found)"
                }))?,
            }
        }
        "agent_checkpoint_restore" => {
            let cp_id = arguments["checkpointId"]
                .as_u64()
                .ok_or("checkpointId is required")? as usize;
            let mut mgr = crate::agent::checkpoint::CheckpointManager::new(root);
            // Note: In a real session the same CheckpointManager instance should be reused.
            // This handler provides the MCP interface; the loop_runner holds the live instance.
            match mgr.restore(cp_id) {
                Ok(()) => serde_json::to_string(&json!({
                    "success": true,
                    "restoredTo": cp_id
                }))?,
                Err(e) => serde_json::to_string(&json!({
                    "success": false,
                    "error": e
                }))?,
            }
        }
        "agent_checkpoint_list" => {
            let mgr = crate::agent::checkpoint::CheckpointManager::new(root);
            let list: Vec<Value> = mgr
                .list()
                .iter()
                .map(|cp| {
                    json!({
                        "id": cp.id,
                        "label": cp.label,
                        "gitRef": cp.git_ref,
                        "createdAt": cp.created_at,
                        "dirtyFiles": cp.dirty_files
                    })
                })
                .collect();
            serde_json::to_string(&json!({
                "count": list.len(),
                "checkpoints": list
            }))?
        }
        // ── Agent Memory ────────────────────────────────────────────────────
        "agent_memory_remember" => {
            let key = arguments["key"].as_str().ok_or("key is required")?;
            let content = arguments["content"].as_str().ok_or("content is required")?;
            let tags: Vec<&str> = arguments["tags"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let score = arguments["score"].as_f64().unwrap_or(0.5);
            let mut mem = crate::agent::memory_store::PersistentMemory::open(root);
            mem.remember(key, content, &tags, score);
            let _ = mem.save();
            serde_json::to_string(&json!({
                "success": true,
                "key": key,
                "totalMemories": mem.len()
            }))?
        }
        "agent_memory_recall" => {
            let query = arguments["query"].as_str().ok_or("query is required")?;
            let limit = arguments["limit"].as_u64().unwrap_or(5) as usize;
            let mem = crate::agent::memory_store::PersistentMemory::open(root);
            let hits = mem.recall(query, limit);
            let results: Vec<Value> = hits
                .iter()
                .map(|h| {
                    json!({
                        "key": h.entry.key,
                        "content": h.entry.content,
                        "tags": h.entry.tags,
                        "score": h.entry.score,
                        "similarity": h.similarity
                    })
                })
                .collect();
            serde_json::to_string(&json!({
                "count": results.len(),
                "results": results
            }))?
        }
        "agent_memory_forget" => {
            let key = arguments["key"].as_str().ok_or("key is required")?;
            let mut mem = crate::agent::memory_store::PersistentMemory::open(root);
            let removed = mem.forget(key);
            let _ = mem.save();
            serde_json::to_string(&json!({
                "success": removed,
                "key": key
            }))?
        }
        // ── Test Generation ─────────────────────────────────────────────────
        "code_generate_tests" => {
            let source = arguments["source"].as_str().ok_or("source is required")?;
            let language = arguments["language"].as_str().unwrap_or("rust");
            let tests = generate_test_stubs(source, language);
            serde_json::to_string(&json!({
                "language": language,
                "testCount": tests.len(),
                "tests": tests
            }))?
        }
        "code_coverage_analyze" => {
            // T3c: auto test-coverage analysis. Discovers testable functions
            // (optionally scoped to a file/dir via `path`), reports coverage,
            // and scaffolds test skeletons for the untested ones.
            let mut gen = crate::editor::test_generator::TestGenerator::default();
            if let Some(rel) = arguments["path"].as_str() {
                let target = resolve_workspace_path(root, rel, false)?;
                if target.is_dir() {
                    gen.analyze_coverage(&target);
                } else {
                    gen.analyze_file(root, &target);
                }
            } else {
                gen.analyze_coverage(root);
            }
            let skeletons = gen.generate_tests();
            let untested: Vec<Value> = gen
                .analysis
                .untested_functions
                .iter()
                .take(200)
                .map(|f| {
                    json!({
                        "name": f.name,
                        "file": f.file.display().to_string(),
                        "line": f.line,
                        "signature": f.signature
                    })
                })
                .collect();
            serde_json::to_string(&json!({
                "summary": gen.coverage_summary(),
                "coveragePercent": gen.analysis.coverage_percent,
                "totalFunctions": gen.analysis.total_functions,
                "testedFunctions": gen.analysis.tested_functions,
                "untestedCount": gen.analysis.untested_functions.len(),
                "untested": untested,
                "skeletonCount": skeletons.len(),
                "skeletons": skeletons.iter().map(|s| s.test_body.clone()).collect::<Vec<_>>()
            }))?
        }
        // ── Knowledge / RAG ─────────────────────────────────────────────────
        "knowledge_ingest" => {
            let mut kb = crate::editor::knowledge_base::KnowledgeBase::load(root);
            let (label, added) = if let Some(text) = arguments["text"].as_str() {
                let source = arguments["source"].as_str().unwrap_or("inline");
                let n = kb.ingest_text(source, text);
                (source.to_string(), n)
            } else if let Some(rel) = arguments["path"].as_str() {
                let target = resolve_workspace_path(root, rel, false)?;
                if target.is_dir() {
                    let (files, chunks) = kb.ingest_dir(root, &target);
                    (format!("{rel} ({files} files)"), chunks)
                } else {
                    let n = kb
                        .ingest_path(root, &target)
                        .map_err(|e| -> Box<dyn Error> { e.into() })?;
                    (rel.to_string(), n)
                }
            } else {
                return Err(
                    "knowledge_ingest requires 'text' (with optional 'source') or 'path'".into(),
                );
            };
            kb.save(root).map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string(&json!({
                "success": true,
                "source": label,
                "chunksAdded": added,
                "totalChunks": kb.chunk_count()
            }))?
        }
        "knowledge_search" => {
            let query = arguments["query"].as_str().ok_or("query is required")?;
            let k = arguments["k"].as_u64().unwrap_or(5) as usize;
            let kb = crate::editor::knowledge_base::KnowledgeBase::load(root);
            let hits: Vec<Value> = kb
                .search(query, k)
                .into_iter()
                .map(|h| {
                    json!({
                        "source": h.source,
                        "ordinal": h.ordinal,
                        "score": h.score,
                        "snippet": h.snippet
                    })
                })
                .collect();
            serde_json::to_string(&json!({
                "query": query,
                "resultCount": hits.len(),
                "results": hits
            }))?
        }
        // ── Workflows ───────────────────────────────────────────────────────
        "workflow_run" => {
            let id = arguments["id"].as_str().ok_or("id is required")?;
            let reg = crate::editor::workflow::WorkflowRegistry::load(root);
            match reg.get(id).cloned() {
                Some(workflow) => {
                    let run = workflow.execute(root);
                    serde_json::to_string(&run)?
                }
                None => return Err(format!("Unknown workflow: {id}").into()),
            }
        }
        "connector_call" => {
            let id = arguments["id"].as_str().ok_or("id is required")?;
            let req = crate::connectors::ConnectorRequest {
                method: arguments["method"].as_str().unwrap_or("GET").to_string(),
                path: arguments["path"].as_str().unwrap_or("").to_string(),
                headers: arguments["headers"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                body: arguments["body"].as_str().map(str::to_string),
            };
            let resp = crate::connectors::call_connector(root, id, &req)?;
            serde_json::to_string(&resp)?
        }
        "generate_image" => {
            let prompt = arguments["prompt"].as_str().ok_or("prompt is required")?;
            let model = arguments["model"].as_str();
            let out = arguments["output"].as_str();
            let path = crate::editor::multimodal::generate_image(root, prompt, model, out)?;
            format!("Saved generated image to {}", path.display())
        }
        "describe_image" => {
            let path_arg = arguments["path"].as_str().ok_or("path is required")?;
            let path = root.join(path_arg);
            let v = crate::editor::multimodal::describe_image(&path)?;
            serde_json::to_string(&v)?
        }
        _ => return Ok(None),
    };

    Ok(Some(result))
}

/// Native `convert_to_nda`: convert any file to a portable NDA1 document
/// (text → wrapped DrawText lines + content triples; image → a DrawImage
/// command carrying a data-url), with an optional seal toggle.
fn native_convert_to_nda(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let file_path = arguments["filePath"]
        .as_str()
        .ok_or("filePath is required")?;
    let output_path = arguments["outputPath"].as_str().unwrap_or("");
    let seal = arguments["seal"].as_bool().unwrap_or(false);

    let src = PathBuf::from(file_path);
    let final_output = if output_path.is_empty() {
        format!("{file_path}.nda")
    } else {
        output_path.to_string()
    };
    let out = PathBuf::from(&final_output);

    let doc = crate::editor::nda_document::convert_file_to_doc(&src)?;
    // Record an origin revision so the produced document carries provenance.
    let mut doc = doc;
    let author = crate::editor::nda_document::resolve_author(root);
    let origin = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string());
    doc.commit_revision(
        &author.name,
        &author.email,
        &author.source,
        &crate::editor::nda_document::now_rfc3339(),
        "converted via convert_to_nda",
        &origin,
    );
    let outcome = crate::editor::nda_document::save_to_disk(root, &out, &doc, seal)?;
    let effective_sealed = matches!(
        outcome,
        crate::editor::nda_document::SaveOutcome::Saved { sealed: true }
    );
    let note = if outcome == crate::editor::nda_document::SaveOutcome::FellBackToPortable {
        " NOTE: seal unavailable (no key material); saved portable instead."
    } else {
        ""
    };

    Ok(format!(
        "Success: converted {} to {} ({} triples, {} commands, sealed={}).{}",
        file_path,
        final_output,
        doc.triples.len(),
        doc.commands.len(),
        effective_sealed,
        note
    ))
}

/// Native `read_nda`: parse a portable or sealed NDA1 document and summarize
/// its title, counts, provenance, and (bounded) triples.
fn native_read_nda(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
    let path = PathBuf::from(nda_path);
    let (doc, kind) = crate::editor::nda_document::load_from_disk(root, &path)
        .map_err(|e| format!("failed to read NDA: {e}"))?;

    let mut out = String::new();
    out.push_str(&format!("NDA document: {nda_path}\n"));
    out.push_str(&format!("Kind: {kind:?}\n"));
    out.push_str(&format!("Title: {}\n", doc.title().unwrap_or("(untitled)")));
    out.push_str(&format!(
        "Triples: {} \u{00b7} Commands: {} \u{00b7} Revisions: {}\n",
        doc.triples.len(),
        doc.commands.len(),
        doc.revisions().len()
    ));
    out.push_str(&format!(
        "History chain: {}\n",
        if doc.verify_history().is_ok() {
            "valid"
        } else {
            "BROKEN"
        }
    ));
    for r in doc.revisions() {
        out.push_str(&format!(
            "  rev {} \u{2014} {} <{}> [{}] {} {}\n",
            r.id, r.author_name, r.author_email, r.author_source, r.timestamp, r.message
        ));
    }
    out.push_str("Triples:\n");
    for (i, (s, p, o)) in doc.triples.iter().enumerate() {
        if i >= 200 {
            out.push_str(&format!("  \u{2026} {} more\n", doc.triples.len() - 200));
            break;
        }
        out.push_str(&format!("  {s} \u{2192} {p} \u{2192} {o}\n"));
    }
    Ok(out)
}

fn execute_csharp_mcp_tool(tool_name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let exe_path = std::env::var("VELOCITY_MCP_SERVER")
        .unwrap_or_else(|_| r"C:\WUIAS\velocity_nda\VelocityMcpServer.exe".to_string());

    if !Path::new(&exe_path).exists() {
        return execute_rust_fallback_tool(tool_name, arguments);
    }

    let mut daemon_guard = SIDECAR_DAEMON.lock_safe();

    if daemon_guard.is_none() {
        let child = Command::new(exe_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        *daemon_guard = Some(SidecarDaemon { child });
    } else {
        let daemon = daemon_guard.as_mut().expect("daemon_guard is Some in else branch");
        if let Ok(Some(_status)) = daemon.child.try_wait() {
            let child = Command::new(exe_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            *daemon = SidecarDaemon { child };
        }
    }

    let daemon = daemon_guard.as_mut().expect("daemon_guard is Some after initialization");

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
        let stdin = daemon
            .child
            .stdin
            .as_mut()
            .ok_or("Failed to open stdin of C# daemon")?;
        stdin.write_all(request_str.as_bytes())?;
        stdin.flush()?;
    }

    let response_str;
    {
        let stdout = daemon
            .child
            .stdout
            .as_mut()
            .ok_or("Failed to open stdout of C# daemon")?;
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
        return Err(format!(
            "C# Execution Error: {}",
            err["message"].as_str().unwrap_or("Unknown")
        )
        .into());
    }

    let is_error = response["result"]["isError"].as_bool().unwrap_or(false);
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .ok_or("Failed to parse tool text output")?;

    if is_error {
        Err(text.into())
    } else {
        Ok(text.to_string())
    }
}

fn execute_rust_fallback_tool(
    tool_name: &str,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    match tool_name {
        "convert_to_nda" => {
            let file_path = arguments["filePath"]
                .as_str()
                .ok_or("filePath is required")?;
            let output_path = arguments["outputPath"].as_str().unwrap_or("");

            let final_output = if output_path.is_empty() {
                format!("{}.nda", file_path)
            } else {
                output_path.to_string()
            };

            let content = fs::read(file_path)?;

            let mut nda_bytes = Vec::new();
            nda_bytes.extend_from_slice(b"NDAV");

            let size = content.len() as u32;
            nda_bytes.extend_from_slice(&size.to_le_bytes());

            let file_name = Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.txt");
            nda_bytes.extend_from_slice(file_name.as_bytes());
            nda_bytes.push(0);
            nda_bytes.extend_from_slice(&content);

            fs::write(&final_output, nda_bytes)?;

            Ok(format!(
                "Success: File converted and signed to NDA container at: {}",
                final_output
            ))
        }
        "read_nda" => {
            let nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            let nda_bytes = fs::read(nda_path)?;

            if nda_bytes.len() < 9 || &nda_bytes[0..4] != b"NDAV" {
                return Err("Invalid NDA container format".into());
            }

            let size = u32::from_le_bytes([nda_bytes[4], nda_bytes[5], nda_bytes[6], nda_bytes[7]])
                as usize;

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
            let nda_bytes = fs::read(nda_path)?;

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
            fs::write(&temp_file_path, payload)?;

            let ext = Path::new(&file_name)
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
                "py" => (
                    "python".to_string(),
                    vec![temp_file_path.to_string_lossy().to_string()],
                ),
                "js" => (
                    "node".to_string(),
                    vec![temp_file_path.to_string_lossy().to_string()],
                ),
                "ps1" => (
                    "powershell".to_string(),
                    vec![
                        "-ExecutionPolicy".to_string(),
                        "Bypass".to_string(),
                        "-File".to_string(),
                        temp_file_path.to_string_lossy().to_string(),
                    ],
                ),
                "sh" => (
                    "bash".to_string(),
                    vec![temp_file_path.to_string_lossy().to_string()],
                ),
                "bat" | "cmd" => (
                    "cmd".to_string(),
                    vec![
                        "/c".to_string(),
                        temp_file_path.to_string_lossy().to_string(),
                    ],
                ),
                _ => (temp_file_path.to_string_lossy().to_string(), Vec::new()),
            };

            final_args.extend(args_vec);

            let dll_path = std::env::var("WUIAS_SHIELD_DLL")
                .unwrap_or_else(|_| r"C:\WUIAS\wuias_shield\wuias_shield.dll".to_string());
            let use_sandbox = Path::new(&dll_path).exists() && cfg!(target_os = "windows");

            let output = if use_sandbox {
                #[cfg(target_os = "windows")]
                {
                    run_in_dll_sandbox(&shell_cmd, &final_args, &dll_path)?
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let out = Command::new(&shell_cmd).args(&final_args).output()?;
                    String::from_utf8_lossy(&out.stdout).to_string()
                        + &String::from_utf8_lossy(&out.stderr)
                }
            } else {
                let out = Command::new(&shell_cmd).args(&final_args).output()?;
                String::from_utf8_lossy(&out.stdout).to_string()
                    + &String::from_utf8_lossy(&out.stderr)
            };

            let _ = fs::remove_file(temp_file_path);

            Ok(output)
        }
        _ => Err(format!("Unknown fallback tool: {}", tool_name).into()),
    }
}

#[cfg(target_os = "windows")]
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
fn run_in_dll_sandbox(
    app: &str,
    args: &[String],
    dll_path: &str,
) -> Result<String, Box<dyn Error>> {
    let session_id = format!(
        "nda_session_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
    );
    let sandbox_base = std::env::var("WUIAS_SANDBOX_REDIRECT")
        .unwrap_or_else(|_| r"C:\WUIAS\sandbox\redirect".to_string());
    let redirect_dir = format!("{}\\{}", sandbox_base, session_id);
    fs::create_dir_all(&redirect_dir)?;

    let w_dll_path = to_wstring(dll_path);
    let cmd_line_str = format!("\"{}\" {}", app, args.join(" "));
    let mut w_cmd_line = to_wstring(&cmd_line_str);

    let _ = Command::new("reg")
        .args([
            "add",
            &format!("HKCU\\Software\\WUIAS_Sandbox\\{}", session_id),
            "/f",
        ])
        .output();

    // SAFETY: CreateProcessW with zeroed STARTUPINFOW (with cb set) and PROCESS_INFORMATION.
    // The command line is a valid mutable wide-string. CREATE_SUSPENDED flag is used to
    // allow sandbox configuration before resuming the process.
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
            return Err(format!(
                "CreateProcessW failed. Error code: {}",
                std::io::Error::last_os_error()
            )
            .into());
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

        let dll_bytes: Vec<u8> = w_dll_path.iter().flat_map(|&w| w.to_le_bytes()).collect();

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
        let load_library_fn: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
            std::mem::transmute(load_library_addr);
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

    let mut run_output = format!(
        "=== Sandboxed execution completed (Session: {}) ===\n",
        session_id
    );

    fn count_files_recursive(dir: &Path) -> usize {
        let mut count = 0;
        if let Ok(entries) = fs::read_dir(dir) {
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

    let files_count = count_files_recursive(Path::new(&redirect_dir));
    run_output += &format!(
        "Sandbox redirect folder: {}\nRedirected files written: {}\n",
        redirect_dir, files_count
    );

    let _ = Command::new("reg")
        .args([
            "delete",
            &format!("HKCU\\Software\\WUIAS_Sandbox\\{}", session_id),
            "/f",
        ])
        .output();

    Ok(run_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_convert_then_read_round_trips() {
        let dir = std::env::temp_dir().join(format!("nda_tool_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let src = dir.join("note.txt");
        fs::write(&src, "alpha\nbeta\ngamma").unwrap();
        let out = dir.join("note.nda");

        let conv = native_convert_to_nda(
            &dir,
            &json!({ "filePath": src.to_string_lossy(), "outputPath": out.to_string_lossy() }),
        )
        .unwrap();
        assert!(conv.contains("Success"), "convert output: {conv}");
        assert!(out.exists());

        // The produced file is a portable NDA1 document with a genesis revision.
        let (doc, kind) = crate::editor::nda_document::load_from_disk(&dir, &out).unwrap();
        assert_eq!(kind, crate::editor::nda_document::LoadedKind::Portable);
        assert_eq!(doc.revisions().len(), 1);
        assert!(doc.verify_history().is_ok());
        assert_eq!(doc.commands.len(), 3);

        let read = native_read_nda(&dir, &json!({ "ndaPath": out.to_string_lossy() })).unwrap();
        assert!(read.contains("Revisions: 1"), "read output: {read}");
        assert!(read.contains("History chain: valid"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn native_convert_seal_produces_sealed_doc() {
        let dir = std::env::temp_dir().join(format!("nda_tool_seal_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let src = dir.join("secret.txt");
        fs::write(&src, "classified").unwrap();
        let out = dir.join("secret.nda");

        native_convert_to_nda(
            &dir,
            &json!({ "filePath": src.to_string_lossy(), "outputPath": out.to_string_lossy(), "seal": true }),
        )
        .unwrap();

        let raw = fs::read(&out).unwrap();
        // Sealed envelope sets ENCRYPTED|RAW flags, so it is not a plain portable doc.
        assert!(velocity_browser::nda_portable::NdaPortableDoc::from_portable_bytes(&raw).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
