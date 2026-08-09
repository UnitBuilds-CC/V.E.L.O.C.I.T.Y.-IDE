//! Script execution pipeline for `<script>` tags.
//!
//! When a page is loaded, this module walks the DOM tree for `<script>` elements
//! and executes them in document order. Inline scripts run immediately; external
//! scripts (`src="..."`) are fetched via HttpClient first. The `defer` attribute
//! queues scripts for execution after the DOM is fully built. Non-JS type
//! attributes (like `application/json` or `type="module"`) are skipped gracefully.

use crate::dom::DomTree;
use crate::engine::TraceCollector;
use crate::js::event_loop::JsEventLoopScheduler;
use crate::js::vm::JsVirtualMachine;
use crate::net::HttpClient;
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

    trace.record_console(
        "info",
        &format!("Script execution complete for {}", current_url),
    );
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
        let has_src = node
            .attributes
            .get("src")
            .map(|s| !s.is_empty())
            .unwrap_or(false);

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
pub fn resolve_script_url(base_url: &str, src: &str) -> String {
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

/// Install a module resolver that fetches cross-file ES module imports.
///
/// When a page script contains `import { x } from './lib.js'`, the interpreter
/// invokes the registered module resolver to obtain the module source. This
/// wires that callback to the network stack: relative specifiers are resolved
/// against `base_url` and fetched over HTTP(S). The resolver owns its own
/// [`HttpClient`] (behind a mutex) because the callback must be `Send + Sync +
/// 'static`.
pub fn install_module_fetch_resolver(base_url: &str) {
    use crate::js::interpreter::set_module_resolver;
    use std::sync::{Arc, Mutex};

    let base = base_url.to_string();
    let client = Arc::new(Mutex::new(HttpClient::new()));
    set_module_resolver(move |specifier: &str| {
        let url = resolve_script_url(&base, specifier);
        let mut guard = client.lock().ok()?;
        match guard.get(&url) {
            Ok(resp) if (200..400).contains(&resp.status_code) => Some(resp.body),
            _ => None,
        }
    });
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
    // Provide on-demand fetching for cross-file ES module imports and start
    // from a clean module registry so re-navigation re-evaluates modules.
    crate::js::interpreter::clear_module_registry();
    install_module_fetch_resolver(current_url);

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

    trace.record_console(
        "info",
        &format!("Full script pipeline complete for {}", current_url),
    );
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

        execute_page_scripts(
            &mut tree,
            &mut vm,
            &mut scheduler,
            &mut http,
            &mut trace,
            "about:test",
        );

        let node = tree
            .nodes
            .iter()
            .find(|n| n.attributes.get("id").map(|s| s.as_str()) == Some("target"))
            .unwrap();
        assert_eq!(
            node.attributes.get("data-ran").map(|s| s.as_str()),
            Some("true")
        );
    }

    #[test]
    fn skips_json_type_scripts() {
        let mut tree = make_tree(
            "<script type='application/json'>{\"key\":\"val\"}</script><script>var x = 1</script>",
        );
        let mut vm = JsVirtualMachine::new();
        let mut scheduler = JsEventLoopScheduler::new();
        let mut http = HttpClient::new();
        let mut trace = TraceCollector::new();

        // Should not error on JSON script
        execute_page_scripts(
            &mut tree,
            &mut vm,
            &mut scheduler,
            &mut http,
            &mut trace,
            "about:test",
        );
    }

    #[test]
    fn skips_module_scripts() {
        let mut tree = make_tree("<script type='module'>import foo from './foo.js'</script>");
        let mut vm = JsVirtualMachine::new();
        let mut scheduler = JsEventLoopScheduler::new();
        let mut http = HttpClient::new();
        let mut trace = TraceCollector::new();

        execute_page_scripts(
            &mut tree,
            &mut vm,
            &mut scheduler,
            &mut http,
            &mut trace,
            "about:test",
        );
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
             <script>document.getElementById('order').setAttribute('data-s','1')</script>",
        );
        let mut vm = JsVirtualMachine::new();
        let mut scheduler = JsEventLoopScheduler::new();
        let mut http = HttpClient::new();
        let mut trace = TraceCollector::new();

        execute_page_scripts(
            &mut tree,
            &mut vm,
            &mut scheduler,
            &mut http,
            &mut trace,
            "about:test",
        );

        let node = tree
            .nodes
            .iter()
            .find(|n| n.attributes.get("id").map(|s| s.as_str()) == Some("order"))
            .unwrap();
        // Both should have run
        assert_eq!(node.attributes.get("data-s").map(|s| s.as_str()), Some("1"));
        assert_eq!(node.attributes.get("data-d").map(|s| s.as_str()), Some("2"));
    }

    /// Spin up a tiny single-threaded HTTP server that serves `body` for every
    /// request and records the requested paths. Returns (base_url, paths_log).
    fn serve_module(body: String) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::{Arc, Mutex};

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().unwrap();
        let paths: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let paths_thread = paths.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    continue;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                paths_thread.lock().unwrap().push(path);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://{}/", addr), paths)
    }

    #[test]
    fn module_fetch_resolver_resolves_cross_file_import() {
        // Serialize with other module-system tests (global resolver/registry).
        let _guard = crate::js::interpreter::MODULE_TEST_LOCK.lock().unwrap();
        let (base, _paths) =
            serve_module("export function add(a, b) { return a + b; }".to_string());
        install_module_fetch_resolver(&base);

        let mut tree = make_tree("<div id='out'></div>");
        let mut vm = JsVirtualMachine::new();
        // The relative import './math.js' is fetched from the local server.
        let result = vm.eval_statement(&mut tree, "import { add } from './math.js'; add(40, 2)");
        assert_eq!(result.unwrap(), crate::js::vm::JsValue::Number(42.0));

        crate::js::interpreter::clear_module_resolver();
        crate::js::interpreter::clear_module_registry();
    }

    #[test]
    fn page_pipeline_fetches_cross_file_module() {
        // Serialize with other module-system tests (global resolver/registry).
        let _guard = crate::js::interpreter::MODULE_TEST_LOCK.lock().unwrap();
        let (base, paths) = serve_module("export const VALUE = 99;".to_string());

        let mut tree = make_tree("<div id='out'></div><script type='module'>import { VALUE } from './config.js';</script>");
        let mut vm = JsVirtualMachine::new();
        let mut scheduler = JsEventLoopScheduler::new();
        let mut http = HttpClient::new();
        let mut trace = TraceCollector::new();

        execute_page_scripts_full(
            &mut tree,
            &mut vm,
            &mut scheduler,
            &mut http,
            &mut trace,
            &base,
        );

        // The module file was actually fetched over the network by the resolver.
        let log = paths.lock().unwrap();
        assert!(
            log.iter().any(|p| p.contains("config.js")),
            "expected config.js fetch, got {:?}",
            *log
        );

        crate::js::interpreter::clear_module_resolver();
        crate::js::interpreter::clear_module_registry();
    }
}
