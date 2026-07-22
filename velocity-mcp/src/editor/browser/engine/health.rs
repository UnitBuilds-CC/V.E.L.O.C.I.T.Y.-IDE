use super::*;
use std::fs;
use std::path::Path;
use std::time::SystemTime;


fn html_contains_compatibility_marker(html: &str, needles: &[&str]) -> bool {
    contains_any_case_insensitive(html, needles)
}

fn html_count_case_insensitive(html: &str, needle: &str) -> usize {
    html.to_ascii_lowercase()
        .matches(&needle.to_ascii_lowercase())
        .count()
}

fn build_compatibility_report(
    snapshot: Option<&BrowserPageSnapshot>,
    html_fallback: Option<&str>,
    access_diagnostics: &BrowserAccessDiagnosticsReport,
) -> BrowserCompatibilityReport {
    let mut signals = Vec::new();
    let mut script_count = 0usize;
    let mut canvas_count = 0usize;
    let mut spa_shell = false;
    let mut hydration_markers = false;
    let html_mentions_webgl = html_fallback
        .map(|html| {
            contains_any_case_insensitive(html, &["webgl", "webgpu", "three.js", "babylon", "pixi"])
        })
        .unwrap_or(false);
    let html_mentions_device_features = html_fallback
        .map(|html| {
            contains_any_case_insensitive(
                html,
                &[
                    "navigator.webdriver",
                    "deviceorientation",
                    "pointerlock",
                    "getusermedia",
                    "webauthn",
                    "passkey",
                ],
            )
        })
        .unwrap_or(false);

    if let Some(html) = html_fallback {
        script_count = html_count_case_insensitive(html, "<script");
        canvas_count = html_count_case_insensitive(html, "<canvas");
        spa_shell = html_contains_compatibility_marker(
            html,
            &[
                "id=\"app\"",
                "id='app'",
                "id=\"root\"",
                "id='root'",
                "id=\"__next\"",
                "id='__next'",
                "data-reactroot",
                "ng-version",
            ],
        );
        hydration_markers = html_contains_compatibility_marker(
            html,
            &[
                "hydrate",
                "hydration",
                "hydrateroot",
                "__nuxt",
                "webpack",
                "vite",
                "svelte",
            ],
        );
        signals.push(format!("html:script_tags={script_count}"));
        if canvas_count > 0 {
            signals.push(format!("html:canvas_tags={canvas_count}"));
        }
        if spa_shell {
            signals.push("html:spa_shell".to_string());
        }
        if hydration_markers {
            signals.push("html:hydration_markers".to_string());
        }
        if html_mentions_webgl {
            signals.push("html:webgl_markers".to_string());
        }
        if html_mentions_device_features {
            signals.push("html:device_feature_markers".to_string());
        }
    } else {
        signals.push("html:fallback_missing".to_string());
    }

    let challenge_blocked = access_diagnostics.diagnosis != "clear";
    let anti_bot_limited = matches!(
        access_diagnostics.diagnosis.as_str(),
        "captcha_required" | "anti_bot_challenge" | "rate_limited" | "access_blocked"
    );

    let (level, cause, summary, recommended_action) = match snapshot {
        Some(snapshot) => {
            let actionable_count = snapshot
                .elements
                .iter()
                .filter(|element| describe_element_actionability(element).actionable)
                .count();
            let semantic_element_count = snapshot.elements.len();
            let form_count = snapshot.forms.len();
            let field_count = snapshot
                .forms
                .iter()
                .map(|form| form.fields.len())
                .sum::<usize>();
            let runtime_state_count = snapshot.runtime_state.len();
            let network_summary = summarize_network_activity(&snapshot.protocol_events);
            let live_runtime_count =
                network_summary.event_stream_count + network_summary.websocket_count;
            let runtime_heavy = script_count >= 6
                || spa_shell
                || hydration_markers
                || runtime_state_count >= 4
                || live_runtime_count > 0;
            let canvas_only = (canvas_count > 0 || html_mentions_webgl)
                && semantic_element_count == 0
                && form_count == 0;
            let semantic_surface_missing =
                semantic_element_count == 0 && form_count == 0 && field_count == 0;
            let device_or_identity_limited = html_mentions_device_features || anti_bot_limited;

            signals.push(format!("snapshot:elements={semantic_element_count}"));
            signals.push(format!("snapshot:forms={form_count}"));
            signals.push(format!("snapshot:fields={field_count}"));
            signals.push(format!("snapshot:actionable={actionable_count}"));
            if runtime_state_count > 0 {
                signals.push(format!("runtime:state={runtime_state_count}"));
            }
            if live_runtime_count > 0 {
                signals.push(format!("runtime:live_channels={live_runtime_count}"));
            }
            let frame_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_session" && entry.key == "frame_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            let shadow_host_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_session" && entry.key == "shadow_host_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            let runtime_canvas_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_session" && entry.key == "canvas_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            let runtime_webgl_canvas_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_session" && entry.key == "webgl_canvas_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or_else(|| {
                    snapshot
                        .runtime_state
                        .iter()
                        .find(|entry| entry.scope == "runtime_canvas" && entry.key == "webgl_count")
                        .and_then(|entry| entry.value.parse::<usize>().ok())
                        .unwrap_or(0)
                });
            let runtime_canvas_evidence_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| {
                    entry.scope == "runtime_canvas" && entry.key == "runtime_evidence_count"
                })
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            let runtime_canvas_animated_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_canvas" && entry.key == "animated_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            let accessible_frame_count = snapshot
                .runtime_state
                .iter()
                .find(|entry| entry.scope == "runtime_frames" && entry.key == "accessible_count")
                .and_then(|entry| entry.value.parse::<usize>().ok())
                .unwrap_or(0);
            if frame_count > 0 {
                signals.push(format!("runtime:frames={frame_count}"));
                signals.push(format!(
                    "runtime:accessible_frames={accessible_frame_count}"
                ));
            }
            if shadow_host_count > 0 {
                signals.push(format!("runtime:shadow_hosts={shadow_host_count}"));
            }
            if runtime_canvas_count > 0 {
                signals.push(format!("runtime:canvases={runtime_canvas_count}"));
                signals.push(format!(
                    "runtime:webgl_canvases={runtime_webgl_canvas_count}"
                ));
                if runtime_canvas_evidence_count > 0 {
                    signals.push(format!(
                        "runtime:canvas_evidence={runtime_canvas_evidence_count}"
                    ));
                }
                if runtime_canvas_animated_count > 0 {
                    signals.push(format!(
                        "runtime:animated_canvases={runtime_canvas_animated_count}"
                    ));
                }
            }
            let runtime_canvas_only = runtime_canvas_count > 0 && semantic_surface_missing;
            let runtime_canvas_heavy = runtime_canvas_count > 0
                && (runtime_webgl_canvas_count > 0
                    || runtime_canvas_evidence_count > 0
                    || runtime_canvas_animated_count > 0);

            if challenge_blocked {
                (
                    "unsupported".to_string(),
                    "challenge_or_policy_block".to_string(),
                    format!(
                        "The current page is blocked by '{}', so the static browser engine cannot usefully continue.",
                        access_diagnostics.diagnosis
                    ),
                    access_diagnostics.recommended_action.clone(),
                )
            } else if canvas_only || runtime_canvas_only {
                (
                    "unsupported".to_string(),
                    "canvas_or_webgl_surface".to_string(),
                    "The page appears canvas- or WebGL-driven without a usable semantic surface, which the current static browser engine cannot operate reliably.".to_string(),
                    "Escalate to a richer browser/runtime with canvas or WebGL understanding, or use a site path that exposes ordinary semantic controls instead of a drawn surface.".to_string(),
                )
            } else if runtime_canvas_heavy {
                (
                    "runtime_limited".to_string(),
                    "canvas_runtime_surface".to_string(),
                    "Runtime capture found active canvas or WebGL surfaces, but the current semantic snapshot still may not expose the underlying controls reliably.".to_string(),
                    "Prefer runtime-backed capture and verification for the needed flow, and treat canvas or WebGL evidence as a sign to verify each interaction outcome instead of assuming the rendered surface is fully represented semantically.".to_string(),
                )
            } else if semantic_surface_missing && runtime_heavy {
                (
                    "unsupported".to_string(),
                    "runtime_only_surface".to_string(),
                    "The page looks runtime-driven but exposes no usable semantic controls in the persisted snapshot, so it is effectively unsupported by the current static engine.".to_string(),
                    "Escalate to a browser/runtime with real JS execution, or capture the same workflow through a server-rendered or less dynamic route if one exists.".to_string(),
                )
            } else if frame_count > 0 && accessible_frame_count < frame_count {
                (
                    "runtime_limited".to_string(),
                    "cross_origin_embeds".to_string(),
                    "The page includes embedded frames that are not all same-origin or script-accessible, so the current browser evidence can only partially inspect the full surface.".to_string(),
                    "Prefer same-origin routes or richer runtime flows that can explicitly operate the embedded experience; treat inaccessible frames as a hard limit instead of assuming their controls are available.".to_string(),
                )
            } else if shadow_host_count > 0 && semantic_surface_missing {
                (
                    "runtime_limited".to_string(),
                    "shadow_dom_surface".to_string(),
                    "The page appears to rely on shadow-DOM components, and the persisted snapshot surface may still be incomplete even though runtime discovery found shadow hosts.".to_string(),
                    "Use runtime-backed capture/action flows for the current page and verify the needed controls become visible before proceeding; do not assume hidden shadow content is already reflected in the persisted snapshot.".to_string(),
                )
            } else if device_or_identity_limited && runtime_heavy {
                (
                    "runtime_limited".to_string(),
                    "device_or_identity_expectations".to_string(),
                    "The page exposes some semantic structure, but its runtime/device expectations suggest only partial support in the current static browser engine.".to_string(),
                    "Try the visible semantic controls that already exist, but expect degraded support; if progress depends on anti-bot checks, passkeys, media capture, or device APIs, move to a richer browser/runtime.".to_string(),
                )
            } else if runtime_heavy {
                (
                    "runtime_limited".to_string(),
                    "spa_or_live_runtime".to_string(),
                    "The page exposes some semantic structure, but runtime-heavy markers suggest only partial support in the current static browser engine.".to_string(),
                    "Proceed only with currently visible semantic controls, save checkpoints aggressively, and escalate to a richer browser/runtime if the flow depends on JS-driven state transitions or live app updates.".to_string(),
                )
            } else if semantic_element_count > 0 || form_count > 0 || actionable_count > 0 {
                (
                    "supported".to_string(),
                    "semantic_static_surface".to_string(),
                    "The persisted snapshot looks compatible with the current static browser engine.".to_string(),
                    "Proceed with semantic browser actions against the current snapshot and use checkpoints/transcripts for recovery if needed.".to_string(),
                )
            } else {
                (
                    "runtime_limited".to_string(),
                    "sparse_semantic_surface".to_string(),
                    "The page lacks enough semantic structure to confirm reliable support in the current static browser engine.".to_string(),
                    "Refresh or navigate to a more semantic page state if possible; otherwise inspect the HTML fallback and escalate if the required control never appears as a semantic element.".to_string(),
                )
            }
        }
        None => {
            if challenge_blocked {
                (
                    "unsupported".to_string(),
                    "challenge_or_policy_block".to_string(),
                    format!(
                        "No usable snapshot is available and the current page is blocked by '{}'.",
                        access_diagnostics.diagnosis
                    ),
                    access_diagnostics.recommended_action.clone(),
                )
            } else if script_count >= 3
                || canvas_count > 0
                || spa_shell
                || hydration_markers
                || html_mentions_webgl
            {
                (
                    "unsupported".to_string(),
                    "html_only_runtime_surface".to_string(),
                    "Only raw HTML fallback is available, and it looks runtime-, canvas-, or WebGL-driven beyond the current static browser engine.".to_string(),
                    "Escalate to a richer browser/runtime or capture the flow through a server-rendered route; the current engine cannot confirm or operate the needed controls from HTML fallback alone.".to_string(),
                )
            } else if html_fallback.is_some() {
                (
                    "runtime_limited".to_string(),
                    "html_only_without_snapshot".to_string(),
                    "Only raw HTML fallback is available, so compatibility remains limited until a semantic snapshot is captured.".to_string(),
                    "Refresh or re-navigate to rebuild a semantic snapshot before continuing; rely on HTML fallback only for inspection, not as proof that interactions are supported.".to_string(),
                )
            } else {
                (
                    "runtime_limited".to_string(),
                    "missing_browser_evidence".to_string(),
                    "No persisted snapshot or HTML fallback is available to confirm browser compatibility.".to_string(),
                    "Navigate or refresh the session to collect fresh browser evidence before attempting more actions.".to_string(),
                )
            }
        }
    };

    signals.sort();
    signals.dedup();
    BrowserCompatibilityReport {
        level,
        cause,
        summary,
        recommended_action,
        signal_count: signals.len(),
        signals,
    }
}

fn latest_session_checkpoint_summary(
    workspace_root: &Path,
    session_id: &str,
) -> Result<(usize, Option<BrowserSessionCheckpointSummary>), String> {
    let dir = workspace_root
        .join(".velocity")
        .join("browser-session-checkpoints")
        .join(sanitize_file_stem(session_id));
    if !dir.exists() {
        return Ok((0, None));
    }

    let mut checkpoint_count = 0usize;
    let mut latest: Option<(Option<SystemTime>, String, BrowserSessionCheckpointSummary)> = None;
    for entry in fs::read_dir(&dir).map_err(|err| format!("read checkpoint dir: {err}"))? {
        let entry: fs::DirEntry = match entry {
            Ok(e) => e,
            Err(err) => return Err(format!("read checkpoint dir entry: {err}")),
        };
        let path = entry.path();
        if path.extension().and_then(|ext: &std::ffi::OsStr| ext.to_str()) != Some("json") {
            continue;
        }
        checkpoint_count += 1;
        let raw = fs::read(&path).map_err(|err| format!("read browser checkpoint: {err}"))?;
        let checkpoint: BrowserSessionCheckpoint = serde_json::from_slice(&raw)
            .map_err(|err| format!("parse browser checkpoint: {err}"))?;
        let mut summary = summarize_session_checkpoint(checkpoint);
        let path_string = path.display().to_string();
        summary.checkpoint_json_path = Some(path_string.clone());
        let modified: Option<SystemTime> = entry
            .metadata()
            .ok()
            .and_then(|metadata: fs::Metadata| metadata.modified().ok());
        let replace = match latest.as_ref() {
            None => true,
            Some((best_modified, best_path, _)) => {
                let current_mod: Option<SystemTime> = modified;
                let best_mod: Option<SystemTime> = *best_modified;
                match (current_mod, best_mod) {
                    (Some(current), Some(best)) => {
                        current > best || (current == best && path_string > *best_path)
                    }
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => path_string > *best_path,
                }
            }
        };
        if replace {
            latest = Some((modified, path_string, summary));
        }
    }

    Ok((checkpoint_count, latest.map(|(_, _, summary)| summary)))
}

pub fn session_health_report(
    workspace_root: &Path,
    session_id: &str,
    sitemap_path: &Path,
) -> Result<BrowserSessionHealthReport, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    normalize_network_config(&mut session.network);
    let snapshot = match session.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    let auth_diagnostics = build_auth_diagnostics_report(
        workspace_root,
        session.clone(),
        snapshot.clone(),
        snapshot_json_path.clone(),
    );
    let access_diagnostics = build_access_diagnostics_report(
        workspace_root,
        session.clone(),
        snapshot.clone(),
        snapshot_json_path.clone(),
    );
    let snapshot_summary = snapshot.as_ref().map(|snapshot: &BrowserPageSnapshot| {
        let mut summary = summarize_snapshot(snapshot.clone());
        summary.json_path = snapshot_json_path.clone();
        summary
    });
    let html_fallback_path = session.current_url.as_deref().and_then(|url| {
        let path = browser_html_fallback_path(url, sitemap_path);
        path.exists().then(|| path.display().to_string())
    });
    let html_fallback = session
        .current_url
        .as_deref()
        .and_then(|url| load_html_fallback(url, sitemap_path).ok());
    let compatibility = build_compatibility_report(
        snapshot.as_ref(),
        html_fallback.as_deref(),
        &access_diagnostics,
    );
    let (checkpoint_count, latest_checkpoint): (usize, Option<BrowserSessionCheckpointSummary>) =
        latest_session_checkpoint_summary(workspace_root, session_id)?;
    let session_json_path = session_file_path(workspace_root, session_id)
        .display()
        .to_string();
    let mut session_summary = summarize_session(session.clone());
    session_summary.session_json_path = Some(session_json_path.clone());
    let recent_failures = recent_failed_session_transcript_entries(workspace_root, session_id, 3)?;
    let recent_failure_count = recent_failures.len();
    let latest_failure = recent_failures.first().cloned();

    let failure_recovery = latest_failure.as_ref().map(|failure| {
        let posture = match failure.event_kind.as_str() {
            "save_checkpoint" | "restore_checkpoint" => "recover_checkpoint",
            "navigate" => "recover_navigation",
            "wait" => "recover_wait",
            "click" | "fill_field" | "submit_form" => "recover_interaction",
            _ => "investigate",
        }
        .to_string();
        let action = match failure.event_kind.as_str() {
            "save_checkpoint" => "Retry saving the checkpoint or choose a new checkpoint name after confirming the session state is still available.".to_string(),
            "restore_checkpoint" => "Retry restoring the checkpoint or inspect the saved checkpoint artifact before continuing.".to_string(),
            "navigate" => "Retry navigation with the current network/auth settings, or restore a known-good checkpoint before proceeding.".to_string(),
            "wait" => "Refresh the session state or navigate again before retrying the wait condition with updated evidence.".to_string(),
            "click" | "fill_field" | "submit_form" => "Inspect the current snapshot or HTML fallback, then retry the interaction against visible elements or restore a stable checkpoint.".to_string(),
            _ => format!("Investigate the latest browser failure before continuing: {}", failure.summary),
        };
        (posture, action)
    });

    let (recovery_posture, recommended_action) = if access_diagnostics.diagnosis != "clear" {
        (
            "blocked".to_string(),
            access_diagnostics.recommended_action.clone(),
        )
    } else if matches!(
        auth_diagnostics.diagnosis.as_str(),
        "session_expired" | "csrf_missing" | "login_required"
    ) {
        (
            "recover_auth".to_string(),
            auth_diagnostics.recommended_action.clone(),
        )
    } else if session.current_url.is_none() {
        (
            "seed_session".to_string(),
            "Navigate the session to the target page or apply a saved auth profile before continuing.".to_string(),
        )
    } else if compatibility.level == "unsupported" {
        (
            "unsupported_site".to_string(),
            compatibility.recommended_action.clone(),
        )
    } else if snapshot_summary.is_none() && session.current_url.is_some() {
        (
            "recover_snapshot".to_string(),
            "Refresh the current URL or restore a checkpoint to rebuild persisted page evidence before continuing.".to_string(),
        )
    } else if let Some((posture, action)) = failure_recovery {
        (posture, action)
    } else if compatibility.level == "runtime_limited" {
        (
            "runtime_limited".to_string(),
            compatibility.recommended_action.clone(),
        )
    } else if auth_diagnostics.diagnosis == "auth_ready" {
        (
            "ready".to_string(),
            auth_diagnostics.recommended_action.clone(),
        )
    } else {
        (
            "investigate".to_string(),
            auth_diagnostics.recommended_action.clone(),
        )
    };

    let mut evidence_signals = vec![
        format!("auth:{}", auth_diagnostics.diagnosis),
        format!("access:{}", access_diagnostics.diagnosis),
        format!("compatibility:{}", compatibility.level),
        format!("checkpoints:{}", checkpoint_count),
    ];
    evidence_signals.push(if session.current_url.is_some() {
        "session:url_present".to_string()
    } else {
        "session:url_missing".to_string()
    });
    evidence_signals.push(if snapshot_summary.is_some() {
        "snapshot:available".to_string()
    } else {
        "snapshot:missing".to_string()
    });
    if html_fallback_path.is_some() {
        evidence_signals.push("html_fallback:available".to_string());
    }
    if session.network.user_agent.is_some() {
        evidence_signals.push("network:user_agent_override".to_string());
    }
    if !session.network.headers.is_empty() {
        evidence_signals.push(format!("network:headers={}", session.network.headers.len()));
    }
    if !session.network.allowed_url_prefixes.is_empty()
        || !session.network.blocked_url_prefixes.is_empty()
    {
        evidence_signals.push("network:policy".to_string());
    }
    if let Some(checkpoint) = latest_checkpoint.as_ref() {
        evidence_signals.push(format!("checkpoint:latest={}", checkpoint.name));
    }
    if recent_failure_count > 0 {
        evidence_signals.push(format!("transcript:failures={}", recent_failure_count));
    }
    if let Some(latest_failure) = latest_failure.as_ref() {
        evidence_signals.push(format!(
            "transcript:latest_failure_kind={}",
            latest_failure.event_kind
        ));
    }
    evidence_signals.extend(compatibility.signals.iter().cloned());
    evidence_signals.sort();
    evidence_signals.dedup();
    let evidence_signal_count = evidence_signals.len();

    Ok(BrowserSessionHealthReport {
        session: session_summary,
        network: session.network,
        auth_diagnostics,
        access_diagnostics,
        compatibility,
        snapshot: snapshot_summary,
        html_fallback_path,
        checkpoint_count,
        latest_checkpoint,
        recent_failure_count,
        recent_failures,
        latest_failure,
        recovery_posture,
        recommended_action,
        evidence_signal_count,
        evidence_signals,
        session_json_path,
    })
}

