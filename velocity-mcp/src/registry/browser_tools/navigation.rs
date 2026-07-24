use serde_json::Value;
use std::error::Error;
use std::path::Path;

pub fn handle_navigation_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    let result = match name {
        "web_navigate" => {
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report =
                    crate::editor::browser::crawl_and_sync_sitemap_report(url, &sitemap_path)
                        .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser crawl summary: {err}")))?
            } else {
                crate::editor::browser::crawl_and_sync_sitemap(url, &sitemap_path)
                    .map_err(Box::<dyn Error>::from)?
            }
        }
        "browser_runtime_capture" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let timeout_ms = arguments["timeoutMs"].as_u64().unwrap_or(15_000);
            let api_base = arguments["apiBase"].as_str();
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::runtime_capture_report(
                    root,
                    session_id,
                    url,
                    timeout_ms,
                    api_base,
                    &sitemap_path,
                )
                .map_err(Box::<dyn Error>::from)?;
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser runtime capture summary: {err}"))
                })?
            } else {
                crate::editor::browser::runtime_capture(
                    root,
                    session_id,
                    url,
                    timeout_ms,
                    api_base,
                    &sitemap_path,
                )
                .map_err(Box::<dyn Error>::from)?
            }
        }
        "browser_runtime_visual_capture" => crate::editor::browser::browser_runtime_visual_capture(
            root,
            arguments["url"].as_str().ok_or("url is required")?,
            arguments["apiBase"].as_str(),
            arguments["compact"].as_bool().unwrap_or(false),
        )
        .map_err(Box::<dyn Error>::from)?,
        "runtime_create_session" => crate::editor::browser::create_runtime_session(
            root,
            arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?,
            arguments["startUrl"].as_str(),
            arguments["waitTimeoutMs"].as_u64(),
            arguments["apiBase"].as_str(),
            arguments["compact"].as_bool().unwrap_or(false),
        )
        .map_err(Box::<dyn Error>::from)?,
        "runtime_get_session" => crate::editor::browser::get_runtime_session(
            root,
            arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?,
            arguments["compact"].as_bool().unwrap_or(false),
        )
        .map_err(Box::<dyn Error>::from)?,
        "runtime_close_session" => crate::editor::browser::close_runtime_session(
            root,
            arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?,
            arguments["compact"].as_bool().unwrap_or(false),
        )
        .map_err(Box::<dyn Error>::from)?,
        "runtime_capture_session" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::capture_runtime_session(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(Box::<dyn Error>::from)?
        }
        "runtime_session_navigate" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_navigate_session(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["url"].as_str().ok_or("url is required")?,
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(Box::<dyn Error>::from)?
        }
        "runtime_session_click" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_click_session(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["nodeId"].as_str(),
                arguments["selector"].as_str(),
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(Box::<dyn Error>::from)?
        }
        "runtime_session_js_click" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_js_click_session(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["nodeId"].as_str().ok_or("nodeId is required")?,
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(Box::<dyn Error>::from)?
        }
        "runtime_session_evaluate" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_evaluate_session(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["script"].as_str().ok_or("script is required")?,
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(Box::<dyn Error>::from)?
        }
        "runtime_session_fill" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_fill_session(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["nodeId"].as_str(),
                arguments["selector"].as_str(),
                arguments["value"].as_str().ok_or("value is required")?,
                arguments["natural"].as_bool().unwrap_or(false),
                arguments["clear"].as_bool().unwrap_or(false),
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(Box::<dyn Error>::from)?
        }
        "runtime_session_submit" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_submit_session(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["nodeId"].as_str(),
                arguments["selector"].as_str(),
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(Box::<dyn Error>::from)?
        }
        "runtime_session_press_key" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_press_key_session(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["key"].as_str().ok_or("key is required")?,
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(Box::<dyn Error>::from)?
        }
        "runtime_reseed_auth" => {
            let target_session_id = arguments["targetSessionId"]
                .as_str()
                .ok_or("targetSessionId is required")?;
            let source_session_id = arguments["sourceSessionId"]
                .as_str()
                .ok_or("sourceSessionId is required")?;
            let source_checkpoint_name = arguments["sourceCheckpointName"].as_str();
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::reseed_runtime_auth_state_report(
                root,
                target_session_id,
                source_session_id,
                source_checkpoint_name,
                &sitemap_path,
                arguments["waitTimeoutMs"].as_u64(),
            )
            .map_err(Box::<dyn Error>::from)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise runtime auth reseed report: {err}"))
                })?
            } else {
                crate::editor::browser::render_runtime_auth_reseed_report(
                    &report,
                )
            }
        }
        _ => return Ok(None),
    };

    Ok(Some(result))
}
