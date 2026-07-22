use super::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub fn render_web_navigate_report(report: &BrowserWebNavigateReport) -> String {
    let network_summary = render_network_summary(&report.snapshot.network_summary)
        .map(|value| format!("\nNetwork summary: {}", value))
        .unwrap_or_default();
    let html_fallback = render_html_fallback_line(report.html_fallback_path.as_deref());
    format!(
        "Crawler finished.\nURL: {}\nTitle: {}\nInteractive Elements: {}\nForms: {}\nCookies: {}\nRequests: {}\nSettle signals: {}\nRuntime state: {}\nProtocol events: {}{}\nRegistered in Merkle SiteMap at {}\nSnapshot JSON: {}{}\nNDA Facts: {}",
        report.snapshot.url,
        report.snapshot.title,
        report.snapshot.element_count,
        report.snapshot.form_count,
        report.snapshot.cookie_count,
        report.snapshot.request_count,
        report.snapshot.settle_signal_count,
        report.snapshot.runtime_state_count,
        report.snapshot.protocol_event_count,
        network_summary,
        report.sitemap_path,
        report.snapshot_json_path,
        html_fallback,
        report.nda_facts_path,
    )
}

pub fn crawl_and_sync_sitemap_report(
    url: &str,
    sitemap_path: &Path,
) -> Result<BrowserWebNavigateReport, String> {
    let mut session = BrowserSessionState {
        id: "ephemeral".to_string(),
        current_url: Some(url.to_string()),
        cookies: Vec::new(),
        runtime_cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    };
    let snapshot = crawl_page_snapshot_with_session(&mut session, url)?;
    persist_snapshot_to_sitemap(&snapshot, sitemap_path)?;
    let facts_path = write_crawl_facts(
        &snapshot.url,
        &snapshot.title,
        &snapshot.summary,
        &snapshot.elements,
        &snapshot.forms,
        &snapshot.cookies,
        &snapshot.storage,
        &snapshot.mutations,
        &snapshot.requests,
        &snapshot.settle_signals,
        &snapshot.runtime_state,
        &snapshot.protocol_events,
        sitemap_path,
    )?;
    let snapshot_path = write_snapshot_json(&snapshot, sitemap_path)?;
    let html_fallback_path = write_html_fallback(
        &snapshot.url,
        session.last_html.as_deref().unwrap_or_default(),
        sitemap_path,
    )?;
    let snapshot_summary = snapshot.summary.clone();
    let summary = summarize_snapshot(snapshot);

    Ok(BrowserWebNavigateReport {
        snapshot: summary,
        snapshot_summary,
        snapshot_json_path: snapshot_path.display().to_string(),
        nda_facts_path: facts_path.display().to_string(),
        sitemap_path: sitemap_path.display().to_string(),
        html_fallback_path: html_fallback_path.map(|path: PathBuf| path.display().to_string()),
    })
}

pub fn crawl_and_sync_sitemap(url: &str, sitemap_path: &Path) -> Result<String, String> {
    let report = crawl_and_sync_sitemap_report(url, sitemap_path)?;
    Ok(render_web_navigate_report(&report))
}

fn render_workflow_step_lines(lines: &mut Vec<String>, step: &BrowserWorkflowStep, prefix: &str) {
    match step {
        BrowserWorkflowStep::Navigate { url } => {
            lines.push(format!("{}\tnavigate\t{}", prefix, encode_nda_text(url)));
        }
        BrowserWorkflowStep::Click { role, name } => {
            lines.push(format!(
                "{}\tclick\trole={}\tname={}",
                prefix,
                encode_nda_text(role),
                encode_nda_text(name)
            ));
        }
        BrowserWorkflowStep::FillField { field, value } => {
            lines.push(format!(
                "{}\tfill_field\tfield={}\tvalue={}",
                prefix,
                encode_nda_text(field),
                encode_nda_text(value)
            ));
        }
        BrowserWorkflowStep::SubmitForm { form } => {
            lines.push(format!(
                "{}\tsubmit_form\tform={}",
                prefix,
                encode_nda_text(form.as_deref().unwrap_or("default"))
            ));
        }
        BrowserWorkflowStep::WaitForText {
            text,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_text\ttext={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(text),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForElement {
            role,
            name,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_element\trole={}\tname={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(role),
                encode_nda_text(name),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForTitle {
            title,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_title\ttitle={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(title),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForUrlContains {
            fragment,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_url_contains\tfragment={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(fragment),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForMutation {
            label,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_mutation\tlabel={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(label),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
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
            lines.push(format!(
                "{}\twait_for_request\tmethod={}\turl_contains={}\tstatus={}\tresource={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                encode_nda_text(method.as_deref().unwrap_or_default()),
                encode_nda_text(url_contains.as_deref().unwrap_or_default()),
                status.map(|value: u16| value.to_string()).unwrap_or_default(),
                encode_nda_text(resource.as_deref().unwrap_or_default()),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForStorage {
            scope,
            key,
            value,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_storage\tscope={}\tkey={}\tvalue={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(scope),
                encode_nda_text(key),
                encode_nda_text(value.as_deref().unwrap_or_default()),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForSettle {
            label,
            scope,
            state,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_settle\tlabel={}\tscope={}\tstate={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                encode_nda_text(label.as_deref().unwrap_or_default()),
                encode_nda_text(scope.as_deref().unwrap_or_default()),
                encode_nda_text(state.as_deref().unwrap_or_default()),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForRuntimeState {
            scope,
            key,
            value,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_runtime_state\tscope={}\tkey={}\tvalue={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                encode_nda_text(scope),
                encode_nda_text(key),
                encode_nda_text(value.as_deref().unwrap_or_default()),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
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
            lines.push(format!(
                "{}\twait_for_protocol_event\tkind={}\tphase={}\ttarget={}\tdetail={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                encode_nda_text(event_kind.as_deref().unwrap_or_default()),
                encode_nda_text(phase.as_deref().unwrap_or_default()),
                encode_nda_text(target.as_deref().unwrap_or_default()),
                encode_nda_text(detail.as_deref().unwrap_or_default()),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForStable {
            stable_polls,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_stable\tstable_polls={}\ttimeout_ms={}\tinterval_ms={}",
                prefix,
                stable_polls.unwrap_or(DEFAULT_STABLE_POLLS),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::ExtractText {
            output,
            source,
            role,
            name,
            field,
        } => {
            lines.push(format!(
                "{}\textract_text\toutput={}\tsource={}\trole={}\tname={}\tfield={}",
                prefix,
                encode_nda_text(output),
                encode_nda_text(source),
                encode_nda_text(role.as_deref().unwrap_or_default()),
                encode_nda_text(name.as_deref().unwrap_or_default()),
                encode_nda_text(field.as_deref().unwrap_or_default())
            ));
        }
        BrowserWorkflowStep::SaveCheckpoint { name } => {
            lines.push(format!(
                "{}\tsave_checkpoint\tname={}",
                prefix,
                encode_nda_text(name)
            ));
        }
        BrowserWorkflowStep::RestoreCheckpoint { name } => {
            lines.push(format!(
                "{}\trestore_checkpoint\tname={}",
                prefix,
                encode_nda_text(name)
            ));
        }
        BrowserWorkflowStep::IfTextContains {
            text,
            then_steps,
            else_steps,
        } => {
            lines.push(format!(
                "{}\tif_text_contains\ttext={}",
                prefix,
                encode_nda_text(text)
            ));
            for (idx, nested) in then_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:then:{}", prefix, idx));
            }
            for (idx, nested) in else_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:else:{}", prefix, idx));
            }
        }
        BrowserWorkflowStep::IfOutputEquals {
            output,
            equals,
            then_steps,
            else_steps,
        } => {
            lines.push(format!(
                "{}\tif_output_equals\toutput={}\tequals={}",
                prefix,
                encode_nda_text(output),
                encode_nda_text(equals)
            ));
            for (idx, nested) in then_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:then:{}", prefix, idx));
            }
            for (idx, nested) in else_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:else:{}", prefix, idx));
            }
        }
        BrowserWorkflowStep::AssertElement { role, name } => {
            lines.push(format!(
                "{}\tassert_element\trole={}\tname={}",
                prefix,
                encode_nda_text(role),
                encode_nda_text(name)
            ));
        }
        BrowserWorkflowStep::AssertTextContains { text } => {
            lines.push(format!(
                "{}\tassert_text\t{}",
                prefix,
                encode_nda_text(text)
            ));
        }
        BrowserWorkflowStep::AssertOutput {
            output,
            equals,
            contains,
        } => {
            lines.push(format!(
                "{}\tassert_output\toutput={}\tequals={}\tcontains={}",
                prefix,
                encode_nda_text(output),
                encode_nda_text(equals.as_deref().unwrap_or_default()),
                encode_nda_text(contains.as_deref().unwrap_or_default())
            ));
        }
    }
}

pub fn render_workflow_dsl(workflow: &BrowserWorkflow) -> String {
    let mut lines = vec![
        "browser-workflow version 2".to_string(),
        format!("name\t{}", encode_nda_text(&workflow.name)),
        format!("start_url\t{}", encode_nda_text(&workflow.start_url)),
    ];

    for (idx, step) in workflow.steps.iter().enumerate() {
        let prefix = format!("step\t{}", idx);
        render_workflow_step_lines(&mut lines, step, &prefix);
    }

    lines.join("\n") + "\n"
}

pub fn render_workflow_save_report(report: &BrowserWorkflowSaveReport) -> String {
    format!(
        "Saved browser workflow '{}'\nJSON: {}\nNDA: {}",
        report.workflow.name, report.json_path, report.nda_path,
    )
}

pub fn save_workflow_report(
    workspace_root: &Path,
    workflow: &BrowserWorkflow,
) -> Result<BrowserWorkflowSaveReport, String> {
    let json_path = browser_workflow_json_path(workspace_root, &workflow.name);
    let nda_path = browser_workflow_nda_path(workspace_root, &workflow.name);
    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create workflow dir: {err}"))?;
    }
    let json =
        serde_json::to_vec_pretty(workflow).map_err(|err| format!("serialise workflow: {err}"))?;
    fs::write(&json_path, json).map_err(|err| format!("write workflow json: {err}"))?;
    fs::write(&nda_path, render_workflow_dsl(workflow))
        .map_err(|err| format!("write workflow nda: {err}"))?;
    Ok(BrowserWorkflowSaveReport {
        workflow: summarize_workflow(workflow.clone()),
        json_path: json_path.display().to_string(),
        nda_path: nda_path.display().to_string(),
    })
}

pub fn save_workflow(
    workspace_root: &Path,
    workflow: &BrowserWorkflow,
) -> Result<(PathBuf, PathBuf), String> {
    let report = save_workflow_report(workspace_root, workflow)?;
    Ok((
        PathBuf::from(report.json_path),
        PathBuf::from(report.nda_path),
    ))
}

pub fn load_workflow(path: &Path) -> Result<BrowserWorkflow, String> {
    let raw = fs::read(path).map_err(|err| format!("read workflow: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse workflow: {err}"))
}

pub fn read_workflow_report(path: &Path) -> Result<BrowserWorkflowReadReport, String> {
    let workflow = load_workflow(path)?;
    Ok(BrowserWorkflowReadReport {
        nda_path: browser_workflow_nda_path(
            path.parent()
                .and_then(|parent: &Path| parent.parent())
                .and_then(|parent: &Path| parent.parent())
                .ok_or("workflow path is not inside a workspace")?,
            &workflow.name,
        )
        .display()
        .to_string(),
        workflow: summarize_workflow(workflow),
        json_path: path.display().to_string(),
    })
}

pub fn list_workflows(
    workspace_root: &Path,
    workflow_name_contains: Option<&str>,
    start_url_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserWorkflowSummary>, String> {
    let dir = workspace_root.join(".velocity").join("browser-workflows");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read workflow dir: {err}"))? {
        let entry: fs::DirEntry = match entry {
            Ok(e) => e,
            Err(err) => return Err(format!("read workflow dir entry: {err}")),
        };
        let path = entry.path();
        if path.extension().and_then(|ext: &std::ffi::OsStr| ext.to_str()) != Some("json") {
            continue;
        }
        if path
            .file_name()
            .and_then(|name: &std::ffi::OsStr| name.to_str())
            .map(|name: &str| !name.ends_with(".browser.json"))
            .unwrap_or(true)
        {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read workflow: {err}"))?;
        let workflow: BrowserWorkflow =
            serde_json::from_slice(&raw).map_err(|err| format!("parse workflow: {err}"))?;
        let mut summary = summarize_workflow(workflow);
        summary.json_path = Some(path.display().to_string());
        summary.nda_path = Some(
            browser_workflow_nda_path(workspace_root, &summary.name)
                .display()
                .to_string(),
        );
        if workflow_name_contains
            .map(|needle| contains_case_insensitive(&summary.name, needle))
            .unwrap_or(true)
            && start_url_contains
                .map(|needle| contains_case_insensitive(&summary.start_url, needle))
                .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.name.cmp(&right.name)
    });
    Ok(items)
}

pub fn render_workflow_suite_save_report(report: &BrowserWorkflowSuiteSaveReport) -> String {
    format!(
        "Saved browser workflow suite '{}'\nJSON: {}",
        report.suite.name, report.json_path,
    )
}

pub fn save_workflow_suite_report(
    workspace_root: &Path,
    suite: &BrowserWorkflowSuite,
) -> Result<BrowserWorkflowSuiteSaveReport, String> {
    let path = browser_workflow_suite_json_path(workspace_root, &suite.name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create workflow suite dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(suite)
        .map_err(|err| format!("serialise workflow suite: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write workflow suite: {err}"))?;
    Ok(BrowserWorkflowSuiteSaveReport {
        suite: summarize_workflow_suite(suite.clone()),
        json_path: path.display().to_string(),
    })
}

pub fn save_workflow_suite(
    workspace_root: &Path,
    suite: &BrowserWorkflowSuite,
) -> Result<PathBuf, String> {
    let report = save_workflow_suite_report(workspace_root, suite)?;
    Ok(PathBuf::from(report.json_path))
}

pub fn load_workflow_suite(path: &Path) -> Result<BrowserWorkflowSuite, String> {
    let raw = fs::read(path).map_err(|err| format!("read workflow suite: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse workflow suite: {err}"))
}

pub fn read_workflow_suite_report(path: &Path) -> Result<BrowserWorkflowSuiteReadReport, String> {
    let suite = load_workflow_suite(path)?;
    Ok(BrowserWorkflowSuiteReadReport {
        suite: summarize_workflow_suite(suite),
        json_path: path.display().to_string(),
    })
}

pub fn list_workflow_suites(
    workspace_root: &Path,
    suite_name_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserWorkflowSuiteSummary>, String> {
    let dir = workspace_root.join(".velocity").join("browser-suites");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read workflow suite dir: {err}"))? {
        let entry: fs::DirEntry = match entry {
            Ok(e) => e,
            Err(err) => return Err(format!("read workflow suite dir entry: {err}")),
        };
        let path = entry.path();
        if path.extension().and_then(|ext: &std::ffi::OsStr| ext.to_str()) != Some("json") {
            continue;
        }
        if path
            .file_name()
            .and_then(|name: &std::ffi::OsStr| name.to_str())
            .map(|name: &str| !name.ends_with(".suite.json"))
            .unwrap_or(true)
        {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read workflow suite: {err}"))?;
        let suite: BrowserWorkflowSuite =
            serde_json::from_slice(&raw).map_err(|err| format!("parse workflow suite: {err}"))?;
        let mut summary = summarize_workflow_suite(suite);
        summary.json_path = Some(path.display().to_string());
        if suite_name_contains
            .map(|needle| contains_case_insensitive(&summary.name, needle))
            .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.name.cmp(&right.name)
    });
    Ok(items)
}

pub fn read_workflow_run(
    workspace_root: &Path,
    workflow_name: &str,
    session_id: &str,
) -> Result<BrowserWorkflowRunReport, String> {
    let path = browser_workflow_run_path(workspace_root, workflow_name, session_id);
    let raw = fs::read(&path).map_err(|err| format!("read browser run report: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse browser run report: {err}"))
}

pub fn read_workflow_run_report(
    workspace_root: &Path,
    workflow_name: &str,
    session_id: &str,
) -> Result<BrowserWorkflowRunReadReport, String> {
    let report = read_workflow_run(workspace_root, workflow_name, session_id)?;
    Ok(BrowserWorkflowRunReadReport {
        workflow: summarize_workflow_run(report),
        run_report_path: browser_workflow_run_path(workspace_root, workflow_name, session_id)
            .display()
            .to_string(),
    })
}

pub fn list_workflow_runs(
    workspace_root: &Path,
    workflow_name_contains: Option<&str>,
    session_id_contains: Option<&str>,
    final_url_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserWorkflowRunSummary>, String> {
    let dir = workspace_root.join(".velocity").join("browser-runs");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read browser run dir: {err}"))? {
        let entry: fs::DirEntry = match entry {
            Ok(e) => e,
            Err(err) => return Err(format!("read browser run dir entry: {err}")),
        };
        let path = entry.path();
        if path.extension().and_then(|ext: &std::ffi::OsStr| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read browser run report: {err}"))?;
        let report: BrowserWorkflowRunReport = serde_json::from_slice(&raw)
            .map_err(|err| format!("parse browser run report: {err}"))?;
        let mut summary = summarize_workflow_run(report);
        summary.run_report_path = Some(path.display().to_string());
        if workflow_name_contains
            .map(|needle| contains_case_insensitive(&summary.workflow_name, needle))
            .unwrap_or(true)
            && session_id_contains
                .map(|needle| contains_case_insensitive(&summary.session_id, needle))
                .unwrap_or(true)
            && final_url_contains
                .map(|needle| contains_case_insensitive(&summary.final_url, needle))
                .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.workflow_name
            .cmp(&right.workflow_name)
            .then(left.session_id.cmp(&right.session_id))
    });
    Ok(items)
}

pub fn read_workflow_suite_run(
    workspace_root: &Path,
    suite_name: &str,
) -> Result<BrowserWorkflowSuiteRunReport, String> {
    let path = browser_workflow_suite_run_path(workspace_root, suite_name);
    let raw = fs::read(&path).map_err(|err| format!("read browser suite run report: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse browser suite run report: {err}"))
}

pub fn read_workflow_suite_run_report(
    workspace_root: &Path,
    suite_name: &str,
) -> Result<BrowserWorkflowSuiteRunReadReport, String> {
    let report = read_workflow_suite_run(workspace_root, suite_name)?;
    Ok(BrowserWorkflowSuiteRunReadReport {
        suite: summarize_workflow_suite_run(report),
        suite_report_path: browser_workflow_suite_run_path(workspace_root, suite_name)
            .display()
            .to_string(),
    })
}

pub fn list_workflow_suite_runs(
    workspace_root: &Path,
    suite_name_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserWorkflowSuiteRunSummary>, String> {
    let dir = workspace_root.join(".velocity").join("browser-suite-runs");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read browser suite run dir: {err}"))? {
        let entry: fs::DirEntry = match entry {
            Ok(e) => e,
            Err(err) => return Err(format!("read browser suite run dir entry: {err}")),
        };
        let path = entry.path();
        if path.extension().and_then(|ext: &std::ffi::OsStr| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read browser suite run report: {err}"))?;
        let report: BrowserWorkflowSuiteRunReport = serde_json::from_slice(&raw)
            .map_err(|err| format!("parse browser suite run report: {err}"))?;
        let mut summary = summarize_workflow_suite_run(report);
        summary.suite_report_path = Some(path.display().to_string());
        if suite_name_contains
            .map(|needle| contains_case_insensitive(&summary.suite_name, needle))
            .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.suite_name.cmp(&right.suite_name)
    });
    Ok(items)
}

