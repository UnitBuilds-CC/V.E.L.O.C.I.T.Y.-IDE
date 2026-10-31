//! Settings → Rules: the human-facing surface for the deterministic rule engine.
//!
//! The gate in `registry::call_tool_in_workspace` is what refuses a tool call;
//! this panel is where a rule is read, tuned, retired, bypassed, and where an
//! agent's pending request gets signed. It re-reads the rule files whenever their
//! revision moves, so a rule another agent created mid-run shows up here without
//! a restart — and an edit made here binds the next dispatched call.

use std::collections::BTreeMap;
use std::path::Path;

use egui::{self, RichText, Ui};
use serde_json::{json, Value};

use crate::editor::theme::{FONT_CAPTION, FONT_SMALL};
use crate::rules::model::RuleRecord;
use crate::rules::store::RulePaths;
use crate::rules::{
    self, BypassGrant, BypassOutcome, BypassScope, BypassSpec, ChangeOutcome, NewRule, ParamKind,
    ParamSpec, ParamValue, Rule, RuleKind, RulePatch, RuleScope, RuleSnapshot, Witnesses,
};

use super::struct_def::VelocityApp;
use super::substructs::{RulesTab, RulesUiState};

const TABS: [(RulesTab, &str); 3] = [
    (RulesTab::Active, "Active"),
    (RulesTab::Pending, "Pending"),
    (RulesTab::History, "History"),
];

const BYPASS_SCOPES: [BypassScope; 3] = [
    BypassScope::Event,
    BypassScope::Session,
    BypassScope::Workspace,
];

/// The scope names as section headers; `label()` is the lowercase file spelling.
fn scope_title(scope: RuleScope) -> &'static str {
    match scope {
        RuleScope::Task => "This task",
        RuleScope::Workspace => "This workspace",
        RuleScope::Global => "Every workspace",
    }
}

impl VelocityApp {
    /// The Rules section of the Settings panel.
    pub fn render_rules_section(&mut self, ui: &mut Ui) {
        let palette = self.palette();
        let root = self.workspace_root.clone();
        let paths = RulePaths::for_workspace(&root);

        // Moved out of `self` for the frame: the row builders take `&snapshot`
        // and `&mut state` together, which a single `&mut self` cannot hand out.
        let mut state = std::mem::take(&mut self.governance.rules);
        let mut snapshot = std::mem::take(&mut state.snapshot);
        if snapshot.revision != paths.revision() {
            snapshot = paths.snapshot(&root);
        }

        ui.add_space(10.0);
        ui.label(
            RichText::new("RULES \u{2014} ENFORCED OUTSIDE THE MODEL")
                .small()
                .strong()
                .color(palette.accent),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Every tool call is checked against these rules before it runs. A rule fires \
                 whether the model remembers it or not; editing one here takes effect on the \
                 next call, in every agent.",
            )
            .size(FONT_CAPTION)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        if !snapshot.errors.is_empty() {
            for error in &snapshot.errors {
                ui.label(
                    RichText::new(format!(
                        "\u{26a0} rules unavailable \u{2014} tool calls are being refused until \
                         this is fixed: {error}"
                    ))
                    .size(FONT_SMALL)
                    .strong()
                    .color(palette.error),
                );
            }
            ui.add_space(4.0);
        }

        if !state.notice.is_empty() {
            let color = if state.notice_is_error {
                palette.error
            } else {
                palette.success
            };
            ui.label(RichText::new(&state.notice).size(FONT_SMALL).color(color));
            ui.add_space(2.0);
        }

        ui.horizontal(|ui| {
            for (tab, title) in TABS {
                let count = match tab {
                    RulesTab::Active => snapshot.active().len(),
                    RulesTab::Pending => snapshot.pending_requests().len(),
                    RulesTab::History => snapshot.retired().len(),
                };
                let label = format!("{title} ({count})");
                if ui
                    .selectable_label(state.tab == tab, RichText::new(label).size(FONT_SMALL))
                    .clicked()
                {
                    state.tab = tab;
                }
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("Reload").size(FONT_CAPTION))
                        .fill(palette.bg_secondary),
                )
                .on_hover_text("Re-read the rule files now")
                .clicked()
            {
                snapshot = paths.snapshot(&root);
            }
        });
        ui.add_space(6.0);

        match state.tab {
            RulesTab::Active => active_tab(ui, palette, &paths, &root, &snapshot, &mut state),
            RulesTab::Pending => pending_tab(ui, palette, &paths, &root, &snapshot, &mut state),
            RulesTab::History => history_tab(ui, palette, &snapshot, &mut state),
        }

        ui.add_space(6.0);
        new_rule_form(ui, palette, &paths, &root, &mut state);

        state.snapshot = snapshot;
        self.governance.rules = state;
    }
}

// ─── Active ───────────────────────────────────────────────────────────────────

fn active_tab(
    ui: &mut Ui,
    palette: crate::editor::theme::IdePalette,
    paths: &RulePaths,
    root: &Path,
    snapshot: &RuleSnapshot,
    state: &mut RulesUiState,
) {
    let grants: Vec<(RuleScope, BypassGrant)> = snapshot
        .live_grants()
        .into_iter()
        .map(|(scope, g)| (scope, g.clone()))
        .collect();
    if !grants.is_empty() {
        ui.label(
            RichText::new("Bypasses in effect")
                .size(FONT_CAPTION)
                .strong()
                .color(palette.warning),
        );
        for (scope, grant) in &grants {
            ui.label(
                RichText::new(format!(
                    "{} \u{2014} {} \u{00b7} {} \u{00b7} {}",
                    grant.rule_id,
                    grant.scope.label(),
                    grant_remaining(grant),
                    grant.issued_by
                ))
                .size(FONT_CAPTION)
                .color(palette.text_muted),
            );
            let _ = scope;
        }
        ui.add_space(4.0);
    }

    if snapshot.active().is_empty() && snapshot.errors.is_empty() {
        ui.label(
            RichText::new(
                "No rules yet. Add one below, or let an agent call `rule_create` the moment a \
                 mistake is worth preventing.",
            )
            .size(FONT_SMALL)
            .color(palette.text_muted),
        );
        return;
    }

    for scope in RuleScope::ALL {
        let rows: Vec<RuleRecord> = snapshot
            .active()
            .into_iter()
            .filter(|(s, _)| *s == scope)
            .map(|(_, r)| r.clone())
            .collect();
        if rows.is_empty() {
            continue;
        }
        let header = format!("{} ({} rules)", scope_title(scope), rows.len());
        egui::CollapsingHeader::new(
            RichText::new(header)
                .size(FONT_SMALL)
                .strong()
                .color(palette.text),
        )
        .id_salt(egui::Id::new(format!("rules-scope-{}", scope.label())))
        .default_open(scope != RuleScope::Global)
        .show(ui, |ui| {
            for record in &rows {
                rule_row(ui, palette, paths, root, snapshot, state, scope, record);
            }
        });
    }
}

/// One rule, and the parameter sub-panel under the row that was clicked.
fn rule_row(
    ui: &mut Ui,
    palette: crate::editor::theme::IdePalette,
    paths: &RulePaths,
    root: &Path,
    snapshot: &RuleSnapshot,
    state: &mut RulesUiState,
    scope: RuleScope,
    record: &RuleRecord,
) {
    let is_open = state.selected.as_deref() == Some(record.id.as_str());
    let live = snapshot
        .live_grants()
        .into_iter()
        .find(|(_, g)| g.rule_id == record.id)
        .map(|(_, g)| g.clone());

    ui.horizontal(|ui| {
        let title = format!(
            "{}  \u{00b7}  {}",
            record.current.name,
            record.current.summary()
        );
        // Two rules in different scopes can share a name and a summary; egui ids
        // come from the visible text, so the row is namespaced by the rule id.
        let clicked = ui
            .push_id(egui::Id::new(format!("rules-row-{}", record.id)), |ui| {
                ui.selectable_label(
                    is_open,
                    RichText::new(title)
                        .size(FONT_SMALL)
                        .color(if live.is_some() {
                            palette.warning
                        } else {
                            palette.text
                        }),
                )
                .clicked()
            })
            .inner;
        if clicked {
            if is_open {
                state.selected = None;
            } else {
                state.selected = Some(record.id.clone());
                seed_drafts(state, &record.current);
            }
            state.retire_armed = false;
        }
        ui.label(
            RichText::new(format!("v{}", record.current.version))
                .size(FONT_CAPTION)
                .color(palette.text_muted),
        );
        match &live {
            Some(grant) => {
                ui.label(
                    RichText::new(format!("bypassed: {}", grant_remaining(grant)))
                        .size(FONT_CAPTION)
                        .color(palette.warning),
                );
            }
            None => {
                ui.label(
                    RichText::new(record.current.kind.label())
                        .size(FONT_CAPTION)
                        .color(palette.text_muted),
                );
            }
        }
    });

    if is_open {
        ui.indent(egui::Id::new(format!("rules-editor-{}", record.id)), |ui| {
            rule_editor(ui, palette, paths, root, state, scope, record);
        });
    }
}

/// The sub-panel: every parameter of the rule as a typed control, plus the
/// refusal it will produce, the save/retire/bypass actions.
fn rule_editor(
    ui: &mut Ui,
    palette: crate::editor::theme::IdePalette,
    paths: &RulePaths,
    root: &Path,
    state: &mut RulesUiState,
    scope: RuleScope,
    record: &RuleRecord,
) {
    ensure_slots(record.current.kind, &mut state.draft_params);
    field_row(
        ui,
        "Name",
        &mut state.draft_name,
        240.0,
        "A short handle the refusal quotes",
    );
    field_row(
        ui,
        "Intent",
        &mut state.draft_description,
        420.0,
        "Shown verbatim to the agent in every refusal",
    );
    field_row(
        ui,
        "Remedy",
        &mut state.draft_remedy,
        420.0,
        "e.g. use \"192.168.0.2\" instead",
    );
    ui.add_space(2.0);
    ui.label(
        RichText::new("Parameters")
            .size(FONT_CAPTION)
            .strong()
            .color(palette.accent),
    );
    for spec in record.current.kind.params() {
        if let Some((_, text)) = state
            .draft_params
            .iter_mut()
            .find(|(key, _)| key == spec.key)
        {
            param_field(ui, palette, spec, text);
        }
    }

    let provenance = record.current.provenance.trim();
    ui.label(
        RichText::new(format!(
            "v{} \u{00b7} owned by {} \u{00b7} {} \u{00b7} {}",
            record.current.version,
            record.current.owner,
            age_label(record.current.updated_at),
            if provenance.is_empty() {
                "no source recorded"
            } else {
                provenance
            }
        ))
        .size(FONT_CAPTION)
        .color(palette.text_muted),
    );

    if let Some(preview) = refusal_preview(scope, &record.current) {
        ui.add_space(2.0);
        ui.label(
            RichText::new("The agent reads this when it fires:")
                .size(FONT_CAPTION)
                .color(palette.text_muted),
        );
        ui.label(
            RichText::new(preview)
                .size(FONT_CAPTION)
                .color(palette.info),
        );
    } else if record.current.kind != RuleKind::ToolBudget {
        ui.label(
            RichText::new("This rule has no deny-witness, so nothing is blocked yet.")
                .size(FONT_CAPTION)
                .strong()
                .color(palette.error),
        );
    }

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .add(egui::Button::new(
                RichText::new("Save changes").size(FONT_SMALL).strong(),
            ))
            .on_hover_text("Writes a new immutable version; the next call uses it")
            .clicked()
        {
            match apply_patch(paths, root, record, state) {
                Ok(message) => {
                    state.notice = message;
                    state.notice_is_error = false;
                }
                Err(error) => {
                    state.notice = error;
                    state.notice_is_error = true;
                }
            }
        }
        if ui.button("Revert").clicked() {
            seed_drafts(state, &record.current);
        }
        let label = if state.retire_armed {
            "Confirm retire"
        } else {
            "Retire"
        };
        if ui
            .add(
                egui::Button::new(RichText::new(label).size(FONT_SMALL)).fill(
                    if state.retire_armed {
                        palette.error
                    } else {
                        palette.bg_secondary
                    },
                ),
            )
            .on_hover_text(
                "A retire is a tombstone: the rule stops blocking and stays on file for audit",
            )
            .clicked()
        {
            if state.retire_armed {
                let outcome = rules::lifecycle::retire(
                    paths,
                    root,
                    &record.id,
                    rules::USER,
                    "retired from the Rules panel",
                );
                state.retire_armed = false;
                match outcome {
                    Ok(ChangeOutcome::Applied(_)) => {
                        state.notice = format!("Retired \"{}\".", record.current.name);
                        state.notice_is_error = false;
                        state.selected = None;
                    }
                    Ok(ChangeOutcome::Pending(id)) => {
                        state.notice = format!("Retire filed as request {id}.");
                        state.notice_is_error = false;
                    }
                    Err(error) => {
                        state.notice = error;
                        state.notice_is_error = true;
                    }
                }
            } else {
                state.retire_armed = true;
            }
        }
    });

    ui.add_space(4.0);
    bypass_controls(ui, palette, paths, root, state, record);
    ui.add_space(4.0);
}

fn bypass_controls(
    ui: &mut Ui,
    palette: crate::editor::theme::IdePalette,
    paths: &RulePaths,
    root: &Path,
    state: &mut RulesUiState,
    record: &RuleRecord,
) {
    ui.label(
        RichText::new("Bypass")
            .size(FONT_CAPTION)
            .strong()
            .color(palette.accent),
    );
    ui.horizontal_wrapped(|ui| {
        let current = state.bypass_scope.label().to_string();
        egui::ComboBox::from_id_salt("rules_bypass_scope")
            .selected_text(RichText::new(current).size(FONT_SMALL))
            .show_ui(ui, |ui| {
                for scope in BYPASS_SCOPES {
                    ui.selectable_value(
                        &mut state.bypass_scope,
                        scope,
                        RichText::new(scope.label()).size(FONT_SMALL),
                    );
                }
            });
        if state.bypass_scope != BypassScope::Event {
            let mut minutes = state.bypass_minutes.max(1);
            ui.add(
                egui::DragValue::new(&mut minutes)
                    .range(1..=rules::lifecycle::MAX_BYPASS_MINUTES as i64)
                    .speed(5)
                    .suffix(" min"),
            );
            state.bypass_minutes = minutes;
        }
        ui.add(
            egui::TextEdit::singleline(&mut state.bypass_reason)
                .hint_text("why this call must be allowed")
                .desired_width(240.0),
        );
        if ui
            .add(
                egui::Button::new(RichText::new("Grant").size(FONT_SMALL))
                    .fill(palette.bg_secondary),
            )
            .on_hover_text("Allows matching calls past this rule, as you")
            .clicked()
        {
            let outcome = rules::lifecycle::bypass(
                paths,
                root,
                &record.id,
                BypassSpec {
                    scope: state.bypass_scope,
                    minutes: state.bypass_minutes.max(0) as u32,
                    reason: state.bypass_reason.clone(),
                },
                rules::USER,
            );
            match outcome {
                Ok(BypassOutcome::Granted(grant)) => {
                    state.notice = format!(
                        "Bypass granted for \"{}\" ({}).",
                        record.current.name,
                        grant.scope.label()
                    );
                    state.notice_is_error = false;
                    state.bypass_reason.clear();
                }
                Ok(BypassOutcome::Pending(id)) => {
                    state.notice = format!("Bypass filed as request {id}.");
                    state.notice_is_error = false;
                }
                Err(error) => {
                    state.notice = error;
                    state.notice_is_error = true;
                }
            }
        }
    });
}

// ─── Pending ──────────────────────────────────────────────────────────────────

fn pending_tab(
    ui: &mut Ui,
    palette: crate::editor::theme::IdePalette,
    paths: &RulePaths,
    root: &Path,
    snapshot: &RuleSnapshot,
    state: &mut RulesUiState,
) {
    let pending: Vec<(RuleScope, rules::model::ChangeRequest)> = snapshot
        .pending_requests()
        .into_iter()
        .map(|(s, r)| (s, r.clone()))
        .collect();
    if pending.is_empty() {
        ui.label(
            RichText::new(
                "Nothing waiting. An agent may create a rule at once, but loosening or removing \
                 one is filed here for your signature.",
            )
            .size(FONT_SMALL)
            .color(palette.text_muted),
        );
        return;
    }
    for (scope, request) in &pending {
        let rule_name = snapshot
            .find(&request.rule_id)
            .map(|(_, r)| r.current.name.clone())
            .unwrap_or_else(|| "(rule removed)".to_string());
        let heading = format!(
            "{} \"{}\" \u{2014} asked by {} \u{00b7} {}",
            request.action_label(),
            rule_name,
            request.requested_by,
            age_label(request.created_at),
        );
        ui.group(|ui| {
            ui.label(
                RichText::new(heading)
                    .size(FONT_SMALL)
                    .strong()
                    .color(palette.text),
            );
            ui.label(
                RichText::new(format!(
                    "{} \u{00b7} {}",
                    scope_title(*scope),
                    if request.reason.trim().is_empty() {
                        "no reason given"
                    } else {
                        request.reason.trim()
                    }
                ))
                .size(FONT_CAPTION)
                .color(palette.text_muted),
            );
            if let rules::model::ChangeAction::Adjust { patch } = request.action.clone() {
                describe_patch(ui, palette, &patch);
            }
            ui.horizontal(|ui| {
                for (label, approve) in [("Approve", true), ("Reject", false)] {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(label).size(FONT_SMALL).color(
                                if approve {
                                    palette.success
                                } else {
                                    palette.error
                                },
                            ))
                            .fill(palette.bg_secondary),
                        )
                        .clicked()
                    {
                        let decided = if approve {
                            rules::lifecycle::approve_request(paths, root, &request.id, rules::USER)
                        } else {
                            rules::lifecycle::reject_request(paths, root, &request.id, rules::USER)
                        };
                        state.notice = match decided {
                            Ok(_) => {
                                format!("{label}d {} \"{}\".", request.action_label(), rule_name)
                            }
                            Err(error) => error,
                        };
                        state.notice_is_error = state.notice.contains("not found")
                            || state.notice.contains("already")
                            || state.notice.contains("no longer");
                    }
                }
            });
        });
        ui.add_space(4.0);
    }
}

/// Show what a filed adjust actually changes, so approval is not a blind sign.
fn describe_patch(ui: &mut Ui, palette: crate::editor::theme::IdePalette, patch: &RulePatch) {
    let mut lines: Vec<String> = Vec::new();
    if let Some(name) = &patch.name {
        lines.push(format!("name \u{2192} {name}"));
    }
    if let Some(description) = &patch.description {
        lines.push(format!("intent \u{2192} {description}"));
    }
    if let Some(remedy) = &patch.remedy {
        lines.push(format!("remedy \u{2192} {remedy}"));
    }
    if let Some(params) = &patch.params {
        for (key, value) in params {
            lines.push(format!("{key} \u{2192} {}", param_value_label(value)));
        }
    }
    for line in lines {
        ui.label(
            RichText::new(line)
                .size(FONT_CAPTION)
                .color(palette.warning),
        );
    }
}

// ─── History ──────────────────────────────────────────────────────────────────

fn history_tab(
    ui: &mut Ui,
    palette: crate::editor::theme::IdePalette,
    snapshot: &RuleSnapshot,
    state: &mut RulesUiState,
) {
    let retired: Vec<(RuleScope, RuleRecord)> = snapshot
        .retired()
        .into_iter()
        .map(|(s, r)| (s, r.clone()))
        .collect();
    if retired.is_empty() {
        ui.label(
            RichText::new("No retired rules. A retire keeps the rule and its versions on file.")
                .size(FONT_SMALL)
                .color(palette.text_muted),
        );
    }
    for (scope, record) in &retired {
        let age = record
            .retired_at
            .map(age_label)
            .unwrap_or_else(|| "retired".to_string());
        ui.label(
            RichText::new(format!(
                "\u{2717} {} \u{00b7} {} \u{00b7} {} \u{00b7} {} \u{00b7} {}",
                record.current.name,
                record.current.kind.label(),
                record.current.summary(),
                scope_title(*scope),
                age
            ))
            .size(FONT_SMALL)
            .color(palette.text_disabled),
        );
    }

    let selected = state
        .selected
        .as_ref()
        .and_then(|id| snapshot.find(id))
        .map(|(scope, record)| (scope, record.clone()));
    if let Some((scope, record)) = selected {
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!("Versions of \"{}\"", record.current.name))
                .size(FONT_CAPTION)
                .strong()
                .color(palette.accent),
        );
        let mut chain: Vec<Rule> = record.history.clone();
        chain.push(record.current.clone());
        for version in chain.iter().rev() {
            ui.label(
                RichText::new(format!(
                    "v{} \u{00b7} {} \u{00b7} {} \u{00b7} {} \u{00b7} {}",
                    version.version,
                    version.owner,
                    age_label(version.updated_at),
                    scope_title(scope),
                    version.summary()
                ))
                .size(FONT_CAPTION)
                .color(if version.version == record.current.version {
                    palette.text
                } else {
                    palette.text_muted
                }),
            );
        }
    }
}

// ─── New rule ─────────────────────────────────────────────────────────────────

fn new_rule_form(
    ui: &mut Ui,
    palette: crate::editor::theme::IdePalette,
    paths: &RulePaths,
    root: &Path,
    state: &mut RulesUiState,
) {
    ensure_slots(state.new_kind, &mut state.new_params);
    egui::CollapsingHeader::new(
        RichText::new("Add a rule")
            .size(FONT_SMALL)
            .strong()
            .color(palette.accent),
    )
    .id_salt("rules-new-form")
    .default_open(false)
    .show(ui, |ui| {
        field_row(ui, "Name", &mut state.new_name, 240.0, "live-db");
        field_row(
            ui,
            "Intent",
            &mut state.new_description,
            420.0,
            "Why this must never happen again",
        );
        ui.horizontal(|ui| {
            ui.label(RichText::new("Kind").size(FONT_SMALL));
            let current = state.new_kind.label().to_string();
            egui::ComboBox::from_id_salt("rules_new_kind")
                .selected_text(RichText::new(current).size(FONT_SMALL))
                .show_ui(ui, |ui| {
                    for kind in RuleKind::ALL {
                        ui.selectable_value(
                            &mut state.new_kind,
                            kind,
                            RichText::new(kind.label()).size(FONT_SMALL),
                        );
                    }
                });
            ui.label(RichText::new("Scope").size(FONT_SMALL));
            let current = state.new_scope.label().to_string();
            egui::ComboBox::from_id_salt("rules_new_scope")
                .selected_text(RichText::new(current).size(FONT_SMALL))
                .show_ui(ui, |ui| {
                    for scope in RuleScope::ALL {
                        ui.selectable_value(
                            &mut state.new_scope,
                            scope,
                            RichText::new(scope_title(scope)).size(FONT_SMALL),
                        );
                    }
                });
        });
        ui.label(
            RichText::new(state.new_kind.intent_hint())
                .size(FONT_CAPTION)
                .color(palette.text_muted),
        );
        for spec in state.new_kind.params() {
            if let Some((_, text)) = state.new_params.iter_mut().find(|(k, _)| k == spec.key) {
                param_field(ui, palette, spec, text);
            }
        }
        field_row(
            ui,
            "Remedy",
            &mut state.new_remedy,
            420.0,
            "the sanctioned alternative the refusal points to",
        );
        field_row(
            ui,
            "Source",
            &mut state.new_provenance,
            420.0,
            "what made this rule necessary",
        );

        if state.new_kind == RuleKind::CommandPattern {
            ui.label(
                RichText::new(
                    "A command-text rule must bring its own proof: one command it blocks and one \
                     near-miss it allows.",
                )
                .size(FONT_CAPTION)
                .color(palette.text_muted),
            );
            field_row(
                ui,
                "Blocks",
                &mut state.new_deny_command,
                420.0,
                "e.g. kubectl delete pod",
            );
            field_row(
                ui,
                "Allows",
                &mut state.new_allow_command,
                420.0,
                "e.g. kubectl get pods",
            );
        }

        ui.horizontal_wrapped(|ui| {
            if ui
                .add(egui::Button::new(
                    RichText::new("Create rule").size(FONT_SMALL).strong(),
                ))
                .on_hover_text("Live immediately, on the next dispatched tool call")
                .clicked()
            {
                match create_rule(paths, root, state) {
                    Ok(message) => {
                        state.notice = message;
                        state.notice_is_error = false;
                        state.new_name.clear();
                        state.new_description.clear();
                        state.new_remedy.clear();
                        state.new_provenance.clear();
                        state.new_deny_command.clear();
                        state.new_allow_command.clear();
                        for (_, text) in &mut state.new_params {
                            text.clear();
                        }
                    }
                    Err(error) => {
                        state.notice = error;
                        state.notice_is_error = true;
                    }
                }
            }
            if ui.button("Clear").clicked() {
                for (_, text) in &mut state.new_params {
                    text.clear();
                }
            }
        });
    });
}

fn create_rule(paths: &RulePaths, root: &Path, state: &mut RulesUiState) -> Result<String, String> {
    let params = draft_to_params(state.new_kind, &state.new_params)?;
    let witnesses = if state.new_kind == RuleKind::CommandPattern {
        let deny = state.new_deny_command.trim();
        let allow = state.new_allow_command.trim();
        if deny.is_empty() || allow.is_empty() {
            return Err(
                "a command-text rule needs both proof commands: one blocked, one allowed"
                    .to_string(),
            );
        }
        Some(Witnesses {
            deny: rules::model::WitnessCall::new("run_command", json!({ "command": deny })),
            allow: rules::model::WitnessCall::new("run_command", json!({ "command": allow })),
        })
    } else {
        None
    };
    let rule = rules::lifecycle::create(
        paths,
        root,
        NewRule {
            name: state.new_name.clone(),
            description: state.new_description.clone(),
            kind: state.new_kind,
            params,
            scope: state.new_scope,
            remedy: Some(state.new_remedy.clone()),
            witnesses,
            owner: rules::USER.to_string(),
            provenance: if state.new_provenance.trim().is_empty() {
                "created in the Rules panel".to_string()
            } else {
                state.new_provenance.clone()
            },
        },
    )?;
    Ok(format!(
        "\"{}\" is live at {} scope, v{}.",
        rule.name,
        state.new_scope.label(),
        rule.version
    ))
}

// ─── Editing ──────────────────────────────────────────────────────────────────

fn seed_drafts(state: &mut RulesUiState, rule: &Rule) {
    state.draft_name = rule.name.clone();
    state.draft_description = rule.description.clone();
    state.draft_remedy = rule.remedy.clone().unwrap_or_default();
    state.draft_params = rule
        .kind
        .params()
        .iter()
        .map(|spec| {
            (
                spec.key.to_string(),
                param_to_text(rule.params.get(spec.key)),
            )
        })
        .collect();
    state.bypass_scope = BypassScope::Event;
    state.bypass_minutes = 60;
    state.bypass_reason.clear();
    state.retire_armed = false;
}

/// Keep the draft slots exactly the kind's parameter set: drop keys from a
/// previous kind, add missing ones. Called every frame so a kind change in the
/// new-rule form cannot leave a stale field behind.
fn ensure_slots(kind: RuleKind, drafts: &mut Vec<(String, String)>) {
    let keys: Vec<&str> = kind.params().iter().map(|s| s.key).collect();
    drafts.retain(|(key, _)| keys.iter().any(|k| *k == key));
    for key in keys {
        if !drafts.iter().any(|(existing, _)| existing == key) {
            drafts.push((key.to_string(), String::new()));
        }
    }
}

fn apply_patch(
    paths: &RulePaths,
    root: &Path,
    record: &RuleRecord,
    state: &mut RulesUiState,
) -> Result<String, String> {
    let kind = record.current.kind;
    let params = draft_to_params(kind, &state.draft_params)?;
    let mut patch = RulePatch::default();
    let name = state.draft_name.trim();
    if !name.is_empty() && name != record.current.name {
        patch.name = Some(name.to_string());
    }
    let description = state.draft_description.trim();
    if !description.is_empty() && description != record.current.description {
        patch.description = Some(description.to_string());
    }
    let remedy = state.draft_remedy.trim();
    if record.current.remedy.as_deref().unwrap_or("") != remedy {
        patch.remedy = Some(remedy.to_string());
    }
    if params != record.current.params {
        patch.params = Some(params.clone());
        // The engine refuses to store a command-text rule it cannot prove. Re-point
        // the existing deny-witness at the new fragment so a parameter tweak in the
        // panel does not need a hand-written witness.
        if kind == RuleKind::CommandPattern {
            patch.witnesses = witnesses_for_command_edit(&record.current, &params);
        }
    }
    if patch.is_empty() {
        return Ok("Nothing changed to save.".to_string());
    }
    match rules::lifecycle::adjust(
        paths,
        root,
        &record.id,
        patch,
        rules::USER,
        "edited in the Rules panel",
    )? {
        ChangeOutcome::Applied(record) => Ok(format!(
            "Saved \"{}\" as v{} \u{2014} live for the next call.",
            record.current.name, record.current.version
        )),
        ChangeOutcome::Pending(id) => Ok(format!("Filed as request {id} for approval.")),
    }
}

fn param_field(
    ui: &mut Ui,
    palette: crate::editor::theme::IdePalette,
    spec: &ParamSpec,
    text: &mut String,
) {
    match spec.kind {
        ParamKind::Int { min, max, unit } => {
            let mut value = parse_int_field(text).unwrap_or(min);
            let suffix = if unit.is_empty() {
                None
            } else {
                Some(format!(" {unit}"))
            };
            let label = format!("{} (int)", spec.label);
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(FONT_SMALL).color(palette.text));
                let mut drag = egui::DragValue::new(&mut value).range(min..=max).speed(1);
                if let Some(suffix) = &suffix {
                    drag = drag.suffix(suffix.clone());
                }
                if ui.add(drag).changed() {
                    changed = true;
                }
            });
            if changed {
                *text = value.to_string();
            }
        }
        ParamKind::List(hint) => {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} (list)", spec.label))
                        .size(FONT_SMALL)
                        .color(palette.text),
                );
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(format!("comma-separated \u{2014} {hint}"))
                        .desired_width(320.0),
                );
            });
        }
    }
}

fn field_row(ui: &mut Ui, label: &str, text: &mut String, width: f32, hint: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(FONT_SMALL));
        ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(hint)
                .desired_width(width),
        );
    });
}

// ─── Pure helpers ─────────────────────────────────────────────────────────────

fn param_to_text(value: Option<&ParamValue>) -> String {
    match value {
        Some(ParamValue::Int(v)) => v.to_string(),
        Some(ParamValue::Text(t)) => t.clone(),
        Some(ParamValue::List(items)) => items.join(", "),
        None => String::new(),
    }
}

fn param_value_label(value: &ParamValue) -> String {
    param_to_text(Some(value))
}

/// `"192.168.1.100, db.internal"` \u{2192} the entries, in order, empty ones dropped.
pub fn parse_list_field(text: &str) -> Vec<String> {
    text.split(',')
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

/// Reads what a person types into a whole-number field: `" 100 "`, `"1,000"`,
/// `"60 min"`. A trailing word is taken as the unit; a trailing digit or a dot
/// is not, so `"60.5"` is refused rather than silently rounded down.
pub fn parse_int_field(text: &str) -> Option<i64> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != ',')
        .collect();
    let head_end = cleaned
        .find(|c: char| c != '-' && !c.is_ascii_digit())
        .unwrap_or(cleaned.len());
    let (head, tail) = cleaned.split_at(head_end);
    if head.is_empty() || head == "-" {
        return None;
    }
    if !tail.is_empty() && !tail.chars().all(|c| c.is_alphabetic()) {
        return None;
    }
    head.parse::<i64>().ok()
}

pub fn draft_to_params(
    kind: RuleKind,
    drafts: &[(String, String)],
) -> Result<BTreeMap<String, ParamValue>, String> {
    let mut out = BTreeMap::new();
    for spec in kind.params() {
        let text = drafts
            .iter()
            .find(|(key, _)| key == spec.key)
            .map(|(_, value)| value.as_str())
            .unwrap_or("");
        let value = match spec.kind {
            ParamKind::List(_) => ParamValue::List(parse_list_field(text)),
            ParamKind::Int { min, max, .. } => {
                let parsed = parse_int_field(text)
                    .ok_or_else(|| format!("`{}` needs a whole number", spec.key))?;
                if parsed < min || parsed > max {
                    return Err(format!("`{}` must be between {min} and {max}", spec.key));
                }
                ParamValue::Int(parsed)
            }
        };
        out.insert(spec.key.to_string(), value);
    }
    Ok(out)
}

/// Re-point a command-text rule's proof pair at the new banned fragment. The deny
/// witness is a command that contained the old one, so substituting keeps it a
/// valid proof; the allow witness is a near-miss and is left alone. `None` when
/// there is nothing to substitute, which the engine then refuses to store.
pub fn witnesses_for_command_edit(
    current: &Rule,
    params: &BTreeMap<String, ParamValue>,
) -> Option<Witnesses> {
    let old = current.list("patterns").first()?.to_string();
    let new = params.get("patterns")?.as_list()?.first()?.clone();
    let mut witnesses = current.witnesses.clone()?;
    if old == new {
        return Some(witnesses);
    }
    let (arguments, replaced) = substitute(&witnesses.deny.arguments, &old, &new);
    if !replaced {
        return None;
    }
    witnesses.deny.arguments = arguments;
    Some(witnesses)
}

fn substitute(value: &Value, old: &str, new: &str) -> (Value, bool) {
    match value {
        Value::String(text) => match replace_ignore_case(text, old, new) {
            Some(replaced) => (Value::String(replaced), true),
            None => (Value::String(text.clone()), false),
        },
        Value::Array(items) => {
            let mut replaced_any = false;
            let out: Vec<Value> = items
                .iter()
                .map(|item| {
                    let (value, hit) = substitute(item, old, new);
                    replaced_any |= hit;
                    value
                })
                .collect();
            (Value::Array(out), replaced_any)
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            let mut replaced_any = false;
            for (key, item) in map {
                let (value, hit) = substitute(item, old, new);
                replaced_any |= hit;
                out.insert(key.clone(), value);
            }
            (Value::Object(out), replaced_any)
        }
        other => (other.clone(), false),
    }
}

fn replace_ignore_case(text: &str, needle: &str, replacement: &str) -> Option<String> {
    if needle.is_empty() {
        return None;
    }
    let lower = text.to_lowercase();
    let target = needle.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut replaced = false;
    while let Some(offset) = lower[cursor..].find(&target) {
        let at = cursor + offset;
        out.push_str(&text[cursor..at]);
        out.push_str(replacement);
        replaced = true;
        cursor = at + needle.len();
    }
    if !replaced {
        return None;
    }
    out.push_str(&text[cursor..]);
    Some(out)
}

/// The text the gate would hand the model for this rule's own deny-witness.
pub fn refusal_preview(scope: RuleScope, rule: &Rule) -> Option<String> {
    // A budget is proved by its parameters rather than by one call, so the
    // preview is the first call over the ceiling. Same sentence the gate emits,
    // because both go through `engine::budget_detail`.
    let detail = if rule.kind == RuleKind::ToolBudget {
        let limit = rule.int("tool_calls").unwrap_or(0);
        rules::engine::budget_detail(rule.int("window_minutes").unwrap_or(0), limit, limit + 1)
    } else {
        let witnesses = rule.witnesses.as_ref()?;
        let call = rules::ToolCall::new(
            witnesses.deny.tool.clone(),
            witnesses.deny.arguments.clone(),
        );
        rules::matcher::matches(rule, &call)?
    };
    Some(
        rules::Hit {
            rule: rule.clone(),
            scope,
            detail,
        }
        .refusal(),
    )
}

/// How much of a bypass is left, in the units a person reads.
pub fn grant_remaining(grant: &BypassGrant) -> String {
    match (grant.uses_left, grant.expires_at) {
        (Some(uses), _) => format!("{uses} call(s) left"),
        (None, Some(expires)) => {
            let left = expires.saturating_sub(rules::model::now_secs());
            format!("{} left", human_duration(left))
        }
        (None, None) => "open".to_string(),
    }
}

fn human_duration(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

/// "3h ago" from a unix timestamp; the raw second only matters at the seconds
/// level, where "just now" is the honest label.
pub fn age_label(secs: u64) -> String {
    let delta = rules::model::now_secs().saturating_sub(secs);
    if delta < 60 {
        return "just now".to_string();
    }
    if delta < 3600 {
        return format!("{}m ago", delta / 60);
    }
    if delta < 86_400 {
        return format!("{}h ago", delta / 3600);
    }
    format!("{}d ago", delta / 86_400)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::verify;
    use crate::rules::WitnessCall;

    fn command_rule(patterns: &[&str], deny: &str) -> Rule {
        let mut params = BTreeMap::new();
        params.insert(
            "patterns".to_string(),
            ParamValue::List(patterns.iter().map(|s| s.to_string()).collect()),
        );
        Rule {
            id: "rule-test".into(),
            name: "no-cluster-wipe".into(),
            description: "Deleting pods in the shared cluster takes the site down.".into(),
            kind: RuleKind::CommandPattern,
            params,
            remedy: Some("scale the deployment instead".into()),
            owner: rules::USER.into(),
            provenance: "test".into(),
            version: 1,
            created_at: rules::model::now_secs(),
            updated_at: rules::model::now_secs(),
            witnesses: Some(Witnesses {
                deny: WitnessCall::new("run_command", json!({ "command": deny })),
                allow: WitnessCall::new("run_command", json!({ "command": "kubectl get pods" })),
            }),
        }
    }

    #[test]
    fn a_list_field_splits_on_commas_and_drops_blanks() {
        assert_eq!(
            parse_list_field(" 192.168.1.100, db.internal ,,"),
            vec!["192.168.1.100".to_string(), "db.internal".to_string()]
        );
        assert!(parse_list_field(" , ,").is_empty());
    }

    #[test]
    fn an_int_field_reads_what_a_person_types() {
        assert_eq!(parse_int_field(" 100 "), Some(100));
        assert_eq!(parse_int_field("1,000"), Some(1000));
        assert_eq!(parse_int_field("60 min"), Some(60));
        assert_eq!(parse_int_field("-5"), Some(-5));
        assert_eq!(parse_int_field("ten"), None);
        assert_eq!(parse_int_field(""), None);
        assert_eq!(parse_int_field("-"), None);
        assert_eq!(
            parse_int_field("60.5"),
            None,
            "a decimal is not rounded away"
        );
    }

    #[test]
    fn a_budget_draft_becomes_typed_params_within_bounds() {
        let drafts = vec![
            ("tool_calls".to_string(), "100".to_string()),
            ("window_minutes".to_string(), "60".to_string()),
        ];
        let params = draft_to_params(RuleKind::ToolBudget, &drafts).unwrap();
        assert_eq!(params.get("tool_calls"), Some(&ParamValue::Int(100)));
        let error = draft_to_params(
            RuleKind::ToolBudget,
            &[
                ("tool_calls".into(), "0".into()),
                ("window_minutes".into(), "60".into()),
            ],
        )
        .unwrap_err();
        assert!(error.contains("between 1 and"), "{error}");
        let error = draft_to_params(
            RuleKind::ToolBudget,
            &[
                ("tool_calls".into(), "plenty".into()),
                ("window_minutes".into(), "60".into()),
            ],
        )
        .unwrap_err();
        assert!(error.contains("whole number"), "{error}");
    }

    #[test]
    fn slots_follow_the_kind_so_a_stale_field_cannot_be_saved() {
        let mut drafts = vec![
            ("targets".to_string(), "10.0.0.1".to_string()),
            ("tool_calls".to_string(), "50".to_string()),
        ];
        ensure_slots(RuleKind::ToolBudget, &mut drafts);
        assert_eq!(
            drafts.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            vec!["tool_calls", "window_minutes"]
        );
        assert_eq!(drafts[0].1, "50", "the value the user typed must survive");
    }

    #[test]
    fn editing_a_banned_fragment_moves_the_proof_with_it() {
        let current = command_rule(&["kubectl delete pod"], "sudo kubectl delete pod --all");
        let mut params = BTreeMap::new();
        params.insert(
            "patterns".to_string(),
            ParamValue::List(vec!["helm uninstall".to_string()]),
        );
        let witnesses = witnesses_for_command_edit(&current, &params).unwrap();
        assert_eq!(
            witnesses.deny.arguments["command"],
            json!("sudo helm uninstall --all")
        );
        assert_eq!(
            witnesses.allow.arguments["command"],
            json!("kubectl get pods")
        );
        // The panel's substitution must still satisfy the engine's proof rule.
        let mut candidate = current.clone();
        candidate.params = params;
        candidate.witnesses = Some(witnesses);
        verify::prepare(&mut candidate).unwrap();
    }

    #[test]
    fn a_proof_that_cannot_be_repointed_is_refused_rather_than_dropped() {
        let current = command_rule(&["kubectl delete pod"], "kubectl get pods");
        let mut params = BTreeMap::new();
        params.insert(
            "patterns".to_string(),
            ParamValue::List(vec!["rm -rf".to_string()]),
        );
        assert!(witnesses_for_command_edit(&current, &params).is_none());
    }

    #[test]
    fn the_preview_is_the_refusal_the_gate_will_send() {
        let mut rule = command_rule(&["kubectl delete pod"], "kubectl delete pod --all");
        rule.remedy = Some("scale the deployment instead".into());
        let preview = refusal_preview(RuleScope::Workspace, &rule).unwrap();
        assert!(preview.starts_with("action blocked"), "{preview}");
        assert!(
            preview.contains("command containing \"kubectl delete pod\""),
            "{preview}"
        );
        assert!(preview.contains("no-cluster-wipe"), "{preview}");
        assert!(
            preview.contains("Remedy: scale the deployment instead."),
            "{preview}"
        );
        let unproven = Rule {
            witnesses: None,
            ..rule
        };
        assert!(refusal_preview(RuleScope::Workspace, &unproven).is_none());
    }

    #[test]
    fn ages_and_remainders_read_as_durations_not_seconds() {
        let now = rules::model::now_secs();
        assert_eq!(age_label(now), "just now");
        assert_eq!(age_label(now - 120), "2m ago");
        assert_eq!(age_label(now - 7200), "2h ago");
        assert_eq!(age_label(now - 172_800), "2d ago");
        assert_eq!(
            age_label(now + 9999),
            "just now",
            "a future stamp is not a huge age"
        );
        let event = BypassGrant {
            id: "byp-1".into(),
            rule_id: "rule-1".into(),
            scope: BypassScope::Event,
            expires_at: None,
            uses_left: Some(1),
            issued_by: "user".into(),
            reason: "migration".into(),
            created_at: now,
        };
        assert_eq!(grant_remaining(&event), "1 call(s) left");
        let timed = BypassGrant {
            scope: BypassScope::Workspace,
            expires_at: Some(now + 5400),
            uses_left: None,
            ..event
        };
        assert_eq!(grant_remaining(&timed), "1h 30m left");
    }

    #[test]
    fn case_insensitive_substitution_keeps_the_surrounding_command() {
        assert_eq!(
            replace_ignore_case(
                "Sudo Kubectl Delete Pod --all",
                "kubectl delete",
                "helm uninstall"
            )
            .as_deref(),
            Some("Sudo helm uninstall Pod --all")
        );
        assert_eq!(replace_ignore_case("nothing here", "kubectl", "helm"), None);
        assert_eq!(replace_ignore_case("text", "", "helm"), None);
    }

    /// The panel writes where the gate reads: a rule created here must refuse the
    /// next dispatched call, and must not refuse the near-miss.
    #[test]
    fn a_rule_created_in_the_panel_blocks_the_next_dispatched_call() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let paths = RulePaths::for_workspace(root);
        let mut state = RulesUiState::default();
        state.new_kind = RuleKind::EgressBan;
        state.new_scope = RuleScope::Workspace;
        state.new_name = "live-db".into();
        state.new_description = "The live database is off limits.".into();
        state.new_remedy = "use 192.168.0.2 instead".into();
        state.new_params = vec![("targets".into(), "192.168.1.100, db.internal".into())];
        create_rule(&paths, root, &mut state).unwrap();

        let blocked = rules::gate_with_paths(
            &paths,
            root,
            "run_command",
            &json!({ "command": "psql -h 192.168.1.100 -U app" }),
        )
        .unwrap_err();
        assert!(blocked.contains("server \"192.168.1.100\""), "{blocked}");
        assert!(
            blocked.contains("Remedy: use 192.168.0.2 instead."),
            "{blocked}"
        );
        rules::gate_with_paths(
            &paths,
            root,
            "run_command",
            &json!({ "command": "psql -h 192.168.0.2 -U app" }),
        )
        .unwrap();
    }

    /// Tuning a number in the sub-panel is a real change of behaviour, not a
    /// label: the tightened budget blocks sooner, on the very next call.
    #[test]
    fn saving_a_budget_parameter_rebinds_the_gate() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let paths = RulePaths::for_workspace(root);
        let mut state = RulesUiState::default();
        state.new_kind = RuleKind::ToolBudget;
        state.new_scope = RuleScope::Workspace;
        state.new_name = "call-ceiling".into();
        state.new_description = "A run this long has lost the plot.".into();
        state.new_params = vec![
            ("tool_calls".into(), "3".into()),
            ("window_minutes".into(), "60".into()),
        ];
        create_rule(&paths, root, &mut state).unwrap();

        let call = json!({ "relativeFilePath": "src/lib.rs" });
        rules::gate_with_paths(&paths, root, "read_file", &call).unwrap();
        rules::gate_with_paths(&paths, root, "read_file", &call).unwrap();

        let snapshot = paths.snapshot(root);
        let (scope, record) = snapshot.active().swap_remove(0);
        let mut state = RulesUiState::default();
        seed_drafts(&mut state, &record.current);
        assert_eq!(scope, RuleScope::Workspace);
        assert_eq!(
            parse_int_field(
                &state
                    .draft_params
                    .iter()
                    .find(|(key, _)| key == "tool_calls")
                    .unwrap()
                    .1
            ),
            Some(3),
            "the sub-panel must open with the stored value"
        );
        state.draft_params = vec![
            ("tool_calls".into(), "1".into()),
            ("window_minutes".into(), "60".into()),
        ];
        apply_patch(&paths, root, record, &mut state).unwrap();

        // A new version starts a fresh window, so the second call is the one that breaks it.
        rules::gate_with_paths(&paths, root, "read_file", &call).unwrap();
        let blocked = rules::gate_with_paths(&paths, root, "read_file", &call).unwrap_err();
        assert!(blocked.contains("call-ceiling"), "{blocked}");
        assert!(blocked.contains("limit 1"), "{blocked}");
    }
}
