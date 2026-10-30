use super::super::models::*;
use super::context_budget;
use crate::registry::event_store::MAX_TOOL_RESULT_FOR_CONTEXT;
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

pub(crate) use crate::registry::ToolSurface;

/// Tells an inline-tool model that its catalog is the whole of its surface, and
/// what it would have to ask for to see more. Without this it probes for the
/// browsing and messaging tools it has been given on other turns and wastes the
/// turn when they are refused.
const SURFACE_NOTE: &str = "This turn advertises only the tool groups its task needs: \
the tools below are the only ones you can call.";

/// The leading sentence of `text`, whitespace-collapsed and clamped to
/// `max_chars` on a character boundary.
///
/// Registry descriptions are a one-line summary followed by pages of guidance —
/// `plan_org_browser_login` alone runs to 12 KB — and rendering them verbatim
/// costs tens of thousands of tokens per request, which on the small local
/// windows that take the inline path *is* the entire context. The first sentence
/// is what identifies the tool; the rest is advice for a caller that can already
/// read the schema.
fn brief(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let sentence = match collapsed.find(". ") {
        Some(dot) => &collapsed[..dot + 1],
        None => collapsed.as_str(),
    };
    let clipped = clamp_chars(sentence, max_chars);
    if clipped.len() < sentence.len() {
        format!("{}…", clipped)
    } else {
        clipped.to_string()
    }
}

/// `text` truncated to `max_chars` characters, on a byte-safe boundary.
fn clamp_chars(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((cut, _)) => &text[..cut],
        None => text,
    }
}

/// The declared type of one parameter, for the line a schema-less model reads to
/// decide whether a value needs quoting. Union types are joined, and a schema
/// with no `type` (a bare `enum`, or one nested under `anyOf`) reports `any`.
fn schema_type(schema: &Value) -> String {
    match &schema["type"] {
        Value::String(t) => t.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join("|"),
        _ => "any".to_string(),
    }
}

/// The allowed values of an enumerated parameter, which no parameter name
/// conveys: `capability: "deploy"` and `capability: "read"` are unguessable.
fn enum_values(schema: &Value) -> Option<String> {
    let values = schema["enum"].as_array()?;
    let rendered = values
        .iter()
        .map(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| v.to_string())
        })
        .collect::<Vec<_>>()
        .join("|");
    (!rendered.is_empty()).then(|| clamp_chars(&rendered, 60).to_string())
}

/// Prose documentation of the tools this turn can actually call, for models that
/// receive no native schema array (inline-tool and OpenRouter requests).
///
/// `surface` must be the one the loop builds its schema array from — see
/// [`ToolSurface`] — because for these models this text *is* the tool list. A
/// narrowed surface states which groups it withheld and how to ask for them, the
/// same escape hatch the native schema array carries.
pub fn build_inline_tool_docs(surface: &ToolSurface) -> String {
    const TOOL_DOC_CHARS: usize = 160;
    const PARAM_DOC_CHARS: usize = 120;
    const PROSE_SURFACE_LIMIT: usize = 12;
    let advertised = surface.tools();
    let narrow = surface.is_narrow();
    // Per-parameter prose is only affordable while the advertised set is small.
    // Measured over the registry: 347 tools and 1,183 parameter descriptions
    // render 141k characters (~40k tokens) verbatim, of which the descriptions
    // alone are 52k — more than several local windows hold in total, on exactly
    // the models that read their toolset from this prose. A broad surface
    // therefore states name, type, requiredness and allowed values, which is what
    // a call needs to be *valid*; the narrow one keeps the guidance, which is
    // what `edit_file`'s verbatim-match parameter needs to be useful.
    let verbose_params = advertised.len() <= PROSE_SURFACE_LIMIT;
    let mut doc = String::from(
        "\n\n## Available Tools\n\
        Call tools using this exact syntax (one block per call):\n\
        <tool_call>\n\
        <function=TOOL_NAME>\n\
        <parameter=PARAM_NAME>VALUE</parameter>\n\
        </function>\n\n",
    );
    if narrow {
        doc.push_str(SURFACE_NOTE);
        doc.push_str("\n\n");
        // Stated in prose for the same reason the schema array carries it: with
        // no schema, the model cannot discover the one tool that widens its own
        // surface.
        doc.push_str(&crate::registry::request_tool_groups_prose(
            &surface.hidden_slugs(),
        ));
        doc.push('\n');
    }
    for t in &advertised {
        doc.push_str(&format!(
            "### {}\n{}\n",
            t.name,
            brief(&t.description, TOOL_DOC_CHARS)
        ));
        if let Some(props) = t.input_schema["properties"].as_object() {
            let required: Vec<&str> = t.input_schema["required"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            for (param, schema) in props {
                let obligation = if required.contains(&param.as_str()) {
                    "required"
                } else {
                    "optional"
                };
                doc.push_str(&format!(
                    "  - `{}` ({}, {})",
                    param,
                    schema_type(schema),
                    obligation
                ));
                if let Some(values) = enum_values(schema) {
                    doc.push_str(&format!(": one of {values}"));
                }
                if verbose_params {
                    doc.push_str(&format!(
                        " — {}",
                        brief(
                            schema["description"].as_str().unwrap_or(""),
                            PARAM_DOC_CHARS
                        )
                    ));
                }
                doc.push('\n');
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

/// True when a LOCAL Ollama model reasons by default and therefore must not be
/// told to stop thinking with `think:false`. Detection is by model name rather
/// than `profile.supports_thinking`: the catalog profile for a local qwen3.x
/// build is NOT flagged thinking (infer_model_info keys on "thinking"/"qwq"/
/// "r1", none of which appear in "qwen3.6"), yet the model still emits a
/// reasoning trace. The list mirrors `parse_ollama_tags` so the two capability
/// views cannot drift apart, plus a generic "reasoning" catch.
///
/// Also drives `lean_context_for_reasoning_model` in the loop: a model whose
/// catalog profile is NOT flagged thinking (a local qwen3.x build) still emits a
/// reasoning trace and still needs a lean, on-task prompt, so both capability
/// views must key off this one detector rather than `profile.supports_thinking`.
pub(crate) fn ollama_model_reasons(model: &str) -> bool {
    let lower = model.to_lowercase();
    ["qwen3", "r1", "qwq", "thinking", "reasoning"]
        .iter()
        .any(|tag| lower.contains(tag))
}

/// The `think` value the native /api/chat endpoint needs from a local model on
/// a non-thinking turn, chosen by live behaviour on Ollama 0.40.6:
///   * Reasoning models (qwen3.x, r1, qwq): send `think:"low"`. `think:false`
///     is actively harmful — the model degenerates to empty content and burns
///     the entire `num_predict` budget before emitting a call (measured:
///     eval_count=1200, content="", done=length). `think:"low"` keeps a short
///     trace and yields a real tool call with a clean stop (eval_count=628).
///     An explicit thinking turn gets full `think:true`.
///   * Non-reasoning coders (qwen2.5-coder): send `think:false`. They reject a
///     truthy `think` (HTTP 400 "does not support thinking") but accept
///     `think:false` as a harmless no-op (measured: 200).
///
/// Exported for unit testing.
pub fn ollama_think_value(model: &str, thinking: bool) -> Value {
    if ollama_model_reasons(model) {
        if thinking {
            json!(true)
        } else {
            json!("low")
        }
    } else {
        json!(false)
    }
}

/// The context window every local Ollama turn asks for.
///
/// Ollama's native endpoint defaults to a 2048-token window and SILENTLY drops
/// the front of the prompt instead of refusing the request: measured on 0.40.6,
/// a ~50k-token prompt sent with `num_ctx` unset came back with
/// `prompt_eval_count = 2050`. Only reasoning models and write-forced turns set
/// the window before this const existed, so on a non-reasoning coder
/// (qwen2.5-coder) every ordinary tool turn lost the preloaded source, the tool
/// schemas and its own history — the model then re-reads files it was already
/// given, which reads as a wandering agent rather than a clamped one.
///
/// 20480 is the largest window measured to complete a multi-KB write on an 8 GB
/// host running a 22 GB model (32768 overflowed VRAM and returned empty turns).
/// It is now the *floor*: [`local_ollama_num_ctx`] raises it for a model
/// measured to serve more on the host it is actually running on.
pub const LOCAL_OLLAMA_NUM_CTX: u32 = 20480;

/// The window each local model is measured to serve on an 8 GB host.
///
/// The floor above was set by the largest thing this harness runs locally — a
/// 22.6 GB MoE whose weights alone nearly fill the card — and applying it
/// unchanged to a 4.7 GB coder GGUF spends the headroom on nothing. Measured on
/// Colossus (RX 9060 XT, 8 GB, Ollama 0.40.9) with one request and a
/// 24,080-token prompt, `num_predict` held at 9000 so only the window varied:
///
/// | num_ctx | `prompt_eval_count` | outcome |
/// |---|---|---|
/// | 20480 | **10,242** | 13,838 prompt tokens dropped off the FRONT, 81 bytes of answer in 15.7 s |
/// | 32768 | 24,080 | whole prompt kept, 9,000 tokens to the output bound in 310 s |
/// | 40960 | 24,080 | whole prompt kept, 1,676 tokens, natural stop in 64.8 s |
///
/// So the clamp is not a smaller request, it is a request the model answers
/// without its source — and the front it drops is the task anchor. 32768 is the
/// row's number because it is that GGUF's own trained length, which the table
/// must not exceed; the 40960 column shows the card had more to give.
///
/// A row here is a measurement of a model on a host, not a property of the
/// model: the same GGUF on a 6 GB card does not fit 32768, and the operator says
/// so with `VELOCITY_LOCAL_OLLAMA_NUM_CTX`. Unknown models keep the floor.
const MEASURED_LOCAL_NUM_CTX: &[(&str, u32)] = &[("qwen2.5-coder:7b", 32768)];

/// The smallest window an operator may state.
///
/// This is Ollama's own default when `num_ctx` is left out, and it is already too
/// small for a convert turn: the segmented tool surface plus one multi-KB source
/// goes past it before the task text does. A setting below it is a typo, not a
/// request, so `local_ollama_num_ctx_for` treats it as one.
const MIN_USABLE_LOCAL_WINDOW: u32 = 2048;

/// The context window to ask a local Ollama server to allocate for this model.
///
/// Split from the environment so every caller — the request body's
/// `options.num_ctx`, the forced-write turn's override, and
/// [`super::context_budget::declared_window`], which is what bounds how much
/// prompt the loop assembles — reads one number. Those three drifting apart is
/// how a run ends up building a 30k-token prompt against a window the server was
/// told to make smaller.
pub fn local_ollama_num_ctx(model: &str) -> u32 {
    local_ollama_num_ctx_for(model, std::env::var("VELOCITY_LOCAL_OLLAMA_NUM_CTX").ok())
}

/// The rule behind [`local_ollama_num_ctx`], with the operator's setting passed
/// in rather than read.
///
/// Unlike the run ceilings this mirrors, the window has no honest "off": taking
/// `num_ctx` out does not mean unlimited, it means Ollama's own 2048-token
/// default, which silently drops all but the tail of the prompt. So a number is
/// that number *down to* [`MIN_USABLE_LOCAL_WINDOW`], and every other spelling —
/// `off`, `0`, `32k`, a number too small to hold a turn — keeps the measured
/// value. The worst outcome of an operator's mistake must be a smaller window,
/// never a request with no room to stand up in.
pub(crate) fn local_ollama_num_ctx_for(model: &str, env_raw: Option<String>) -> u32 {
    let stated = env_raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<u32>().ok());
    if let Some(number) = stated.filter(|number| *number >= MIN_USABLE_LOCAL_WINDOW) {
        return number;
    }
    let model_lower = model.to_lowercase();
    MEASURED_LOCAL_NUM_CTX
        .iter()
        .find(|(pattern, _)| model_lower.contains(pattern))
        .map(|(_, window)| *window)
        .unwrap_or(LOCAL_OLLAMA_NUM_CTX)
}

/// What an operator typed into an env var that bounds one part of a run.
pub(crate) enum LimitSetting {
    /// Nothing set, or a value that is not a number: keep the built-in default.
    Default,
    /// An explicit "no limit" word, which means the caller must not set the
    /// field at all and let the provider decide.
    Unbounded,
    /// A number the operator chose.
    Value(u64),
}

/// Parse a limit whose ceiling the operator may raise — or remove.
///
/// Three rules, learned from the wall clock this mirrors: a number means that
/// number, the "off" words mean no limit, and anything else means the default
/// rather than a silent unbounded run. A typo must never launch a limit-free
/// request, because then the operator's mistake becomes the next person's
/// nine-hour investigation of why one turn filled a 131k window.
///
/// Split from every caller so tests exercise each spelling without mutating the
/// environment the rest of the process reads.
pub(crate) fn parse_limit_setting(raw: Option<&str>) -> LimitSetting {
    const UNBOUNDED_WORDS: [&str; 6] = ["0", "off", "none", "unbounded", "unlimited", "infinite"];
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return LimitSetting::Default;
    };
    let normalized = value.to_ascii_lowercase();
    if UNBOUNDED_WORDS.contains(&normalized.as_str()) {
        LimitSetting::Unbounded
    } else {
        match normalized.parse::<u64>() {
            Ok(number) => LimitSetting::Value(number),
            Err(_) => LimitSetting::Default,
        }
    }
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
    } else if provider == AiProvider::LocalOllama {
        request["think"] = ollama_think_value(model, thinking);
    }
    // Ollama's NATIVE /api/chat endpoint (see ollama_chat_url) ignores the
    // top-level OpenAI `max_tokens` field and reads its output ceiling from
    // `options.num_predict` instead, with the context window from
    // `options.num_ctx`. Ollama's defaults (num_predict -1 bounded by a 2048
    // window, num_ctx 2048) truncate long tool-call arguments (write_file with
    // multi-KB content) mid-stream, so both must be raised explicitly.
    //
    // Reasoning models run on a reduced `think:"low"` trace (see
    // ollama_think_value): the trace is short but non-zero, so they get the
    // larger output ceiling. The window itself is the same for both — see
    // LOCAL_OLLAMA_NUM_CTX for why leaving it unset on a non-reasoning coder is
    // not a smaller prompt but an invisible one. Detection is by model name, not
    // profile supports_thinking, because the catalog profile for a local qwen3.x
    // build is not flagged thinking (infer_model_info keys on "thinking"/"qwq"/
    // "r1", none of which appear in "qwen3.6"), yet the model still reasons by
    // default — the exact mismatch that made think:false starve the turn.
    if provider == AiProvider::LocalOllama {
        // A turn that only reboots the model between requests wastes 40-90 s
        // reloading multi-GB weights on the buildbox (and the reload is dead
        // time on the socket that can trip read timeouts). Hold it resident.
        request["keep_alive"] = json!("30m");
        let num_ctx = local_ollama_num_ctx(model);
        let preferred = if ollama_model_reasons(model) {
            14_000
        } else {
            8_192
        };
        // The default is a wish, and Ollama grants it by deleting the prompt.
        // Measured on this host against `qwen2.5-coder:7b`: `num_ctx` 2048, a
        // 1088-token prompt, so 960 tokens of room, and `num_predict` 6000 — the
        // server produced all 6000 tokens, slid ~5,000 of the oldest KV out of the
        // window to do it, and reported `done_reason: "stop"` as though it had run
        // out of things to say. A turn cannot tell the difference and neither can
        // the loop, which is why this number is clamped here rather than left to
        // the sampler: what the window holds back is the most any request may ask
        // for, and the derived write ceiling uses the same room.
        let room = (num_ctx as usize)
            .saturating_sub(context_budget::request_input_tokens(messages, tools))
            .max(context_budget::MIN_LOCAL_WRITE_CEILING);
        request["options"] = json!({
            "num_ctx": num_ctx,
            "num_predict": preferred.min(room),
        });
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
fn dedupe_inline_tool_docs(sys_msg: &mut ChatMessage, supports_tools: bool, surface: &ToolSurface) {
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
            base.push_str(&build_inline_tool_docs(surface));
        }
        if !preserved_tail.is_empty() {
            base.push_str("\n\n");
            base.push_str(&preserved_tail);
            base.push('\n');
        }
        sys_msg.content = base;
    } else if !supports_tools {
        sys_msg.content.push_str(&build_inline_tool_docs(surface));
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

pub fn compress_history(
    messages: &[ChatMessage],
    supports_tools: bool,
    surface: &ToolSurface,
) -> Vec<ChatMessage> {
    compress_history_with_elisions(messages, supports_tools, surface).0
}

/// [`compress_history`], plus every message whose bytes did not survive it.
///
/// The base pass loses history three ways, all of them silent today: a tool
/// result replaced by a hash stub, a long message cut to head-and-tail, and the
/// character floor that summarizes the older turns or stops carrying them at all.
/// A run cannot get any of it back from inside itself — the only complete copy of
/// its transcript is the encrypted `chatlogs.nda` — so the caller writes these
/// originals to the overflow ledger and names that file to the model.
pub fn compress_history_with_elisions(
    messages: &[ChatMessage],
    supports_tools: bool,
    surface: &ToolSurface,
) -> (Vec<ChatMessage>, Vec<ChatMessage>) {
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
            dedupe_inline_tool_docs(sys_msg, supports_tools, surface);
        }
    }

    let mut compressed: Vec<ChatMessage> = Vec::new();
    // The originals whose bytes this pass replaces with a stub or a head-and-tail
    // excerpt, in the order they were elided.
    let mut elided: Vec<ChatMessage> = Vec::new();
    for (idx, m) in messages.iter().enumerate() {
        let mut m_copy = m.clone();
        if m_copy.role == "assistant" {
            m_copy.content = strip_think_tags(&m_copy.content);
        }

        if m.role == "tool" {
            let has_subsequent_assistant_msg = messages[idx + 1..]
                .iter()
                .any(|msg| msg.role == "assistant");
            if has_subsequent_assistant_msg && m_copy.content.len() > MAX_TOOL_RESULT_FOR_CONTEXT {
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
                //
                // The read_file hint must NOT be an instruction to re-read the
                // whole file. A result over the threshold is re-compressed on
                // every later turn, so the old "call read_file again" was a
                // trap: the model re-read, the fresh copy compressed next turn,
                // the stub told it to read again, and it never reached the task.
                // The LiteLLM porters spun ~18 re-reads of a 19 KB conventions.md
                // per dispatch, wrote zero files, and the gates went green over
                // an empty crate.
                //
                // But "use what you already read" is its own dead end for a model
                // that has to COPY text: an `edit_file` oldString is verbatim or
                // it matches nothing, and the text it would copy is exactly what
                // was elided. The escape is a bounded re-read — a range survives
                // the budget the whole file could not — so the hint forbids the
                // unbounded re-read and names the bounded one.
                let hint = match tool_name.as_str() {
                    "read_file" => {
                        "This file was already read in full earlier in the conversation; \
                     only this older copy is elided to save context. Do NOT re-read the whole file \
                     to recover the text \u{2014} the fresh copy is elided again next turn. If you \
                     need exact lines \u{2014} to build an `edit_file` `oldString`, for instance \
                     \u{2014} call read_file again with `startLine` and `endLine` for just that \
                     region: a bounded window survives, and its body is verbatim. Otherwise use \
                     what you already read and continue the task."
                    }
                    "run_command" => {
                        "Re-run the command to see this output again; it may have changed."
                    }
                    _ => "Query site_map to retrieve specific symbol details.",
                };

                // The whole payload goes to the ledger before the stub replaces
                // it: the hash proves the model saw it, and nothing in the run can
                // show it what it saw.
                elided.push(m.clone());
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
                // The middle is what disappears, and it is the part a model needs
                // to copy verbatim into an `edit_file` needle.
                elided.push(m.clone());
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

    // The floor's own losses: it summarizes the older turns or stops carrying them
    // outright, and both leave no trace in `result`. Matched as a `(role, content)`
    // multiset because the survivors are shifted, and only the surplus of a
    // repeated message is actually gone.
    if needs_truncation {
        let mut owed: std::collections::HashMap<(&str, &str), usize> =
            std::collections::HashMap::new();
        for m in result.iter().filter(|m| m.role != "system") {
            *owed
                .entry((m.role.as_str(), m.content.as_str()))
                .or_default() += 1;
        }
        for m in &non_system {
            let key = (m.role.as_str(), m.content.as_str());
            match owed.get_mut(&key) {
                Some(count) if *count > 0 => *count -= 1,
                _ => elided.push(m.clone()),
            }
        }
    }

    (result, elided)
}

/// Extract the value of a `key="value"` attribute from an XML-like tag string.
/// Returns `None` when the attribute is absent or malformed.
/// Used by the DSML and inline-tool-call parsers in the agent loop.
pub fn extract_quoted_attr(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// A tool call recovered from structured assistant text by a fallback parser.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedToolCall {
    pub name: String,
    pub arguments: serde_json::Map<String, Value>,
}

/// Byte offset just past the `}` matching the `{` at `start`, respecting JSON
/// string literals and escapes. `None` when the braces never balance (truncated).
fn match_json_brace(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else if b == b'"' {
            in_string = true;
        } else if b == b'{' {
            depth += 1;
        } else if b == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

/// Interpret a `{...}` span as one tool call only when it is unambiguously a
/// call to a KNOWN tool: it has a string `name` in `known_tools` and an
/// `arguments`/`parameters` field (object, or a JSON string encoding one).
/// Anything else returns `None`, so a genuine JSON final answer is never taken
/// for a call.
fn as_bare_tool_call(
    span: &str,
    known_tools: &std::collections::HashSet<String>,
) -> Option<ParsedToolCall> {
    let obj = serde_json::from_str::<Value>(span)
        .ok()?
        .as_object()
        .cloned()?;
    let name = obj.get("name").and_then(|n| n.as_str())?;
    if !known_tools.contains(name) {
        return None;
    }
    let args_field = obj.get("arguments").or_else(|| obj.get("parameters"))?;
    let args_map = match args_field {
        Value::Object(m) => m.clone(),
        Value::String(s) => serde_json::from_str::<Value>(s)
            .ok()
            .and_then(|p| p.as_object().cloned())
            .unwrap_or_default(),
        _ => return None,
    };
    Some(ParsedToolCall {
        name: name.to_string(),
        arguments: args_map,
    })
}

/// A character JSON admits outside a string literal, so a run of anything else
/// at the end of the content is not part of the call.
fn json_structure_char(c: char) -> bool {
    matches!(c, '{' | '}' | '[' | ']' | ':' | ',' | '"' | '-' | '+' | '.')
        || c.is_ascii_alphanumeric()
        || c.is_ascii_whitespace()
}

/// Complete an unclosed `{...}` region that runs to the end of the content, or
/// `None` when it cannot be completed without inventing text.
///
/// A model that emits its call as text instead of a native `tool_calls` delta
/// routinely botches the envelope while leaving the payload intact. `lr7b_v18`
/// turn 6 streamed a 19,673-char `write_file` carrying the whole deliverable and
/// ended it `…}\n"})`: one missing brace, with a stray paren between it and the
/// last real one. Both defects are repaired here — the junk is trimmed off the
/// end and the owed closers go where the call stopped, not at EOF. Without this
/// the turn graded as a final answer that wrote nothing and the run was blocked
/// with the file untouched.
///
/// `None` when the scan ends inside a string literal or on a dangling escape:
/// that is a value cut mid-text, and appending braces there would fabricate the
/// rest of it. The same distinction `salvage_complete_calls` makes at the
/// wall-clock deadline — complete the structure, never the content.
fn complete_trailing_call(content: &str, start: usize) -> Option<String> {
    let body = content[start..].trim_end_matches(|c: char| !json_structure_char(c));
    let mut stack: Vec<u8> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for &b in body.as_bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' => stack.push(b'}'),
                b'[' => stack.push(b']'),
                b'}' | b']' => {
                    stack.pop();
                }
                _ => {}
            }
        }
    }
    if in_string || escaped || stack.is_empty() {
        return None;
    }
    let closers: String = stack.iter().rev().map(|&c| c as char).collect();
    Some(format!("{body}{closers}"))
}

/// Recover tool calls a model emitted as bare OpenAI-style JSON objects in its
/// text content rather than structured `tool_calls` deltas — the shape some
/// Ollama/Qwen builds return (`{"name": "list_dir", "arguments": {...}}`, or an
/// array of them). Scans every balanced `{...}` region, keeps the ones that are
/// calls to registered tools, and returns the content with those spans stripped
/// plus the calls in document order. An empty Vec means the dialect is absent.
pub fn parse_bare_json_tool_calls(
    content: &str,
    known_tools: &std::collections::HashSet<String>,
) -> (String, Vec<ParsedToolCall>) {
    let mut calls = Vec::new();
    let mut cleaned = String::new();
    let mut i = 0usize;
    while i < content.len() {
        if content.as_bytes()[i] == b'{' {
            if let Some(end) = match_json_brace(content, i) {
                if let Some(call) = as_bare_tool_call(&content[i..end], known_tools) {
                    calls.push(call);
                    i = end;
                    continue;
                }
            } else if let Some(completed) = complete_trailing_call(content, i) {
                // Unclosed to EOF, so the rest of the content is this one call.
                if let Some(call) = as_bare_tool_call(&completed, known_tools) {
                    calls.push(call);
                    i = content.len();
                    continue;
                }
            }
        }
        // Advance a whole char so multi-byte text never splits a boundary.
        let ch = content[i..].chars().next().unwrap();
        cleaned.push(ch);
        i += ch.len_utf8();
    }
    (cleaned.trim().to_string(), calls)
}

/// Parse DSML (DeepSeek Markup Language) tool calls out of assistant text.
/// Some DeepSeek-family models emit tool calls as structured text using the
/// fullwidth-pipe delimiter U+FF5C instead of native function_call deltas:
///
/// ```text
/// <｜DSML｜calls>
/// <｜DSML｜ invoke name="read_file">
/// <｜DSML｜ parameter name="relativeFilePath" string="true">TASK.md</｜DSML｜ parameter>
/// </｜DSML｜ invoke>
/// </｜DSML｜calls>
/// ```
///
/// Without this parser the loop sees "final answer, no tool calls" and the
/// worker makes zero progress (the primary failure of the Sept 2026
/// agent_eval run). Returns the content with every DSML block stripped plus
/// the calls in document order; an empty Vec when there is nothing to parse.
pub fn parse_dsml_tool_calls(content: &str) -> (String, Vec<ParsedToolCall>) {
    const DSML_DELIM: &str = "\u{FF5C}";
    let dsml_invoke_open = format!("<{}DSML{} invoke", DSML_DELIM, DSML_DELIM);
    let dsml_invoke_close = format!("</{}DSML{} invoke>", DSML_DELIM, DSML_DELIM);
    let dsml_param_open = format!("<{}DSML{} parameter", DSML_DELIM, DSML_DELIM);
    let dsml_param_close = format!("</{}DSML{} parameter>", DSML_DELIM, DSML_DELIM);
    if !content.contains(&dsml_invoke_open) {
        return (content.to_string(), Vec::new());
    }
    let mut calls = Vec::new();
    let mut cleaned = String::new();
    let mut rest = content;
    while let Some(inv_start) = rest.find(&dsml_invoke_open) {
        cleaned.push_str(&rest[..inv_start]);
        // Extract function name from the invoke tag
        let tag_end = rest[inv_start..]
            .find('>')
            .map(|p| inv_start + p)
            .unwrap_or(rest.len());
        let invoke_tag = &rest[inv_start..tag_end];
        let fname = extract_quoted_attr(invoke_tag, "name").unwrap_or_default();
        // Find the matching close tag
        let body_start = (tag_end + 1).min(rest.len());
        let inv_close_pos = rest[body_start..]
            .find(&dsml_invoke_close)
            .map(|p| body_start + p)
            .unwrap_or(rest.len());
        let invoke_body = &rest[body_start..inv_close_pos];
        // Parse parameters
        let mut args = serde_json::Map::new();
        let mut param_rest = invoke_body;
        while let Some(ps) = param_rest.find(&dsml_param_open) {
            let after_ps = &param_rest[ps + dsml_param_open.len()..];
            let key_tag_end = after_ps.find('>').unwrap_or(after_ps.len());
            let key_tag = &after_ps[..key_tag_end];
            let key = extract_quoted_attr(key_tag, "name").unwrap_or_default();
            let val_start = (key_tag_end + 1).min(after_ps.len());
            let val_end = after_ps[val_start..]
                .find(&dsml_param_close)
                .map(|e| val_start + e)
                .unwrap_or(after_ps.len());
            let val = after_ps[val_start..val_end].trim().to_string();
            if !key.is_empty() {
                args.insert(key, Value::String(val));
            }
            let next_from = (val_end + dsml_param_close.len()).min(after_ps.len());
            param_rest = &after_ps[next_from..];
        }
        if !fname.is_empty() {
            calls.push(ParsedToolCall {
                name: fname,
                arguments: args,
            });
        }
        // Advance past the invoke close tag
        rest = if inv_close_pos < rest.len() {
            &rest[(inv_close_pos + dsml_invoke_close.len()).min(rest.len())..]
        } else {
            ""
        };
    }
    cleaned.push_str(rest);
    // Drop the surrounding calls wrapper tags so only prose remains. Models
    // emit it with and without the space after the second delimiter.
    let wrappers = [
        format!("<{}DSML{} calls>", DSML_DELIM, DSML_DELIM),
        format!("</{}DSML{} calls>", DSML_DELIM, DSML_DELIM),
        format!("<{}DSML{}calls>", DSML_DELIM, DSML_DELIM),
        format!("</{}DSML{}calls>", DSML_DELIM, DSML_DELIM),
    ];
    let cleaned = wrappers
        .into_iter()
        .fold(cleaned, |acc, wrapper| acc.replace(&wrapper, ""));
    (cleaned.trim().to_string(), calls)
}

/// Parse the Claude-family inline tool dialect out of assistant text.
///
/// Models trained on the Anthropic wire format emit their calls as plain text
/// whenever no native schema is attached to the request. The LiteLLM crucible
/// hit exactly this on the token-plan glm-5.2 route while the profile still
/// claimed the model was not tool-capable: the batch opened with a
/// call-list tag, each call was an invoke marker carrying a quoted name
/// attribute with quoted-name parameter markers inside, every tag was
/// namespace-qualified with the provider prefix, and the whole block was
/// interleaved with the models reasoning prose. None of the other readers
/// (native deltas, the wrapped inline syntax, DSML) match that spelling, so
/// the turn looked like a final answer with no tool calls and the mission
/// ended after one turn.
///
/// Returns the content with every recovered call stripped, plus the calls in
/// document order. An empty Vec means the dialect was not present.
pub fn parse_claude_inline_tool_calls(content: &str) -> (String, Vec<ParsedToolCall>) {
    // Tag assembly goes through these two builders rather than hand-joined
    // format strings: an earlier hand-written closer carried both a literal
    // angle bracket and the escape constant, so every close marker began
    // "</<" and matched nothing in the wild — the parser then read each
    // parameter value straight to the end of its call and swallowed the tags.
    let open_tag = |prefix: &str, name: &str| format!("<{}{}>", prefix, name);
    let close_tag = |prefix: &str, name: &str| format!("{}/{}{}>", '\u{3c}', prefix, name);
    // Provider-qualified and bare spellings of the same marker.
    let mut invoke_opens: Vec<String> = Vec::new();
    let mut invoke_closes: Vec<String> = Vec::new();
    let mut param_opens: Vec<String> = Vec::new();
    let mut param_closes: Vec<String> = Vec::new();
    for prefix in ["", "antml:"] {
        invoke_opens.push(format!("<{}invoke name=", prefix));
        invoke_closes.push(close_tag(prefix, "invoke"));
        param_opens.push(format!("<{}parameter name=", prefix));
        param_closes.push(close_tag(prefix, "parameter"));
    }

    /// Earliest occurrence of any candidate, with its length.
    fn find_any(haystack: &str, candidates: &[String]) -> Option<(usize, usize)> {
        candidates
            .iter()
            .filter_map(|c| haystack.find(c.as_str()).map(|p| (p, c.len())))
            .min_by_key(|(p, _)| *p)
    }

    let mut calls = Vec::new();
    let mut cleaned = String::new();
    let mut rest = content;
    // No early exit when no invoke marker is present: the reasoning-span and
    // wrapper cleanup below is wanted for those turns too.
    while let Some((inv_start, _)) = find_any(rest, &invoke_opens) {
        cleaned.push_str(&rest[..inv_start]);
        // The invoke tag ends at its closing bracket; a stream cut mid-tag
        // leaves none, so treat the remainder of the text as the tag.
        let tag_end = rest[inv_start..]
            .find('>')
            .map(|p| inv_start + p)
            .unwrap_or(rest.len());
        let fname = extract_quoted_attr(&rest[inv_start..tag_end], "name").unwrap_or_default();
        let body_start = (tag_end + 1).min(rest.len());
        // The call ends at its close marker, at the next call, or at the end
        // of the text — whichever comes first (a truncated stream leaves the
        // close marker unsent, and the call is still executable).
        let inv_close = [
            find_any(&rest[body_start..], &invoke_closes).map(|(p, l)| (body_start + p, l)),
            find_any(&rest[body_start..], &invoke_opens).map(|(p, _)| (body_start + p, 0)),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|(p, _)| *p);
        let (body_end, consumed) = match inv_close {
            Some((p, l)) => (p, l),
            None => (rest.len(), 0),
        };
        let mut args = serde_json::Map::new();
        let mut cursor = body_start;
        while let Some((ps, _)) = find_any(&rest[cursor..body_end], &param_opens) {
            let open_at = cursor + ps;
            let key_tag_end = rest[open_at..body_end]
                .find('>')
                .map(|p| open_at + p)
                .unwrap_or(body_end);
            let key = extract_quoted_attr(&rest[open_at..key_tag_end], "name").unwrap_or_default();
            let val_start = (key_tag_end + 1).min(body_end);
            let val_end = [
                find_any(&rest[val_start..body_end], &param_closes).map(|(p, _)| val_start + p),
                find_any(&rest[val_start..body_end], &param_opens).map(|(p, _)| val_start + p),
            ]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(body_end);
            if !key.is_empty() {
                args.insert(key, coerced_param_value(&rest[val_start..val_end]));
            }
            cursor = val_end;
            // Step over the parameter close marker when the value ended at
            // one (rather than at the next marker or the end of the call).
            if let Some(close) = param_closes
                .iter()
                .find(|c| rest[cursor..].starts_with(c.as_str()))
            {
                cursor += close.len();
            }
        }
        if !fname.is_empty() {
            calls.push(ParsedToolCall {
                name: fname,
                arguments: args,
            });
        }
        let next_from = (body_end + consumed).min(rest.len());
        rest = &rest[next_from..];
    }
    cleaned.push_str(rest);

    // Batch wrappers and reasoning-block tags: the call list is opened and
    // closed in either spelling (with or without the namespace prefix), and
    // the same routes smuggle the models reasoning block into the content
    // channel. These carry no arguments, so cutting them out of the prose is
    // enough to make the turn readable.
    let mut wrappers: Vec<String> = Vec::new();
    for prefix in ["", "antml:"] {
        for name in ["tool_calls", "function_calls", "thinking"] {
            wrappers.push(open_tag(prefix, name));
            wrappers.push(close_tag(prefix, name));
        }
    }
    // A complete reasoning block carries no answer, so drop the whole span
    // rather than only its tags: cutting the tags alone would leak the models
    // private prose into the visible turn and straight into chat history. An
    // unterminated opener keeps its text (only the tag is cut) because
    // assuming the block runs to end-of-stream can swallow a real answer that
    // the provider simply never finished tagging.
    for prefix in ["", "antml:"] {
        let open = open_tag(prefix, "thinking");
        let close = close_tag(prefix, "thinking");
        while let Some(start) = cleaned.find(&open) {
            let after = start + open.len();
            match cleaned[after..].find(&close) {
                Some(rel) => {
                    let end = after + rel + close.len();
                    cleaned = format!("{}{}", &cleaned[..start], &cleaned[end..]);
                }
                None => break,
            }
        }
    }
    cleaned = wrappers
        .into_iter()
        .fold(cleaned, |acc, wrapper| acc.replace(&wrapper, ""));
    (cleaned.trim().to_string(), calls)
}

/// A parameter value as the model wrote it: JSON where the payload is a valid
/// JSON literal (object, array or bare number — large `write_file` bodies
/// arrive that way), otherwise the trimmed text.
fn coerced_param_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() || v.is_array() || v.is_number() || v.is_boolean() {
            return v;
        }
    }
    Value::String(trimmed.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Render DSML markup from an ASCII sketch where `|` stands for the
    /// fullwidth delimiter U+FF5C the models actually emit.
    fn dsml(sketch: &str) -> String {
        sketch.replace('|', "\u{FF5C}")
    }

    #[test]
    fn parses_wellformed_dsml_invoke_block() {
        // Shape captured byte-for-byte from the agent_eval task-8 transcript.
        let content = dsml(
            "Let me read the task.\n\n<|DSML| calls>\n<|DSML| invoke name=\"read_file\">\n<|DSML| parameter name=\"relativeFilePath\" string=\"true\">TASK.md</|DSML| parameter>\n</|DSML| invoke>\n</|DSML| calls>",
        );
        let (cleaned, calls) = parse_dsml_tool_calls(&content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(
            calls[0].arguments["relativeFilePath"],
            Value::String("TASK.md".into())
        );
        assert_eq!(cleaned, "Let me read the task.");
    }

    #[test]
    fn parses_multiple_invokes_in_document_order() {
        let content = dsml(
            "<|DSML| calls><|DSML| invoke name=\"list_dir\"><|DSML| parameter name=\"relativeDirPath\" string=\"true\">fd_poc</|DSML| parameter></|DSML| invoke><|DSML| invoke name=\"read_file\"><|DSML| parameter name=\"relativeFilePath\">a.md</|DSML| parameter></|DSML| invoke></|DSML| calls>",
        );
        let (cleaned, calls) = parse_dsml_tool_calls(&content);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "list_dir");
        assert_eq!(calls[1].name, "read_file");
        assert_eq!(
            calls[1].arguments["relativeFilePath"],
            Value::String("a.md".into())
        );
        assert!(
            !cleaned.contains("DSML"),
            "markup must be stripped: {cleaned}"
        );
    }

    #[test]
    fn unclosed_invoke_still_yields_its_parameters() {
        // Truncated stream: no close tag. Recovering the partial call beats
        // reporting "no tool calls" and ending the turn.
        let content = dsml(
            "<|DSML| invoke name=\"read_file\"><|DSML| parameter name=\"relativeFilePath\">TASK.md",
        );
        let (_cleaned, calls) = parse_dsml_tool_calls(&content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert!(calls[0].arguments.contains_key("relativeFilePath"));
    }

    #[test]
    fn plain_content_is_untouched() {
        let content = "Just a final answer, no markup at all.";
        let (cleaned, calls) = parse_dsml_tool_calls(content);
        assert!(calls.is_empty());
        assert_eq!(cleaned, content);
    }

    #[test]
    fn extract_quoted_attr_rejects_malformed_tags() {
        assert_eq!(
            extract_quoted_attr(" name=\"x\"", "name").as_deref(),
            Some("x")
        );
        assert_eq!(extract_quoted_attr(" name=", "name"), None);
        assert_eq!(extract_quoted_attr("other=\"v\"", "name"), None);
    }

    /// Render the Claude-family dialect from an ASCII sketch where `~` stands
    /// for the angle bracket the model actually emits.
    fn claude(sketch: &str) -> String {
        sketch.replace('~', "\u{3c}")
    }

    #[test]
    fn claude_inline_parses_the_live_batch_shape() {
        // Byte shape of the LiteLLM crucible phase-1 turn (token-plan
        // glm-5.2): provider-qualified tags, a call-list wrapper closed by
        // the function-plural spelling, and the reasoning block interleaved.
        let content = claude(
            "Let me map the litellm package.\n\n~thinking>I must issue one call per block.~/thinking>\n~antml:tool_calls>\n~antml:invoke name=\"list_dir\">~antml:parameter name=\"relativeDirPath\">.velocity/clones/litellm/litellm~/antml:parameter>~/antml:invoke>\n~antml:invoke name=\"read_file\">~antml:parameter name=\"filePath\">litellm/__init__.py~/antml:parameter>~/antml:invoke>\n~/antml:function_calls>\n",
        );
        let (cleaned, calls) = parse_claude_inline_tool_calls(&content);
        assert_eq!(calls.len(), 2, "both calls in the batch: {calls:?}");
        assert_eq!(calls[0].name, "list_dir");
        assert_eq!(
            calls[0].arguments["relativeDirPath"],
            Value::String(".velocity/clones/litellm/litellm".into())
        );
        assert_eq!(calls[1].name, "read_file");
        assert_eq!(
            calls[1].arguments["filePath"],
            Value::String("litellm/__init__.py".into())
        );
        assert_eq!(cleaned, "Let me map the litellm package.");
    }

    #[test]
    fn claude_inline_accepts_bare_unprefixed_markers() {
        let content = claude(
            "Reading now.~tool_calls>~invoke name=\"grep_search\">~parameter name=\"searchText\">def completion~parameter name=\"maxResults\">25~/invoke>~/tool_calls>",
        );
        let (cleaned, calls) = parse_claude_inline_tool_calls(&content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "grep_search");
        assert_eq!(
            calls[0].arguments["searchText"],
            Value::String("def completion".into())
        );
        // A numeric payload arrives as JSON so the schema check sees a number.
        assert_eq!(calls[0].arguments["maxResults"], Value::from(25));
        assert_eq!(cleaned, "Reading now.");
    }

    #[test]
    fn claude_inline_recovers_a_call_truncated_at_eof() {
        // Stream died before any close marker: the call is still executable.
        let content = claude(
            "~antml:invoke name=\"write_file\">~antml:parameter name=\"relativeFilePath\">rust_port/component_map.md~/antml:parameter>~antml:parameter name=\"content\"># Component map",
        );
        let (_cleaned, calls) = parse_claude_inline_tool_calls(&content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(
            calls[0].arguments["relativeFilePath"],
            Value::String("rust_port/component_map.md".into())
        );
        assert!(calls[0].arguments.contains_key("content"));
    }

    #[test]
    fn claude_inline_drops_a_whole_reasoning_block() {
        // The private prose inside a closed thinking span is not an answer:
        // leaking it into the visible turn also leaks it into chat history,
        // where the model later reads it back as its own statement.
        let content = claude(
            "~thinking>Weighing three crates, then picking reqwest.~/thinking>\nPorting the HTTP layer now.",
        );
        let (cleaned, calls) = parse_claude_inline_tool_calls(&content);
        assert!(calls.is_empty());
        assert_eq!(cleaned, "Porting the HTTP layer now.");
    }

    #[test]
    fn claude_inline_keeps_text_of_an_unclosed_reasoning_opener() {
        // No close marker: the span boundary is unknowable, so only the tag
        // is cut and the prose stays — swallowing it could delete the answer.
        let content = claude("~thinking>Deciding between reqwest and hyper.");
        let (cleaned, calls) = parse_claude_inline_tool_calls(&content);
        assert!(calls.is_empty());
        assert_eq!(cleaned, "Deciding between reqwest and hyper.");
    }

    #[test]
    fn claude_inline_leaves_plain_prose_alone() {
        let content = "The proxy package depends on httpx and fastapi; port it to reqwest.";
        let (cleaned, calls) = parse_claude_inline_tool_calls(content);
        assert!(calls.is_empty());
        assert_eq!(cleaned, content);
    }

    // ── bare OpenAI-style JSON tool calls (Ollama/Qwen dialect) ──────────

    fn known(tools: &[&str]) -> std::collections::HashSet<String> {
        tools.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn bare_json_recovers_a_single_object_call() {
        // Byte-for-byte the phase-1 14B turn that stalled the mission.
        let content =
            r#"{"name": "list_dir", "arguments": {"path": ".velocity/clones/litellm/litellm"}}"#;
        let (cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["list_dir"]));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_dir");
        assert_eq!(
            calls[0].arguments["path"].as_str(),
            Some(".velocity/clones/litellm/litellm")
        );
        assert_eq!(cleaned, "");
    }

    #[test]
    fn bare_json_recovers_an_array_of_calls_in_order() {
        let content = r#"[{"name": "read_file", "arguments": {"path": "a.rs"}}, {"name": "grep_search", "arguments": {"pattern": "fn main"}}]"#;
        let (_cleaned, calls) =
            parse_bare_json_tool_calls(content, &known(&["read_file", "grep_search"]));
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[1].name, "grep_search");
    }

    #[test]
    fn bare_json_keeps_surrounding_prose() {
        let content = r#"Let me look. {"name": "list_dir", "arguments": {"path": "."}} done"#;
        let (cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["list_dir"]));
        assert_eq!(calls.len(), 1);
        assert!(cleaned.contains("Let me look."));
        assert!(cleaned.contains("done"));
        assert!(!cleaned.contains("list_dir"));
    }

    #[test]
    fn bare_json_ignores_unknown_tool_names() {
        // A JSON final answer that happens to carry a `name`/`arguments` pair
        // must not be hijacked unless `name` is a registered tool.
        let content = r#"{"name": "some_report", "arguments": {"x": 1}}"#;
        let (cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["list_dir"]));
        assert!(calls.is_empty());
        assert_eq!(cleaned, content);
    }

    #[test]
    fn bare_json_requires_an_arguments_field() {
        // No `arguments`/`parameters` key: too ambiguous to treat as a call.
        let content = r#"{"name": "list_dir"}"#;
        let (cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["list_dir"]));
        assert!(calls.is_empty());
        assert_eq!(cleaned, content);
    }

    #[test]
    fn bare_json_parses_stringified_arguments() {
        let content = r#"{"name": "read_file", "arguments": "{\"path\": \"x.rs\"}"}"#;
        let (_cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["read_file"]));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments["path"].as_str(), Some("x.rs"));
    }

    #[test]
    fn bare_json_plain_prose_is_untouched() {
        let content = "The map is complete; no further tool calls are needed.";
        let (cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["list_dir"]));
        assert!(calls.is_empty());
        assert_eq!(cleaned, content);
    }

    #[test]
    fn a_call_that_dropped_its_last_brace_is_still_recovered() {
        // `lr7b_v18` turn 6, minus its stray paren: a whole-file write,
        // correctly terminated, one envelope brace short. The run was graded
        // "wrote no files" and blocked with the deliverable untouched.
        let content = r#"{"name": "write_file", "arguments": {"relativeFilePath": "src/a.rs", "content": "pub fn a() {}\n"}"#;
        let (cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["write_file"]));
        assert_eq!(calls.len(), 1, "{cleaned}");
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(
            calls[0].arguments["content"].as_str(),
            Some("pub fn a() {}\n")
        );
        assert_eq!(
            calls[0].arguments["relativeFilePath"].as_str(),
            Some("src/a.rs")
        );
        // The salvaged span is the whole rest of the content, so nothing is left
        // behind to be read as a final answer.
        assert_eq!(cleaned, "");
    }

    #[test]
    fn nested_closers_are_completed_in_the_order_they_are_owed() {
        // The trailing `}` closes the edit object; the array, the arguments and
        // the outer object are all still owed, innermost first.
        let content =
            r#"{"name": "edit_file", "arguments": {"edits": [{"oldString": "a", "newString": "b"}"#;
        let (_cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["edit_file"]));
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].arguments["edits"].as_array().map(|a| a.len()),
            Some(1)
        );
        assert_eq!(
            complete_trailing_call(content, 0).as_deref(),
            Some(
                r#"{"name": "edit_file", "arguments": {"edits": [{"oldString": "a", "newString": "b"}]}}"#
            )
        );
        // A brace inside a string literal is not structure and must not be
        // counted: this port is full of `fn a() {`.
        let braces_in_strings =
            r#"{"name": "read_file", "arguments": {"relativeFilePath": "src/{a}.rs"}"#;
        assert_eq!(
            complete_trailing_call(braces_in_strings, 0).as_deref(),
            Some(r#"{"name": "read_file", "arguments": {"relativeFilePath": "src/{a}.rs"}}"#)
        );
    }

    #[test]
    fn junk_after_the_call_does_not_block_its_completion() {
        // `lr7b_v18` turn 6 in shape: the payload arrived whole and the envelope
        // ended `…}\n"})`. The owed brace belongs where the call stopped, so the
        // paren has to go first — appending it at EOF cannot parse.
        let content = r#"{"name": "write_file", "arguments": {"relativeFilePath": "src/a.rs", "content": "mod t {\n}\n"})"#;
        let (cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["write_file"]));
        assert_eq!(calls.len(), 1, "{cleaned}");
        assert_eq!(calls[0].arguments["content"].as_str(), Some("mod t {\n}\n"));
        assert_eq!(cleaned, "");

        // Trailing prose is not separable from a `true`/`null` literal by this
        // rule, so the completion is attempted and just fails to parse. Nothing
        // is invented out of it.
        let narrated = r#"{"name": "write_file", "arguments": {"content": "x"} and done"#;
        let (cleaned, calls) = parse_bare_json_tool_calls(narrated, &known(&["write_file"]));
        assert!(calls.is_empty());
        assert_eq!(cleaned, narrated);
    }

    #[test]
    fn a_value_cut_mid_string_is_not_completed() {
        // Completing this would fabricate the rest of the file. `in_string` at
        // EOF means content was lost, not just structure, and the honest answer
        // is that there is no call here.
        let content = r#"{"name": "write_file", "arguments": {"relativeFilePath": "src/a.rs", "content": "pub fn a("#;
        assert_eq!(complete_trailing_call(content, 0), None);
        let (cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["write_file"]));
        assert!(calls.is_empty());
        assert_eq!(cleaned, content);

        // Same for a dangling escape: the next character was never delivered.
        let escaped = "{\"name\": \"write_file\", \"arguments\": {\"content\": \"line\\";
        assert_eq!(complete_trailing_call(escaped, 0), None);
    }

    #[test]
    fn an_unclosed_blob_that_is_not_a_call_is_left_alone() {
        // Completing the braces is not a licence to hijack a JSON answer: the
        // name still has to be a registered tool.
        let content = r#"{"name": "drop_table", "arguments": {"table": "users"}"#;
        let (cleaned, calls) = parse_bare_json_tool_calls(content, &known(&["write_file"]));
        assert!(calls.is_empty());
        assert_eq!(cleaned, content);
    }

    #[test]
    fn reasoning_models_never_receive_think_false() {
        // Live defect on Ollama 0.40.6: qwen3.6:35b-a3b with think:false emits
        // empty content and burns the whole num_predict budget before any tool
        // call (eval_count=1200, content="", done=length). think:"low" restores
        // a real tool call with a clean stop (eval_count=628). A non-thinking
        // turn must never send `false` to a model that reasons.
        assert_eq!(ollama_think_value("qwen3.6:35b-a3b", false), json!("low"));
        assert_eq!(ollama_think_value("qwen3:32b", false), json!("low"));
        assert_eq!(ollama_think_value("deepseek-r1:8b", false), json!("low"));
        assert_eq!(ollama_think_value("qwq:32b", false), json!("low"));
        // An explicit thinking turn gets the full trace.
        assert_eq!(ollama_think_value("qwen3.6:35b-a3b", true), json!(true));
    }

    #[test]
    fn non_reasoning_models_keep_think_false_noop() {
        // qwen2.5-coder rejects a truthy think (HTTP 400 does not support
        // thinking) but accepts false as a no-op (200), so it must keep false
        // and must NOT be swept in by the qwen3 reasoning detection.
        assert_eq!(ollama_think_value("qwen2.5-coder:32b", false), json!(false));
        assert_eq!(ollama_think_value("qwen2.5-coder:32b", true), json!(false));
        assert!(!ollama_model_reasons("qwen2.5-coder:32b"));
    }

    #[test]
    fn every_ollama_turn_declares_its_context_window() {
        // Left unset, Ollama's native endpoint allocates 2048 tokens and SILENTLY
        // drops the front of the prompt instead of erroring: measured on 0.40.6,
        // a ~50k-token prompt came back with prompt_eval_count=2050. The window
        // used to be set only for reasoning models, so each ordinary turn of a
        // non-reasoning coder lost the preloaded source, the tool schemas and its
        // own history — which the loop then read back as a wandering agent.
        let profile = |id: &str| ModelInfo {
            id: id.to_string(),
            label: id.to_string(),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: false,
        };
        for (model, num_predict) in [("qwen2.5-coder:7b", 8192u32), ("qwen3.6:35b-a3b", 14000)] {
            let req = build_request(
                &profile(model),
                model,
                &[],
                &[json!({"name": "write_file"})],
                false,
                AiProvider::LocalOllama,
            );
            assert_eq!(
                req["options"]["num_ctx"],
                json!(local_ollama_num_ctx(model)),
                "{model} got no window"
            );
            assert_eq!(req["options"]["num_predict"], json!(num_predict), "{model}");
        }
        // … and the window is per model, not one number for every local route.
        // The floor was measured on a 22.6 GB MoE whose weights nearly fill an
        // 8 GB card; holding a 4.7 GB coder GGUF to it spends headroom on nothing
        // and costs the prompt, because an overflowing window drops its front —
        // the task anchor and the preloaded source — rather than refusing.
        assert_eq!(local_ollama_num_ctx("qwen2.5-coder:7b"), 32768);
        assert_eq!(
            local_ollama_num_ctx("qwen3.6:35b-a3b"),
            LOCAL_OLLAMA_NUM_CTX
        );
        // A cloud provider must not gain the Ollama-only `options` block.
        let req = build_request(
            &profile("qwen-plus"),
            "qwen-plus",
            &[],
            &[],
            false,
            AiProvider::OpenRouter,
        );
        assert!(req.get("options").is_none());
    }

    /// The window is one allocation shared by the prompt and the reply, and Ollama
    /// does not police the pairing for us. Measured on this host against
    /// `qwen2.5-coder:7b`: `num_ctx` 2048, a 1088-token prompt, and
    /// `num_predict` 6000 — the server generated all 6000 tokens, sliding more than
    /// 5,000 of the oldest prompt KV out of the window, then reported
    /// `done_reason: "stop"` with a coherent completion and no change in tokens/s.
    /// Nothing downstream can see that the task anchor had been eaten, so the
    /// planted default has to be inside the room it leaves: the same arithmetic the
    /// loop's write ceiling uses, tool surface included, at every turn rather than
    /// only the forced one.
    #[test]
    fn a_fat_local_prompt_shrinks_num_predict_instead_of_evicting_itself() {
        let profile = ModelInfo {
            id: "qwen2.5-coder:7b".to_string(),
            label: "qwen2.5-coder:7b".to_string(),
            api_style: ApiStyle::OpenAiTools,
            supports_tools: true,
            supports_thinking: false,
        };
        let tools = vec![json!({"name": "write_file"})];
        // A source body big enough that the 8192 default no longer fits behind it:
        // the clamp is what binds here rather than the minimum, and the assertion
        // below checks that the fixture still bites instead of passing vacuously.
        let fat = user_msg(&"fn main() {}\n".repeat(7000));
        let req = build_request(
            &profile,
            "qwen2.5-coder:7b",
            &[sys_msg("You are a porting agent.".to_string()), fat.clone()],
            &tools,
            false,
            AiProvider::LocalOllama,
        );
        let num_ctx = req["options"]["num_ctx"].as_u64().unwrap() as usize;
        let predict = req["options"]["num_predict"].as_u64().unwrap() as usize;
        let input = context_budget::request_input_tokens(
            &[sys_msg("You are a porting agent.".to_string()), fat],
            &tools,
        );
        assert!(
            input + predict <= num_ctx,
            "prompt {input} + num_predict {predict} in a window of {num_ctx} \
             over-subscribes it, which the server answers by dropping the front"
        );
        assert_eq!(
            predict,
            num_ctx - input,
            "and it is given exactly its room, not the 8192 default"
        );
        // Non-vacuity: had the prompt left 8192 tokens or more of room, the two
        // assertions above would pass with the clamp absent. It is the room being
        // *smaller* than the default that this test is about.
        assert!(
            input > num_ctx - 8_192,
            "fixture too small to exercise the clamp: prompt {input} in a window \
             of {num_ctx} still leaves room for the 8192 default, so the branch \
             under test was never reached"
        );
        // An empty request still gets the model's full default: the clamp only
        // takes away room the prompt actually consumed, so it cannot shrink a turn
        // that has the whole window in front of it.
        let bare = build_request(
            &profile,
            "qwen2.5-coder:7b",
            &[],
            &tools,
            false,
            AiProvider::LocalOllama,
        );
        assert_eq!(bare["options"]["num_predict"], json!(8192));
    }

    /// The window is a fact about a model *on a host*, so the operator has to be
    /// able to state it: the same 4.7 GB GGUF that serves 32768 on an 8 GB card
    /// does not fit one on a 6 GB card. Unlike the run ceilings this mirrors,
    /// there is no honest "off" — removing `num_ctx` does not mean unlimited, it
    /// means Ollama's 2048-token default, which silently keeps only the tail of
    /// the prompt. So every non-number spelling buys the measured value, and a
    /// typo degrades the window rather than destroying it.
    #[test]
    fn an_operators_window_number_outranks_the_measured_table() {
        assert_eq!(local_ollama_num_ctx_for("qwen2.5-coder:7b", None), 32768);
        assert_eq!(
            local_ollama_num_ctx_for("qwen2.5-coder:7b", Some("16384".to_string())),
            16384,
            "a smaller stated window is what the host gets"
        );
        assert_eq!(
            local_ollama_num_ctx_for("qwen3.6:35b-a3b", Some(" 65536 ".to_string())),
            65536,
            "the stated number is not clamped to the table's floor"
        );
        for spelling in ["off", "0", "1024", "unbounded", "32k", ""] {
            assert_eq!(
                local_ollama_num_ctx_for("qwen3.6:35b-a3b", Some(spelling.to_string())),
                LOCAL_OLLAMA_NUM_CTX,
                "`{spelling}` must not mean an unset window"
            );
        }
        // The floor of what an operator may state is Ollama's own default, so a
        // stated 2048 is a choice, not a typo.
        assert_eq!(
            local_ollama_num_ctx_for("qwen3.6:35b-a3b", Some("2048".to_string())),
            2048
        );
        // The table is keyed on the id as it arrives, tag and all.
        assert_eq!(
            local_ollama_num_ctx_for("Qwen2.5-Coder:7b", None),
            32768,
            "the match is case-insensitive, like the budget table"
        );
        assert_eq!(
            local_ollama_num_ctx_for("qwen2.5-coder:14b", None),
            LOCAL_OLLAMA_NUM_CTX,
            "a model nobody has measured keeps the floor"
        );
    }

    /// One parser, three ceilings (wall clock, preload bytes, write-turn
    /// tokens): a number means that number, the "off" words mean no limit, and a
    /// typo means the default — never an accidental limit-free run.
    #[test]
    fn an_operators_limit_reads_as_a_number_off_or_a_typo() {
        assert!(matches!(parse_limit_setting(None), LimitSetting::Default));
        assert!(matches!(
            parse_limit_setting(Some("")),
            LimitSetting::Default
        ));
        assert!(matches!(
            parse_limit_setting(Some("eighteen thousand")),
            LimitSetting::Default
        ));
        assert!(matches!(
            parse_limit_setting(Some("32000")),
            LimitSetting::Value(32000)
        ));
        assert!(matches!(
            parse_limit_setting(Some(" 32000 ")),
            LimitSetting::Value(32000)
        ));
        for off in [
            "0",
            "off",
            "OFF",
            "none",
            "unbounded",
            "unlimited",
            "infinite",
        ] {
            assert!(
                matches!(parse_limit_setting(Some(off)), LimitSetting::Unbounded),
                "`{off}` must mean no limit"
            );
        }
    }

    fn sys_msg(content: String) -> ChatMessage {
        ChatMessage {
            role: "system".to_string(),
            content,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    #[test]
    fn narrow_docs_advertise_only_the_tools_the_loop_accepts() {
        // Regression: the inline catalog used to be rendered from the whole
        // registry, so a dispatch told a model that gets no schema array about
        // 347 tools while the loop refused all but the file tools. Every call to
        // anything else was a guaranteed wasted turn.
        let surface = ToolSurface::code();
        let docs = build_inline_tool_docs(&surface);
        for tool in surface.tools() {
            assert!(
                docs.contains(&format!("\n### {}\n", tool.name)),
                "the code surface advertises {} but its prose does not",
                tool.name
            );
        }
        for withheld in [
            "generate_image",
            "team_dispatch",
            "web_navigate",
            "browser_native_assert",
            "wa_registry_write",
            "plan_org_browser_login",
        ] {
            assert!(
                !docs.contains(&format!("\n### {withheld}\n")),
                "{withheld} is on no group this turn offers, so listing it is a lie"
            );
        }
        assert!(
            docs.contains(SURFACE_NOTE),
            "a narrowed surface must say it is the whole of it"
        );
        // The escape hatch has to be reachable without a schema array, or
        // narrowing costs the run a capability it cannot ask back.
        assert!(docs.contains("\n### request_tool_groups\n"));
        assert!(docs.contains("ui_verify"), "withheld groups must be named");
    }

    #[test]
    fn broad_docs_cover_every_registered_tool() {
        // The other half of the invariant above: the broad surface keeps the full
        // registry, so a media or browser tool can never silently vanish from the
        // surface a tool-less model reads its syntax from.
        let docs = build_inline_tool_docs(&ToolSurface::broad());
        let missing: Vec<String> = crate::registry::get_tools()
            .iter()
            .filter(|t| !docs.contains(&format!("\n### {}\n", t.name)))
            .map(|t| t.name.clone())
            .collect();
        assert!(missing.is_empty(), "advertised on no turn: {missing:?}");
        assert!(
            !docs.contains("\n### request_tool_groups\n"),
            "nothing is withheld, so there is nothing to ask for"
        );
    }

    #[test]
    fn inline_catalog_is_bounded() {
        // Measured over the registry as it stands: 347 tools and 1,183 parameters
        // rendered verbatim are 140,629 characters (~40k tokens) — more than the
        // whole 20,480-token window a local Ollama turn declares, so the server
        // dropped the front of the prompt silently. Leading-sentence tool
        // descriptions plus name/type/required parameters bring the broad render
        // to 85,739; a segmented surface does not have to be rendered at all.
        let broad = build_inline_tool_docs(&ToolSurface::broad());
        let longest_line = broad
            .lines()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or_default();
        assert!(
            longest_line <= 320,
            "one line ran to {longest_line} chars, so the clamp is not applied"
        );
        let chars = broad.chars().count();
        assert!(
            chars < 95_000,
            "catalog renders {chars} chars against a 140,629-char verbatim baseline"
        );
        let narrow = build_inline_tool_docs(&ToolSurface::code()).chars().count();
        assert!(
            narrow * 8 < chars,
            "the surface a dispatched worker actually reads is {narrow} of {chars} chars"
        );
    }

    #[test]
    fn narrow_surfaces_keep_the_parameter_guidance() {
        // The 1,183 parameter descriptions are only dropped when the whole
        // registry is advertised: they cost 52k characters there, and nothing on a
        // twelve-tool surface is worth that. Narrow stays verbose because the
        // guidance is load-bearing — a model that cannot see the schema still has
        // to learn that the path is relative to the workspace root.
        let code = tool_docs(build_inline_tool_docs(&ToolSurface::code()), "edit_file");
        assert!(
            code.contains("`relativeFilePath` (string, required) — Path of the existing file"),
            "narrow surface must keep parameter prose: {code}"
        );
        let general = tool_docs(build_inline_tool_docs(&ToolSurface::broad()), "edit_file");
        assert!(
            !general.contains(" — "),
            "the broad surface must not re-add the 52k of parameter prose: {general}"
        );
        assert!(
            general.contains("`relativeFilePath` (string, required)"),
            "the broad surface must still name the parameter and its type: {general}"
        );
    }

    fn tool_docs(docs: String, name: &str) -> String {
        docs.split(&format!("\n### {name}\n"))
            .nth(1)
            .and_then(|rest| rest.split("\n### ").next())
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn compression_reinjects_the_narrowed_surface() {
        // Compression strips the inline catalog and rebuilds it on every request,
        // so rebuilding it from the registry undid the dispatch's narrowing after
        // the first compressed turn. The surface has to ride along with the
        // message, and the runtime-appended sections after the catalog survive it.
        let stale = format!(
            "{}{}\n\n## Recalled Context\nkeep me",
            SYSTEM_PROMPT_BASE,
            build_inline_tool_docs(&ToolSurface::broad())
        );
        let compressed = compress_history(
            &[sys_msg(stale), user_msg("port the file")],
            false,
            &ToolSurface::code(),
        );
        let out = &compressed
            .iter()
            .find(|m| m.role == "system")
            .expect("system message survives")
            .content;
        assert!(
            !out.contains("\n### generate_image\n"),
            "compression re-widened the dispatched surface"
        );
        assert!(out.contains(SURFACE_NOTE));
        assert!(out.contains("keep me"), "Recalled Context must survive");
        assert!(
            out.starts_with("You are Velocity"),
            "the base prompt must survive"
        );
    }
}
