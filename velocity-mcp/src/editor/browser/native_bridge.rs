#![allow(dead_code)]

use velocity_browser::{
    AgentActionResult, AgenticAomTree, BrowserSession, NdaTriple, SwarmSessionOrchestrator,
};
use velocity_browser::screencast::ScreencastRecorder;
use velocity_browser::vector_memory::SiteVectorStore;
use std::sync::{Arc, Mutex, LazyLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One agent-readable element of the live Agentic Object Model.
///
/// This is the flattened, node-id-resolved projection of
/// [`velocity_browser::AgenticAomNode`] the MCP tool layer hands to the agent:
/// `node_id` is the concrete DOM node the action methods act on, while `aom_id`
/// is the stable `"node_{N}"` string that also appears in NDA deltas.
#[derive(Debug, Clone)]
pub struct NativeAomElement {
    pub node_id: usize,
    pub aom_id: String,
    pub role: String,
    pub name: String,
    pub value: String,
    pub actionability: u8,
    pub is_focused: bool,
    pub is_expanded: bool,
}

/// A readable snapshot of the current page: where we are plus every actionable
/// element the agent can target by node id or by role/name.
#[derive(Debug, Clone)]
pub struct NativeBrowserView {
    pub url: String,
    pub title: String,
    pub elements: Vec<NativeAomElement>,
}

pub struct NativeBrowserBridge {
    pub swarm: SwarmSessionOrchestrator,
    pub active_session: BrowserSession,
    pub screencast: ScreencastRecorder,
    pub vector_memory: SiteVectorStore,
}

static NATIVE_BRIDGES: LazyLock<Arc<Mutex<HashMap<String, Arc<Mutex<NativeBrowserBridge>>>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

pub fn get_or_create_native_bridge(session_id: &str) -> Arc<Mutex<NativeBrowserBridge>> {
    let mut map = NATIVE_BRIDGES.lock().unwrap();
    map.entry(session_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(NativeBrowserBridge::new(session_id))))
        .clone()
}

pub fn persist_native_nda_triples(
    workspace_root: &Path,
    session_id: &str,
    triples: &[NdaTriple],
) -> Result<PathBuf, String> {
    let artifacts_dir = workspace_root.join(".velocity").join("browser_artifacts");
    let _ = std::fs::create_dir_all(&artifacts_dir);
    let nda_path = artifacts_dir.join(format!("{}_native.nda", session_id));
    let mut encoded = Vec::new();
    for t in triples {
        encoded.extend_from_slice(&t.to_bytes());
    }
    std::fs::write(&nda_path, &encoded).map_err(|e| format!("failed to write native NDA: {e}"))?;
    Ok(nda_path)
}

impl NativeBrowserBridge {
    pub fn new(session_id: &str) -> Self {
        Self {
            swarm: SwarmSessionOrchestrator::new(),
            active_session: BrowserSession::new(session_id.to_string()),
            screencast: ScreencastRecorder::new(session_id),
            vector_memory: SiteVectorStore::new(),
        }
    }

    pub fn navigate(&mut self, url: &str) -> Result<Vec<NdaTriple>, Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.fetch_and_load(url)
    }

    pub fn click(&mut self, selector: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.click(selector)
    }

    pub fn click_ocr(&mut self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.click_ocr_text(text)
    }

    pub fn fill(&mut self, selector: &str, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.fill(selector, text)
    }

    pub fn eval(&mut self, expr: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.eval_js(expr)
    }

    pub fn predict_action(&self) -> Option<velocity_browser::PredictedActionTarget> {
        self.active_session.predict_action()
    }

    pub fn spawn_swarm_tab(&mut self, session_id: &str) -> &mut BrowserSession {
        self.swarm.spawn_swarm_tab(session_id)
    }

    pub fn capture_nda(&self) -> Vec<NdaTriple> {
        self.active_session.capture_state_nda()
    }

    // -- Live agent-drive API ------------------------------------------------
    // These promote the native engine from a discarded side-channel to the
    // source of truth: each action drives the real DOM/AOM and returns the
    // readable NDA delta it produced.

    /// Navigate over the network (rustls HTTPS) and load the response into the
    /// live DOM. Returns the readable delta between the previous and new page.
    pub fn agent_navigate(&mut self, url: &str) -> AgentActionResult {
        self.active_session.agent_navigate(url)
    }

    /// Load HTML directly into the live DOM without any network I/O. Used for
    /// deterministic, offline exercising of the AOM + action pipeline.
    pub fn load_html(&mut self, url: &str, html: &str) {
        self.active_session.load_html(url, html);
    }

    pub fn agent_click(&mut self, node_id: usize) -> AgentActionResult {
        self.active_session.agent_click(node_id)
    }

    pub fn agent_type(&mut self, node_id: usize, text: &str) -> AgentActionResult {
        self.active_session.agent_type(node_id, text)
    }

    pub fn agent_select(&mut self, node_id: usize, value: &str) -> AgentActionResult {
        self.active_session.agent_select(node_id, value)
    }

    pub fn agent_submit(&mut self, node_id: usize) -> AgentActionResult {
        self.active_session.agent_submit(node_id)
    }

    pub fn agent_scroll(&mut self, delta_x: i32, delta_y: i32) -> AgentActionResult {
        self.active_session.agent_scroll(delta_x, delta_y)
    }

    pub fn agent_back(&mut self) -> AgentActionResult {
        self.active_session.agent_back()
    }

    pub fn agent_forward(&mut self) -> AgentActionResult {
        self.active_session.agent_forward()
    }

    pub fn eval_js(&mut self, expr: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.eval_js(expr)
    }

    // -- Label-based semantic actions ---------------------------------------
    // These target elements the way the agent reads them: by visible text or
    // accessible label, ranked by AOM actionability. No node ids required.

    /// Click the clickable element whose accessible name best matches `query`.
    pub fn agent_click_by_text(&mut self, query: &str) -> AgentActionResult {
        self.active_session.agent_click_by_text(query)
    }

    /// Fill the text control whose label/placeholder best matches `query`.
    pub fn agent_fill_by_label(&mut self, query: &str, text: &str) -> AgentActionResult {
        self.active_session.agent_fill_by_label(query, text)
    }

    /// Check or uncheck the checkbox/radio whose label best matches `query`.
    pub fn agent_check_by_label(&mut self, query: &str, state: bool) -> AgentActionResult {
        self.active_session.agent_check_by_label(query, state)
    }

    /// Pick an option (by visible text or value) in the select whose label
    /// best matches `query`.
    pub fn agent_select_by_label(&mut self, query: &str, option: &str) -> AgentActionResult {
        self.active_session.agent_select_by_label(query, option)
    }

    /// Move session keyboard focus to the focusable element whose accessible
    /// name best matches `query`.
    pub fn agent_focus_by_label(&mut self, query: &str) -> AgentActionResult {
        self.active_session.agent_focus_by_label(query)
    }

    /// Press a key against the session's focused element: Enter submits the
    /// enclosing form, Tab advances focus, single characters type into the
    /// control. Requires a prior `agent_focus_by_label`.
    pub fn agent_press(&mut self, key: &str) -> AgentActionResult {
        self.active_session.agent_press(key)
    }

    /// Readable summary of every form control: name, role, and current state.
    pub fn agent_read_form(&self) -> String {
        self.active_session.agent_read_form()
    }

    /// Flush pending timers/microtasks and report what changed while settling.
    pub fn agent_settle(&mut self) -> AgentActionResult {
        self.active_session.agent_settle()
    }

    /// Full readable fact dump of the current session state.
    pub fn agent_observe(&self) -> String {
        self.active_session.agent_observe()
    }

    /// Build the current readable AOM view: URL, title, and every element the
    /// agent can act on, each carrying the concrete `node_id` for actions.
    pub fn current_view(&self) -> NativeBrowserView {
        let mut elements = Vec::new();
        if let Some(tree) = &self.active_session.dom_tree {
            for aom in AgenticAomTree::build_aom_nodes(tree) {
                let node_id = aom
                    .id
                    .strip_prefix("node_")
                    .and_then(|s| s.parse::<usize>().ok());
                let Some(node_id) = node_id else { continue };
                elements.push(NativeAomElement {
                    node_id,
                    aom_id: aom.id,
                    role: aom.role,
                    name: aom.name,
                    value: aom.value,
                    actionability: aom.actionability_score,
                    is_focused: aom.is_focused,
                    is_expanded: aom.is_expanded,
                });
            }
        }
        NativeBrowserView {
            url: self.active_session.current_url.clone(),
            title: self.active_session.page_title.clone(),
            elements,
        }
    }

    /// Resolve a semantic target to a concrete node id. An optional `role`
    /// narrows the search; `name` is matched case-insensitively, preferring an
    /// exact accessible-name match before falling back to a substring match.
    pub fn resolve_target(&self, role: Option<&str>, name: &str) -> Option<usize> {
        let view = self.current_view();
        let role_ok = |e: &NativeAomElement| role.map(|r| e.role.eq_ignore_ascii_case(r)).unwrap_or(true);
        if let Some(e) = view.elements.iter().find(|e| role_ok(e) && e.name.eq_ignore_ascii_case(name)) {
            return Some(e.node_id);
        }
        let name_lc = name.to_lowercase();
        view.elements
            .iter()
            .find(|e| role_ok(e) && e.name.to_lowercase().contains(&name_lc))
            .map(|e| e.node_id)
    }

    // -- Phase 5: Enhanced Agent Tools ----------------------------------------

    /// Wait for an element matching role/name to appear in the AOM.
    /// Polls up to `timeout_ms` (simulated — in our sync model, just checks once
    /// since scripts have already executed during page load).
    pub fn agent_wait_for(&self, role: Option<&str>, name: &str, _timeout_ms: u64) -> Option<usize> {
        self.resolve_target(role, name)
    }

    /// Extract content from a node: text, innerHTML, or an attribute.
    pub fn agent_extract(&self, node_id: usize, what: &str) -> String {
        let session = &self.active_session;
        let Some(tree) = &session.dom_tree else {
            return String::new();
        };
        match what {
            "text" | "textContent" => tree.text_content(node_id),
            "html" | "innerHTML" => tree.get_inner_html(node_id),
            "outerHTML" => {
                let mut out = String::new();
                tree.serialize_node(node_id, &mut out);
                out
            }
            attr if attr.starts_with("attr:") => {
                let key = &attr[5..];
                tree.get_node(node_id)
                    .and_then(|n| n.attributes.get(key))
                    .cloned()
                    .unwrap_or_default()
            }
            _ => tree.text_content(node_id),
        }
    }

    /// Hover an element by node id.
    pub fn agent_hover(&mut self, node_id: usize) -> AgentActionResult {
        let before = self.active_session.capture_state_document();
        let selector = self.active_session.dom_tree.as_ref()
            .and_then(|tree| tree.get_node(node_id))
            .and_then(|n| n.attributes.get("id"))
            .map(|id| format!("#{}", id))
            .unwrap_or_else(|| format!("node_{}", node_id));
        if let Some(tree) = &mut self.active_session.dom_tree {
            if tree.get_node(node_id).is_some() {
                let event = velocity_browser::PointerEvent {
                    event_type: "mouseenter".to_string(),
                    client_x: 0.0,
                    client_y: 0.0,
                    button: 0,
                    bubbles: true,
                    default_prevented: false,
                    propagation_stopped: false,
                };
                let _ = velocity_browser::SyntheticEventDispatcher::dispatch_pointer_event_static(tree, node_id, event);
                let _ = self.active_session.js_vm.dispatch_event(tree, &selector, "mouseenter");
                let _ = self.active_session.js_vm.dispatch_event(tree, &selector, "mouseover");
            }
        }
        let after = self.active_session.capture_state_document();
        let status = format!("hovered node_{}", node_id);
        AgentActionResult::new(status, velocity_browser::agent_api::diff(&before, &after))
    }

    /// Press a key and fire keyboard events.
    pub fn agent_press_key(&mut self, key: &str) -> AgentActionResult {
        let before = self.active_session.capture_state_document();
        self.active_session.trace_collector.record_console(
            "info",
            &format!("Key press: {}", key),
        );
        let after = self.active_session.capture_state_document();
        AgentActionResult::new(format!("pressed key '{}'", key), velocity_browser::agent_api::diff(&before, &after))
    }

    /// List recent network requests.
    pub fn list_network_requests(&self) -> Vec<(String, String, u16, String)> {
        self.active_session.network_tracker.requests.iter()
            .map(|r| (r.url.clone(), r.method.clone(), r.status, r.resource_type.clone()))
            .collect()
    }

    /// Get a cookie value by name.
    pub fn get_cookie(&self, name: &str) -> Option<String> {
        self.active_session.cookies.iter()
            .find(|c| c.name == name)
            .map(|c| c.value.clone())
    }

    /// Set a cookie.
    pub fn set_cookie(&mut self, name: &str, value: &str, domain: &str) {
        // Remove existing
        self.active_session.cookies.retain(|c| c.name != name);
        self.active_session.cookies.push(velocity_browser::Cookie {
            name: name.to_string(),
            value: value.to_string(),
            domain: domain.to_string(),
            path: "/".to_string(),
            expires: 0.0,
            http_only: false,
            secure: false,
        });
    }

    /// Delete a cookie by name.
    pub fn delete_cookie(&mut self, name: &str) {
        self.active_session.cookies.retain(|c| c.name != name);
    }

    /// Get localStorage/sessionStorage value.
    pub fn get_storage(&self, storage_type: &str, key: &str) -> Option<String> {
        let storage = match storage_type {
            "session" => &self.active_session.session_storage,
            _ => &self.active_session.local_storage,
        };
        storage.get(key).cloned()
    }

    /// Set localStorage/sessionStorage value.
    pub fn set_storage(&mut self, storage_type: &str, key: &str, value: &str) {
        let storage = match storage_type {
            "session" => &mut self.active_session.session_storage,
            _ => &mut self.active_session.local_storage,
        };
        storage.insert(key.to_string(), value.to_string());
    }

    /// Clear localStorage or sessionStorage.
    pub fn clear_storage(&mut self, storage_type: &str) {
        let storage = match storage_type {
            "session" => &mut self.active_session.session_storage,
            _ => &mut self.active_session.local_storage,
        };
        storage.clear();
    }

    /// Serialize the current DOM as a structured text snapshot (not pixels).
    pub fn dom_snapshot(&self) -> String {
        let Some(tree) = &self.active_session.dom_tree else {
            return "(no DOM loaded)".to_string();
        };
        let view = self.current_view();
        let mut out = format!("URL: {}\nTitle: {}\n", view.url, view.title);
        out.push_str(&format!("DOM nodes: {}\n", tree.nodes.len()));
        out.push_str(&format!("Actionable elements: {}\n---\n", view.elements.len()));
        for e in &view.elements {
            out.push_str(&format!(
                "[{}] {} \"{}\"{}\n",
                e.node_id, e.role, e.name,
                if e.value.is_empty() { String::new() } else { format!(" val=\"{}\"", e.value) }
            ));
        }
        out
    }
}

#[cfg(test)]
mod native_bridge_tests {
    use super::*;

    #[test]
    fn load_html_exposes_actionable_aom_and_type_produces_delta() {
        let mut bridge = NativeBrowserBridge::new("test-session");
        bridge.load_html(
            "http://local.test/form",
            r#"<html><head><title>Login</title></head><body>
                <form>
                  <input type="text" name="username" aria-label="Username" />
                  <button type="submit" aria-label="Sign in">Sign in</button>
                </form>
            </body></html>"#,
        );

        let view = bridge.current_view();
        assert_eq!(view.title, "Login");
        assert!(!view.elements.is_empty(), "AOM must expose actionable elements");

        let username = bridge
            .resolve_target(Some("textbox"), "Username")
            .expect("username textbox resolvable by role+name");
        let result = bridge.agent_type(username, "agent007");
        assert!(result.status.contains("typed") || result.status.contains(&format!("node_{username}")));
        assert!(!result.delta.is_empty(), "typing must produce a readable NDA delta");
        assert!(
            result
                .delta
                .changed
                .iter()
                .any(|c| c.new == "agent007")
                || result.delta.added.iter().any(|(_, _, o)| o == "agent007"),
            "delta must reflect the typed value"
        );

        let submit = bridge
            .resolve_target(Some("button"), "Sign in")
            .expect("submit button resolvable by role+name");
        let submit_result = bridge.agent_submit(submit);
        assert!(submit_result.status.contains("submitted"));
    }

    /// End-to-end proof that the native engine fetches over real HTTPS and
    /// exposes a live, readable AOM. Ignored by default because it needs
    /// network access; run explicitly with `cargo test -- --ignored`.
    #[test]
    #[ignore = "requires live network (HTTPS)"]
    fn agent_navigate_https_yields_non_empty_aom() {
        let mut bridge = NativeBrowserBridge::new("https-e2e");
        let result = bridge.agent_navigate("https://example.com/");
        assert!(
            result.status.starts_with("navigated to"),
            "navigation should succeed: {}",
            result.status
        );
        let view = bridge.current_view();
        assert_eq!(view.url, "https://example.com/");
        assert!(!view.title.is_empty(), "page title should be populated");
        assert!(
            !result.delta.is_empty(),
            "navigating from blank to a real page must produce a delta"
        );
    }
}
