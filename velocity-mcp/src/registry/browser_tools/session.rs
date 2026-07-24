use serde_json::Value;
use std::error::Error;
use std::path::Path;

pub fn handle_session_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    let result = match name {
        "browser_create_session" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let report = crate::editor::browser::create_session_report(root, session_id)
                .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser session creation summary: {err}"))
                })?
            } else {
                crate::editor::browser::render_session_create_report(&report)
            }
        }
        "browser_get_session" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let session = crate::editor::browser::load_session_state(root, session_id)
                .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_session_report(root, session_id)
                    .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser session summary: {err}")))?
            } else {
                crate::editor::browser::session_state_to_json(&session).map_err(Box::<dyn Error>::from)?
            }
        }
        "browser_list_snapshots" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(Box::<dyn Error>::from)?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let snapshots = crate::editor::browser::list_snapshots(
                &sitemap_path,
                arguments["urlContains"].as_str(),
                arguments["titleContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(Box::<dyn Error>::from)?;
            serde_json::to_string_pretty(&snapshots)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise browser snapshots: {err}")))?
        }
        "browser_read_snapshot" => {
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let snapshot = crate::editor::browser::read_snapshot(url, &sitemap_path)
                .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_snapshot_report(url, &sitemap_path)
                    .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser snapshot summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&snapshot)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser snapshot: {err}")))?
            }
        }
        "browser_read_visual_fallback" => {
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report =
                    crate::editor::browser::read_visual_fallback_report(url, &sitemap_path)
                        .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser html fallback summary: {err}")))?
            } else {
                crate::editor::browser::read_visual_fallback(url, &sitemap_path)
                    .map_err(Box::<dyn Error>::from)?
            }
        }
        "browser_diff_snapshots" => {
            let before_url = arguments["beforeUrl"]
                .as_str()
                .ok_or("beforeUrl is required")?;
            let after_url = arguments["afterUrl"]
                .as_str()
                .ok_or("afterUrl is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report =
                crate::editor::browser::diff_saved_snapshots(before_url, after_url, &sitemap_path)
                    .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let compact = crate::editor::browser::read_snapshot_diff_report(
                    before_url,
                    after_url,
                    &sitemap_path,
                )
                .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&compact)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser snapshot diff summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser snapshot diff: {err}")))?
            }
        }
        "browser_list_sessions" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(Box::<dyn Error>::from)?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let sessions = crate::editor::browser::list_sessions(
                root,
                arguments["sessionIdContains"].as_str(),
                arguments["urlContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(Box::<dyn Error>::from)?;
            serde_json::to_string_pretty(&sessions)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise browser sessions: {err}")))?
        }
        "browser_get_storage" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let scope = arguments["scope"].as_str().ok_or("scope is required")?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::get_session_storage_entries_report(
                    root, session_id, scope,
                )
                .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser storage summary: {err}")))?
            } else {
                crate::editor::browser::get_session_storage_entries(root, session_id, scope)
                    .map_err(Box::<dyn Error>::from)?
            }
        }
        "browser_set_storage" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let scope = arguments["scope"].as_str().ok_or("scope is required")?;
            let entries_value = arguments["entries"]
                .as_object()
                .ok_or("entries is required")?;
            let mut entries = std::collections::HashMap::new();
            for (key, value) in entries_value {
                let value = value
                    .as_str()
                    .ok_or("storage entry values must be strings")?;
                entries.insert(key.clone(), value.to_string());
            }
            let report = crate::editor::browser::set_session_storage_entries_report(
                root, session_id, scope, &entries,
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser storage update summary: {err}"))
                })?
            } else {
                crate::editor::browser::render_storage_update_report(
                    &report,
                )
            }
        }
        "browser_get_cookies" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::get_session_cookies_report(root, session_id)
                    .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser cookie summary: {err}")))?
            } else {
                crate::editor::browser::get_session_cookies(root, session_id).map_err(Box::<dyn Error>::from)?
            }
        }
        "browser_set_cookies" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let cookies_value = arguments["cookies"]
                .as_array()
                .ok_or("cookies is required")?;
            let mut cookies = Vec::new();
            for cookie in cookies_value {
                let name = cookie["name"].as_str().ok_or("cookie name is required")?;
                let value = cookie["value"].as_str().ok_or("cookie value is required")?;
                cookies.push(crate::editor::browser::BrowserCookie {
                    name: name.to_string(),
                    value: value.to_string(),
                });
            }
            let report =
                crate::editor::browser::set_session_cookies_report(root, session_id, &cookies)
                    .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser cookie update summary: {err}")))?
            } else {
                crate::editor::browser::render_cookie_update_report(&report)
            }
        }
        "browser_auth_diagnostics" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report =
                crate::editor::browser::auth_diagnostics_report(root, session_id, &sitemap_path)
                    .map_err(Box::<dyn Error>::from)?;
            serde_json::to_string_pretty(&report)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise browser auth diagnostics: {err}")))?
        }
        "browser_save_auth_profile" => {
            let profile_name = arguments["profileName"]
                .as_str()
                .ok_or("profileName is required")?;
            let source_session_id = arguments["sourceSessionId"]
                .as_str()
                .ok_or("sourceSessionId is required")?;
            let source_checkpoint_name = arguments["sourceCheckpointName"].as_str();
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::save_auth_profile_report(
                root,
                profile_name,
                source_session_id,
                source_checkpoint_name,
                &sitemap_path,
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser auth profile save report: {err}"))
                })?
            } else {
                crate::editor::browser::render_auth_profile_save_report(
                    &report,
                )
            }
        }
        "browser_list_auth_profiles" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(Box::<dyn Error>::from)?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let profiles = crate::editor::browser::list_auth_profiles(
                root,
                arguments["profileNameContains"].as_str(),
                arguments["sourceSessionIdContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(Box::<dyn Error>::from)?;
            serde_json::to_string_pretty(&profiles)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise browser auth profiles: {err}")))?
        }
        "browser_read_auth_profile" => {
            let profile_name = arguments["profileName"]
                .as_str()
                .ok_or("profileName is required")?;
            let profile = crate::editor::browser::load_auth_profile(root, profile_name)
                .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_auth_profile_report(root, profile_name)
                    .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser auth profile summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&profile)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser auth profile: {err}")))?
            }
        }
        "browser_apply_auth_profile" => {
            let profile_name = arguments["profileName"]
                .as_str()
                .ok_or("profileName is required")?;
            let target_session_id = arguments["targetSessionId"]
                .as_str()
                .ok_or("targetSessionId is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::apply_auth_profile_report(
                root,
                profile_name,
                target_session_id,
                &sitemap_path,
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser auth profile apply report: {err}"))
                })?
            } else {
                crate::editor::browser::render_auth_profile_apply_report(
                    &report,
                )
            }
        }
        "browser_reseed_auth" => {
            let target_session_id = arguments["targetSessionId"]
                .as_str()
                .ok_or("targetSessionId is required")?;
            let source_session_id = arguments["sourceSessionId"]
                .as_str()
                .ok_or("sourceSessionId is required")?;
            let source_checkpoint_name = arguments["sourceCheckpointName"].as_str();
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::reseed_auth_state_report(
                root,
                target_session_id,
                source_session_id,
                source_checkpoint_name,
                &sitemap_path,
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser auth reseed report: {err}")))?
            } else {
                crate::editor::browser::render_auth_reseed_report(&report)
            }
        }
        "browser_access_diagnostics" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report =
                crate::editor::browser::access_diagnostics_report(root, session_id, &sitemap_path)
                    .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(true) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser access diagnostics: {err}")))?
            } else {
                crate::editor::browser::render_access_diagnostics_report(
                    &report,
                )
            }
        }
        "browser_get_session_network" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let report = crate::editor::browser::read_session_network_report(root, session_id)
                .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser session network report: {err}"))
                })?
            } else {
                crate::editor::browser::render_session_network_read_report(
                    &report,
                )
            }
        }
        "browser_read_session_transcript" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            if let Some(sequence) = arguments["sequence"].as_u64() {
                let entry = crate::editor::browser::read_session_transcript_entry(
                    root, session_id, sequence,
                )
                .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&entry).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser session transcript entry: {err}"))
                })?
            } else {
                let sort_direction = crate::editor::browser::parse_list_sort_direction(
                    arguments["sortDirection"].as_str(),
                )
                .map_err(Box::<dyn Error>::from)?;
                let limit = arguments["limit"].as_u64().map(|value| value as usize);
                let report = crate::editor::browser::read_session_transcript_report(
                    root,
                    session_id,
                    limit,
                    sort_direction,
                )
                .map_err(Box::<dyn Error>::from)?;
                if arguments["compact"].as_bool().unwrap_or(false) {
                    serde_json::to_string_pretty(&report).map_err(|err| {
                        Box::<dyn Error>::from(format!("serialise browser session transcript report: {err}"))
                    })?
                } else {
                    crate::editor::browser::render_session_transcript_report(
                        &report,
                    )
                }
            }
        }
        "browser_session_health" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report =
                crate::editor::browser::session_health_report(root, session_id, &sitemap_path)
                    .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser session health report: {err}")))?
            } else {
                crate::editor::browser::render_session_health_report(
                    &report,
                )
            }
        }
        "browser_get_trace_summary" => {
            let compact = arguments["compact"].as_bool().unwrap_or(false);
            crate::editor::browser::get_trace_summary(root, compact)
                .map_err(Box::<dyn Error>::from)?
        }
        "browser_get_trace_logs" => {
            let compact = arguments["compact"].as_bool().unwrap_or(false);
            crate::editor::browser::get_trace_logs(root, compact)
                .map_err(Box::<dyn Error>::from)?
        }
        "browser_set_session_network" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let headers = arguments["headers"].as_object().map(|entries| {
                entries
                    .iter()
                    .map(|(key, value)| {
                        (key.clone(), value.as_str().unwrap_or_default().to_string())
                    })
                    .collect::<std::collections::HashMap<_, _>>()
            });
            let allowed_url_prefixes = arguments["allowedUrlPrefixes"].as_array().map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(|value| value.to_string()))
                    .collect::<Vec<_>>()
            });
            let blocked_url_prefixes = arguments["blockedUrlPrefixes"].as_array().map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(|value| value.to_string()))
                    .collect::<Vec<_>>()
            });
            let report = crate::editor::browser::update_session_network_report(
                root,
                session_id,
                arguments["userAgent"].as_str(),
                headers,
                arguments["timeoutMs"].as_u64(),
                arguments["clearTimeout"].as_bool().unwrap_or(false),
                arguments["followRedirects"].as_bool(),
                arguments["clearFollowRedirects"].as_bool().unwrap_or(false),
                allowed_url_prefixes,
                blocked_url_prefixes,
                arguments["replaceHeaders"].as_bool().unwrap_or(false),
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser session network update: {err}"))
                })?
            } else {
                crate::editor::browser::render_session_network_update_report(&report)
            }
        }
        "browser_session_navigate" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::navigate_session_report(
                    root,
                    session_id,
                    url,
                    &sitemap_path,
                )
                .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser session navigation summary: {err}"))
                })?
            } else {
                crate::editor::browser::navigate_session(root, session_id, url, &sitemap_path)
                    .map_err(Box::<dyn Error>::from)?
            }
        }
        "browser_session_click" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let role = arguments["role"].as_str().ok_or("role is required")?;
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::session_click_report(
                root,
                session_id,
                role,
                name,
                &sitemap_path,
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser click summary: {err}")))?
            } else {
                crate::editor::browser::render_session_action_report(
                    &report,
                )
            }
        }
        "browser_session_fill" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let field = arguments["field"].as_str().ok_or("field is required")?;
            let value = arguments["value"].as_str().ok_or("value is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::session_fill_report(
                root,
                session_id,
                field,
                value,
                &sitemap_path,
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser fill summary: {err}")))?
            } else {
                crate::editor::browser::render_session_action_report(
                    &report,
                )
            }
        }
        "browser_session_submit" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::session_submit_report(
                root,
                session_id,
                arguments["form"].as_str(),
                &sitemap_path,
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser submit summary: {err}")))?
            } else {
                crate::editor::browser::render_session_action_report(
                    &report,
                )
            }
        }
        "browser_session_wait" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let text = arguments["text"].as_str();
            let title = arguments["title"].as_str();
            let url_contains = arguments["urlContains"].as_str();
            let mutation = arguments["mutation"].as_str();
            let request_method = arguments["requestMethod"].as_str();
            let request_url_contains = arguments["requestUrlContains"].as_str();
            let request_status = arguments["requestStatus"]
                .as_u64()
                .map(|value| value as u16);
            let request_resource = arguments["requestResource"].as_str();
            let storage_scope = arguments["storageScope"].as_str();
            let storage_key = arguments["storageKey"].as_str();
            let storage_value = arguments["storageValue"].as_str();
            let settle = arguments["settle"].as_str();
            let settle_scope = arguments["settleScope"].as_str();
            let settle_state = arguments["settleState"].as_str();
            let runtime_scope = arguments["runtimeScope"].as_str();
            let runtime_key = arguments["runtimeKey"].as_str();
            let runtime_value = arguments["runtimeValue"].as_str();
            let protocol_kind = arguments["protocolKind"].as_str();
            let protocol_phase = arguments["protocolPhase"].as_str();
            let protocol_target = arguments["protocolTarget"].as_str();
            let protocol_detail = arguments["protocolDetail"].as_str();
            let network_idle = arguments["networkIdle"].as_bool().unwrap_or(false);
            let app_ready = arguments["appReady"].as_bool().unwrap_or(false);
            let mutation_settled = arguments["mutationSettled"].as_bool().unwrap_or(false);
            let stream_complete = arguments["streamComplete"].as_bool().unwrap_or(false);
            let role = arguments["role"].as_str();
            let name = arguments["name"].as_str();
            let require_actionable = arguments["requireActionable"].as_bool().unwrap_or(false);
            let stable_polls = arguments["stablePolls"].as_u64().map(|value| value as u32);
            let timeout_ms = arguments["timeoutMs"].as_u64();
            let interval_ms = arguments["intervalMs"].as_u64();
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::wait_for_session_report(
                    root,
                    session_id,
                    text,
                    title,
                    url_contains,
                    mutation,
                    request_method,
                    request_url_contains,
                    request_status,
                    request_resource,
                    storage_scope,
                    storage_key,
                    storage_value,
                    settle,
                    settle_scope,
                    settle_state,
                    runtime_scope,
                    runtime_key,
                    runtime_value,
                    protocol_kind,
                    protocol_phase,
                    protocol_target,
                    protocol_detail,
                    network_idle,
                    app_ready,
                    mutation_settled,
                    stream_complete,
                    role,
                    name,
                    require_actionable,
                    stable_polls,
                    timeout_ms,
                    interval_ms,
                    &sitemap_path,
                )
                .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser session wait summary: {err}")))?
            } else {
                crate::editor::browser::wait_for_session(
                    root,
                    session_id,
                    text,
                    title,
                    url_contains,
                    mutation,
                    request_method,
                    request_url_contains,
                    request_status,
                    request_resource,
                    storage_scope,
                    storage_key,
                    storage_value,
                    settle,
                    settle_scope,
                    settle_state,
                    runtime_scope,
                    runtime_key,
                    runtime_value,
                    protocol_kind,
                    protocol_phase,
                    protocol_target,
                    protocol_detail,
                    network_idle,
                    app_ready,
                    mutation_settled,
                    stream_complete,
                    role,
                    name,
                    require_actionable,
                    stable_polls,
                    timeout_ms,
                    interval_ms,
                    &sitemap_path,
                )
                .map_err(Box::<dyn Error>::from)?
            }
        }
        "browser_save_checkpoint" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let checkpoint_name = arguments["checkpointName"]
                .as_str()
                .ok_or("checkpointName is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::save_session_checkpoint_report(
                root,
                session_id,
                checkpoint_name,
                &sitemap_path,
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser checkpoint save summary: {err}"))
                })?
            } else {
                crate::editor::browser::render_checkpoint_save_report(
                    &report,
                )
            }
        }
        "browser_restore_checkpoint" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let checkpoint_name = arguments["checkpointName"]
                .as_str()
                .ok_or("checkpointName is required")?;
            let target_session_id = arguments["targetSessionId"].as_str();
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::restore_session_checkpoint_report(
                    root,
                    session_id,
                    checkpoint_name,
                    target_session_id,
                    &sitemap_path,
                )
                .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser checkpoint restore summary: {err}"))
                })?
            } else {
                crate::editor::browser::restore_session_checkpoint(
                    root,
                    session_id,
                    checkpoint_name,
                    target_session_id,
                    &sitemap_path,
                )
                .map_err(Box::<dyn Error>::from)?
            }
        }
        "browser_list_checkpoints" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(Box::<dyn Error>::from)?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let checkpoints = crate::editor::browser::list_session_checkpoints(
                root,
                session_id,
                arguments["checkpointNameContains"].as_str(),
                arguments["titleContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(Box::<dyn Error>::from)?;
            serde_json::to_string_pretty(&checkpoints)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise checkpoint list: {err}")))?
        }
        "browser_read_checkpoint" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let checkpoint_name = arguments["checkpointName"]
                .as_str()
                .ok_or("checkpointName is required")?;
            let checkpoint =
                crate::editor::browser::read_session_checkpoint(root, session_id, checkpoint_name)
                    .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_session_checkpoint_report(
                    root,
                    session_id,
                    checkpoint_name,
                )
                .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise checkpoint summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&checkpoint)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise checkpoint: {err}")))?
            }
        }
        "browser_diff_checkpoints" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let before_checkpoint_name = arguments["beforeCheckpointName"]
                .as_str()
                .ok_or("beforeCheckpointName is required")?;
            let after_checkpoint_name = arguments["afterCheckpointName"]
                .as_str()
                .ok_or("afterCheckpointName is required")?;
            let report = crate::editor::browser::diff_session_checkpoints(
                root,
                session_id,
                before_checkpoint_name,
                after_checkpoint_name,
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let compact = crate::editor::browser::read_checkpoint_diff_report(
                    root,
                    session_id,
                    before_checkpoint_name,
                    after_checkpoint_name,
                )
                .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&compact)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise checkpoint diff summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise checkpoint diff: {err}")))?
            }
        }
        _ => return Ok(None),
    };

    Ok(Some(result))
}
