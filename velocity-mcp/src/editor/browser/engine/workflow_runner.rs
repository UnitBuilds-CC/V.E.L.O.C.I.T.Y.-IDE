use super::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn execute_workflow_steps(
    steps: &[BrowserWorkflowStep],
    state: &mut BrowserReplayState,
    log: &mut Vec<String>,
    checkpoints: &mut HashMap<String, BrowserReplayState>,
    workspace_root: Option<&Path>,
) -> Result<(), String> {
    for step in steps {
        match step {
            BrowserWorkflowStep::Navigate { url } => {
                let resolved_url = resolve_template(url, state);
                state.snapshot =
                    crawl_page_snapshot_with_session(&mut state.session, &resolved_url)?;
                log.push(format!(
                    "navigate {} -> {}",
                    resolved_url, state.snapshot.title
                ));
            }
            BrowserWorkflowStep::Click { role, name } => {
                let resolved_role = resolve_template(role, state);
                let resolved_name = resolve_template(name, state);
                let target = find_element(&state.snapshot, &resolved_role, &resolved_name)
                    .ok_or_else(|| {
                        format!(
                            "workflow click target not found: role='{}' name='{}'",
                            resolved_role, resolved_name
                        )
                    })?;
                let target_url = target.target_url.clone().ok_or_else(|| {
                    format!(
                        "workflow click target '{}' is not a navigable link in the current static browser engine",
                        resolved_name
                    )
                })?;
                state.snapshot = crawl_page_snapshot_with_session(&mut state.session, &target_url)?;
                log.push(format!(
                    "click {}:{} -> {}",
                    resolved_role, resolved_name, state.snapshot.title
                ));
            }
            BrowserWorkflowStep::FillField { field, value } => {
                let resolved_field = resolve_template(field, state);
                let resolved_value = resolve_template(value, state);
                apply_fill_field(state, &resolved_field, &resolved_value)?;
                log.push(format!("fill_field {} ok", resolved_field));
            }
            BrowserWorkflowStep::SubmitForm { form } => {
                let resolved_form = form.as_ref().map(|value| resolve_template(value, state));
                submit_current_form(state, resolved_form.as_deref())?;
                log.push(format!(
                    "submit_form {} -> {}",
                    resolved_form.as_deref().unwrap_or("default"),
                    state.snapshot.title
                ));
            }
            BrowserWorkflowStep::WaitForText {
                text,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_text = resolve_template(text, state);
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| snapshot_contains_text(candidate, &resolved_text),
                )?;
                log.push(format!(
                    "wait_for_text '{}' -> {}",
                    resolved_text,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForElement {
                role,
                name,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_role = resolve_template(role, state);
                let resolved_name = resolve_template(name, state);
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| find_element(candidate, &resolved_role, &resolved_name).is_some(),
                )?;
                log.push(format!(
                    "wait_for_element {}:{} -> {}",
                    resolved_role,
                    resolved_name,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForTitle {
                title,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_title = resolve_template(title, state);
                let lowered = resolved_title.to_ascii_lowercase();
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| candidate.title.to_ascii_lowercase().contains(&lowered),
                )?;
                log.push(format!(
                    "wait_for_title '{}' -> {}",
                    resolved_title,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForUrlContains {
                fragment,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_fragment = resolve_template(fragment, state);
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| candidate.url.contains(&resolved_fragment),
                )?;
                log.push(format!(
                    "wait_for_url_contains '{}' -> {}",
                    resolved_fragment,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForMutation {
                label,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_label = resolve_template(label, state);
                let lowered = resolved_label.to_ascii_lowercase();
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        candidate.mutations.iter().any(|entry: &String| {
                            entry.to_ascii_lowercase().contains(lowered.as_str())
                        })
                    },
                )?;
                log.push(format!(
                    "wait_for_mutation '{}' -> {}",
                    resolved_label,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForRequest {
                method,
                url_contains,
                status,
                resource,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_method = method.as_ref().map(|value| resolve_template(value, state));
                let resolved_url_contains = url_contains
                    .as_ref()
                    .map(|value| resolve_template(value, state));
                let resolved_resource = resource
                    .as_ref()
                    .map(|value| resolve_template(value, state));
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        candidate.requests.iter().any(|entry| {
                            request_record_matches(
                                entry,
                                resolved_method.as_deref(),
                                resolved_url_contains.as_deref(),
                                *status,
                                resolved_resource.as_deref(),
                            )
                        })
                    },
                )?;
                log.push(format!(
                    "wait_for_request method={} url_contains={} status={} resource={} -> {}",
                    resolved_method.as_deref().unwrap_or("*"),
                    resolved_url_contains.as_deref().unwrap_or("*"),
                    status
                        .map(|value: u16| value.to_string())
                        .unwrap_or_else(|| "*".to_string()),
                    resolved_resource.as_deref().unwrap_or("*"),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForStorage {
                scope,
                key,
                value,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_scope = resolve_template(scope, state);
                let resolved_key = resolve_template(key, state);
                let resolved_value = value.as_ref().map(|entry| resolve_template(entry, state));
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        storage_entry_matches(
                            candidate,
                            &resolved_scope,
                            &resolved_key,
                            resolved_value.as_deref(),
                        )
                    },
                )?;
                log.push(format!(
                    "wait_for_storage {}:{}={} -> {}",
                    resolved_scope,
                    resolved_key,
                    resolved_value.as_deref().unwrap_or("*"),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForSettle {
                label,
                scope,
                state: settle_state,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_label = label.as_ref().map(|value| resolve_template(value, state));
                let resolved_scope = scope.as_ref().map(|value| resolve_template(value, state));
                let resolved_state = settle_state
                    .as_ref()
                    .map(|value| resolve_template(value, state));
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        candidate.settle_signals.iter().any(|entry| {
                            settle_signal_matches(
                                entry,
                                resolved_label.as_deref(),
                                resolved_scope.as_deref(),
                                resolved_state.as_deref(),
                            )
                        })
                    },
                )?;
                log.push(format!(
                    "wait_for_settle {} -> {}",
                    resolved_label.clone().unwrap_or_else(|| format!(
                        "{}:{}",
                        resolved_scope.as_deref().unwrap_or("*"),
                        resolved_state.as_deref().unwrap_or("*")
                    )),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForRuntimeState {
                scope,
                key,
                value,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_scope = resolve_template(scope, state);
                let resolved_key = resolve_template(key, state);
                let resolved_value = value.as_ref().map(|value| resolve_template(value, state));
                let lowered_scope = resolved_scope.to_ascii_lowercase();
                let lowered_key = resolved_key.to_ascii_lowercase();
                let lowered_value = resolved_value
                    .as_ref()
                    .map(|value: &String| value.to_ascii_lowercase());
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        candidate.runtime_state.iter().any(|entry| {
                            entry.scope.eq_ignore_ascii_case(&lowered_scope)
                                && entry.key.eq_ignore_ascii_case(&lowered_key)
                                && lowered_value
                                    .as_ref()
                                    .map(|value: &String| {
                                        entry.value.to_ascii_lowercase().contains(value.as_str())
                                    })
                                    .unwrap_or(true)
                        })
                    },
                )?;
                log.push(format!(
                    "wait_for_runtime_state {}:{}={} -> {}",
                    resolved_scope,
                    resolved_key,
                    resolved_value.as_deref().unwrap_or("*"),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForProtocolEvent {
                event_kind,
                phase,
                target,
                detail,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_kind = event_kind
                    .as_ref()
                    .map(|value| resolve_template(value, state));
                let resolved_phase = phase.as_ref().map(|value| resolve_template(value, state));
                let resolved_target = target.as_ref().map(|value| resolve_template(value, state));
                let resolved_detail = detail.as_ref().map(|value| resolve_template(value, state));
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| {
                        candidate.protocol_events.iter().any(|entry| {
                            protocol_event_matches(
                                entry,
                                resolved_kind.as_deref(),
                                resolved_phase.as_deref(),
                                resolved_target.as_deref(),
                                resolved_detail.as_deref(),
                            )
                        })
                    },
                )?;
                log.push(format!(
                    "wait_for_protocol_event kind={} phase={} target={} detail={} -> {}",
                    resolved_kind.as_deref().unwrap_or("*"),
                    resolved_phase.as_deref().unwrap_or("*"),
                    resolved_target.as_deref().unwrap_or("*"),
                    resolved_detail.as_deref().unwrap_or("*"),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForStable {
                stable_polls,
                timeout_ms,
                interval_ms,
            } => {
                let diff = wait_for_stable_snapshot(
                    &mut state.session,
                    &mut state.snapshot,
                    *stable_polls,
                    *timeout_ms,
                    *interval_ms,
                )?;
                log.push(format!(
                    "wait_for_stable polls={} -> {}",
                    stable_polls.unwrap_or(DEFAULT_STABLE_POLLS),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::ExtractText {
                output,
                source,
                role,
                name,
                field,
            } => {
                let resolved_source = resolve_template(source, state);
                let resolved_role = role.as_ref().map(|value| resolve_template(value, state));
                let resolved_name = name.as_ref().map(|value| resolve_template(value, state));
                let resolved_field = field.as_ref().map(|value| resolve_template(value, state));
                let extracted = extract_snapshot_value(
                    &state.snapshot,
                    &resolved_source,
                    resolved_role.as_deref(),
                    resolved_name.as_deref(),
                    resolved_field.as_deref(),
                )?;
                state.outputs.insert(output.clone(), extracted.clone());
                log.push(format!(
                    "extract_text {}='{}'",
                    output,
                    truncate_string(&extracted, 80)
                ));
            }
            BrowserWorkflowStep::SaveCheckpoint { name } => {
                let resolved_name = resolve_template(name, state);
                checkpoints.insert(resolved_name.clone(), state.clone());
                if let Some(root) = workspace_root {
                    let path = persist_checkpoint_from_replay_state(root, state, &resolved_name)?;
                    log.push(format!(
                        "save_checkpoint {} -> {}",
                        resolved_name,
                        path.display()
                    ));
                } else {
                    log.push(format!("save_checkpoint {} ok", resolved_name));
                }
            }
            BrowserWorkflowStep::RestoreCheckpoint { name } => {
                let resolved_name = resolve_template(name, state);
                let restored = checkpoints.get(&resolved_name).cloned().ok_or_else(|| {
                    format!("workflow restore checkpoint not found: '{}'", resolved_name)
                })?;
                *state = restored;
                log.push(format!(
                    "restore_checkpoint {} -> {}",
                    resolved_name, state.snapshot.title
                ));
            }
            BrowserWorkflowStep::IfTextContains {
                text,
                then_steps,
                else_steps,
            } => {
                let resolved_text = resolve_template(text, state);
                let matched = snapshot_contains_text(&state.snapshot, &resolved_text);
                log.push(format!(
                    "if_text_contains '{}' -> {}",
                    resolved_text,
                    if matched { "then" } else { "else" }
                ));
                let branch = if matched { then_steps } else { else_steps };
                execute_workflow_steps(branch, state, log, checkpoints, workspace_root)?;
            }
            BrowserWorkflowStep::IfOutputEquals {
                output,
                equals,
                then_steps,
                else_steps,
            } => {
                let actual = state.outputs.get(output).cloned().unwrap_or_default();
                let resolved_expected = resolve_template(equals, state);
                let matched = actual == resolved_expected;
                log.push(format!(
                    "if_output_equals {}='{}' -> {}",
                    output,
                    truncate_string(&actual, 80),
                    if matched { "then" } else { "else" }
                ));
                let branch = if matched { then_steps } else { else_steps };
                execute_workflow_steps(branch, state, log, checkpoints, workspace_root)?;
            }
            BrowserWorkflowStep::AssertElement { role, name } => {
                let resolved_role = resolve_template(role, state);
                let resolved_name = resolve_template(name, state);
                find_element(&state.snapshot, &resolved_role, &resolved_name).ok_or_else(|| {
                    format!(
                        "workflow assertion failed: missing element role='{}' name='{}'",
                        resolved_role, resolved_name
                    )
                })?;
                log.push(format!(
                    "assert_element {}:{} ok",
                    resolved_role, resolved_name
                ));
            }
            BrowserWorkflowStep::AssertTextContains { text } => {
                let resolved_text = resolve_template(text, state);
                if !snapshot_contains_text(&state.snapshot, &resolved_text) {
                    return Err(format!(
                        "workflow assertion failed: text '{}' not present",
                        resolved_text
                    ));
                }
                log.push(format!("assert_text '{}' ok", resolved_text));
            }
            BrowserWorkflowStep::AssertOutput {
                output,
                equals,
                contains,
            } => {
                let actual = state.outputs.get(output).cloned().ok_or_else(|| {
                    format!("workflow assertion failed: output '{}' not present", output)
                })?;
                if let Some(expected) = equals {
                    let resolved_expected = resolve_template(expected, state);
                    if actual != resolved_expected {
                        return Err(format!(
                            "workflow assertion failed: output '{}' expected '{}' but was '{}'",
                            output, resolved_expected, actual
                        ));
                    }
                }
                if let Some(expected_fragment) = contains {
                    let resolved_fragment = resolve_template(expected_fragment, state);
                    if !actual.contains(&resolved_fragment) {
                        return Err(format!(
                            "workflow assertion failed: output '{}' does not contain '{}'",
                            output, resolved_fragment
                        ));
                    }
                }
                log.push(format!("assert_output {} ok", output));
            }
        }
    }
    Ok(())
}

pub fn render_workflow_replay_result(
    result: &str,
    snapshot_path: &Path,
    session_path: &Path,
    facts_path: &Path,
    report_path: &Path,
    html_fallback_path: Option<&Path>,
) -> String {
    let html_fallback = html_fallback_path
        .map(|path: &Path| format!("\nHTML fallback: {}", path.display()))
        .unwrap_or_default();
    format!(
        "{}\nSnapshot JSON: {}\nSession JSON: {}{}\nNDA Facts: {}\nRun Report: {}",
        result,
        snapshot_path.display(),
        session_path.display(),
        html_fallback,
        facts_path.display(),
        report_path.display()
    )
}

pub fn render_workflow_suite_execution_report(
    suite_name: &str,
    summary: &BrowserWorkflowSuiteRunSummary,
    report_path: &Path,
) -> String {
    format!(
        "Workflow suite '{}' completed.\nTotal: {}\nPassed: {}\nFailed: {}\nSuite Report: {}",
        suite_name,
        summary.total,
        summary.passed,
        summary.failed,
        report_path.display()
    )
}

fn replay_workflow_with_state(
    workflow: &BrowserWorkflow,
    mut state: BrowserReplayState,
    workspace_root: Option<&Path>,
) -> Result<(String, BrowserReplayState, BrowserWorkflowRunReport), String> {
    let mut log = vec![format!(
        "start {} -> {}",
        state.snapshot.url, state.snapshot.title
    )];
    let mut checkpoints = HashMap::new();
    execute_workflow_steps(
        &workflow.steps,
        &mut state,
        &mut log,
        &mut checkpoints,
        workspace_root,
    )?;

    let network_summary = summarize_network_activity(&state.snapshot.protocol_events);
    let report = BrowserWorkflowRunReport {
        workflow_name: workflow.name.clone(),
        session_id: state.session.id.clone(),
        final_url: state.snapshot.url.clone(),
        final_title: state.snapshot.title.clone(),
        step_count: workflow.steps.len(),
        cookie_count: state.session.cookies.len(),
        local_storage_count: state.session.local_storage.len(),
        session_storage_count: state.session.session_storage.len(),
        mutation_count: state.snapshot.mutations.len(),
        request_count: state.snapshot.requests.len(),
        settle_signal_count: state.snapshot.settle_signals.len(),
        runtime_state_count: state.snapshot.runtime_state.len(),
        protocol_event_count: state.snapshot.protocol_events.len(),
        network_summary: network_summary.clone(),
        outputs: state.outputs.clone(),
        log: log.clone(),
    };
    let network_summary_line = render_network_summary(&network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let result = format!(
        "Workflow '{}' completed.\nFinal URL: {}\nFinal title: {}\nSession: {}\nSteps executed: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nLocal storage: {}\nSession storage: {}\nMutations: {}\nOutputs: {}\n{}",
        workflow.name,
        state.snapshot.url,
        state.snapshot.title,
        state.session.id,
        workflow.steps.len(),
        state.session.cookies.len(),
        state.snapshot.requests.len(),
        state.snapshot.settle_signals.len(),
        state.snapshot.runtime_state.len(),
        state.snapshot.protocol_events.len(),
        network_summary_line,
        state.session.local_storage.len(),
        state.session.session_storage.len(),
        state.snapshot.mutations.len(),
        state.outputs.len(),
        log.join("\n")
    );
    Ok((result, state, report))
}

pub fn persist_replay_state(
    workspace_root: &Path,
    state: &BrowserReplayState,
    sitemap_path: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf, Option<PathBuf>), String> {
    persist_snapshot_to_sitemap(&state.snapshot, sitemap_path)?;
    let facts_path = write_crawl_facts(
        &state.snapshot.url,
        &state.snapshot.title,
        &state.snapshot.summary,
        &state.snapshot.elements,
        &state.snapshot.forms,
        &state.snapshot.cookies,
        &state.snapshot.storage,
        &state.snapshot.mutations,
        &state.snapshot.requests,
        &state.snapshot.settle_signals,
        &state.snapshot.runtime_state,
        &state.snapshot.protocol_events,
        sitemap_path,
    )?;
    let snapshot_path = write_snapshot_json(&state.snapshot, sitemap_path)?;
    let html_fallback_path = write_html_fallback(
        &state.snapshot.url,
        state.session.last_html.as_deref().unwrap_or_default(),
        sitemap_path,
    )?;
    let session_path = save_session_state(workspace_root, &state.session)?;
    Ok((snapshot_path, session_path, facts_path, html_fallback_path))
}

fn persist_run_report(
    workspace_root: &Path,
    report: &BrowserWorkflowRunReport,
) -> Result<PathBuf, String> {
    let path = browser_workflow_run_path(workspace_root, &report.workflow_name, &report.session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser run dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(report)
        .map_err(|err| format!("serialise browser run report: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser run report: {err}"))?;
    Ok(path)
}

fn persist_suite_run_report(
    workspace_root: &Path,
    report: &BrowserWorkflowSuiteRunReport,
) -> Result<PathBuf, String> {
    let path = browser_workflow_suite_run_path(workspace_root, &report.suite_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser suite run dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(report)
        .map_err(|err| format!("serialise browser suite run report: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser suite run report: {err}"))?;
    Ok(path)
}

pub fn replay_workflow(workflow: &BrowserWorkflow) -> Result<String, String> {
    let mut session = BrowserSessionState {
        id: format!("replay-{}", sanitize_file_stem(&workflow.name)),
        current_url: Some(workflow.start_url.clone()),
        cookies: Vec::new(),
        runtime_cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    };
    let snapshot = crawl_page_snapshot_with_session(&mut session, &workflow.start_url)?;
    let state = BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
        variables: workflow.variables.clone(),
        outputs: HashMap::new(),
    };
    let (result, _, _) = replay_workflow_with_state(workflow, state, None)?;
    Ok(result)
}

pub fn replay_workflow_with_artifacts_report(
    workspace_root: &Path,
    workflow: &BrowserWorkflow,
    sitemap_path: &Path,
) -> Result<BrowserWorkflowReplayReport, String> {
    let mut session = BrowserSessionState {
        id: format!("replay-{}", sanitize_file_stem(&workflow.name)),
        current_url: Some(workflow.start_url.clone()),
        cookies: Vec::new(),
        runtime_cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    };
    let snapshot = crawl_page_snapshot_with_session(&mut session, &workflow.start_url)?;
    let state = BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
        variables: workflow.variables.clone(),
        outputs: HashMap::new(),
    };
    let (result, final_state, report) =
        replay_workflow_with_state(workflow, state, Some(workspace_root))?;
    let (snapshot_path, session_path, facts_path, html_fallback_path): (
        PathBuf,
        PathBuf,
        PathBuf,
        Option<PathBuf>,
    ) = persist_replay_state(workspace_root, &final_state, sitemap_path)?;
    let report_path = persist_run_report(workspace_root, &report)?;
    let workflow = summarize_workflow_run(report);
    let _ = result;
    Ok(BrowserWorkflowReplayReport {
        workflow,
        snapshot_json_path: snapshot_path.display().to_string(),
        session_json_path: session_path.display().to_string(),
        nda_facts_path: facts_path.display().to_string(),
        run_report_path: report_path.display().to_string(),
        html_fallback_path: html_fallback_path.map(|path: PathBuf| path.display().to_string()),
    })
}

pub fn replay_workflow_with_artifacts(
    workspace_root: &Path,
    workflow: &BrowserWorkflow,
    sitemap_path: &Path,
) -> Result<String, String> {
    let report = replay_workflow_with_artifacts_report(workspace_root, workflow, sitemap_path)?;
    let network_summary = render_network_summary(&report.workflow.network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let result = format!(
        "Workflow '{}' completed.\nFinal URL: {}\nFinal title: {}\nSession: {}\nSteps executed: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nLocal storage: {}\nSession storage: {}",
        report.workflow.workflow_name,
        report.workflow.final_url,
        report.workflow.final_title,
        report.workflow.session_id,
        report.workflow.step_count,
        report.workflow.cookie_count,
        report.workflow.request_count,
        report.workflow.settle_signal_count,
        report.workflow.runtime_state_count,
        report.workflow.protocol_event_count,
        network_summary,
        report.workflow.local_storage_count,
        report.workflow.session_storage_count,
    );
    Ok(render_workflow_replay_result(
        &result,
        Path::new(&report.snapshot_json_path),
        Path::new(&report.session_json_path),
        Path::new(&report.nda_facts_path),
        Path::new(&report.run_report_path),
        report.html_fallback_path.as_deref().map(Path::new),
    ))
}

pub fn run_workflow_suite_report(
    workspace_root: &Path,
    suite: &BrowserWorkflowSuite,
    sitemap_path: &Path,
) -> Result<BrowserWorkflowSuiteExecutionReport, String> {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut items = Vec::with_capacity(suite.workflows.len());

    for workflow_path in &suite.workflows {
        let full_path = workspace_root.join(PathBuf::from(workflow_path));
        match load_workflow(&full_path) {
            Ok(workflow) => {
                match replay_workflow_with_artifacts(workspace_root, &workflow, sitemap_path) {
                    Ok(summary) => {
                        passed += 1;
                        items.push(BrowserWorkflowSuiteRunItem {
                            workflow_path: workflow_path.clone(),
                            workflow_name: workflow.name,
                            status: "passed".to_string(),
                            summary: truncate_string(&summary, 400),
                        });
                    }
                    Err(err) => {
                        failed += 1;
                        items.push(BrowserWorkflowSuiteRunItem {
                            workflow_path: workflow_path.clone(),
                            workflow_name: workflow.name,
                            status: "failed".to_string(),
                            summary: err,
                        });
                    }
                }
            }
            Err(err) => {
                failed += 1;
                items.push(BrowserWorkflowSuiteRunItem {
                    workflow_path: workflow_path.clone(),
                    workflow_name: workflow_path.clone(),
                    status: "failed".to_string(),
                    summary: err,
                });
            }
        }
    }

    let report = BrowserWorkflowSuiteRunReport {
        suite_name: suite.name.clone(),
        total: suite.workflows.len(),
        passed,
        failed,
        items,
    };
    let report_path = persist_suite_run_report(workspace_root, &report)?;
    let suite = summarize_workflow_suite_run(report);
    Ok(BrowserWorkflowSuiteExecutionReport {
        suite,
        suite_report_path: report_path.display().to_string(),
    })
}

pub fn run_workflow_suite(
    workspace_root: &Path,
    suite: &BrowserWorkflowSuite,
    sitemap_path: &Path,
) -> Result<String, String> {
    let report = run_workflow_suite_report(workspace_root, suite, sitemap_path)?;
    Ok(render_workflow_suite_execution_report(
        &report.suite.suite_name,
        &report.suite,
        Path::new(&report.suite_report_path),
    ))
}

pub fn replay_workflow_in_session_report(
    workspace_root: &Path,
    session_id: &str,
    workflow: &BrowserWorkflow,
    sitemap_path: &Path,
) -> Result<BrowserWorkflowReplayReport, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    let current_url = session.current_url.clone();
    let snapshot = match current_url.as_ref() {
        Some(url) => load_snapshot_json(url, sitemap_path)
            .or_else(|_| crawl_page_snapshot_with_session(&mut session, url)),
        None => crawl_page_snapshot_with_session(&mut session, &workflow.start_url),
    }?;
    session.id = session_id.to_string();
    let state = BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
        variables: workflow.variables.clone(),
        outputs: HashMap::new(),
    };
    let (_result, final_state, report) =
        replay_workflow_with_state(workflow, state, Some(workspace_root))?;
    let (snapshot_path, session_path, facts_path, html_fallback_path): (
        PathBuf,
        PathBuf,
        PathBuf,
        Option<PathBuf>,
    ) = persist_replay_state(workspace_root, &final_state, sitemap_path)?;
    let report_path = persist_run_report(workspace_root, &report)?;
    let workflow = summarize_workflow_run(report);
    Ok(BrowserWorkflowReplayReport {
        workflow,
        snapshot_json_path: snapshot_path.display().to_string(),
        session_json_path: session_path.display().to_string(),
        nda_facts_path: facts_path.display().to_string(),
        run_report_path: report_path.display().to_string(),
        html_fallback_path: html_fallback_path.map(|path: PathBuf| path.display().to_string()),
    })
}

pub fn replay_workflow_in_session(
    workspace_root: &Path,
    session_id: &str,
    workflow: &BrowserWorkflow,
    sitemap_path: &Path,
) -> Result<String, String> {
    let report =
        replay_workflow_in_session_report(workspace_root, session_id, workflow, sitemap_path)?;
    let network_summary = render_network_summary(&report.workflow.network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let result = format!(
        "Workflow '{}' completed.\nFinal URL: {}\nFinal title: {}\nSession: {}\nSteps executed: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nLocal storage: {}\nSession storage: {}",
        report.workflow.workflow_name,
        report.workflow.final_url,
        report.workflow.final_title,
        report.workflow.session_id,
        report.workflow.step_count,
        report.workflow.cookie_count,
        report.workflow.request_count,
        report.workflow.settle_signal_count,
        report.workflow.runtime_state_count,
        report.workflow.protocol_event_count,
        network_summary,
        report.workflow.local_storage_count,
        report.workflow.session_storage_count,
    );
    Ok(render_workflow_replay_result(
        &result,
        Path::new(&report.snapshot_json_path),
        Path::new(&report.session_json_path),
        Path::new(&report.nda_facts_path),
        Path::new(&report.run_report_path),
        report.html_fallback_path.as_deref().map(Path::new),
    ))
}
