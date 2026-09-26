use super::super::models::*;
use crate::usage::{CloudflareAccount, OpenRouterAccount, UsageTracker};
use crossbeam_channel::Sender;
use serde_json::{json, Value};
use velocity_ide::hash_str;

pub fn send_usage_update(
    tracker: &mut UsageTracker,
    accounts: &[CloudflareAccount],
    or_accounts: &[OpenRouterAccount],
    ui_tx: &Sender<AgentToUiMessage>,
) {
    let views = tracker.build_views(accounts, or_accounts);
    let date = tracker.current_date();
    ui_tx
        .send(AgentToUiMessage::AccountUsage {
            accounts: views,
            date,
        })
        .ok();
}

pub fn is_quota_exhausted_error(body: &str) -> bool {
    body.contains("4006") || body.to_lowercase().contains("quota")
}

/// Estimate the number of tokens in `text` using a code-aware heuristic.
///
/// Pure prose averages ~4 chars/token, but code (Rust, Python, JSON) averages
/// ~2.5–3.5 due to symbols, indentation, and short identifiers. This function
/// blends both ratios based on the proportion of non-alphanumeric characters.
pub fn estimate_tokens(text: &str) -> u64 {
    let len = text.len().max(1);
    // Base: ~3.5 chars/token (mixed code+prose average).
    let base = len as f64 / 3.5;
    // Penalty for high-symbol content (code, JSON): up to 30% more tokens.
    let special_count = text
        .chars()
        .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
        .count();
    let special_ratio = special_count as f64 / len as f64;
    let adjustment = 1.0 + (special_ratio * 0.3);
    (base * adjustment).max(1.0) as u64
}

pub fn render_prompt(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|message| format!("{}: {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The canonical base of the agent's system prompt. Every prompt builder
/// (thread init, workspace switch, ClearHistory, headless) must render from
/// this constant so the grounding rules cannot drift apart per site again.
///
/// The anti-fabrication clause fixes an observed defect: given a one-line
/// source file, a fast model produced a fluent summary inventing "file types,
/// directories and structures" it had actually read verbatim in the tool
/// result. Nothing prohibited inventing content, and the old "high-quality
/// responses" wording actively rewarded polished over faithful output.
pub const SYSTEM_PROMPT_BASE: &str = concat!(
    "You are Velocity, the native AI agent of V.E.L.O.C.I.T.Y.-IDE, running directly in this workspace. ",
    "You have direct local workspace access via tools. NEVER ask the user to paste code snippets, ",
    "upload files, or provide repository links. ",
    "Immediately call `list_dir`, `read_file`, or `grep_search` to inspect and review the workspace. ",
    "Ground everything you write in what you actually read. When summarizing or describing a file, ",
    "state only what the source really contains: never invent structure, sections, examples, or ",
    "details that are not present in it. If a source is short, repetitive, or lacks detail, say so ",
    "plainly; an accurate sparse summary beats a polished fabricated one.",
);

pub fn build_inline_tool_docs() -> String {
    use crate::registry::get_tools;
    let tools = get_tools();
    let mut doc = String::from(
        "\n\n## Available Tools\n\
        Call tools using this exact syntax (one block per call):\n\
        <tool_call>\n\
        <function=TOOL_NAME>\n\
        <parameter=PARAM_NAME>VALUE</parameter>\n\
        </function>\n\n",
    );
    for t in &tools {
        doc.push_str(&format!("### {}\n{}\n", t.name, t.description));
        if let Some(props) = t.input_schema["properties"].as_object() {
            let required: Vec<&str> = t.input_schema["required"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            for (param, schema) in props {
                let desc = schema["description"].as_str().unwrap_or("");
                let req = if required.contains(&param.as_str()) {
                    " (required)"
                } else {
                    " (optional)"
                };
                doc.push_str(&format!("  - `{}`{}: {}\n", param, req, desc));
            }
        }
        doc.push('\n');
    }
    doc.push_str(
        "Always call exactly one tool per <tool_call> block. \
        Wait for the tool result before continuing.\n",
    );
    doc
}

pub fn build_request(
    profile: &ModelInfo,
    model: &str,
    messages: &[ChatMessage],
    tools: &[Value],
    thinking: bool,
    provider: AiProvider,
) -> Value {
    let mut request = json!({"model": model, "stream": true});
    match profile.api_style {
        ApiStyle::PromptCompletion => {
            request["prompt"] = json!(render_prompt(messages));
        }
        ApiStyle::OpenAiTools | ApiStyle::OpenAiChat => {
            request["messages"] = json!(messages);
            if profile.supports_tools && !tools.is_empty() {
                request["tools"] = json!(tools);
            }
        }
    }
    if profile.supports_thinking && thinking {
        match provider {
            AiProvider::CloudflareWorkersAi => {
                request["thinking"] = json!(true);
            }
            AiProvider::OpenRouter => {
                request["reasoning"] = json!({
                    "effort": "high",
                    "exclude": false
                });
            }
            AiProvider::AzureOpenAi => {
                request["reasoning_effort"] = json!("high");
            }
            AiProvider::LocalOllama => {
                request["think"] = json!(true);
            }
            _ => {
                request["reasoning_effort"] = json!("high");
            }
        }
    }
    request
}

pub fn strip_think_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start..].find("</think>") {
            rest = &rest[start + end + "</think>".len()..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// Minimum length of a user message considered a substantive mission brief.
/// The first such message is the task anchor and is pinned against eviction.
pub const MISSION_MIN_CHARS: usize = 400;

/// Detect the first substantive user message (the "mission brief"). Nudges,
/// pleasantries and synthetic summary/tool-result messages do not qualify.
pub fn find_mission_anchor(messages: &[ChatMessage]) -> Option<usize> {
    messages.iter().position(|m| {
        m.role == "user"
            && m.name.is_none()
            && m.tool_call_id.is_none()
            && !m.content.starts_with("[Earlier")
            && !m.content.starts_with("[Tool result for")
            && m.content.chars().count() >= MISSION_MIN_CHARS
    })
}

/// Strip the inline "## Available Tools" documentation block from the system
/// prompt and re-append it only for models without native tool calling.
///
/// For tool-capable models the same schemas already ride along in the request's
/// `tools` array, so the inline copy was pure duplication (it pushed the
/// baseline request past 300 KB). Unlike the previous wholesale rebuild, this
/// preserves the real base prompt plus the runtime-injected
/// "## Previously Learned Patterns" / "## Recalled Context" sections.
fn dedupe_inline_tool_docs(sys_msg: &mut ChatMessage, supports_tools: bool) {
    const DOCS_MARKER: &str = "## Available Tools";
    const TRAILING_SECTIONS: [&str; 2] = ["## Previously Learned Patterns", "## Recalled Context"];
    if let Some(docs_start) = sys_msg.content.find(DOCS_MARKER) {
        let rest = sys_msg.content[docs_start..].to_string();
        let tail_rel = TRAILING_SECTIONS
            .iter()
            .filter_map(|marker| rest.find(marker))
            .min()
            .unwrap_or(rest.len());
        let preserved_tail = rest[tail_rel..].trim_end().to_string();
        let mut base = sys_msg.content[..docs_start].trim_end().to_string();
        if !supports_tools {
            base.push_str(&build_inline_tool_docs());
        }
        if !preserved_tail.is_empty() {
            base.push_str("\n\n");
            base.push_str(&preserved_tail);
            base.push('\n');
        }
        sys_msg.content = base;
    } else if !supports_tools {
        sys_msg.content.push_str(&build_inline_tool_docs());
    }
}

/// The file a `read_file` tool result came from, recovered from the assistant
/// message that issued the matching tool call. Returns `None` when the call
/// carries no path (older transcripts, synthetic messages, inline-tool models
/// whose tool_calls were already flattened).
fn read_file_source_path(
    messages: &[ChatMessage],
    result_idx: usize,
    tool_call_id: Option<&str>,
) -> Option<String> {
    let wanted_id = tool_call_id?;
    for m in messages[..result_idx].iter().rev() {
        let Some(calls) = m.tool_calls.as_ref().and_then(|v| v.as_array()) else {
            continue;
        };
        for call in calls {
            if call.get("id").and_then(|v| v.as_str()) != Some(wanted_id) {
                continue;
            }
            let function = call.get("function")?;
            if function.get("name").and_then(|v| v.as_str()) != Some("read_file") {
                continue;
            }
            let raw = function.get("arguments").and_then(|v| v.as_str())?;
            let args: serde_json::Value = serde_json::from_str(raw).ok()?;
            for key in ["relativeFilePath", "path", "file_path"] {
                if let Some(p) = args.get(key).and_then(|v| v.as_str()) {
                    return Some(p.to_string());
                }
            }
            return None;
        }
    }
    None
}

pub fn compress_history(messages: &[ChatMessage], supports_tools: bool) -> Vec<ChatMessage> {
    const VALID_ROLES: &[&str] = &[
        "system",
        "user",
        "assistant",
        "tool",
        "function",
        "developer",
    ];
    let messages: Vec<ChatMessage> = messages.iter().filter_map(|m| {
        let trimmed_content = m.content.trim();
        if trimmed_content.contains("Tool '' is not registered")
            || trimmed_content == "</tool_call>"
            || trimmed_content == "<tool_call>\n</tool_call>"
            || trimmed_content == "Tool name came through empty. Let me retry with the correct tool:"
            || trimmed_content == "The tool invocation isn't registering the function name. Let me use the proper `read_file` tool:"
            || trimmed_content == "The tool name is still being stripped from my calls, so I can't write the file through the tool right now. But I can give you the exact file to create manually, which will permanently fix the validation error."
            || trimmed_content == "The tool-call parser is consistently dropping the `<function>` tag on my side, so I can't fetch the file right now. However, I already have enough information from the earlier `list_dir` to resolve your build error confidently."
            || trimmed_content == "Apologies \u{2014} the tool name field is being dropped in my calls. Let me retry explicitly with `read_file`:"
            || trimmed_content == "My `write_file` tool calls are being rejected because the function name isn't being transmitted correctly on my side. I cannot directly create the file through tools right now."
            || trimmed_content == "My tool calls keep getting stripped of the function name, so I can't browse the files right now. Based on the earlier `list_dir` of `velocity-mcp/src/editor/`, the UI panels live there (e.g. `chat_panel.rs`, `status_bar.rs`, `app.rs`, `top_bar.rs` or similar)."
            || trimmed_content.starts_with("[Tool result for '']: ")
            || (m.role == "assistant" && trimmed_content.is_empty() && m.tool_calls.is_none())
        {
            return None;
        }

        if m.role.trim().is_empty() {
            if !m.content.trim().is_empty() {
                let mut fixed = m.clone();
                fixed.role = "assistant".to_string();
                Some(fixed)
            } else {
                None
            }
        } else if !VALID_ROLES.contains(&m.role.as_str()) {
            let mut fixed = m.clone();
            fixed.role = "user".to_string();
            Some(fixed)
        } else {
            Some(m.clone())
        }
    }).collect();

    let mut messages = messages;
    if let Some(sys_msg) = messages.iter_mut().find(|m| m.role == "system") {
        // Match the shipped identity prefixes (legacy "Antigravity" prompts can
        // still arrive via restored chat logs) so dedupe applies to our own
        // prompts without touching user-supplied system prompts.
        if sys_msg.content.starts_with("You are Antigravity")
            || sys_msg
                .content
                .starts_with("You are Velocity, the native AI agent")
        {
            dedupe_inline_tool_docs(sys_msg, supports_tools);
        }
    }

    let mut compressed: Vec<ChatMessage> = Vec::new();
    for (idx, m) in messages.iter().enumerate() {
        let mut m_copy = m.clone();
        if m_copy.role == "assistant" {
            m_copy.content = strip_think_tags(&m_copy.content);
        }

        if m.role == "tool" {
            let has_subsequent_assistant_msg = messages[idx + 1..]
                .iter()
                .any(|msg| msg.role == "assistant");
            if has_subsequent_assistant_msg && m_copy.content.len() > 4_000 {
                let tool_name = m_copy
                    .name
                    .clone()
                    .unwrap_or_else(|| "unknown_tool".to_string());
                let content_len = m_copy.content.len();
                let content_hash = hash_str(&m_copy.content);

                // A declaration index only earns its tokens when the payload
                // is actual source code: read_file on a markdown doc used to
                // harvest prose words into a fake "Parsed Declarations" list
                // ("fn read_at" from a code fence), which misled the model.
                // The originating tool call names the file, so trust its
                // extension; when it does not, fall back to the old behaviour.
                let source_path =
                    read_file_source_path(&messages, idx, m_copy.tool_call_id.as_deref());
                let prose_document = source_path
                    .as_deref()
                    .map(|p| {
                        matches!(
                            std::path::Path::new(p)
                                .extension()
                                .and_then(|e| e.to_str())
                                .map(|e| e.to_ascii_lowercase())
                                .as_deref(),
                            Some("md")
                                | Some("markdown")
                                | Some("txt")
                                | Some("rst")
                                | Some("adoc")
                                | Some("log")
                                | Some("csv")
                        )
                    })
                    .unwrap_or(false);

                let mut decls = Vec::new();
                if !prose_document
                    && (tool_name == "read_file"
                        || m_copy.content.contains("fn ")
                        || m_copy.content.contains("class "))
                {
                    for line in m_copy.content.lines() {
                        let line = line.trim();
                        if line.contains("fn ")
                            || line.contains("void ")
                            || line.contains("def ")
                            || line.contains("class ")
                        {
                            let parts: Vec<&str> = line.split_whitespace().collect();
                            for (i, &word) in parts.iter().enumerate() {
                                if word == "fn"
                                    || word == "def"
                                    || word == "class"
                                    || word == "void"
                                {
                                    if let Some(&name) = parts.get(i + 1) {
                                        let name_cleaned = name.split('(').next().unwrap_or(name);
                                        decls.push(format!("{} {}", word, name_cleaned));
                                    }
                                }
                            }
                        }
                    }
                }

                let decl_summary = if decls.is_empty() {
                    if tool_name == "read_file" {
                        // Prose or structured data: a short verbatim excerpt
                        // orients the model better than a hash alone. Fenced
                        // code is skipped: quoting "fn foo" verbatim is how
                        // prose docs got mistaken for a symbol index.
                        let mut excerpt = String::new();
                        let mut in_fence = false;
                        for line in m_copy.content.lines() {
                            let t = line.trim();
                            if t.starts_with("```") {
                                in_fence = !in_fence;
                                continue;
                            }
                            if in_fence || t.is_empty() {
                                continue;
                            }
                            if !excerpt.is_empty() {
                                excerpt.push(' ');
                            }
                            excerpt.push_str(t);
                            if excerpt.len() >= 140 {
                                break;
                            }
                        }
                        if excerpt.is_empty() {
                            String::new()
                        } else {
                            format!("\nExcerpt: {} ...", excerpt)
                        }
                    } else {
                        String::new()
                    }
                } else {
                    format!("\nParsed Declarations:\n  {}", decls.join("\n  "))
                };

                // The recovery hint must be actionable for the tool that
                // produced the output: site_map holds code symbols, not tool
                // results, so telling the model to "query site_map" after a
                // compressed run_command output sent it on a dead end.
                let hint = match tool_name.as_str() {
                    "read_file" => "Call read_file again to retrieve the full content.",
                    "run_command" => {
                        "Re-run the command to see this output again; it may have changed."
                    }
                    _ => "Query site_map to retrieve specific symbol details.",
                };

                m_copy.content = format!(
                    "[Tool output of '{}' compressed to optimize context budget.\n\
                     Merkle Hash: {:016x}\n\
                     Original Size: {} characters.{}\n\
                     (This data was successfully read and processed in a previous turn. {})]",
                    tool_name, content_hash, content_len, decl_summary, hint
                );
            } else if m_copy.content.len() > 12_000 {
                let tool_name = m_copy
                    .name
                    .clone()
                    .unwrap_or_else(|| "unknown_tool".to_string());
                // Snap to char boundaries: slicing at a fixed byte offset
                // panics when the cut lands inside a multi-byte character.
                let mut head_end = 6_000.min(m_copy.content.len());
                while head_end > 0 && !m_copy.content.is_char_boundary(head_end) {
                    head_end -= 1;
                }
                let mut tail_start = m_copy.content.len().saturating_sub(6_000);
                while tail_start < m_copy.content.len()
                    && !m_copy.content.is_char_boundary(tail_start)
                {
                    tail_start += 1;
                }
                let head = &m_copy.content[..head_end];
                let tail_str = &m_copy.content[tail_start..];
                m_copy.content = format!(
                    "{}\n\n[... Truncated middle output of '{}' ({} chars total) to optimize context budget ...]\n\n{}",
                    head, tool_name, m_copy.content.len(), tail_str
                );
            }
        }

        if !supports_tools {
            if m_copy.role == "tool" {
                m_copy.role = "user".to_string();
                let tool_name = m_copy
                    .name
                    .clone()
                    .unwrap_or_else(|| "unknown_tool".to_string());
                m_copy.content = format!("[Tool result for '{}']: {}", tool_name, m_copy.content);
                m_copy.name = None;
                m_copy.tool_call_id = None;
            } else if m_copy.role == "assistant" && m_copy.tool_calls.is_some() {
                if let Some(tool_calls) = m_copy.tool_calls.take() {
                    let mut desc = String::new();
                    if let Some(arr) = tool_calls.as_array() {
                        for tc in arr {
                            let name = tc["function"]["name"].as_str().unwrap_or("unknown");
                            let args = tc["function"]["arguments"].as_str().unwrap_or("{}");
                            if !desc.is_empty() {
                                desc.push('\n');
                            }
                            desc.push_str(&format!(
                                "[Calling tool '{}' with arguments '{}']",
                                name, args
                            ));
                        }
                    }
                    if m_copy.content.trim().is_empty() {
                        m_copy.content = desc;
                    } else {
                        m_copy.content = format!("{}\n\n{}", m_copy.content, desc);
                    }
                }
            }
        }

        if m_copy.role == "assistant"
            && m_copy.content.trim().is_empty()
            && m_copy.tool_calls.is_none()
        {
            continue;
        }

        compressed.push(m_copy);
    }

    // Character floor for the base pass. Model-aware trimming happens in the
    // follow-up budget compression pass, so this only guards against pathological
    // unbounded histories.
    const BUDGET: usize = 200_000;
    let system: Vec<ChatMessage> = compressed
        .iter()
        .filter(|m| m.role == "system")
        .cloned()
        .collect();
    let non_system: Vec<ChatMessage> = compressed
        .into_iter()
        .filter(|m| m.role != "system")
        .collect();

    let system_chars: usize = system.iter().map(|m| m.content.len()).sum();
    let remaining_budget = BUDGET.saturating_sub(system_chars);

    // Calculate total size to determine if we need to drop messages
    let total_chars: usize = non_system.iter().map(|m| m.content.len()).sum();
    let needs_truncation = total_chars > remaining_budget;

    // If we need to truncate, create a summary of older messages that will be dropped
    let mut summary_message: Option<ChatMessage> = None;
    if needs_truncation && non_system.len() > 4 {
        // Keep the last 4 messages intact, summarize the rest
        let messages_to_summarize = &non_system[..non_system.len().saturating_sub(4)];
        if !messages_to_summarize.is_empty() {
            let mut summary_parts = Vec::new();
            let mut user_msg_count = 0;
            let mut assistant_msg_count = 0;
            let mut tool_uses = std::collections::HashSet::new();

            for m in messages_to_summarize {
                match m.role.as_str() {
                    "user" => {
                        user_msg_count += 1;
                        // Extract key topics from user messages (first 100 chars)
                        let preview = m.content.chars().take(100).collect::<String>();
                        if !preview.trim().is_empty() {
                            summary_parts.push(format!("User asked about: {}...", preview.trim()));
                        }
                    }
                    "assistant" => {
                        assistant_msg_count += 1;
                        // Track tool usage
                        if let Some(arr) = m.tool_calls.as_ref().and_then(|v| v.as_array()) {
                            for tc in arr {
                                if let Some(name) = tc
                                    .get("function")
                                    .and_then(|f| f.get("name"))
                                    .and_then(|n| n.as_str())
                                {
                                    tool_uses.insert(name.to_string());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            let mut summary_text = format!(
                "[Earlier conversation summary: {} user messages, {} assistant responses",
                user_msg_count, assistant_msg_count
            );
            if !tool_uses.is_empty() {
                let tools: Vec<_> = tool_uses.iter().take(5).cloned().collect();
                summary_text.push_str(&format!(", tools used: {}", tools.join(", ")));
            }
            summary_text.push_str(". Details compressed to optimize context budget.]");

            if !summary_parts.is_empty() {
                summary_text.push_str("\n\nKey topics from earlier conversation:");
                for part in summary_parts.iter().take(3) {
                    summary_text.push_str(&format!("\n- {}", part));
                }
            }

            summary_message = Some(ChatMessage {
                role: "user".to_string(),
                content: summary_text,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            });
        }
    }

    // Pin the task anchor: the first substantive user message carries the
    // mission brief. Unlike tool results and chatter it must survive truncation
    // verbatim — without it the agent loses its objective and re-orientates
    // every turn.
    let anchor: Option<ChatMessage> = if needs_truncation {
        find_mission_anchor(&non_system).map(|idx| non_system[idx].clone())
    } else {
        None
    };

    let mut tail: Vec<ChatMessage> = Vec::new();
    let mut used = 0usize;
    for m in non_system.iter().rev() {
        let len = m.content.len();
        if used + len > remaining_budget && !tail.is_empty() {
            break;
        }
        tail.push(m.clone());
        used += len;
    }
    tail.reverse();

    if supports_tools {
        let mut valid_tool_call_ids = std::collections::HashSet::new();
        for m in &tail {
            if m.role == "assistant" {
                if let Some(arr) = m.tool_calls.as_ref().and_then(|v| v.as_array()) {
                    for tc in arr {
                        if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                            valid_tool_call_ids.insert(id.to_string());
                        }
                    }
                }
            }
        }
        for m in tail.iter_mut() {
            if m.role == "tool" {
                let has_parent = m
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| valid_tool_call_ids.contains(id));
                if !has_parent {
                    let tool_name = m.name.clone().unwrap_or_else(|| "unknown_tool".to_string());
                    m.role = "user".to_string();
                    m.content = format!("[Tool result for '{}']: {}", tool_name, m.content);
                    m.name = None;
                    m.tool_call_id = None;
                }
            }
        }
    }

    let mut result = system;
    // Insert the pinned mission brief, then the conversation summary, then the
    // recent tail (skip the anchor if the tail already carries it).
    if let Some(anchor) = anchor {
        if !tail
            .iter()
            .any(|m| m.role == "user" && m.content == anchor.content)
        {
            result.push(anchor);
        }
    }
    if let Some(summary) = summary_message {
        result.push(summary);
    }
    result.extend(tail);
    result
}

pub fn sanitize_chat_token(s: &str) -> String {
    let mut out = s.to_string();
    let tags = [
        "</tool_call>",
        "<tool_call>",
        "</function>",
        "<function>",
        "</parameter>",
        "<parameter>",
    ];
    for tag in &tags {
        out = out.replace(&format!("{}\r\n", tag), "");
        out = out.replace(&format!("{}\n", tag), "");
        out = out.replace(tag, "");
    }
    let mut result = String::with_capacity(out.len());
    let chars: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            let mut is_tag_structure = false;
            while j < chars.len() && chars[j] != '>' {
                let c = chars[j];
                if c.is_alphabetic()
                    || c == '/'
                    || c == '='
                    || c == '_'
                    || c == '-'
                    || c.is_ascii_digit()
                    || c == '\"'
                    || c == '\''
                    || c == '.'
                {
                    is_tag_structure = true;
                } else {
                    is_tag_structure = false;
                    break;
                }
                j += 1;
            }
            if is_tag_structure && j < chars.len() && chars[j] == '>' {
                i = j + 1;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}
