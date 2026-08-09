//! Workflows composer and governance policy panels, extracted from
//! `tier3_panels.rs` to keep individual modules under the LOC target.

use eframe::egui;
use egui::RichText;

use super::struct_def::VelocityApp;

impl VelocityApp {
    pub fn render_workflows_panel(&mut self, ui: &mut egui::Ui) {
        use crate::editor::workflow::{RunStatus, StepOutcome, Workflow, WorkflowStep};
        let palette = self.palette();
        Self::tier3_header(
            ui,
            "Workflows",
            &format!("{} workflow(s) · visual builder", self.workflows.len()),
            palette.accent,
            palette.text_muted,
        );

        // Mode tabs: List | Visual | Templates | AI Generate
        let mut mode: &str = if self.workflow_visual_mode {
            "Visual"
        } else {
            "List"
        };
        ui.horizontal(|ui| {
            let list_btn =
                ui.selectable_label(!self.workflow_visual_mode, RichText::new("List").size(9.0));
            let visual_btn =
                ui.selectable_label(self.workflow_visual_mode, RichText::new("Visual").size(9.0));
            let templates_btn = ui.selectable_label(false, RichText::new("Templates").size(9.0));
            let ai_btn = ui.selectable_label(false, RichText::new("AI Generate").size(9.0));
            if list_btn.clicked() {
                self.workflow_visual_mode = false;
                mode = "List";
            }
            if visual_btn.clicked() {
                self.workflow_visual_mode = true;
                mode = "Visual";
            }
            if templates_btn.clicked() { /* render templates inline below */ }
            if ai_btn.clicked() { /* render AI generate inline below */ }
        });
        ui.add_space(4.0);

        if self.workflow_visual_mode {
            self.render_workflow_visual(ui);
            return;
        }

        // ── List mode (original) ──
        Self::tier3_header(
            ui,
            "List Composer",
            &format!("{} workflow(s)", self.workflows.len()),
            palette.accent,
            palette.text_muted,
        );

        // Create a new workflow.
        let mut create = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.workflow_name_input)
                    .hint_text("new workflow name…")
                    .desired_width(ui.available_width() - 80.0),
            );
            if ui.button(RichText::new("Create").size(10.0)).clicked() {
                create = true;
            }
        });
        ui.add_space(6.0);

        // Workflow list.
        let mut select: Option<String> = None;
        let mut run: Option<String> = None;
        let mut remove: Option<String> = None;
        let selected = self.workflow_selected.clone();
        egui::ScrollArea::vertical()
            .id_salt("workflow_list_scroll")
            .max_height(130.0)
            .show(ui, |ui| {
                if self.workflows.is_empty() {
                    ui.label(
                        RichText::new("No workflows yet. Create one above.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
                for wf in &self.workflows.workflows {
                    let is_sel = selected.as_deref() == Some(wf.id.as_str());
                    egui::Frame::new()
                        .fill(if is_sel {
                            palette.bg_tertiary
                        } else {
                            palette.bg_secondary
                        })
                        .corner_radius(5.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&wf.name)
                                        .size(10.0)
                                        .strong()
                                        .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!("({} step(s))", wf.steps.len()))
                                        .size(8.0)
                                        .color(palette.text_muted),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button(RichText::new("✖").size(8.0)).clicked()
                                        {
                                            remove = Some(wf.id.clone());
                                        }
                                        if ui.small_button(RichText::new("Run").size(8.0)).clicked()
                                        {
                                            run = Some(wf.id.clone());
                                        }
                                        if ui
                                            .small_button(RichText::new("Edit").size(8.0))
                                            .clicked()
                                        {
                                            select = Some(wf.id.clone());
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(3.0);
                }
            });
        ui.add_space(6.0);

        // Step editor for the selected workflow (clone steps to avoid holding a
        // borrow of self.workflows while rendering the input rows).
        let mut add_tool = false;
        let mut add_agent = false;
        let mut move_up: Option<usize> = None;
        let mut move_down: Option<usize> = None;
        let mut remove_step: Option<usize> = None;
        if let Some(sel_id) = self.workflow_selected.clone() {
            let snapshot = self
                .workflows
                .get(&sel_id)
                .map(|w| (w.name.clone(), w.steps.clone()));
            match snapshot {
                None => self.workflow_selected = None,
                Some((wf_name, steps)) => {
                    ui.label(
                        RichText::new(format!("STEPS · {wf_name}"))
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    let step_count = steps.len();
                    for (i, step) in steps.iter().enumerate() {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{}. {}", i + 1, step.kind_label()))
                                            .size(9.0)
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .small_button(RichText::new("✖").size(8.0))
                                                .clicked()
                                            {
                                                remove_step = Some(i);
                                            }
                                            if i + 1 < step_count
                                                && ui
                                                    .small_button(RichText::new("↓").size(8.0))
                                                    .clicked()
                                            {
                                                move_down = Some(i);
                                            }
                                            if i > 0
                                                && ui
                                                    .small_button(RichText::new("↑").size(8.0))
                                                    .clicked()
                                            {
                                                move_up = Some(i);
                                            }
                                        },
                                    );
                                });
                                let detail = step_detail(step);
                                if !detail.is_empty() {
                                    ui.label(
                                        RichText::new(detail).size(8.0).color(palette.text_muted),
                                    );
                                }
                            });
                        ui.add_space(2.0);
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.workflow_step_tool_input)
                                .hint_text("tool name")
                                .desired_width(110.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.workflow_step_args_input)
                                .hint_text("{\"json\":\"args\"}")
                                .desired_width(ui.available_width() - 70.0),
                        );
                        if ui.button(RichText::new("+tool").size(9.0)).clicked() {
                            add_tool = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.workflow_step_prompt_input)
                                .hint_text("agent prompt")
                                .desired_width(ui.available_width() - 70.0),
                        );
                        if ui.button(RichText::new("+agent").size(9.0)).clicked() {
                            add_agent = true;
                        }
                    });
                }
            }
        }

        // Run log.
        if let Some(runrec) = &self.workflow_last_run {
            ui.add_space(6.0);
            let status_color = match runrec.status {
                RunStatus::Success => palette.success,
                RunStatus::Failed => palette.error,
                RunStatus::Partial => palette.warning,
            };
            ui.label(
                RichText::new(format!("LAST RUN · {}", runrec.status.label()))
                    .small()
                    .strong()
                    .color(status_color),
            );
            egui::ScrollArea::vertical()
                .id_salt("workflow_run_log")
                .max_height(140.0)
                .show(ui, |ui| {
                    for rec in &runrec.steps {
                        let color = match rec.outcome {
                            StepOutcome::Ok => palette.success,
                            StepOutcome::Failed => palette.error,
                            StepOutcome::Skipped => palette.text_muted,
                        };
                        ui.label(
                            RichText::new(format!(
                                "{}. {} [{}]",
                                rec.index + 1,
                                rec.kind,
                                rec.outcome.label()
                            ))
                            .size(9.0)
                            .color(color),
                        );
                        let snippet: String = rec.output.chars().take(120).collect();
                        if !snippet.is_empty() {
                            ui.label(RichText::new(snippet).size(8.0).color(palette.text_muted));
                        }
                    }
                });
        }

        // Deferred mutations.
        let ws = self.workspace_root.clone();
        if create {
            let name = self.workflow_name_input.trim().to_string();
            if name.is_empty() {
                self.toasts
                    .push(crate::editor::toast::Toast::error("Workflow needs a name"));
            } else {
                let id = format!("wf-{}", crate::editor::triggers::now_secs());
                self.workflows.add(Workflow::new(id.clone(), name));
                let _ = self.workflows.save(&ws);
                self.workflow_selected = Some(id);
                self.workflow_name_input.clear();
            }
        }
        if let Some(id) = select {
            self.workflow_selected = Some(id);
            self.workflow_last_run = None;
        }
        if let Some(id) = remove {
            if self.workflows.remove(&id) {
                let _ = self.workflows.save(&ws);
                if self.workflow_selected.as_deref() == Some(id.as_str()) {
                    self.workflow_selected = None;
                }
            }
        }
        if add_tool {
            if let Some(sel) = self.workflow_selected.clone() {
                let name = self.workflow_step_tool_input.trim().to_string();
                if !name.is_empty() {
                    let args_raw = self.workflow_step_args_input.trim().to_string();
                    let parsed = if args_raw.is_empty() {
                        Some(serde_json::json!({}))
                    } else {
                        match serde_json::from_str::<serde_json::Value>(&args_raw) {
                            Ok(v) => Some(v),
                            Err(e) => {
                                self.toasts.push(crate::editor::toast::Toast::error(format!(
                                    "Invalid JSON args: {e}"
                                )));
                                None
                            }
                        }
                    };
                    if let Some(args) = parsed {
                        if let Some(wf) = self.workflows.get_mut(&sel) {
                            wf.steps.push(WorkflowStep::Tool { name, args });
                        }
                        let _ = self.workflows.save(&ws);
                        self.workflow_step_tool_input.clear();
                        self.workflow_step_args_input.clear();
                    }
                }
            }
        }
        if add_agent {
            if let Some(sel) = self.workflow_selected.clone() {
                let prompt = self.workflow_step_prompt_input.trim().to_string();
                if !prompt.is_empty() {
                    if let Some(wf) = self.workflows.get_mut(&sel) {
                        wf.steps
                            .push(WorkflowStep::AgentTask { prompt, team: None });
                    }
                    let _ = self.workflows.save(&ws);
                    self.workflow_step_prompt_input.clear();
                }
            }
        }
        if let Some(sel) = self.workflow_selected.clone() {
            let mut mutated = false;
            if let Some(i) = remove_step {
                if let Some(wf) = self.workflows.get_mut(&sel) {
                    if i < wf.steps.len() {
                        wf.steps.remove(i);
                        mutated = true;
                    }
                }
            }
            if let Some(i) = move_up {
                if let Some(wf) = self.workflows.get_mut(&sel) {
                    if i > 0 {
                        wf.steps.swap(i, i - 1);
                        mutated = true;
                    }
                }
            }
            if let Some(i) = move_down {
                if let Some(wf) = self.workflows.get_mut(&sel) {
                    if i + 1 < wf.steps.len() {
                        wf.steps.swap(i, i + 1);
                        mutated = true;
                    }
                }
            }
            if mutated {
                let _ = self.workflows.save(&ws);
            }
        }
        if let Some(id) = run {
            if let Some(wf) = self.workflows.get(&id).cloned() {
                let runrec = wf.execute(&ws);
                self.toasts.push(crate::editor::toast::Toast::info(format!(
                    "Workflow '{}' → {}",
                    wf.name,
                    runrec.status.label()
                )));
                self.workflow_last_run = Some(runrec);
                self.workflow_selected = Some(id);
            }
        }
    }

    pub fn render_governance_panel(&mut self, ui: &mut egui::Ui) {
        use crate::editor::governance::{Decision, Rule, RuleEffect};
        let palette = self.palette();
        Self::tier3_header(
            ui,
            "Governance",
            &format!(
                "{} rule(s) · {} pending · {} secret(s) · {} connector(s)",
                self.policy.rules.len(),
                self.approvals.len(),
                self.secrets.len(),
                self.connectors.len()
            ),
            palette.accent,
            palette.text_muted,
        );
        if !self.gov_status.is_empty() {
            ui.label(
                RichText::new(&self.gov_status)
                    .size(9.0)
                    .color(palette.text_muted),
            );
        }

        egui::ScrollArea::vertical()
            .id_salt("governance_scroll")
            .show(ui, |ui| {
                // ── Policy ──
                ui.add_space(4.0);
                ui.label(RichText::new("POLICY").small().strong().color(palette.accent));
                let mut cycle_default = false;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Default when no rule matches:").size(9.0).color(palette.text_muted));
                    let txt = self.policy.default_decision.label();
                    let col = match self.policy.default_decision {
                        Decision::Allow => palette.success,
                        Decision::Deny => palette.error,
                        Decision::NeedsApproval => palette.warning,
                    };
                    if ui.small_button(RichText::new(txt).size(9.0).color(col)).clicked() {
                        cycle_default = true;
                    }
                });

                // Budget quick-set.
                let mut set_tokens: Option<Option<u64>> = None;
                let mut set_cost: Option<Option<u64>> = None;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "Budget: tokens {}, cost {}¢",
                            self.policy.budget.max_tokens.map(|t| t.to_string()).unwrap_or_else(|| "∞".into()),
                            self.policy.budget.max_cost_cents.map(|c| c.to_string()).unwrap_or_else(|| "∞".into()),
                        ))
                        .size(9.0)
                        .color(palette.text_muted),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("tokens:").size(8.0).color(palette.text_muted));
                    if ui.small_button(RichText::new("∞").size(8.0)).clicked() { set_tokens = Some(None); }
                    if ui.small_button(RichText::new("50k").size(8.0)).clicked() { set_tokens = Some(Some(50_000)); }
                    if ui.small_button(RichText::new("200k").size(8.0)).clicked() { set_tokens = Some(Some(200_000)); }
                    ui.label(RichText::new("cost¢:").size(8.0).color(palette.text_muted));
                    if ui.small_button(RichText::new("∞").size(8.0)).clicked() { set_cost = Some(None); }
                    if ui.small_button(RichText::new("100").size(8.0)).clicked() { set_cost = Some(Some(100)); }
                    if ui.small_button(RichText::new("500").size(8.0)).clicked() { set_cost = Some(Some(500)); }
                });
                // Budget status check
                let budget_decision = crate::editor::governance::evaluate_with_usage(
                    &self.workspace_root,
                    "budget_check",
                    &serde_json::json!({}),
                    self.policy.budget.max_tokens.unwrap_or(0),
                    self.policy.budget.max_cost_cents.unwrap_or(0),
                );
                ui.label(RichText::new(format!("Budget status: {}", budget_decision.label())).size(8.0).color(
                    match budget_decision {
                        crate::editor::governance::Decision::Allow => palette.success,
                        crate::editor::governance::Decision::Deny => palette.error,
                        crate::editor::governance::Decision::NeedsApproval => palette.warning,
                    }
                ));

                // Rule list.
                let mut remove_rule: Option<usize> = None;
                for (i, rule) in self.policy.rules.iter().enumerate() {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(4.0)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let scope = match (&rule.path_prefix, &rule.domain) {
                                    (Some(p), _) => format!(" path:{p}"),
                                    (_, Some(d)) => format!(" domain:{d}"),
                                    _ => String::new(),
                                };
                                ui.label(
                                    RichText::new(format!("{} [{}]{}", rule.tool, rule.effect.label(), scope))
                                        .size(9.0)
                                        .color(palette.text),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button(RichText::new("✖").size(8.0)).clicked() {
                                        remove_rule = Some(i);
                                    }
                                });
                            });
                        });
                    ui.add_space(2.0);
                }
                // Add-rule row.
                let mut add_rule: Option<RuleEffect> = None;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.gov_rule_tool_input)
                        .hint_text("tool or *").desired_width(90.0));
                    ui.add(egui::TextEdit::singleline(&mut self.gov_rule_path_input)
                        .hint_text("path prefix (opt)").desired_width(ui.available_width() - 150.0));
                    if ui.small_button(RichText::new("+allow").size(8.0)).clicked() { add_rule = Some(RuleEffect::Allow); }
                    if ui.small_button(RichText::new("+deny").size(8.0)).clicked() { add_rule = Some(RuleEffect::Deny); }
                    if ui.small_button(RichText::new("+approve").size(8.0)).clicked() { add_rule = Some(RuleEffect::RequireApproval); }
                });

                // ── Approvals ──
                ui.add_space(8.0);
                ui.label(RichText::new("APPROVAL QUEUE").small().strong().color(palette.accent));
                let mut approve: Option<String> = None;
                let mut deny: Option<String> = None;
                if self.approvals.is_empty() {
                    ui.label(RichText::new("Queue empty. No pending or historical approvals.").size(9.0).color(palette.text_muted));
                } else {
                let pending: Vec<crate::editor::governance::ApprovalItem> =
                    self.approvals.pending().into_iter().cloned().collect();
                if pending.is_empty() {
                    ui.label(RichText::new("No pending approvals (all resolved).").size(9.0).color(palette.text_muted));
                }
                for item in &pending {
                    egui::Frame::new().fill(palette.bg_secondary).corner_radius(4.0).inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&item.summary).size(9.0).color(palette.text));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button(RichText::new(crate::editor::governance::ApprovalStatus::Denied.label()).size(8.0).color(palette.error)).clicked() {
                                        deny = Some(item.id.clone());
                                    }
                                    if ui.small_button(RichText::new(crate::editor::governance::ApprovalStatus::Approved.label()).size(8.0).color(palette.success)).clicked() {
                                        approve = Some(item.id.clone());
                                    }
                                });
                            });
                        });
                    ui.add_space(2.0);
                }
                } // end else (approvals not empty)

                // ── Secrets ──
                ui.add_space(8.0);
                ui.label(RichText::new("SECRETS").small().strong().color(palette.accent));
                let mut remove_secret: Option<String> = None;
                let handles: Vec<String> = self.secrets.handles().into_iter().map(str::to_string).collect();
                if handles.is_empty() {
                    ui.label(RichText::new("No secrets stored.").size(9.0).color(palette.text_muted));
                }
                for handle in &handles {
                    let masked = self.secrets.masked(handle).unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(handle).size(9.0).color(palette.text));
                        ui.label(RichText::new(masked).size(8.0).color(palette.text_muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(RichText::new("✖").size(8.0)).clicked() {
                                remove_secret = Some(handle.clone());
                            }
                        });
                    });
                }
                let mut add_secret = false;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.gov_secret_name_input)
                        .hint_text("name").desired_width(110.0));
                    ui.add(egui::TextEdit::singleline(&mut self.gov_secret_value_input)
                        .hint_text("value").password(true).desired_width(ui.available_width() - 70.0));
                    if ui.small_button(RichText::new("+add").size(8.0)).clicked() { add_secret = true; }
                });

                // ── Connectors ──
                ui.add_space(8.0);
                ui.label(RichText::new("CONNECTORS").small().strong().color(palette.accent));
                let mut remove_connector: Option<String> = None;
                let mut update_secret_connector: Option<String> = None;
                if self.connectors.is_empty() {
                    ui.label(RichText::new("No connectors configured.").size(9.0).color(palette.text_muted));
                }
                for c in &self.connectors.connectors {
                    egui::Frame::new().fill(palette.bg_secondary).corner_radius(4.0).inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&c.name).size(9.0).strong().color(palette.text));
                                ui.label(RichText::new(&c.base_url).size(8.0).color(palette.text_muted));
                                if c.auth_secret.is_some() {
                                    ui.label(RichText::new("🔑").size(8.0));
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button(RichText::new("✖").size(8.0)).clicked() {
                                        remove_connector = Some(c.id.clone());
                                    }
                                    if ui.small_button(RichText::new("🔑").size(8.0)).clicked() {
                                        update_secret_connector = Some(c.id.clone());
                                    }
                                });
                            });
                        });
                    ui.add_space(2.0);
                }
                let mut add_connector = false;
                let mut add_preset: Option<&str> = None;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.gov_connector_id_input)
                        .hint_text("id/name").desired_width(90.0));
                    ui.add(egui::TextEdit::singleline(&mut self.gov_connector_url_input)
                        .hint_text("https://base.url").desired_width(ui.available_width() - 190.0));
                    ui.add(egui::TextEdit::singleline(&mut self.gov_connector_secret_input)
                        .hint_text("secret handle (opt)").desired_width(90.0));
                    if ui.small_button(RichText::new("+add").size(8.0)).clicked() { add_connector = true; }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Presets:").size(8.0).color(palette.text_muted));
                    if ui.small_button(RichText::new("GitHub").size(8.0)).clicked() { add_preset = Some("github"); }
                    if ui.small_button(RichText::new("Slack").size(8.0)).clicked() { add_preset = Some("slack"); }
                });

                // ── Deferred mutations ──
                let ws = self.workspace_root.clone();
                if cycle_default {
                    self.policy.default_decision = match self.policy.default_decision {
                        Decision::Allow => Decision::Deny,
                        Decision::Deny => Decision::NeedsApproval,
                        Decision::NeedsApproval => Decision::Allow,
                    };
                    let _ = self.policy.save(&ws);
                }
                if let Some(t) = set_tokens { self.policy.budget.max_tokens = t; let _ = self.policy.save(&ws); }
                if let Some(c) = set_cost { self.policy.budget.max_cost_cents = c; let _ = self.policy.save(&ws); }
                if let Some(i) = remove_rule {
                    if i < self.policy.rules.len() { self.policy.rules.remove(i); let _ = self.policy.save(&ws); }
                }
                if let Some(effect) = add_rule {
                    let tool = self.gov_rule_tool_input.trim().to_string();
                    if tool.is_empty() {
                        self.gov_status = "Rule needs a tool name (or *).".to_string();
                    } else {
                        let path = self.gov_rule_path_input.trim();
                        self.policy.rules.push(Rule {
                            tool, effect,
                            path_prefix: if path.is_empty() { None } else { Some(path.to_string()) },
                            domain: None,
                        });
                        let _ = self.policy.save(&ws);
                        self.gov_rule_tool_input.clear();
                        self.gov_rule_path_input.clear();
                        self.gov_status = "Rule added.".to_string();
                    }
                }
                if let Some(id) = approve { self.approvals.approve(&id); let _ = self.approvals.save(&ws); }
                if let Some(id) = deny { self.approvals.deny(&id); let _ = self.approvals.save(&ws); }
                if add_secret {
                    let name = self.gov_secret_name_input.trim().to_string();
                    let value = self.gov_secret_value_input.clone();
                    if name.is_empty() || value.is_empty() {
                        self.gov_status = "Secret needs a name and value.".to_string();
                    } else if self.secrets.set(name, value) {
                        match self.secrets.save(&ws) {
                            Ok(()) => {
                                self.gov_secret_name_input.clear();
                                self.gov_secret_value_input.clear();
                                self.gov_status = "Secret saved (encrypted).".to_string();
                            }
                            Err(e) => self.gov_status = format!("Secret save failed: {e}"),
                        }
                    }
                }
                if let Some(name) = remove_secret { if self.secrets.remove(&name) { let _ = self.secrets.save(&ws); } }
                if add_connector {
                    let id = self.gov_connector_id_input.trim().to_string();
                    let url = self.gov_connector_url_input.trim().to_string();
                    if id.is_empty() || url.is_empty() {
                        self.gov_status = "Connector needs an id and base URL.".to_string();
                    } else {
                        let secret = self.gov_connector_secret_input.trim();
                        let mut cfg = crate::connectors::ConnectorConfig::generic(id.clone(), id, url);
                        if !secret.is_empty() {
                            cfg.auth_secret = Some(secret.to_string());
                            cfg.auth = crate::connectors::AuthScheme::Bearer;
                        }
                        self.connectors.add(cfg);
                        let _ = self.connectors.save(&ws);
                        self.gov_connector_id_input.clear();
                        self.gov_connector_url_input.clear();
                        self.gov_connector_secret_input.clear();
                        self.gov_status = "Connector added.".to_string();
                    }
                }
                if let Some(id) = remove_connector {
                    if self.connectors.remove(&id) { let _ = self.connectors.save(&ws); }
                }
                if let Some(id) = update_secret_connector {
                    let secret = self.gov_connector_secret_input.trim().to_string();
                    if !secret.is_empty() {
                        if let Some(cfg) = self.connectors.get_mut(&id) {
                            cfg.auth_secret = Some(secret);
                            cfg.auth = crate::connectors::AuthScheme::Bearer;
                            let _ = self.connectors.save(&ws);
                            self.gov_connector_secret_input.clear();
                            self.gov_status = format!("Secret updated for connector '{}'.", id);
                        }
                    }
                }
                if let Some(preset) = add_preset {
                    let secret = self.gov_connector_secret_input.trim();
                    let secret_opt = if secret.is_empty() { None } else { Some(secret.to_string()) };
                    let cfg = match preset {
                        "github" => crate::connectors::ConnectorConfig::github("github", "GitHub", secret_opt),
                        "slack" => crate::connectors::ConnectorConfig::slack("slack", "Slack", secret_opt),
                        _ => return,
                    };
                    self.connectors.add(cfg);
                    let _ = self.connectors.save(&ws);
                    self.gov_connector_secret_input.clear();
                    self.gov_status = format!("{} preset connector added.", preset);
                }
            });
    }

    /// Render the visual workflow canvas editor.
    pub fn render_workflow_visual(&mut self, ui: &mut egui::Ui) {
        use crate::editor::workflow_canvas::{CanvasNodeKind, NodePosition, WorkflowCanvas};
        use crate::editor::workflow_templates;
        let palette = self.palette();

        // Workflow selector + actions bar
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Workflow:")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            let mut selected_id = self.workflow_canvas_selected.clone();

            egui::ComboBox::from_id_salt("workflow_canvas_selector")
                .selected_text(
                    selected_id
                        .as_deref()
                        .and_then(|id| self.workflow_canvases.get(id))
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| "Select workflow…".into()),
                )
                .show_ui(ui, |ui| {
                    for (id, canvas) in &self.workflow_canvases {
                        ui.selectable_value(&mut selected_id, Some(id.clone()), &canvas.name);
                    }
                });

            if ui.button(RichText::new("+ New").size(9.0)).clicked() {
                let id = format!("wf-{}", crate::editor::triggers::now_secs());
                let name = format!("Workflow {}", self.workflow_canvases.len() + 1);
                let canvas = WorkflowCanvas::new(&id, &name);
                self.workflow_canvases.insert(id.clone(), canvas);
                selected_id = Some(id);
            }

            egui::ComboBox::from_id_salt("workflow_template_selector")
                .selected_text("From template…")
                .show_ui(ui, |ui| {
                    for template in workflow_templates::all_templates() {
                        if ui
                            .button(format!("{} — {}", template.name, template.description))
                            .clicked()
                        {
                            let id = format!("wf-{}", crate::editor::triggers::now_secs());
                            let canvas = template.build(&id, template.name);
                            self.workflow_canvases.insert(id.clone(), canvas);
                            selected_id = Some(id);
                        }
                    }
                });

            self.workflow_canvas_selected = selected_id;
        });
        ui.add_space(4.0);

        if let Some(sel_id) = self.workflow_canvas_selected.clone() {
            // Node palette: add-node buttons
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Add node:")
                        .size(8.0)
                        .color(palette.text_muted),
                );
                let add_buttons: &[(&str, CanvasNodeKind)] = &[
                    (
                        "Agent",
                        CanvasNodeKind::AgentTask {
                            prompt: "Describe task…".into(),
                            team: None,
                        },
                    ),
                    (
                        "Tool",
                        CanvasNodeKind::Tool {
                            name: "tool_name".into(),
                            args: serde_json::json!({}),
                        },
                    ),
                    (
                        "Connector",
                        CanvasNodeKind::Connector {
                            id: "connector_id".into(),
                            req: serde_json::json!({}),
                        },
                    ),
                    (
                        "Condition",
                        CanvasNodeKind::Condition {
                            description: "Check condition".into(),
                        },
                    ),
                ];
                for (label, kind) in add_buttons {
                    if ui.small_button(RichText::new(*label).size(8.0)).clicked() {
                        if let Some(canvas) = self.workflow_canvases.get_mut(&sel_id) {
                            let offset = canvas.nodes.len() as f32;
                            let pos = NodePosition {
                                x: 200.0 + offset * 40.0,
                                y: 150.0 + (offset * 30.0) % 200.0,
                            };
                            canvas.add_node(kind.clone(), pos);
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(RichText::new("Snapshot").size(8.0))
                        .clicked()
                    {
                        if let Some(canvas) = self.workflow_canvases.get(&sel_id) {
                            self.workflow_versions.snapshot(canvas, "Manual snapshot");
                            let _ = self.workflow_versions.save(&self.workspace_root);
                        }
                    }
                    if ui
                        .small_button(RichText::new("Delete Node").size(8.0))
                        .clicked()
                    {
                        if let Some(canvas) = self.workflow_canvases.get_mut(&sel_id) {
                            if let Some(selected) = canvas.selected_node() {
                                let nid = selected.id.clone();
                                canvas.remove_node(&nid);
                            }
                        }
                    }
                });
            });
            ui.add_space(4.0);

            // Canvas area
            let canvas_size = egui::vec2(ui.available_width(), 350.0);
            if let Some(canvas) = self.workflow_canvases.get_mut(&sel_id) {
                let _action = canvas.draw(ui, canvas_size);
            }
            ui.add_space(4.0);

            // Edge connection controls
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Connect:")
                        .size(8.0)
                        .color(palette.text_muted),
                );
                if let Some(canvas) = self.workflow_canvases.get(&sel_id) {
                    let node_ids: Vec<_> = canvas
                        .nodes
                        .iter()
                        .map(|n| (n.id.clone(), n.kind.label().to_string()))
                        .collect();
                    let mut from_idx: usize = 0;
                    let mut to_idx: usize = 0;
                    egui::ComboBox::from_id_salt("edge_from")
                        .selected_text("from…")
                        .show_ui(ui, |ui| {
                            for (i, (_, label)) in node_ids.iter().enumerate() {
                                ui.selectable_value(&mut from_idx, i, label.as_str());
                            }
                        });
                    ui.label("→");
                    egui::ComboBox::from_id_salt("edge_to")
                        .selected_text("to…")
                        .show_ui(ui, |ui| {
                            for (i, (_, label)) in node_ids.iter().enumerate() {
                                ui.selectable_value(&mut to_idx, i, label.as_str());
                            }
                        });
                    if ui.small_button(RichText::new("Link").size(8.0)).clicked() {
                        if from_idx != to_idx {
                            if let Some(canvas) = self.workflow_canvases.get_mut(&sel_id) {
                                let from = node_ids[from_idx].0.clone();
                                let to = node_ids[to_idx].0.clone();
                                canvas.add_edge(from, "ok", to);
                            }
                        }
                    }
                }
            });

            // Run button
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("▶ Run Workflow").size(10.0))
                    .clicked()
                {
                    if let Some(canvas) = self.workflow_canvases.get(&sel_id) {
                        if let Some(wf) = canvas.to_workflow() {
                            let ws = self.workspace_root.clone();
                            let runrec = wf.execute(&ws);
                            self.toasts.push(crate::editor::toast::Toast::info(format!(
                                "Workflow '{}' → {}",
                                wf.name,
                                runrec.status.label()
                            )));
                            self.workflow_last_run = Some(runrec);
                        }
                    }
                }
                if let Some(runrec) = &self.workflow_last_run {
                    use crate::editor::workflow::RunStatus;
                    let status_color = match runrec.status {
                        RunStatus::Success => palette.success,
                        RunStatus::Failed => palette.error,
                        RunStatus::Partial => palette.warning,
                    };
                    ui.label(
                        RichText::new(format!(
                            "Last: {} ({} ok / {} steps)",
                            runrec.status.label(),
                            runrec.ok_count(),
                            runrec.steps.len()
                        ))
                        .size(9.0)
                        .color(status_color),
                    );
                }
            });
        } else {
            ui.label(
                RichText::new("Select or create a workflow to begin editing.")
                    .size(10.0)
                    .color(palette.text_muted),
            );
        }
    }
}

fn step_detail(step: &crate::editor::workflow::WorkflowStep) -> String {
    use crate::editor::workflow::WorkflowStep;
    match step {
        WorkflowStep::AgentTask { prompt, .. } => prompt.chars().take(80).collect(),
        WorkflowStep::Tool { args, .. } => args.to_string().chars().take(80).collect(),
        WorkflowStep::Connector { req, .. } => req.to_string().chars().take(80).collect(),
        WorkflowStep::Condition { require } => format!("require prior == {}", require.label()),
    }
}
