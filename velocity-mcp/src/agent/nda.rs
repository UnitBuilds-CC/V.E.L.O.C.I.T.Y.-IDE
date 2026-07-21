use super::models::*;
use serde_json::Value;

pub fn hash_str(s: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

pub fn pack_ndav(filename: &str, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"NDAV");
    let size = payload.len() as u32;
    buf.extend_from_slice(&size.to_le_bytes());
    buf.extend_from_slice(filename.as_bytes());
    buf.push(0);
    buf.extend_from_slice(payload);
    buf
}

pub fn unpack_ndav(data: &[u8]) -> Option<(String, Vec<u8>)> {
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

pub fn generate_sitemap_text(workspace_root: &std::path::Path) -> String {
    let mut entries: Vec<(String, String, Option<u64>)> = Vec::new();

    fn scan_sitemap(
        dir: &std::path::Path,
        base: &std::path::Path,
        entries: &mut Vec<(String, String, Option<u64>)>,
    ) {
        if let Ok(read_dir) = std::fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if file_name == ".git"
                    || file_name == "target"
                    || file_name == "node_modules"
                    || file_name == ".velocity"
                {
                    continue;
                }

                if let Ok(meta) = entry.metadata() {
                    let rel_path = path
                        .strip_prefix(base)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .to_string();
                    if meta.is_dir() {
                        entries.push(("dir".to_string(), rel_path.clone(), None));
                        scan_sitemap(&path, base, entries);
                    } else {
                        entries.push(("file".to_string(), rel_path, Some(meta.len())));
                    }
                }
            }
        }
    }

    scan_sitemap(workspace_root, workspace_root, &mut entries);
    entries.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    let mut lines = vec![
        "sitemap version 2".to_string(),
        format!("entry_count {}", entries.len()),
    ];
    for (index, (kind, rel_path, size)) in entries.into_iter().enumerate() {
        lines.push(format!(
            "entry\t{}\t{}\t{}\t{}",
            index,
            kind,
            encode_nda_text(&rel_path),
            size.map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        ));
    }
    lines.join("\n") + "\n"
}

pub fn write_sitemap_nda(workspace_root: &std::path::Path) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let sitemap_text = generate_sitemap_text(workspace_root);
    let _ = std::fs::write(sitemap_dir.join("sitemap.nda"), sitemap_text);
}

pub fn load_chatlogs_nda(workspace_root: &std::path::Path) -> Option<Vec<ChatMessage>> {
    let nda_path = workspace_root.join(".velocity").join("chatlogs.nda");
    if !nda_path.exists() {
        return None;
    }
    let data = std::fs::read(&nda_path).ok()?;
    let text = if let Some((_filename, payload)) = unpack_ndav(&data) {
        String::from_utf8_lossy(&payload).to_string()
    } else {
        String::from_utf8_lossy(&data).to_string()
    };
    let messages = parse_chatlogs_nda(&text);
    if messages.is_empty() {
        None
    } else {
        Some(messages)
    }
}

pub fn parse_chatlogs_nda(text: &str) -> Vec<ChatMessage> {
    if text.starts_with("chatlogs version 3\n") {
        let mut messages = std::collections::BTreeMap::new();
        let mut tool_calls: std::collections::BTreeMap<
            (usize, usize),
            serde_json::Map<String, Value>,
        > = std::collections::BTreeMap::new();
        for line in text.lines() {
            if line.trim().is_empty() || line == "chatlogs version 3" {
                continue;
            }
            if line.starts_with("message_count ")
                || line.starts_with("message\t")
                || line.starts_with("tool_call\t")
            {
                continue;
            }
            if let Some(rest) = line.strip_prefix("field\t") {
                let parts: Vec<&str> = rest.split('\t').collect();
                if parts.len() != 3 {
                    continue;
                }
                let Ok(index) = parts[0].parse::<usize>() else {
                    continue;
                };
                let field = parts[1];
                let value = parts[2];
                let message = messages.entry(index).or_insert_with(|| ChatMessage {
                    role: String::new(),
                    content: String::new(),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                });
                match field {
                    "role" => message.role = decode_nda_text(value),
                    "content" => message.content = decode_nda_text(value),
                    "name" => message.name = decode_optional_nda_text(value),
                    "tool_call_id" => message.tool_call_id = decode_optional_nda_text(value),
                    _ => {}
                }
                continue;
            }
            let Some(rest) = line.strip_prefix("tool_call_field\t") else {
                continue;
            };
            let parts: Vec<&str> = rest.split('\t').collect();
            if parts.len() != 4 {
                continue;
            }
            let Ok(message_index) = parts[0].parse::<usize>() else {
                continue;
            };
            let Ok(call_index) = parts[1].parse::<usize>() else {
                continue;
            };
            let field = parts[2];
            let value = parts[3];
            let tool_call = tool_calls
                .entry((message_index, call_index))
                .or_insert_with(serde_json::Map::new);
            match field {
                "id" | "type" => {
                    tool_call.insert(field.to_string(), Value::String(decode_nda_text(value)));
                }
                "function_name" => {
                    let function = tool_call
                        .entry("function".to_string())
                        .or_insert_with(|| Value::Object(serde_json::Map::new()));
                    if let Some(object) = function.as_object_mut() {
                        object.insert("name".to_string(), Value::String(decode_nda_text(value)));
                    }
                }
                "arguments" => {
                    let function = tool_call
                        .entry("function".to_string())
                        .or_insert_with(|| Value::Object(serde_json::Map::new()));
                    if let Some(object) = function.as_object_mut() {
                        object.insert(
                            "arguments".to_string(),
                            Value::String(decode_nda_text(value)),
                        );
                    }
                }
                _ => {}
            }
        }
        for ((message_index, _call_index), tool_call) in tool_calls {
            let message = messages
                .entry(message_index)
                .or_insert_with(|| ChatMessage {
                    role: String::new(),
                    content: String::new(),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                });
            let array = message
                .tool_calls
                .get_or_insert_with(|| Value::Array(Vec::new()));
            if let Some(items) = array.as_array_mut() {
                items.push(Value::Object(tool_call));
            }
        }
        return messages.into_values().collect();
    }
    if text.starts_with("chatlogs version 2\n") {
        let mut messages = std::collections::BTreeMap::new();
        for line in text.lines() {
            if line.trim().is_empty() || line == "chatlogs version 2" {
                continue;
            }
            if line.starts_with("message_count ") || line.starts_with("message\t") {
                continue;
            }
            let Some(rest) = line.strip_prefix("field\t") else {
                continue;
            };
            let parts: Vec<&str> = rest.split('\t').collect();
            if parts.len() != 3 {
                continue;
            }
            let Ok(index) = parts[0].parse::<usize>() else {
                continue;
            };
            let field = parts[1];
            let value = parts[2];
            let message = messages.entry(index).or_insert_with(|| ChatMessage {
                role: String::new(),
                content: String::new(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            });
            match field {
                "role" => message.role = decode_nda_text(value),
                "content" => message.content = decode_nda_text(value),
                "name" => message.name = decode_optional_nda_text(value),
                "tool_call_id" => message.tool_call_id = decode_optional_nda_text(value),
                "tool_calls" => {
                    message.tool_calls = if value == "-" {
                        None
                    } else {
                        serde_json::from_str(&decode_nda_text(value)).ok()
                    };
                }
                _ => {}
            }
        }
        return messages.into_values().collect();
    }
    if text.starts_with("chatlogs version 1\n") {
        let mut messages = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() || line == "chatlogs version 1" {
                continue;
            }
            let Some(rest) = line.strip_prefix("message\t") else {
                continue;
            };
            let parts: Vec<&str> = rest.split('\t').collect();
            if parts.len() != 6 {
                continue;
            }
            let tool_calls = if parts[5] == "-" {
                None
            } else {
                serde_json::from_str(&decode_nda_text(parts[5])).ok()
            };
            messages.push(ChatMessage {
                role: decode_nda_text(parts[1]),
                content: decode_nda_text(parts[4]),
                name: decode_optional_nda_text(parts[2]),
                tool_call_id: decode_optional_nda_text(parts[3]),
                tool_calls,
            });
        }
        return messages;
    }

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
    messages
}

pub fn serialize_chatlogs_nda(messages: &[ChatMessage]) -> String {
    let mut lines = vec![
        "chatlogs version 3".to_string(),
        format!("message_count {}", messages.len()),
    ];
    for (index, msg) in messages.iter().enumerate() {
        lines.push(format!("message\t{}", index));
        lines.push(format!(
            "field\t{}\trole\t{}",
            index,
            encode_nda_text(&msg.role)
        ));
        lines.push(format!(
            "field\t{}\tname\t{}",
            index,
            encode_optional_nda_text(msg.name.as_deref())
        ));
        lines.push(format!(
            "field\t{}\ttool_call_id\t{}",
            index,
            encode_optional_nda_text(msg.tool_call_id.as_deref())
        ));
        lines.push(format!(
            "field\t{}\tcontent\t{}",
            index,
            encode_nda_text(&msg.content)
        ));
        if let Some(tool_calls) = msg.tool_calls.as_ref().and_then(|value| value.as_array()) {
            for (call_index, tool_call) in tool_calls.iter().enumerate() {
                lines.push(format!("tool_call\t{}\t{}", index, call_index));
                if let Some(id) = tool_call.get("id").and_then(|v| v.as_str()) {
                    lines.push(format!(
                        "tool_call_field\t{}\t{}\tid\t{}",
                        index,
                        call_index,
                        encode_nda_text(id)
                    ));
                }
                if let Some(kind) = tool_call.get("type").and_then(|v| v.as_str()) {
                    lines.push(format!(
                        "tool_call_field\t{}\t{}\ttype\t{}",
                        index,
                        call_index,
                        encode_nda_text(kind)
                    ));
                }
                if let Some(function) = tool_call.get("function") {
                    if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                        lines.push(format!(
                            "tool_call_field\t{}\t{}\tfunction_name\t{}",
                            index,
                            call_index,
                            encode_nda_text(name)
                        ));
                    }
                    if let Some(arguments) = function.get("arguments").and_then(|v| v.as_str()) {
                        lines.push(format!(
                            "tool_call_field\t{}\t{}\targuments\t{}",
                            index,
                            call_index,
                            encode_nda_text(arguments)
                        ));
                    }
                }
            }
        }
    }
    lines.join("\n") + "\n"
}

pub fn save_chatlogs_nda(workspace_root: &std::path::Path, messages: &[ChatMessage]) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let _ = std::fs::write(
        sitemap_dir.join("chatlogs.nda"),
        serialize_chatlogs_nda(messages),
    );
}

pub fn append_changelog_nda(workspace_root: &std::path::Path, file_path: &str, action: &str) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let changelog_path = sitemap_dir.join("changelog.nda");

    let mut entries = load_changelog_entries(&changelog_path);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    entries.push((now, file_path.to_string(), action.to_string()));
    let _ = std::fs::write(changelog_path, serialize_changelog_nda(&entries));
}

pub fn load_changelog_entries(changelog_path: &std::path::Path) -> Vec<(u64, String, String)> {
    let raw = if let Ok(data) = std::fs::read(changelog_path) {
        if let Some((_filename, payload)) = unpack_ndav(&data) {
            String::from_utf8_lossy(&payload).to_string()
        } else {
            String::from_utf8_lossy(&data).to_string()
        }
    } else {
        String::new()
    };
    parse_changelog_entries(&raw)
}

pub fn parse_changelog_entries(raw: &str) -> Vec<(u64, String, String)> {
    let mut lines = raw.lines();
    let header = lines
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or("");

    if header == "changelog version 2" {
        let mut entries = Vec::new();
        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with("entry_count ") {
                continue;
            }
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.len() != 4 || parts[0] != "entry" {
                continue;
            }
            if let Ok(timestamp) = parts[1].parse() {
                entries.push((
                    timestamp,
                    decode_nda_text(parts[2]),
                    decode_nda_text(parts[3]),
                ));
            }
        }
        return entries;
    }

    let mut entries = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line == "changelog version 1" {
            continue;
        }
        let parts: Vec<&str> = if let Some(rest) = line.strip_prefix("entry\t") {
            rest.split('\t').collect()
        } else {
            line.split('\t').collect()
        };
        if parts.len() != 3 {
            continue;
        }
        if let Ok(timestamp) = parts[0].parse() {
            entries.push((
                timestamp,
                decode_nda_text(parts[1]),
                decode_nda_text(parts[2]),
            ));
        }
    }
    entries
}

pub fn serialize_changelog_nda(entries: &[(u64, String, String)]) -> String {
    let mut lines = vec![
        "changelog version 2".to_string(),
        format!("entry_count {}", entries.len()),
    ];
    for (timestamp, file_path, action) in entries {
        lines.push(format!(
            "entry\t{}\t{}\t{}",
            timestamp,
            encode_nda_text(file_path),
            encode_nda_text(action),
        ));
    }
    lines.join("\n") + "\n"
}

pub fn write_handover_nda(
    workspace_root: &std::path::Path,
    task_state: &str,
    last_active_turn: usize,
    build_status: &str,
    interrupted: bool,
) {
    let sitemap_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&sitemap_dir);
    let handover_path = sitemap_dir.join("handover.nda");

    let payload = [
        "handover version 2".to_string(),
        "field_count 4".to_string(),
        format!("field\tstate\t{}", encode_nda_text(task_state)),
        format!("field\tturn\t{}", last_active_turn),
        format!("field\tbuild\t{}", encode_nda_text(build_status)),
        format!("field\tinterrupted\t{}", interrupted),
    ]
    .join("\n")
        + "\n";
    let _ = std::fs::write(handover_path, payload);
}

pub fn write_last_request_artifacts(
    workspace_root: &std::path::Path,
    profile: &ModelInfo,
    model: &str,
    provider: AiProvider,
    thinking: bool,
    messages: &[ChatMessage],
    tools: &[Value],
    request_body: &Value,
) {
    let velocity_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&velocity_dir);
    let _ = std::fs::write(
        velocity_dir.join("last_request.nda"),
        serialize_last_request_nda(
            profile,
            model,
            provider,
            thinking,
            messages,
            tools,
            request_body,
        ),
    );
    let _ = std::fs::write(
        velocity_dir.join("last_request.json"),
        serde_json::to_string_pretty(request_body).unwrap_or_default(),
    );
}

pub fn serialize_last_request_nda(
    profile: &ModelInfo,
    model: &str,
    provider: AiProvider,
    thinking: bool,
    messages: &[ChatMessage],
    tools: &[Value],
    request_body: &Value,
) -> String {
    let mut lines = vec![
        "last-request version 3".to_string(),
        format!("field\tprovider\t{}", nda_atom(provider.label())),
        format!(
            "field\tprovider_label\t{}",
            encode_nda_text(provider.label())
        ),
        format!("field\tmodel\t{}", encode_nda_text(model)),
        format!("field\tprofile_id\t{}", encode_nda_text(&profile.id)),
        format!("field\tprofile_label\t{}", encode_nda_text(&profile.label)),
        format!(
            "field\tapi_style\t{}",
            nda_atom(api_style_name(profile.api_style))
        ),
        format!("field\tsupports_tools\t{}", profile.supports_tools),
        format!("field\tsupports_thinking\t{}", profile.supports_thinking),
        format!("field\tthinking\t{}", thinking),
        format!(
            "field\tstream\t{}",
            request_body
                .get("stream")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        ),
        format!("message_count {}", messages.len()),
        format!("tool_count {}", tools.len()),
    ];

    if let Some(prompt) = request_body.get("prompt").and_then(|v| v.as_str()) {
        lines.push(format!("field\tprompt\t{}", encode_nda_text(prompt)));
    }

    if let Some(reasoning) = request_body.get("reasoning") {
        lines.push(format!(
            "field\treasoning\t{}",
            encode_nda_text(&reasoning.to_string())
        ));
    }

    for (index, message) in messages.iter().enumerate() {
        lines.push(format!("message\t{}", index));
        lines.push(format!(
            "message_field\t{}\trole\t{}",
            index,
            nda_atom(&message.role)
        ));
        lines.push(format!(
            "message_field\t{}\tname\t{}",
            index,
            encode_optional_nda_text(message.name.as_deref())
        ));
        lines.push(format!(
            "message_field\t{}\ttool_call_id\t{}",
            index,
            encode_optional_nda_text(message.tool_call_id.as_deref())
        ));
        lines.push(format!(
            "message_field\t{}\tcontent\t{}",
            index,
            encode_nda_text(&message.content)
        ));
        if let Some(tool_calls) = message
            .tool_calls
            .as_ref()
            .and_then(|value| value.as_array())
        {
            for (call_index, tool_call) in tool_calls.iter().enumerate() {
                lines.push(format!("message_tool_call\t{}\t{}", index, call_index));
                if let Some(id) = tool_call.get("id").and_then(|v| v.as_str()) {
                    lines.push(format!(
                        "message_tool_call_field\t{}\t{}\tid\t{}",
                        index,
                        call_index,
                        encode_nda_text(id)
                    ));
                }
                if let Some(kind) = tool_call.get("type").and_then(|v| v.as_str()) {
                    lines.push(format!(
                        "message_tool_call_field\t{}\t{}\ttype\t{}",
                        index,
                        call_index,
                        encode_nda_text(kind)
                    ));
                }
                if let Some(function) = tool_call.get("function") {
                    if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                        lines.push(format!(
                            "message_tool_call_field\t{}\t{}\tfunction_name\t{}",
                            index,
                            call_index,
                            encode_nda_text(name)
                        ));
                    }
                    if let Some(arguments) = function.get("arguments").and_then(|v| v.as_str()) {
                        if let Ok(parsed) = serde_json::from_str::<Value>(arguments) {
                            append_nda_json_rows(
                                &mut lines,
                                format!("message_tool_call_arg\t{}\t{}", index, call_index),
                                "$",
                                &parsed,
                            );
                        } else {
                            lines.push(format!(
                                "message_tool_call_field\t{}\t{}\targuments_raw\t{}",
                                index,
                                call_index,
                                encode_nda_text(arguments)
                            ));
                        }
                    }
                }
            }
        }
    }

    for (index, tool) in tools.iter().enumerate() {
        let name = tool
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let description = tool
            .get("function")
            .and_then(|f| f.get("description"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        lines.push(format!("tool\t{}", index));
        lines.push(format!(
            "tool_field\t{}\tname\t{}",
            index,
            encode_nda_text(name)
        ));
        lines.push(format!(
            "tool_field\t{}\tdescription\t{}",
            index,
            encode_nda_text(description)
        ));
        if let Some(parameters) = tool.get("function").and_then(|f| f.get("parameters")) {
            append_nda_json_rows(
                &mut lines,
                format!("tool_parameter\t{}", index),
                "$",
                parameters,
            );
        }
    }

    lines.join("\n") + "\n"
}

pub fn append_nda_json_rows(lines: &mut Vec<String>, prefix: String, path: &str, value: &Value) {
    match value {
        Value::Object(map) => {
            lines.push(format!("{}\t{}\tobject\t-", prefix, encode_nda_text(path)));
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child_path = if path == "$" {
                    format!("$.{}", key)
                } else {
                    format!("{}.{}", path, key)
                };
                append_nda_json_rows(lines, prefix.clone(), &child_path, &map[key]);
            }
        }
        Value::Array(items) => {
            lines.push(format!(
                "{}\t{}\tarray\t{}",
                prefix,
                encode_nda_text(path),
                items.len()
            ));
            for (index, item) in items.iter().enumerate() {
                let child_path = format!("{}[{}]", path, index);
                append_nda_json_rows(lines, prefix.clone(), &child_path, item);
            }
        }
        Value::String(text) => lines.push(format!(
            "{}\t{}\tstring\t{}",
            prefix,
            encode_nda_text(path),
            encode_nda_text(text)
        )),
        Value::Number(number) => lines.push(format!(
            "{}\t{}\tnumber\t{}",
            prefix,
            encode_nda_text(path),
            encode_nda_text(&number.to_string())
        )),
        Value::Bool(boolean) => lines.push(format!(
            "{}\t{}\tbool\t{}",
            prefix,
            encode_nda_text(path),
            boolean
        )),
        Value::Null => lines.push(format!("{}\t{}\tnull\t-", prefix, encode_nda_text(path))),
    }
}

pub fn api_style_name(style: ApiStyle) -> &'static str {
    match style {
        ApiStyle::OpenAiTools => "openai-tools",
        ApiStyle::OpenAiChat => "openai-chat",
        ApiStyle::PromptCompletion => "prompt-completion",
    }
}

pub fn nda_atom(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if !last_dash && !out.is_empty() {
                out.push('-');
            }
            last_dash = true;
        } else {
            out.push(mapped);
            last_dash = false;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "empty".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

pub fn decode_nda_text(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn decode_optional_nda_text(value: &str) -> Option<String> {
    if value == "-" {
        None
    } else {
        Some(decode_nda_text(value))
    }
}

pub fn encode_optional_nda_text(value: Option<&str>) -> String {
    value
        .map(encode_nda_text)
        .unwrap_or_else(|| "-".to_string())
}

pub fn serialize_transcript_nda(content: &[u8]) -> String {
    let text = String::from_utf8_lossy(content);
    let mut lines = vec![
        "transcript version 2".to_string(),
        "field_count 2".to_string(),
        "field\tsource\tjsonl".to_string(),
        format!("field\ttrailing_newline\t{}", text.ends_with('\n')),
        format!("line_count {}", text.lines().count()),
    ];
    for (index, line) in text.lines().enumerate() {
        lines.push(format!("line\t{}\t{}", index, encode_nda_text(line)));
    }
    lines.join("\n") + "\n"
}

pub fn write_workspace_transcript_nda(workspace_root: &std::path::Path, content: &[u8]) {
    let velocity_dir = workspace_root.join(".velocity");
    let _ = std::fs::create_dir_all(&velocity_dir);
    let workspace_nda = velocity_dir.join("transcript.nda");
    let _ = std::fs::write(workspace_nda, serialize_transcript_nda(content));
}

pub fn convert_jsonl_to_nda(workspace_root: &std::path::Path) {
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
        write_workspace_transcript_nda(workspace_root, &content);
    }
}
