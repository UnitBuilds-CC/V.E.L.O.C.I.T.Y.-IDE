#![allow(dead_code)]
//! Browse Panel - AI-powered web research directly in the sidebar.
//!
//! Accepts either a plain-language question ("how much does an iPhone 17 cost?")
//! or a URL + question ("https://example.com — summarize key announcements").
//! Spawns a headless sub-agent with browser tools to fetch, read, and summarize
//! web content, streaming results back into the panel.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::agent::{AiProvider, HeadlessSubAgentProgress, HeadlessSubAgentRequest};

/// System prompt for the browse agent.
pub const BROWSE_SYSTEM_PROMPT: &str = r#"You are a Web Research assistant embedded in an IDE. Your job is to browse the web to find information the user asks about.

Instructions:
1. If the user provides a URL, navigate directly to that URL and extract the requested information.
2. If the user provides only a question, use web search to find relevant pages, then navigate to the most promising results to gather accurate, up-to-date information.
3. Compile your findings into a clear, concise summary. Include key facts, numbers, dates, and quotes where relevant.
4. Cite your sources with URLs so the user can verify.
5. If information is conflicting across sources, mention the discrepancy.
6. Keep the summary focused — the user wants actionable information, not filler.

You have browser tools available (navigate, read page content, search). Use them to gather real-time information."#;

/// A single message in the browse panel history.
#[derive(Debug, Clone)]
pub struct BrowseMessage {
    pub role: String,
    pub content: String,
    pub timestamp: Instant,
}

impl BrowseMessage {
    pub fn user(content: &str) -> Self {
        Self { role: "user".to_string(), content: content.to_string(), timestamp: Instant::now() }
    }
    pub fn assistant(content: &str) -> Self {
        Self { role: "assistant".to_string(), content: content.to_string(), timestamp: Instant::now() }
    }
    pub fn status(content: &str) -> Self {
        Self { role: "status".to_string(), content: content.to_string(), timestamp: Instant::now() }
    }
}

/// State for the Browse sidebar panel.
#[derive(Clone)]
pub struct BrowseState {
    /// The user's query/URL input field.
    pub input: String,
    /// Optional URL field (if user wants to target a specific page).
    pub url_input: String,
    /// Chat-style message history.
    pub messages: Vec<BrowseMessage>,
    /// Whether a browse agent is currently running.
    pub waiting: bool,
    /// Shared progress handle from the running headless sub-agent.
    pub progress: Option<Arc<Mutex<HeadlessSubAgentProgress>>>,
    /// Number of events already consumed from progress.
    pub events_consumed: usize,
    /// Length of transcript already rendered into the streaming message.
    pub transcript_rendered: usize,
}

impl Default for BrowseState {
    fn default() -> Self {
        Self {
            input: String::new(),
            url_input: String::new(),
            messages: vec![BrowseMessage {
                role: "assistant".to_string(),
                content: "Ask me anything and I'll browse the web to find the answer.\n\n\
                    Examples:\n\
                    • \"How much does an iPhone 17 cost?\"\n\
                    • \"What was announced at AMD's summit?\"\n\
                    • Paste a URL + question to research a specific page."
                    .to_string(),
                timestamp: Instant::now(),
            }],
            waiting: false,
            progress: None,
            events_consumed: 0,
            transcript_rendered: 0,
        }
    }
}

impl BrowseState {
    /// Submit a browse request. Spawns a headless sub-agent with browser tools.
    pub fn send(
        &mut self,
        workspace_root: &std::path::Path,
        provider: AiProvider,
        model: &str,
    ) {
        let query = self.input.trim().to_string();
        let url = self.url_input.trim().to_string();
        if query.is_empty() {
            return;
        }

        // Build the user prompt
        let prompt = if url.is_empty() {
            format!(
                "{}\n\nUser request: {}",
                BROWSE_SYSTEM_PROMPT, query
            )
        } else {
            format!(
                "{}\n\nUser request: Go to {} and answer: {}",
                BROWSE_SYSTEM_PROMPT, url, query
            )
        };

        let display_msg = if url.is_empty() {
            query.clone()
        } else {
            format!("{} — {}", url, query)
        };

        self.messages.push(BrowseMessage::user(&display_msg));
        self.input.clear();
        self.url_input.clear();
        self.waiting = true;
        self.events_consumed = 0;
        self.transcript_rendered = 0;

        let progress = Arc::new(Mutex::new(HeadlessSubAgentProgress::default()));
        self.progress = Some(progress.clone());

        let request = HeadlessSubAgentRequest {
            workspace_root: workspace_root.to_path_buf(),
            provider,
            model: model.to_string(),
            thinking: false,
            prompt,
            cancel_rx: None,
            progress: Some(progress),
            scoped_files: None,
        };

        std::thread::spawn(move || {
            crate::agent::run_headless_subagent(request);
        });
    }

    /// Poll the running sub-agent for progress. Returns `true` if finished.
    pub fn poll(&mut self) -> bool {
        let Some(progress) = &self.progress else {
            return false;
        };

        let Ok(guard) = progress.lock() else {
            return false;
        };

        let events = &guard.events;

        // Process new events
        for event in events.iter().skip(self.events_consumed) {
            match event.kind {
                crate::agent::HeadlessSubAgentEventKind::Status => {
                    self.messages.push(BrowseMessage::status(&event.message));
                }
                crate::agent::HeadlessSubAgentEventKind::ToolStarted => {
                    self.messages.push(BrowseMessage::status(
                        &format!("\u{1F310} {}", event.message),
                    ));
                }
                _ => {}
            }
        }
        self.events_consumed = events.len();

        // Stream transcript tokens into a live assistant message
        let transcript = &guard.transcript;
        if transcript.len() > self.transcript_rendered {
            let new_text = &transcript[self.transcript_rendered..];
            // Find or create the streaming assistant message
            let needs_new = self.messages.last()
                .map(|m| m.role != "streaming")
                .unwrap_or(true);
            if needs_new {
                self.messages.push(BrowseMessage {
                    role: "streaming".to_string(),
                    content: new_text.to_string(),
                    timestamp: Instant::now(),
                });
            } else if let Some(last) = self.messages.last_mut() {
                last.content.push_str(new_text);
            }
            self.transcript_rendered = transcript.len();
        }

        // Detect completion via Arc strong_count
        let finished = Arc::strong_count(progress) == 1;
        if finished && self.waiting {
            self.waiting = false;
            let transcript = guard.transcript.clone();
            drop(guard);
            // Replace streaming message with final assistant message
            self.messages.retain(|m| m.role != "streaming");
            if !transcript.trim().is_empty() {
                self.messages.push(BrowseMessage::assistant(&transcript));
            } else {
                self.messages.push(BrowseMessage::assistant(
                    "I wasn't able to find relevant information. Try rephrasing your question.",
                ));
            }
            self.progress = None;
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_welcome_message() {
        let state = BrowseState::default();
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].role, "assistant");
    }

    #[test]
    fn system_prompt_mentions_browser() {
        assert!(BROWSE_SYSTEM_PROMPT.contains("browse the web"));
        assert!(BROWSE_SYSTEM_PROMPT.contains("navigate"));
    }
}
