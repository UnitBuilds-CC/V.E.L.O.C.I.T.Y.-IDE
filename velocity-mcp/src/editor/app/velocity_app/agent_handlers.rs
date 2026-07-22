use std::path::PathBuf;
use std::collections::HashSet;

use crate::agent::{AgentToUiMessage, UiToAgentMessage};
use crate::automation::{read_latest_diagnostics, WorkspaceCoordinator, AgentTaskKind};
use crate::editor::agent_ui_state::AgentUiState;
use super::super::helpers::*;
use super::super::types::*;
use super::struct_def::VelocityApp;

impl VelocityApp {
    pub fn mirror_worker_events_into_timeline(
        &mut self,
        snapshot: &crate::editor::orchestrator_panel::OrchestratorDashboardSnapshot,
    ) {
        let before = self.task_timeline.event_count();
        let task_timeline = &mut self.task_timeline;
        let mission_control = &mut self.mission_control;
        for task in &snapshot.tasks {
            let Some(thread) = &task.live_thread else {
                continue;
            };
            let mirrored_count = mission_control.mirrored_worker_event_count(task.id);
            if thread.events.len() <= mirrored_count {
                continue;
            }
            let task_id = match u32::try_from(task.id) {
                Ok(task_id) => task_id,
                Err(_) => continue,
            };
            for event in thread.events.iter().skip(mirrored_count) {
                match event.kind {
                    crate::orchestrator::worker::WorkerThreadEventKind::Status => {
                        task_timeline.agent_marker("Worker status", &event.message, task_id);
                    }
                    crate::orchestrator::worker::WorkerThreadEventKind::Transcript => {
                        task_timeline.agent_marker("Worker output", &event.message, task_id);
                    }
                    crate::orchestrator::worker::WorkerThreadEventKind::FileChange => {
                        task_timeline.agent_marker("File change", &event.message, task_id);
                    }
                    crate::orchestrator::worker::WorkerThreadEventKind::OperatorNote => {
                        task_timeline.agent_marker("Operator note", &event.message, task_id);
                    }
                    crate::orchestrator::worker::WorkerThreadEventKind::ToolApproval => {
                        task_timeline.tool_call(task_id, "tool approval", &event.message);
                    }
                    crate::orchestrator::worker::WorkerThreadEventKind::ToolStarted => {
                        task_timeline.tool_call(task_id, "tool started", &event.message);
                    }
                    crate::orchestrator::worker::WorkerThreadEventKind::ToolFinished => {
                        task_timeline.tool_result(task_id, "tool finished", true, 0);
                        task_timeline.agent_marker("Tool finished", &event.message, task_id);
                    }
                }
            }
            mission_control.set_mirrored_worker_event_count(task.id, thread.events.len());
        }
        if self.task_timeline.event_count() != before {
            self.persist_mission_activity();
        }
    }

    pub fn refresh_models(&mut self) {
        self.models_loading = true;
        self.chat.models_loading = true;
        self.status_message = "Refreshing model catalog...".into();
        let _ = self.agent_tx.send(UiToAgentMessage::RefreshModels);
    }

    pub fn sync_approval_state_from_pending(&mut self) {
        self.agent_ui_state.approvals = AgentUiState::default().approvals;
        for (id, tool_name, _) in &self.pending_approvals {
            let tool_id = id.parse::<u32>().unwrap_or(0);
            let _ = self
                .agent_ui_state
                .approvals
                .add_approval(tool_id, tool_name, false);
        }
    }

    pub fn approve_pending_tool_at(&mut self, idx: usize) {
        if idx >= self.pending_approvals.len() {
            self.status_message = "No pending tool approval at that index".into();
            return;
        }

        let (id, tool_name, arguments) = self.pending_approvals[idx].clone();
        let _ = self.agent_tx.send(UiToAgentMessage::ApproveTool {
            id: id.clone(),
            arguments,
        });
        self.pending_approvals.remove(idx);
        self.chat
            .pending_approvals
            .retain(|(pending_id, _, _)| pending_id != &id);
        self.sync_approval_state_from_pending();
        self.status_message = format!("Approved tool: {}", tool_name);
        self.toasts
            .push(crate::editor::toast::Toast::success(format!(
                "Approved {}",
                tool_name
            )));
    }

    pub fn reject_pending_tool_at(&mut self, idx: usize) {
        if idx >= self.pending_approvals.len() {
            self.status_message = "No pending tool approval at that index".into();
            return;
        }

        let (id, tool_name, _) = self.pending_approvals[idx].clone();
        let _ = self.agent_tx.send(UiToAgentMessage::RejectTool {
            id: id.clone(),
        });
        self.pending_approvals.remove(idx);
        self.chat
            .pending_approvals
            .retain(|(pending_id, _, _)| pending_id != &id);
        self.sync_approval_state_from_pending();
        self.status_message = format!("Declined tool: {}", tool_name);
        self.toasts.push(crate::editor::toast::Toast::warn(format!(
            "Declined {}",
            tool_name
        )));
    }

    pub fn approve_all_pending_tools(&mut self) {
        let pending_len = self.pending_approvals.len();
        if pending_len == 0 {
            self.status_message = "No pending tool approvals".into();
            return;
        }

        while !self.pending_approvals.is_empty() {
            self.approve_pending_tool_at(0);
        }
        self.status_message = format!("Approved {} pending tool(s)", pending_len);
        self.toasts
            .push(crate::editor::toast::Toast::success(format!(
                "Approved {} tool request(s)",
                pending_len
            )));
    }

    pub fn reject_all_pending_tools(&mut self) {
        let pending_len = self.pending_approvals.len();
        if pending_len == 0 {
            self.status_message = "No pending tool approvals".into();
            return;
        }

        while !self.pending_approvals.is_empty() {
            self.reject_pending_tool_at(0);
        }
        self.status_message = format!("Declined {} pending tool(s)", pending_len);
        self.toasts.push(crate::editor::toast::Toast::warn(format!(
            "Declined {} tool request(s)",
            pending_len
        )));
    }

    pub fn ask_agent_about_active_diff(&mut self) {
        let Some(change_preview) = self.active_change_preview() else {
            self.status_message = "No active diff to send".into();
            return;
        };
        let prompt = format!(
            "Review the following active diff and suggest the best next coding steps.\n\nFile: {}\nAdded lines: {}\nRemoved lines: {}\n\nDiff:\n{}",
            change_preview.file_label,
            change_preview.added_lines,
            change_preview.removed_lines,
            change_preview.full_diff,
        );
        self.chat.push_user(prompt.clone());
        self.chat_history.push_str("\nYou: ");
        self.chat_history.push_str(&prompt);
        self.agent_active = true;
        self.chat.agent_active = true;
        self.status_message = "Sent active diff to agent".into();
        self.toggle_chat();
        let _ = self.agent_tx.send(UiToAgentMessage::UserPrompt(prompt));
    }

    pub fn apply_mission_brief_preset(&mut self, brief: &str, task_kind: AgentTaskKind) {
        self.mission_control.brief = brief.to_string();
        self.mission_control.set_selected_task(None);
        self.orchestrator.set_selected_policy_kind(task_kind);
    }

    pub fn plan_routed_subagents(&mut self) {
        self.mission_control.set_selected_task(None);
        self.mission_control.clear_worker_event_tracking();
        let Some(goal) = self.current_routing_goal() else {
            self.status_message = "Enter a chat prompt or keep a recent user goal to route".into();
            self.toasts.push(crate::editor::toast::Toast::warn(
                "No goal available for routed planning",
            ));
            return;
        };
        let inferred_task_kind = infer_task_kind_from_goal(&goal);
        self.orchestrator
            .set_selected_policy_kind(inferred_task_kind);
        let task_kind = self.orchestrator.selected_policy_kind();
        let scope_files = self.collect_routing_scope_files(&goal);
        if scope_files.is_empty() {
            self.status_message = "No scoped files available for routed planning".into();
            self.toasts.push(crate::editor::toast::Toast::warn(
                "No files found for routed planning",
            ));
            return;
        }

        let site_map = match crate::automation::open_workspace_site_map(&self.workspace_root) {
            Ok(site_map) => site_map,
            Err(err) => {
                self.status_message = format!("SiteMap unavailable: {err}");
                self.toasts.push(crate::editor::toast::Toast::error(format!(
                    "SiteMap unavailable: {err}"
                )));
                return;
            }
        };

        let model_catalogs = vec![(self.provider, self.available_models.clone())];
        let coordinator = WorkspaceCoordinator::new(self.mediator.clone());
        let routed_tasks = coordinator.plan_routed_tasks(
            &self.workspace_root,
            &goal,
            task_kind,
            &scope_files,
            &site_map,
            &model_catalogs,
        );

        if routed_tasks.is_empty() {
            self.status_message = "Routing produced no sub-agent tasks".into();
            self.toasts.push(crate::editor::toast::Toast::warn(
                "Routing produced no sub-agent tasks",
            ));
            return;
        }

        let routed_count = routed_tasks.len();
        let scope_count = scope_files.len();
        self.mission_control.brief = goal.clone();
        self.mission_control.set_selected_task(None);
        self.orchestrator.set_routed_tasks(
            goal.clone(),
            task_kind,
            scope_count,
            routed_tasks.clone(),
        );
        self.task_timeline.session_marker(
            "Sub-agent route planned",
            &format!("{} tasks for {}", routed_count, task_kind.as_str()),
        );
        self.command_output.push_str(&format!(
            "[routed-plan] goal={goal}\n[routed-plan] kind={} scope_files={} tasks={}\n",
            task_kind.as_str(),
            scope_count,
            routed_count,
        ));
        for task in &routed_tasks {
            self.command_output.push_str(&format!(
                "  - {} :: {} / {} :: {} file(s)\n",
                task.task_id,
                task.provider.label(),
                task.model_label,
                task.files.len(),
            ));
        }
        self.status_message = format!("Planned {} routed sub-agent task(s)", routed_count);
        self.toasts
            .push(crate::editor::toast::Toast::success(format!(
                "Planned {} routed sub-agent task(s)",
                routed_count,
            )));
        if self.mission_control.auto_execute {
            self.orchestrator
                .execute_routed_tasks(&self.workspace_root, &self.mediator);
            self.status_message = format!(
                "Planned and launched {} routed sub-agent task(s)",
                routed_count
            );
            self.toasts.push(crate::editor::toast::Toast::info(
                "Mission Control auto-launched routed tasks",
            ));
        }
        self.toggle_mission_control();
    }

    pub fn current_routing_goal(&self) -> Option<String> {
        let draft = self.chat.input.trim();
        if !draft.is_empty() {
            return Some(draft.to_string());
        }
        self.chat
            .messages
            .iter()
            .rev()
            .find(|message| message.role == crate::editor::chat_panel::ChatRole::User)
            .map(|message| message.content.trim().to_string())
            .filter(|message| !message.is_empty())
    }

    pub fn collect_routing_scope_files(&self, goal: &str) -> Vec<PathBuf> {
        let use_workspace = wants_workspace_scope(goal);
        let mut files = if use_workspace {
            collect_workspace_routing_files(&self.workspace_root, 96)
        } else {
            self.collect_open_editor_paths()
        };
        if files.is_empty() {
            files = collect_workspace_routing_files(&self.workspace_root, 96);
        }
        files
    }

    pub fn collect_open_editor_paths(&self) -> Vec<PathBuf> {
        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for tab in &self.tabs {
            if let Some(path) = tab.editor_path() {
                if seen.insert(path.clone()) {
                    files.push(path.clone());
                }
            }
        }
        files
    }

    #[allow(dead_code)]
    pub fn focus_orchestrator_tab(&mut self) {
        if let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| matches!(tab.kind, TabKind::Orchestrator))
            .cloned()
        {
            self.active_tab = Some(tab.id);
        } else {
            self.toggle_orchestrator();
        }
    }

    pub fn update_diagnostics(&mut self) {
        let diag = read_latest_diagnostics(&self.workspace_root);
        let count = if diag.success { 0 } else { diag.errors.len() };
        if count != self.build_errors_count {
            if count == 0 {
                self.toasts
                    .push(crate::editor::toast::Toast::success("Build succeeded!"));
            } else {
                self.toasts.push(crate::editor::toast::Toast::error(format!(
                    "Build failed with {} errors",
                    count
                )));
            }
            self.build_errors_count = count;
        }
    }

    pub fn handle_terminal_messages(&mut self) {
        if let Some(rx) = &self.terminal_rx {
            while let Ok(out) = rx.try_recv() {
                self.command_output.push_str(&out);
            }
        }
    }

    pub fn handle_agent_messages(&mut self) {
        self.handle_terminal_messages();
        while let Ok(msg) = self.agent_rx.try_recv() {
            let mut timeline_dirty = false;
            match msg {
                AgentToUiMessage::OutputToken(token) => {
                    let mut last_you_idx = None;
                    let mut last_agent_idx = None;
                    for (idx, line) in self.chat_history.lines().enumerate() {
                        if line.starts_with("You: ") {
                            last_you_idx = Some(idx);
                        } else if line.starts_with("Agent: ")
                            || line.starts_with("Antigravity: ")
                            || line.starts_with("Kimi: ")
                        {
                            last_agent_idx = Some(idx);
                        }
                    }

                    let needs_prefix = match (last_you_idx, last_agent_idx) {
                        (Some(y), Some(a)) => y > a,
                        (Some(_), None) => true,
                        _ => self.chat_history.is_empty() || self.agent_active,
                    };

                    if needs_prefix {
                        self.chat_history.push_str("\nAgent: ");
                        self.agent_active = false;
                    }
                    self.chat_history.push_str(&token);
                    self.status_message = token.chars().take(80).collect();
                    self.chat.append_agent_token(&token);
                }
                AgentToUiMessage::ThoughtToken(token) => {
                    if self.current_agent_task_id == 0 {
                        self.current_agent_task_id =
                            self.task_timeline
                                .task_started("Agent response", "reasoning", 0);
                        self.task_timeline.agent_marker(
                            "Agent session start",
                            "reasoning stream opened",
                            self.current_agent_task_id,
                        );
                        timeline_dirty = true;
                    }
                    let _ = self.agent_ui_state.thinking.append_token(&token);
                    self.chat.append_thought_token(&token);
                }
                AgentToUiMessage::RequestToolApproval {
                    id,
                    tool_name,
                    arguments,
                } => {
                    let tool_id = id.parse::<u32>().unwrap_or(0);
                    let _ = self
                        .agent_ui_state
                        .approvals
                        .add_approval(tool_id, &tool_name, false);
                    if self.current_agent_task_id == 0 {
                        self.current_agent_task_id = self.task_timeline.task_started(
                            "Tool approval",
                            "awaiting approval",
                            0,
                        );
                    }
                    self.task_timeline.agent_marker(
                        "Approval requested",
                        &tool_name,
                        self.current_agent_task_id,
                    );
                    self.task_timeline.tool_call(
                        self.current_agent_task_id,
                        &tool_name,
                        "approval required",
                    );
                    timeline_dirty = true;

                    self.command_output.push_str(&format!(
                        "[tool-approval-request] {}: {:?}\n",
                        tool_name, arguments
                    ));
                    let should_auto = self.chat.auto_approve || self.auto_approve;
                    if should_auto {
                        let _ = self.agent_tx.send(UiToAgentMessage::ApproveTool {
                            id,
                            arguments,
                        });
                    } else {
                        self.pending_approvals.push((
                            id.clone(),
                            tool_name.clone(),
                            arguments.clone(),
                        ));
                        self.chat
                            .pending_approvals
                            .push((id, tool_name.clone(), arguments));
                        self.sync_approval_state_from_pending();
                        self.toasts.push(crate::editor::toast::Toast::warn(format!(
                            "Approval needed: {}",
                            tool_name
                        )));
                    }
                }
                AgentToUiMessage::ToolExecutionStarted { tool_name } => {
                    self.agent_ui_state.metrics.state =
                        crate::editor::agent_ui_state::AgentState::Running;
                    self.agent_ui_state.metrics.tool_call_count += 1;
                    if self.current_agent_task_id == 0 {
                        self.current_agent_task_id =
                            self.task_timeline
                                .task_started("Tool execution", "agent tool run", 0);
                    }
                    self.task_timeline.agent_marker(
                        "Tool phase",
                        &tool_name,
                        self.current_agent_task_id,
                    );
                    self.task_timeline
                        .tool_call(self.current_agent_task_id, &tool_name, "started");
                    timeline_dirty = true;

                    self.command_output
                        .push_str(&format!("[tool-start] {}\n", tool_name));
                    self.status_message = format!("Running tool: {}", tool_name);
                    self.toasts.push(crate::editor::toast::Toast::info(format!(
                        "Running tool: {}",
                        tool_name
                    )));
                }
                AgentToUiMessage::ToolExecutionFinished { tool_name, result } => {
                    self.agent_ui_state.metrics.state =
                        crate::editor::agent_ui_state::AgentState::Running;
                    if self.current_agent_task_id != 0 {
                        self.task_timeline.tool_result(
                            self.current_agent_task_id,
                            &tool_name,
                            true,
                            0,
                        );
                        timeline_dirty = true;
                    }

                    self.command_output
                        .push_str(&format!("[tool-finish] {}: {}\n", tool_name, result));
                    self.status_message = format!("Tool done: {}", tool_name);
                    self.toasts
                        .push(crate::editor::toast::Toast::success(format!(
                            "Finished tool: {}",
                            tool_name
                        )));
                    self.chat.agent_active = true;
                }
                AgentToUiMessage::StatusUpdate(message) => {
                    if message.to_lowercase().contains("model catalog") {
                        self.models_loading = false;
                        self.chat.models_loading = false;
                        self.task_timeline
                            .session_marker("Model catalog refreshed", &message);
                    } else {
                        self.task_timeline.agent_marker(
                            "Status",
                            &message,
                            self.current_agent_task_id,
                        );
                    }
                    timeline_dirty = true;
                    if self.current_agent_task_id == 0 {
                        self.current_agent_task_id =
                            self.task_timeline
                                .task_started("Status update", &message, 0);
                    }
                    self.status_message = message;
                }
                AgentToUiMessage::AgentFinished => {
                    self.agent_ui_state.metrics.state =
                        crate::editor::agent_ui_state::AgentState::Idle;
                    if self.current_agent_task_id != 0 {
                        self.task_timeline.agent_marker(
                            "Agent session end",
                            "response completed",
                            self.current_agent_task_id,
                        );
                        self.task_timeline
                            .task_completed(self.current_agent_task_id, 0, 0, 0);
                        self.current_agent_task_id = 0;
                        timeline_dirty = true;
                    }

                    self.status_message = "Agent finished".into();
                    self.agent_active = false;
                    self.chat.agent_active = false;
                }
                AgentToUiMessage::UpdateFileBuffer { path, content } => {
                    self.open_editor(Some(path.clone()));
                    if let Some(id) = &self.active_tab {
                        if let Some(buf) = self.buffers.get_mut(id) {
                            buf.load_text(&content);
                        }
                    }
                }
                AgentToUiMessage::ModelCatalog {
                    models,
                    selected,
                    thinking,
                } => {
                    if let Some(model) = models.iter().find(|model| model.id == selected) {
                        self.thinking_supported = model.supports_thinking;
                        self.tools_supported = model.supports_tools;
                    }
                    self.task_timeline
                        .session_marker("Model selected", &selected);
                    timeline_dirty = true;
                    self.agent_ui_state.metrics.thinking_enabled = thinking;

                    self.available_models = models.clone();
                    self.selected_model = selected.clone();
                    self.thinking_enabled = thinking;
                    self.models_loading = false;
                    self.chat.available_models = models;
                    self.chat.selected_model = selected;
                    self.chat.thinking_enabled = thinking;
                    self.chat.thinking_supported = self.thinking_supported;
                    self.chat.tools_supported = self.tools_supported;
                    self.chat.models_loading = false;
                }
                AgentToUiMessage::ProviderChanged(new_provider) => {
                    let provider_name = new_provider.label();
                    self.task_timeline
                        .session_marker("Provider changed", provider_name);
                    timeline_dirty = true;
                    self.provider = new_provider;
                    self.chat.provider = new_provider;
                }
                AgentToUiMessage::AccountUsage { accounts, date } => {
                    self.account_usage = accounts;
                    self.usage_date = date;
                }
                AgentToUiMessage::ChatHistoryRestored(history) => {
                    for (role, content) in &history {
                        if content.trim().is_empty() {
                            continue;
                        }
                        let prefix = if role == "user" { "You: " } else { "Agent: " };
                        self.chat_history
                            .push_str(&format!("\n{}{}\n", prefix, content));
                    }
                    self.chat.restore_history(history);
                }
            }
            if timeline_dirty {
                self.persist_mission_activity();
            }
        }
        self.cap_logs();
    }

    pub fn cap_logs(&mut self) {
        const MAX: usize = 32_000;
        if self.command_output.len() > MAX {
            let mut cut = self.command_output.len() - MAX;
            while cut < self.command_output.len() && !self.command_output.is_char_boundary(cut) {
                cut += 1;
            }
            self.command_output = self.command_output.split_off(cut);
        }
        if self.chat_history.len() > MAX {
            let mut cut = self.chat_history.len() - MAX;
            while cut < self.chat_history.len() && !self.chat_history.is_char_boundary(cut) {
                cut += 1;
            }
            self.chat_history = self.chat_history.split_off(cut);
        }
    }
}
