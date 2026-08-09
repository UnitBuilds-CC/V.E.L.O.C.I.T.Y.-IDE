#![allow(dead_code)]
//! Team Builder Chat - A focused conversational interface for creating expert
//! teams via natural language. Uses a headless sub-agent with team-specific
//! tools (create_expert_team, create_skill_file) to build teams from descriptions.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::agent::{AiProvider, HeadlessSubAgentProgress, HeadlessSubAgentRequest};

/// System prompt injected into the headless sub-agent that powers team creation.
pub const TEAM_BUILDER_SYSTEM_PROMPT: &str = r#"You are the Team Builder assistant for Velocity IDE. Your ONLY job is to create expert teams and skill files using the available tools.

When the user describes a team they want:
1. First call list_expert_teams to check for existing teams that might conflict
2. For each specialized member, determine the best provider/model based on the task type
3. Create appropriate skill files using create_skill_file for any domain-specific knowledge
4. Create the team using create_expert_team with all members configured

Guidelines for team design:
- Each member should have a clear, non-overlapping specialty
- Assign scope_patterns that match the file paths each member owns
- Write workflow_instructions that give the member focused operating rules
- Use tool allow-lists to restrict members to relevant tools only
- The first member is the team lead (gets fallback tasks)
- Use the most capable model for lead/architect roles, efficient models for routine work
- Available providers: cloudflare (@cf/moonshotai/kimi-k2.7-code), openrouter (anthropic/claude-3.5-sonnet, deepseek/deepseek-coder, meta-llama/llama-3.3-70b-instruct), azure (gpt-4o), ollama (llama3.2), openai, anthropic, vertex

After creating, summarize what was built and how to use it (e.g. "@team-slug task" or "send it to the <name> team")."#;

/// A single message in the team builder chat history.
#[derive(Debug, Clone)]
pub struct TeamBuilderMessage {
    pub role: String,
    pub content: String,
    pub timestamp: Instant,
}

impl TeamBuilderMessage {
    pub fn user(content: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Instant::now(),
        }
    }

    pub fn assistant(content: &str) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.to_string(),
            timestamp: Instant::now(),
        }
    }

    pub fn status(content: &str) -> Self {
        Self {
            role: "status".to_string(),
            content: content.to_string(),
            timestamp: Instant::now(),
        }
    }
}

/// Chat state for the team builder sub-chat integrated into Team Studio.
#[derive(Clone)]
pub struct TeamBuilderChat {
    pub input: String,
    pub messages: Vec<TeamBuilderMessage>,
    pub waiting: bool,
    /// Shared progress handle from the running headless sub-agent.
    pub progress: Option<Arc<Mutex<HeadlessSubAgentProgress>>>,
    /// Number of events already consumed from progress (for incremental polling).
    pub events_consumed: usize,
    /// Length of transcript already rendered into the streaming message.
    pub transcript_rendered: usize,
}

impl Default for TeamBuilderChat {
    fn default() -> Self {
        Self {
            input: String::new(),
            messages: vec![TeamBuilderMessage {
                role: "assistant".to_string(),
                content: "Describe the team you want to create. For example:\n\n\
                    \"I need a cloud infrastructure team with members for security, \
                    networking, containers, backend APIs, and monitoring.\"\n\n\
                    I'll create the team with appropriate skills, models, and tool assignments."
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

impl TeamBuilderChat {
    /// Submit the current input as a user message and spawn a headless sub-agent.
    pub fn send(&mut self, workspace_root: &std::path::Path, provider: AiProvider, model: &str) {
        let user_input = self.input.trim().to_string();
        if user_input.is_empty() {
            return;
        }
        self.messages.push(TeamBuilderMessage::user(&user_input));
        self.input.clear();
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
            prompt: format!(
                "{}\n\nUser request: {}",
                TEAM_BUILDER_SYSTEM_PROMPT, user_input
            ),
            cancel_rx: None,
            progress: Some(progress),
            scoped_files: None,
        };

        // Spawn the headless sub-agent on a background thread.
        std::thread::spawn(move || {
            crate::agent::run_headless_subagent(request);
        });
    }

    /// Poll the running sub-agent for new events and update the chat messages.
    /// Returns `true` if the team list should be reloaded (agent finished).
    pub fn poll(&mut self) -> bool {
        let Some(progress) = &self.progress else {
            return false;
        };

        let Ok(guard) = progress.lock() else {
            return false;
        };

        let events = &guard.events;
        let mut should_reload = false;

        // Process new events since last poll
        for event in events.iter().skip(self.events_consumed) {
            match event.kind {
                crate::agent::HeadlessSubAgentEventKind::Status => {
                    self.messages
                        .push(TeamBuilderMessage::status(&event.message));
                }
                crate::agent::HeadlessSubAgentEventKind::ToolStarted => {
                    self.messages.push(TeamBuilderMessage::status(&format!(
                        "Running tool: {}",
                        event.message
                    )));
                }
                crate::agent::HeadlessSubAgentEventKind::ToolFinished => {
                    should_reload = true;
                }
                _ => {}
            }
        }
        self.events_consumed = events.len();

        // Stream transcript tokens into a live message
        let transcript = &guard.transcript;
        if transcript.len() > self.transcript_rendered {
            let new_text = &transcript[self.transcript_rendered..];
            let needs_new = self
                .messages
                .last()
                .map(|m| m.role != "streaming")
                .unwrap_or(true);
            if needs_new {
                self.messages.push(TeamBuilderMessage {
                    role: "streaming".to_string(),
                    content: new_text.to_string(),
                    timestamp: Instant::now(),
                });
            } else if let Some(last) = self.messages.last_mut() {
                last.content.push_str(new_text);
            }
            self.transcript_rendered = transcript.len();
        }

        // Check if finished by seeing if transcript is non-empty and no new events are coming
        // We detect completion by checking Arc strong_count (if only we hold it, agent is done)
        let finished = Arc::strong_count(progress) == 1;
        if finished && self.waiting {
            self.waiting = false;
            let transcript = guard.transcript.clone();
            drop(guard);
            // Replace streaming message with final assistant message
            self.messages.retain(|m| m.role != "streaming");
            if !transcript.trim().is_empty() {
                self.messages
                    .push(TeamBuilderMessage::assistant(&transcript));
            }
            self.progress = None;
            return true;
        }

        should_reload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_welcome_message() {
        let chat = TeamBuilderChat::default();
        assert_eq!(chat.messages.len(), 1);
        assert_eq!(chat.messages[0].role, "assistant");
    }

    #[test]
    fn system_prompt_mentions_tools() {
        assert!(TEAM_BUILDER_SYSTEM_PROMPT.contains("create_expert_team"));
        assert!(TEAM_BUILDER_SYSTEM_PROMPT.contains("create_skill_file"));
    }
}
