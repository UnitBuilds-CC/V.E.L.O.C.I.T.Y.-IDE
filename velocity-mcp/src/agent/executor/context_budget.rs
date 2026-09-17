//! Adaptive context budget management per model.
//!
//! Provides model-specific context window sizes and intelligent compression
//! strategies to optimize token usage while preserving critical information.

use super::super::models::ChatMessage;
use super::utils::{compress_history, estimate_tokens};

/// Default context budget (8k tokens) for unknown models.
const DEFAULT_BUDGET_TOKENS: usize = 8_000;

/// Reserve tokens for model response (output generation).
const RESERVED_OUTPUT_TOKENS: usize = 4_096;

/// Minimum messages to always preserve (last 2).
const MIN_PRESERVE_MESSAGES: usize = 2;

/// Tool results older than this many turns get dropped (keep conclusion only).
const TOOL_RESULT_TURNS_THRESHOLD: usize = 5;

/// A model's context budget entry for fast lookup.
#[derive(Debug, Clone, Copy)]
pub struct ModelContextBudget {
    /// Model identifier pattern (matched case-insensitively with contains).
    pub pattern: &'static str,
    /// Maximum context window size in tokens.
    pub max_tokens: usize,
}

/// Static table of known model context budgets for O(n) lookup.
///
/// Ordered from most specific patterns to least specific for correct matching.
pub const MODEL_BUDGETS: &[ModelContextBudget] = &[
    // GPT-4o and GPT-4-turbo: 128k tokens
    ModelContextBudget {
        pattern: "gpt-4o",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "gpt-4-turbo",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "gpt-4-0125",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "gpt-4-1106",
        max_tokens: 128_000,
    },
    // GPT-3.5-turbo: 16k tokens
    ModelContextBudget {
        pattern: "gpt-3.5-turbo",
        max_tokens: 16_000,
    },
    ModelContextBudget {
        pattern: "gpt-35-turbo",
        max_tokens: 16_000,
    },
    // Claude 3.5 Sonnet and Opus: 200k tokens
    ModelContextBudget {
        pattern: "claude-3.5-sonnet",
        max_tokens: 200_000,
    },
    ModelContextBudget {
        pattern: "claude-3-5-sonnet",
        max_tokens: 200_000,
    },
    ModelContextBudget {
        pattern: "claude-3.5-opus",
        max_tokens: 200_000,
    },
    ModelContextBudget {
        pattern: "claude-3-5-opus",
        max_tokens: 200_000,
    },
    ModelContextBudget {
        pattern: "claude-sonnet-3.5",
        max_tokens: 200_000,
    },
    ModelContextBudget {
        pattern: "claude-opus-3.5",
        max_tokens: 200_000,
    },
    // Claude 3 Haiku: 200k tokens
    ModelContextBudget {
        pattern: "claude-3-haiku",
        max_tokens: 200_000,
    },
    ModelContextBudget {
        pattern: "claude-3-5-haiku",
        max_tokens: 200_000,
    },
    ModelContextBudget {
        pattern: "claude-haiku-3",
        max_tokens: 200_000,
    },
    // Claude 3 Sonnet: 200k tokens
    ModelContextBudget {
        pattern: "claude-3-sonnet",
        max_tokens: 200_000,
    },
    ModelContextBudget {
        pattern: "claude-sonnet-3",
        max_tokens: 200_000,
    },
    // Claude 3 Opus: 200k tokens
    ModelContextBudget {
        pattern: "claude-3-opus",
        max_tokens: 200_000,
    },
    ModelContextBudget {
        pattern: "claude-opus-3",
        max_tokens: 200_000,
    },
    // Llama 3.1 variants: 128k tokens
    ModelContextBudget {
        pattern: "llama-3.1-70b",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "llama-3.1-405b",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "llama-3.1-8b",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "llama-3-1-70b",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "llama-3-1-405b",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "llama-3-1-8b",
        max_tokens: 128_000,
    },
    // Mistral Large: 128k tokens
    ModelContextBudget {
        pattern: "mistral-large",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "mistral-large-2407",
        max_tokens: 128_000,
    },
    // Deepseek V2 and Coder: 128k tokens
    ModelContextBudget {
        pattern: "deepseek-v2",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "deepseek-coder",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "deepseek-chat",
        max_tokens: 128_000,
    },
    // Qwen 2.5: 128k tokens
    ModelContextBudget {
        pattern: "qwen-2.5",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "qwen2.5",
        max_tokens: 128_000,
    },
    ModelContextBudget {
        pattern: "qwen-2-5",
        max_tokens: 128_000,
    },
];

/// Look up the context budget (max tokens) for a given model identifier.
///
/// Returns the max context window size in tokens for the model, or
/// [`DEFAULT_BUDGET_TOKENS`] (8k) if the model is not recognized.
///
/// # Examples
///
/// ```
/// use velocity_mcp::agent::executor::context_budget::get_model_budget;
///
/// assert_eq!(get_model_budget("gpt-4o"), 128_000);
/// assert_eq!(get_model_budget("claude-3.5-sonnet"), 200_000);
/// assert_eq!(get_model_budget("unknown-model"), 8_000);
/// ```
pub fn get_model_budget(model: &str) -> usize {
    let model_lower = model.to_lowercase();

    // Linear scan through budget table (fast for small tables)
    for entry in MODEL_BUDGETS {
        if model_lower.contains(entry.pattern) {
            return entry.max_tokens;
        }
    }

    DEFAULT_BUDGET_TOKENS
}

/// Check if estimated token count fits within the model's context budget.
///
/// Takes into account reserved output tokens to ensure the model has
/// sufficient space for generating a response.
///
/// # Arguments
///
/// * `model` - The model identifier string
/// * `estimated_tokens` - The estimated number of input tokens
///
/// # Returns
///
/// `true` if the estimated tokens fit within the available budget
/// (max budget minus reserved output tokens), `false` otherwise.
///
/// # Examples
///
/// ```
/// use velocity_mcp::agent::executor::context_budget::fits_budget;
///
/// // GPT-4o has 128k budget, minus ~4k reserved = ~124k available
/// assert!(fits_budget("gpt-4o", 100_000));
/// assert!(!fits_budget("gpt-4o", 130_000));
///
/// // GPT-3.5-turbo has 16k budget, minus ~4k reserved = ~12k available
/// assert!(fits_budget("gpt-3.5-turbo", 10_000));
/// assert!(!fits_budget("gpt-3.5-turbo", 15_000));
/// ```
pub fn fits_budget(model: &str, estimated_tokens: usize) -> bool {
    let max_tokens = get_model_budget(model);
    let available = max_tokens.saturating_sub(RESERVED_OUTPUT_TOKENS);
    estimated_tokens <= available
}

/// Calculate the total estimated tokens for a slice of chat messages.
pub fn estimate_messages_tokens(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|m| estimate_tokens(&m.content) as usize)
        .sum()
}

/// Compression strategy result indicating what action to take.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CompressionAction {
    /// Keep message as-is.
    Preserve,
    /// Compress message content into a summary.
    Summarize,
    /// Drop message entirely (keep only conclusion if tool result).
    Drop,
}

/// Determine the compression action for a message at a given position.
///
/// Rules:
/// - Always preserve: system prompt, last 2 messages, file paths, code blocks
/// - Compress: older messages into summaries
/// - Drop: tool results older than 5 turns (keep the conclusion)
fn compression_action_for(
    msg: &ChatMessage,
    index: usize,
    total: usize,
    turn_distance: usize,
) -> CompressionAction {
    // Always preserve system messages
    if msg.role == "system" || msg.role == "developer" {
        return CompressionAction::Preserve;
    }

    // Always preserve last 2 messages
    if index >= total.saturating_sub(MIN_PRESERVE_MESSAGES) {
        return CompressionAction::Preserve;
    }

    // Preserve messages containing file paths or code blocks
    if contains_file_paths(&msg.content) || contains_code_blocks(&msg.content) {
        return CompressionAction::Preserve;
    }

    // Drop old tool results (older than threshold turns)
    if msg.role == "tool" && turn_distance > TOOL_RESULT_TURNS_THRESHOLD {
        return CompressionAction::Drop;
    }

    // Summarize older assistant/user messages
    if turn_distance > 2 {
        return CompressionAction::Summarize;
    }

    CompressionAction::Preserve
}

/// Check if content contains file paths (common patterns).
fn contains_file_paths(content: &str) -> bool {
    let code_extensions = [
        ".rs", ".py", ".js", ".ts", ".json", ".toml", ".md", ".c", ".cpp", ".h", ".hpp", ".java",
        ".go", ".rb", ".css", ".html", ".xml", ".yaml", ".yml", ".sh",
    ];

    // Check for path separators combined with extensions
    let has_path_with_ext = (content.contains('/') || content.contains('\\'))
        && code_extensions.iter().any(|ext| content.contains(ext));

    // Check for bare filenames with known extensions (e.g. "config.json")
    let has_bare_filename = code_extensions.iter().any(|ext| {
        if let Some(pos) = content.find(ext) {
            // Ensure there's a word character before the extension (part of a filename)
            pos > 0
                && content
                    .as_bytes()
                    .get(pos - 1)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        } else {
            false
        }
    });

    has_path_with_ext
        || has_bare_filename
        || content.contains("file://")
        || content.contains("path:")
}

/// Check if content contains code blocks (fenced or indented).
fn contains_code_blocks(content: &str) -> bool {
    content.contains("```")
        || content.contains("fn ")
        || content.contains("pub fn ")
        || content.contains("def ")
        || content.contains("class ")
        || content.contains("impl ")
        || content.contains("struct ")
        || content.contains("enum ")
        || content.contains("const ")
        || content.contains("let mut ")
}

/// Compress conversation history with model-specific budget awareness.
///
/// This function extends the base [`compress_history`] with intelligent
/// budget-aware compression that:
///
/// 1. Looks up the model's context budget
/// 2. Estimates current token usage
/// 3. If over budget, applies progressive compression:
///    - First: summarize older messages
///    - Then: drop old tool results (keep conclusions)
///    - Finally: truncate to fit
///
/// # Arguments
///
/// * `messages` - The full conversation history
/// * `model` - The model identifier for budget lookup
/// * `supports_tools` - Whether the model supports native tool calling
///
/// # Returns
///
/// A compressed vector of messages that fits within the model's budget.
pub fn compress_history_with_budget(
    messages: &[ChatMessage],
    model: &str,
    supports_tools: bool,
) -> Vec<ChatMessage> {
    // First apply base compression (cleans up malformed messages, etc.)
    let base_compressed = compress_history(messages, supports_tools);

    // Get model-specific budget
    let max_tokens = get_model_budget(model);
    let target_tokens = max_tokens.saturating_sub(RESERVED_OUTPUT_TOKENS);

    // Estimate current usage
    let current_tokens = estimate_messages_tokens(&base_compressed);

    // If already within budget, return as-is
    if current_tokens <= target_tokens {
        return base_compressed;
    }

    // Need to compress further - apply budget-aware compression
    apply_budget_compression(&base_compressed, target_tokens)
}

/// Apply progressive budget compression to fit within target tokens.
fn apply_budget_compression(messages: &[ChatMessage], target_tokens: usize) -> Vec<ChatMessage> {
    if messages.is_empty() {
        return Vec::new();
    }

    let total = messages.len();

    // Phase 1: Separate system messages (always preserved)
    let system_msgs: Vec<ChatMessage> = messages
        .iter()
        .filter(|m| m.role == "system" || m.role == "developer")
        .cloned()
        .collect();

    let non_system: Vec<(usize, &ChatMessage)> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role != "system" && m.role != "developer")
        .collect();

    let system_tokens = estimate_messages_tokens(&system_msgs);
    let mut remaining_budget = target_tokens.saturating_sub(system_tokens);

    // Phase 2: Calculate turn distances for each message
    // A "turn" is a user-assistant pair
    let mut turn_count = 0;
    let mut turn_distances: Vec<usize> = Vec::with_capacity(non_system.len());

    for (_, msg) in non_system.iter().rev() {
        if msg.role == "user" {
            turn_count += 1;
        }
        turn_distances.push(turn_count);
    }
    turn_distances.reverse();

    // Phase 3: Classify messages and build result
    let mut result: Vec<ChatMessage> = Vec::new();
    let mut summaries: Vec<String> = Vec::new();

    for ((orig_idx, msg), turn_dist) in non_system.iter().zip(turn_distances.iter()) {
        let action = compression_action_for(msg, *orig_idx, total, *turn_dist);

        match action {
            CompressionAction::Preserve => {
                let msg_tokens = estimate_tokens(&msg.content) as usize;
                if msg_tokens <= remaining_budget || result.is_empty() {
                    result.push((*msg).clone());
                    remaining_budget = remaining_budget.saturating_sub(msg_tokens);
                } else {
                    // Budget exhausted, summarize instead
                    summaries.push(extract_summary_snippet(msg));
                }
            }
            CompressionAction::Summarize => {
                summaries.push(extract_summary_snippet(msg));
            }
            CompressionAction::Drop => {
                // For dropped tool results, keep a brief conclusion
                if msg.role == "tool" {
                    let conclusion = extract_tool_conclusion(msg);
                    if !conclusion.is_empty() {
                        let mut kept = (**msg).clone();
                        kept.content = conclusion;
                        let kept_tokens = estimate_tokens(&kept.content) as usize;
                        if kept_tokens <= remaining_budget {
                            result.push(kept);
                            remaining_budget = remaining_budget.saturating_sub(kept_tokens);
                        }
                    }
                }
            }
        }
    }

    // Phase 4: Build final result with summary if needed
    let mut final_result = system_msgs;

    if !summaries.is_empty() {
        let summary_text = build_conversation_summary(&summaries);
        let summary_msg = ChatMessage {
            role: "user".to_string(),
            content: summary_text,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        };
        let summary_tokens = estimate_tokens(&summary_msg.content) as usize;
        if summary_tokens <= remaining_budget || final_result.is_empty() {
            final_result.push(summary_msg);
        }
    }

    final_result.extend(result);
    final_result
}

/// Extract a brief summary snippet from a message.
fn extract_summary_snippet(msg: &ChatMessage) -> String {
    let role_label = match msg.role.as_str() {
        "user" => "User",
        "assistant" => "Assistant",
        "tool" => "Tool",
        _ => "Message",
    };

    // Take first 100 chars as preview
    let preview: String = msg.content.chars().take(100).collect();
    let preview = preview.trim();

    if preview.is_empty() {
        format!("[{}: (empty)]", role_label)
    } else {
        format!("[{}: {}...]", role_label, preview)
    }
}

/// Extract a brief conclusion from a tool result.
fn extract_tool_conclusion(msg: &ChatMessage) -> String {
    let tool_name = msg.name.as_deref().unwrap_or("unknown_tool");

    // Try to extract the last meaningful line as conclusion
    let lines: Vec<&str> = msg
        .content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();

    if lines.is_empty() {
        return String::new();
    }

    // Take last line as conclusion, truncated if needed
    let conclusion = lines.last().unwrap_or(&"");
    let conclusion: String = conclusion.chars().take(200).collect();

    format!("[Earlier {} result: {}...]", tool_name, conclusion.trim())
}

/// Build a conversation summary from collected snippets.
fn build_conversation_summary(snippets: &[String]) -> String {
    if snippets.is_empty() {
        return String::new();
    }

    let mut summary = String::from(
        "[Earlier conversation compressed to optimize context budget.\n\
         Key points from previous exchanges:",
    );

    for snippet in snippets.iter().take(5) {
        summary.push_str("\n  - ");
        summary.push_str(snippet);
    }

    if snippets.len() > 5 {
        summary.push_str(&format!(
            "\n  ... and {} more exchanges",
            snippets.len() - 5
        ));
    }

    summary.push(']');
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== Budget lookup tests =====

    #[test]
    fn test_gpt4o_budget() {
        assert_eq!(get_model_budget("gpt-4o"), 128_000);
        assert_eq!(get_model_budget("gpt-4o-2024-05-13"), 128_000);
        assert_eq!(get_model_budget("GPT-4O"), 128_000); // case insensitive
    }

    #[test]
    fn test_gpt4turbo_budget() {
        assert_eq!(get_model_budget("gpt-4-turbo"), 128_000);
        assert_eq!(get_model_budget("gpt-4-turbo-preview"), 128_000);
        assert_eq!(get_model_budget("gpt-4-0125-preview"), 128_000);
    }

    #[test]
    fn test_gpt35turbo_budget() {
        assert_eq!(get_model_budget("gpt-3.5-turbo"), 16_000);
        assert_eq!(get_model_budget("gpt-3.5-turbo-16k"), 16_000);
        assert_eq!(get_model_budget("gpt-35-turbo"), 16_000);
    }

    #[test]
    fn test_claude_35_sonnet_budget() {
        assert_eq!(get_model_budget("claude-3.5-sonnet"), 200_000);
        assert_eq!(get_model_budget("claude-3-5-sonnet-20241022"), 200_000);
        assert_eq!(get_model_budget("anthropic/claude-3.5-sonnet"), 200_000);
    }

    #[test]
    fn test_claude_35_opus_budget() {
        assert_eq!(get_model_budget("claude-3.5-opus"), 200_000);
        assert_eq!(get_model_budget("claude-3-5-opus-20241022"), 200_000);
    }

    #[test]
    fn test_claude_3_haiku_budget() {
        assert_eq!(get_model_budget("claude-3-haiku"), 200_000);
        assert_eq!(get_model_budget("claude-3-haiku-20240307"), 200_000);
    }

    #[test]
    fn test_llama_31_budget() {
        assert_eq!(get_model_budget("llama-3.1-70b"), 128_000);
        assert_eq!(get_model_budget("llama-3.1-405b"), 128_000);
        assert_eq!(get_model_budget("llama-3.1-8b"), 128_000);
        assert_eq!(get_model_budget("meta-llama/llama-3.1-70b"), 128_000);
    }

    #[test]
    fn test_mistral_large_budget() {
        assert_eq!(get_model_budget("mistral-large"), 128_000);
        assert_eq!(get_model_budget("mistral-large-2407"), 128_000);
        assert_eq!(get_model_budget("mistral-large-latest"), 128_000);
    }

    #[test]
    fn test_deepseek_budget() {
        assert_eq!(get_model_budget("deepseek-v2"), 128_000);
        assert_eq!(get_model_budget("deepseek-coder"), 128_000);
        assert_eq!(get_model_budget("deepseek-chat"), 128_000);
    }

    #[test]
    fn test_qwen_budget() {
        assert_eq!(get_model_budget("qwen-2.5"), 128_000);
        assert_eq!(get_model_budget("qwen2.5-72b"), 128_000);
        assert_eq!(get_model_budget("qwen-2-5-72b"), 128_000);
    }

    #[test]
    fn test_unknown_model_default_budget() {
        assert_eq!(get_model_budget("unknown-model"), 8_000);
        assert_eq!(get_model_budget("some-random-model-v1"), 8_000);
        assert_eq!(get_model_budget(""), 8_000);
    }

    // ===== fits_budget tests =====

    #[test]
    fn test_fits_budget_gpt4o() {
        // GPT-4o: 128k - 4096 reserved = 123_904 available
        assert!(fits_budget("gpt-4o", 100_000));
        assert!(fits_budget("gpt-4o", 123_904));
        assert!(!fits_budget("gpt-4o", 123_905));
        assert!(!fits_budget("gpt-4o", 130_000));
    }

    #[test]
    fn test_fits_budget_gpt35() {
        // GPT-3.5-turbo: 16k - 4096 reserved = 11_904 available
        assert!(fits_budget("gpt-3.5-turbo", 10_000));
        assert!(fits_budget("gpt-3.5-turbo", 11_904));
        assert!(!fits_budget("gpt-3.5-turbo", 11_905));
        assert!(!fits_budget("gpt-3.5-turbo", 15_000));
    }

    #[test]
    fn test_fits_budget_claude() {
        // Claude 3.5 Sonnet: 200k - 4096 reserved = 195_904 available
        assert!(fits_budget("claude-3.5-sonnet", 150_000));
        assert!(fits_budget("claude-3.5-sonnet", 195_904));
        assert!(!fits_budget("claude-3.5-sonnet", 195_905));
    }

    #[test]
    fn test_fits_budget_unknown() {
        // Unknown: 8k - 4096 reserved = 3_904 available
        assert!(fits_budget("unknown", 3_904));
        assert!(!fits_budget("unknown", 3_905));
        assert!(!fits_budget("unknown", 8_000));
    }

    #[test]
    fn test_fits_budget_zero_tokens() {
        assert!(fits_budget("gpt-4o", 0));
        assert!(fits_budget("unknown", 0));
    }

    // ===== Token estimation tests =====

    #[test]
    fn test_estimate_messages_tokens_empty() {
        let messages: Vec<ChatMessage> = vec![];
        assert_eq!(estimate_messages_tokens(&messages), 0);
    }

    #[test]
    fn test_estimate_messages_tokens_single() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "Hello, world!".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let tokens = estimate_messages_tokens(&messages);
        assert!(tokens >= 1);
    }

    #[test]
    fn test_estimate_messages_tokens_multiple() {
        let messages = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Hi there! How can I help you today?".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ];
        let tokens = estimate_messages_tokens(&messages);
        assert!(tokens >= 2);
    }

    // ===== Compression helper tests =====

    #[test]
    fn test_contains_file_paths() {
        assert!(contains_file_paths("Check src/main.rs for details"));
        assert!(contains_file_paths("See /path/to/file.py"));
        assert!(contains_file_paths("Open config.json"));
        assert!(contains_file_paths("Edit Cargo.toml"));
        assert!(!contains_file_paths("Hello world"));
        assert!(!contains_file_paths("No paths here"));
    }

    #[test]
    fn test_contains_code_blocks() {
        assert!(contains_code_blocks("```rust\nfn main() {}\n```"));
        assert!(contains_code_blocks("pub fn test() {}"));
        assert!(contains_code_blocks("struct Foo {"));
        assert!(contains_code_blocks("impl Bar for Baz {"));
        assert!(contains_code_blocks("let mut x = 5;"));
        assert!(!contains_code_blocks("Just plain text"));
    }

    #[test]
    fn test_extract_summary_snippet_user() {
        let msg = ChatMessage {
            role: "user".to_string(),
            content: "Can you help me fix this bug in the code?".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        };
        let snippet = extract_summary_snippet(&msg);
        assert!(snippet.contains("User"));
        assert!(snippet.contains("help me fix"));
    }

    #[test]
    fn test_extract_summary_snippet_empty() {
        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: "".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        };
        let snippet = extract_summary_snippet(&msg);
        assert!(snippet.contains("empty"));
    }

    #[test]
    fn test_extract_tool_conclusion() {
        let msg = ChatMessage {
            role: "tool".to_string(),
            content: "Line 1\nLine 2\nFinal result: success".to_string(),
            name: Some("read_file".to_string()),
            tool_call_id: None,
            tool_calls: None,
        };
        let conclusion = extract_tool_conclusion(&msg);
        assert!(conclusion.contains("read_file"));
        assert!(conclusion.contains("success"));
    }

    #[test]
    fn test_build_conversation_summary() {
        let snippets = vec![
            "[User: Hello...]".to_string(),
            "[Assistant: Hi...]".to_string(),
        ];
        let summary = build_conversation_summary(&snippets);
        assert!(summary.contains("compressed"));
        assert!(summary.contains("User: Hello"));
        assert!(summary.contains("Assistant: Hi"));
    }

    #[test]
    fn test_build_conversation_summary_truncation() {
        let snippets: Vec<String> = (0..10).map(|i| format!("[Message {}]", i)).collect();
        let summary = build_conversation_summary(&snippets);
        assert!(summary.contains("5 more exchanges"));
    }

    // ===== Integration tests =====

    #[test]
    fn test_compress_history_with_budget_small_context() {
        // Create messages that exceed a small budget
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are a helpful assistant.".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Hi there!".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ];

        // With unknown model (8k budget), small messages should fit
        let compressed = compress_history_with_budget(&messages, "unknown-model", false);
        assert!(!compressed.is_empty());
        // System message should be preserved
        assert!(compressed.iter().any(|m| m.role == "system"));
    }

    #[test]
    fn test_compress_history_with_budget_large_model() {
        let messages = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "What is Rust?".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Rust is a systems programming language.".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ];

        // With GPT-4o (128k budget), these messages easily fit
        let compressed = compress_history_with_budget(&messages, "gpt-4o", false);
        assert_eq!(compressed.len(), 2);
    }

    #[test]
    fn test_compress_history_preserves_system_message() {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "Important system instructions.".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Question".to_string(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ];

        let compressed = compress_history_with_budget(&messages, "gpt-3.5-turbo", false);
        assert!(compressed
            .iter()
            .any(|m| m.role == "system" && m.content.contains("Important system instructions")));
    }
}
