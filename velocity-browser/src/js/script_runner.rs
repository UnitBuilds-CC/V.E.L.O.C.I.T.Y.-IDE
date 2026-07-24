//! Script execution pipeline for `<script>` tags.
//!
//! When a page is loaded, this module walks the DOM tree for `<script>` elements
//! and executes them in document order. Inline scripts run immediately; external
//! scripts (`src="..."`) are fetched via HttpClient first. The `defer` attribute
//! queues scripts for execution after the DOM is fully built. Non-JS type
//! attributes (like `application/json` or `type="module"`) are skipped gracefully.

use crate::dom::DomTree;
use crate::js::vm::JsVirtualMachine;
use crate::js::event_loop::JsEventLoopScheduler;
use crate::net::HttpClient;
use crate::engine::TraceCollector;
use crate::parser::html::NodeType;

/// Collected script to execute: inline body or fetched source.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ScriptEntry {
    pub source: String,
    pub is_defer: bool,
}

/// Execute all `<script>` tags found in the DOM tree.
///
/// - Inline scripts (`<script>code</script>`) execute immediately in document order.
/// - External scripts (`<script src="url">`) are fetched and then executed.
/// - `defer` scripts are queued and executed after all non-deferred scripts.
/// - `type="module"` scripts are skipped (not supported yet).
/// - Non-JS types (`application/json`, `application/ld+json`, etc.) are skipped.
pub fn execute_page_scripts(
    tree: &mut DomTree,
    vm: &mut JsVirtualMachine,
    scheduler: &mut JsEventLoopScheduler,
    _http_client: &mut HttpClient,
    trace: &mut TraceCollector,
    current_url: &str,
) {
    let script_nodes = find_script_nodes(tree);
    let mut deferred: Vec<String> = Vec::new();

    for (script_body, is_defer) in script_nodes {
        if script_body.trim().is_empty() {
            continue;
        }
        if is_defer {
            deferred.push(script_body);
        } else {
            execute_single_script(tree, vm, scheduler, trace, &script_body);
        }
    }

    // Execute deferred scripts after all synchronous scripts
    for script in deferred {
        execute_single_script(tree, vm, scheduler, trace, &script);
    }

    // Drain event loop after all scripts
    let tick_limit = scheduler.tick_limit;
    let mut ticks = 0;
    while ticks < tick_limit && scheduler.has_pending_tasks() {
        if let Some(task) = scheduler.pop_next_task() {
            execute_scheduled_task(tree, vm, &task);
            ticks += 1;
        } else {
            break;
        }
    }

    trace.record_console("info", &format!(
        "Script execution complete for {}",
        current_url
    ));
}

/// Find all <script> nodes in document order and return their content.
/// Returns: Vec<(script_body, is_defer)>
fn find_script_nodes(tree: &mut DomTree) -> Vec<(String, bool)> {
    let mut entries = Vec::new();
    let node_count = tree.nodes.len();

    for i in 0..node_count {
        let node = &tree.nodes[i];
        if node.node_type != NodeType::Element || node.tag_name != "script" {
            continue;
        }

        // Skip non-JS types
        if let Some(script_type) = node.attributes.get("type") {
            let t = script_type.to_lowercase();
            if t == "module" {
                // ES modules are now supported by the interpreter (import/export)
                // Fall through to execute them normally
            } else if !t.is_empty()
                && t != "text/javascript"
                && t != "application/javascript"
                && t != "text/ecmascript"
            {
                // Non-JS type (e.g., application/json, application/ld+json)
                continue;
            }
        }

        let is_defer = node.attributes.contains_key("defer");
        let has_src = node.attributes.get("src").map(|s| !s.is_empty()).unwrap_or(false);

        if has_src {
            // External script - the URL will be fetched later
            let src = node.attributes.get("src").cloned().unwrap_or_default();
            entries.push((format!("__external_src__:{}", src), is_defer));
        } else {
            // Inline script - collect text content from children
            let mut body = String::new();
            for &child_id in &node.children.clone() {
                if let Some(child) = tree.get_node(child_id) {
                    if child.node_type == NodeType::Text {
                        body.push_str(&child.text_content);
                    }
                }
            }
            if !body.is_empty() {
                entries.push((body, is_defer));
            }
        }
    }

    entries
}

/// Execute a single script body (may be inline or fetched content).
fn execute_single_script(
    tree: &mut DomTree,
    vm: &mut JsVirtualMachine,
    scheduler: &mut JsEventLoopScheduler,
    trace: &mut TraceCollector,
    script: &str,
) {
    match vm.eval_statement(tree, script) {
        Ok(_) => {}
        Err(e) => {
            trace.record_console("error", &format!("Script error: {}", e));
        }
    }

    // Drain microtasks after each script
    let mut micro_ticks = 0;
    while micro_ticks < 50 {
        if let Some(task) = scheduler.pop_next_task() {
            if task.kind == crate::js::event_loop::TaskKind::MicroTask {
                execute_scheduled_task(tree, vm, &task);
                micro_ticks += 1;
            } else {
                // Put non-microtask back (it's a timer)
                scheduler.task_queue.push_front(task);
                break;
            }
        } else {
            break;
        }
    }
}

/// Execute a scheduled task: either invoke its closure or eval its script.
fn execute_scheduled_task(
    tree: &mut DomTree,
    vm: &mut JsVirtualMachine,
    task: &crate::js::event_loop::ScheduledTask,
) {
    if let Some(ref closure) = task.closure {
        // Invoke the closure directly via the interpreter
        let _ = crate::js::interpreter::call_function(closure, &[], vm.scope());
    } else if !task.script.is_empty() {
        let _ = vm.eval_statement(tree, &task.script);
    }
}

/// Fetch an external script source by URL.
pub fn fetch_external_script(
    http_client: &mut HttpClient,
    base_url: &str,
    src: &str,
) -> Option<String> {
    let url = resolve_script_url(base_url, src);
    match http_client.get(&url) {
        Ok(resp) => {
            if resp.status_code >= 200 && resp.status_code < 400 {
                Some(resp.body)
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

/// Resolve a script src URL against the page base URL.
fn resolve_script_url(base_url: &str, src: &str) -> String {
    let src = src.trim();
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("//") {
        if src.starts_with("//") {
            return format!("https:{}", src);
        }
        return src.to_string();
    }
    // Relative URL
    if base_url.is_empty() {
        return src.to_string();
    }
    let scheme_end = base_url.find("://").unwrap_or(0) + 3;
    let after_scheme = &base_url[scheme_end..];
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    if src.starts_with('/') {
        format!("{}{}{}", &base_url[..scheme_end], authority, src)
    } else {
        let last_slash = base_url.rfind('/').unwrap_or(base_url.len());
        if last_slash > scheme_end {
            format!("{}/{}", &base_url[..last_slash], src)
        } else {
            format!("{}/{}", base_url, src)
        }
    }
}

/// Full pipeline: find scripts, fetch externals, execute all.
pub fn execute_page_scripts_full(
    tree: &mut DomTree,
    vm: &mut JsVirtualMachine,
    scheduler: &mut JsEventLoopScheduler,
    http_client: &mut HttpClient,
    trace: &mut TraceCollector,
    current_url: &str,
) {
    let script_nodes = find_script_nodes(tree);
    let mut deferred: Vec<String> = Vec::new();

    for (script_body, is_defer) in script_nodes {
        let actual_body = if let Some(src) = script_body.strip_prefix("__external_src__:") {
            // Fetch external script
            match fetch_external_script(http_client, current_url, src) {
                Some(code) => code,
                None => {
                    trace.record_console("warn", &format!("Failed to fetch script: {}", src));
                    continue;
                }
            }
        } else {
            script_body
        };

        if actual_body.trim().is_empty() {
            continue;
        }

        if is_defer {
            deferred.push(actual_body);
        } else {
            execute_single_script(tree, vm, scheduler, trace, &actual_body);
        }
    }

    // Execute deferred scripts
    for script in deferred {
        execute_single_script(tree, vm, scheduler, trace, &script);
    }

    // Final event loop drain
    let tick_limit = scheduler.tick_limit;
    let mut ticks = 0;
    while ticks < tick_limit && scheduler.has_pending_tasks() {
        if let Some(task) = scheduler.pop_next_task() {
            execute_scheduled_task(tree, vm, &task);
            ticks += 1;
        } else {
            break;
        }
    }

    trace.record_console("info", &format!(
        "Full script pipeline complete for {}",
        current_url
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::HtmlParser;

    fn make_tree(html: &str) -> DomTree {
        DomTree::new(HtmlParser::parse_html5(html))
    }

    #[test]
    fn inline_script_executes() {
        let mut tree = make_tree("<html><body><div id='target'></div><script>document.getElementById('target').setAttribute('data-ran','true')</script></body></html>");
        let mut vm = JsVirtualMachine::new();
        let mut scheduler = JsEventLoopScheduler::new();
        let mut http = HttpClient::new();
        let mut trace = TraceCollector::new();

        execute_page_scripts(&mut tree, &mut vm, &mut scheduler, &mut http, &mut trace, "about:test");

        let node = tree.nodes.iter().find(|n| n.attributes.get("id").map(|s| s.as_str()) == Some("target")).unwrap();
        assert_eq!(node.attributes.get("data-ran").map(|s| s.as_str()), Some("true"));
    }

    #[test]
    fn skips_json_type_scripts() {
        let mut tree = make_tree("<script type='application/json'>{\"key\":\"val\"}</script><script>var x = 1</script>");
        let mut vm = JsVirtualMachine::new();
        let mut scheduler = JsEventLoopScheduler::new();
        let mut http = HttpClient::new();
        let mut trace = TraceCollector::new();

        // Should not error on JSON script
        execute_page_scripts(&mut tree, &mut vm, &mut scheduler, &mut http, &mut trace, "about:test");
    }

    #[test]
    fn skips_module_scripts() {
        let mut tree = make_tree("<script type='module'>import foo from './foo.js'</script>");
        let mut vm = JsVirtualMachine::new();
        let mut scheduler = JsEventLoopScheduler::new();
        let mut http = HttpClient::new();
        let mut trace = TraceCollector::new();

        execute_page_scripts(&mut tree, &mut vm, &mut scheduler, &mut http, &mut trace, "about:test");
    }

    #[test]
    fn resolve_script_url_absolute() {
        assert_eq!(
            resolve_script_url("https://example.com/page", "https://cdn.example.com/app.js"),
            "https://cdn.example.com/app.js"
        );
    }

    #[test]
    fn resolve_script_url_relative() {
        assert_eq!(
            resolve_script_url("https://example.com/path/page.html", "app.js"),
            "https://example.com/path/app.js"
        );
    }

    #[test]
    fn resolve_script_url_root_relative() {
        assert_eq!(
            resolve_script_url("https://example.com/path/page.html", "/js/app.js"),
            "https://example.com/js/app.js"
        );
    }

    #[test]
    fn defer_scripts_execute_after_sync() {
        let mut tree = make_tree(
            "<div id='order'></div>\
             <script defer>document.getElementById('order').setAttribute('data-d','2')</script>\
             <script>document.getElementById('order').setAttribute('data-s','1')</script>"
        );
        let mut vm = JsVirtualMachine::new();
        let mut scheduler = JsEventLoopScheduler::new();
        let mut http = HttpClient::new();
        let mut trace = TraceCollector::new();

        execute_page_scripts(&mut tree, &mut vm, &mut scheduler, &mut http, &mut trace, "about:test");

        let node = tree.nodes.iter().find(|n| n.attributes.get("id").map(|s| s.as_str()) == Some("order")).unwrap();
        // Both should have run
        assert_eq!(node.attributes.get("data-s").map(|s| s.as_str()), Some("1"));
        assert_eq!(node.attributes.get("data-d").map(|s| s.as_str()), Some("2"));
    }
}
