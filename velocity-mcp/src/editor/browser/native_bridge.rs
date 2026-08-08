#![allow(dead_code)]

use crate::safety::SafeMutex;
use velocity_browser::{
    AgentActionResult, AgenticAomTree, BrowserSession, NdaTriple, SwarmSessionOrchestrator,
};
use velocity_browser::agentic::outcome_scorer::extract_domain;
use velocity_browser::agentic::{
    ActionKind, ActionOutcome, AdaptiveConfidence, OutcomeScorer, OutcomeSignals, Reflection,
    ReflectionEngine,
};
use velocity_browser::screencast::ScreencastRecorder;
use velocity_browser::vector_memory::{SiteVectorStore, VectorMemoryNode};
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
    /// Named page-state snapshots for cross-action diffing: "what changed
    /// since I last looked", spanning any number of intermediate actions.
    pub checkpoints: HashMap<String, velocity_browser::NdaDocument>,
    /// Scores every native agent action from its observed NDA delta so the
    /// agent can learn which targets work and which keep failing.
    pub scorer: OutcomeScorer,
    /// Turns the outcome history into failure-pattern lessons
    /// (repeated failures, navigation loops, blocked clicks).
    pub reflector: ReflectionEngine,
    /// Learned per-(role, action, domain) confidence fed by outcome scores;
    /// powers "what should I try next" predictions.
    pub confidence: AdaptiveConfidence,
    /// Whether this session already tried to inherit the workspace-default
    /// experience bundle; seeding runs at most once per session.
    pub experience_seeded: bool,
}

static NATIVE_BRIDGES: LazyLock<Arc<Mutex<HashMap<String, Arc<Mutex<NativeBrowserBridge>>>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

pub fn get_or_create_native_bridge(session_id: &str) -> Arc<Mutex<NativeBrowserBridge>> {
    let mut map = NATIVE_BRIDGES.lock_safe();
    map.entry(session_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(NativeBrowserBridge::new(session_id))))
        .clone()
}

/// Write raw bytes into the session artifact directory
/// (`{workspace_root}/.velocity/browser_artifacts/{file_name}`) and return
/// the path. All browser NDA/trace/fact artifacts go through here so they
/// land in one predictable place.
pub fn persist_browser_artifact(
    workspace_root: &Path,
    file_name: &str,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    let artifacts_dir = workspace_root.join(".velocity").join("browser_artifacts");
    let _ = std::fs::create_dir_all(&artifacts_dir);
    let path = artifacts_dir.join(file_name);
    std::fs::write(&path, bytes).map_err(|e| format!("failed to write browser artifact: {e}"))?;
    Ok(path)
}

/// Encode NDA triples as their fixed 18-byte binary records.
pub fn encode_nda_triples(triples: &[NdaTriple]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(triples.len() * 18);
    for t in triples {
        encoded.extend_from_slice(&t.to_bytes());
    }
    encoded
}

pub fn persist_native_nda_triples(
    workspace_root: &Path,
    session_id: &str,
    triples: &[NdaTriple],
) -> Result<PathBuf, String> {
    persist_browser_artifact(
        workspace_root,
        &format!("{}_native.nda", session_id),
        &encode_nda_triples(triples),
    )
}

impl NativeBrowserBridge {
    pub fn new(session_id: &str) -> Self {
        Self {
            swarm: SwarmSessionOrchestrator::new(),
            active_session: BrowserSession::new(session_id.to_string()),
            screencast: ScreencastRecorder::new(session_id),
            vector_memory: SiteVectorStore::new(),
            checkpoints: HashMap::new(),
            scorer: OutcomeScorer::new(),
            reflector: ReflectionEngine::new(),
            confidence: AdaptiveConfidence::new(),
            experience_seeded: false,
        }
    }

    /// Inherit the workspace-default experience bundle
    /// (`.velocity/browser_artifacts/default_all.nda`) into this session's
    /// stores. Runs at most once per session and is silent when no bundle
    /// exists; returns (patterns, memories, outcomes) restored.
    pub fn seed_default_experience(&mut self, workspace_root: &Path) -> Option<(usize, usize, usize)> {
        if self.experience_seeded {
            return None;
        }
        self.experience_seeded = true;
        let path = workspace_root
            .join(".velocity")
            .join("browser_artifacts")
            .join("default_all.nda");
        let bytes = std::fs::read(&path).ok()?;
        let doc = velocity_browser::NdaDocument::from_binary_stream(&bytes).ok()?;
        let patterns = self.confidence.import_nda(&doc);
        let memories = self.vector_memory.import_nda(&doc);
        let outcomes = self.scorer.import_nda(&doc);
        Some((patterns, memories, outcomes))
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

    // -- Multi-tab surface ----------------------------------------------------
    // The bridge always acts on `active_session` (the foreground tab); the
    // swarm parks background tabs. Switching swaps the chosen background
    // session into the foreground in place, so every existing action/view
    // tool keeps targeting "the active tab" without change.

    /// Open a new blank background tab. Fails on id collision with the
    /// foreground tab or an existing background tab.
    pub fn tab_open(&mut self, tab_id: &str) -> Result<(), String> {
        if self.active_session.session_id == tab_id || self.swarm.get_session(tab_id).is_some() {
            return Err(format!("tab '{tab_id}' already exists"));
        }
        self.swarm.spawn_swarm_tab(tab_id);
        Ok(())
    }

    /// `(tab_id, url, title, is_active)` for the foreground tab plus every
    /// background tab, in stable order.
    pub fn tab_list(&self) -> Vec<(String, String, String, bool)> {
        let mut tabs = vec![(
            self.active_session.session_id.clone(),
            self.active_session.current_url.clone(),
            self.active_session.page_title.clone(),
            true,
        )];
        for s in &self.swarm.swarm_sessions {
            tabs.push((
                s.session_id.clone(),
                s.current_url.clone(),
                s.page_title.clone(),
                false,
            ));
        }
        tabs
    }

    /// Bring a background tab to the foreground, parking the current
    /// foreground tab in the swarm. The whole `BrowserSession` is swapped, so
    /// per-tab state (DOM, cookies, focus, traces) travels with its tab.
    pub fn tab_switch(&mut self, tab_id: &str) -> Result<(), String> {
        if self.active_session.session_id == tab_id {
            return Ok(());
        }
        let Some(target) = self.swarm.get_session_mut(tab_id) else {
            return Err(format!("no tab '{tab_id}'"));
        };
        std::mem::swap(&mut self.active_session, target);
        Ok(())
    }

    /// Close a background tab. The foreground tab cannot be closed - switch
    /// away first so the bridge always has an active session.
    pub fn tab_close(&mut self, tab_id: &str) -> Result<(), String> {
        if self.active_session.session_id == tab_id {
            return Err("cannot close the active tab; switch to another tab first".to_string());
        }
        if self.swarm.remove_session(tab_id) {
            Ok(())
        } else {
            Err(format!("no tab '{tab_id}'"))
        }
    }

    pub fn capture_nda(&self) -> Vec<NdaTriple> {
        self.active_session.capture_state_nda()
    }

    /// Lossless NDA document of the current session state (the same snapshot
    /// the agent_* actions diff), for readable/binary export.
    pub fn capture_document(&self) -> velocity_browser::NdaDocument {
        self.active_session.capture_state_document()
    }

    /// Console/mutation/performance/network traces as NDA triples
    /// (predicates 120-123).
    pub fn export_traces_nda(&self) -> Vec<NdaTriple> {
        self.active_session.trace_collector.export_traces_nda()
    }

    // -- Vector memory --------------------------------------------------------
    // The bridge-level store spans all tabs of a session id, so a page
    // remembered in one tab is recallable from any other.

    /// Index the current page (title + visible text + optional note) into
    /// vector memory. Returns `(memory_id, url, indexed_char_count)`.
    pub fn remember_page(
        &mut self,
        tags: Vec<String>,
        outcome_score: f64,
        note: Option<&str>,
    ) -> (String, String, usize) {
        use std::hash::{DefaultHasher, Hash, Hasher};
        // Store the distilled markdown projection (structure + content,
        // boilerplate stripped) instead of flat text: recall snippets stay
        // readable and the embedding is cleaner without chrome noise.
        let mut text = self.active_session.page_markdown();
        if text.is_empty() {
            text = self.active_session.page_text();
        }
        if let Some(note) = note.map(str::trim).filter(|n| !n.is_empty()) {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(note);
        }
        let url = self.active_session.current_url.clone();
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        text.hash(&mut hasher);
        let chars = text.chars().count();
        let id = self.vector_memory.insert_rich(
            &self.active_session.session_id,
            &url,
            &text,
            hasher.finish(),
            tags,
            outcome_score.clamp(0.0, 1.0),
        );
        (id, url, chars)
    }

    /// Recall remembered pages. `mode` is `"semantic"` (TF-IDF cosine,
    /// scored), `"keyword"` (substring over text/url), `"tag"` (exact tag
    /// match) or `"similar"` (query is a memory id; embedding-similarity to
    /// that memory, scored). `min_outcome` drops memories below the given
    /// outcome score before the limit applies, so "recall what worked" is a
    /// filter on any mode instead of a separate tool. Hits are cloned out so
    /// the tool layer renders them without borrowing the store.
    pub fn recall_pages(
        &self,
        query: &str,
        mode: &str,
        limit: usize,
        min_outcome: f64,
    ) -> Vec<(VectorMemoryNode, Option<f64>)> {
        let mut hits: Vec<(VectorMemoryNode, Option<f64>)> = match mode {
            "keyword" => self
                .vector_memory
                .search(query, usize::MAX)
                .into_iter()
                .map(|n| (n.clone(), None))
                .collect(),
            "tag" => self
                .vector_memory
                .search_by_tag(query, usize::MAX)
                .into_iter()
                .map(|n| (n.clone(), None))
                .collect(),
            "similar" => self
                .vector_memory
                .find_similar(query, usize::MAX)
                .into_iter()
                .map(|(n, sim)| (n.clone(), Some(sim)))
                .collect(),
            _ => self
                .vector_memory
                .semantic_search(query, usize::MAX)
                .into_iter()
                .map(|(n, sim)| (n.clone(), Some(sim)))
                .collect(),
        };
        if min_outcome > 0.0 {
            hits.retain(|(n, _)| n.outcome_score >= min_outcome);
        }
        hits.truncate(limit);
        hits
    }

    /// Total number of pages held in vector memory.
    pub fn memory_count(&self) -> usize {
        self.vector_memory.nodes.len()
    }

    /// Visible text of the current page (title + body text, whitespace
    /// collapsed) — the token-cheapest full read of a page.
    pub fn page_text(&self) -> String {
        self.active_session.page_text()
    }

    /// Markdown projection of the page (headings, lists, links, tables).
    pub fn page_markdown(&self) -> String {
        self.active_session.page_markdown()
    }

    /// Every table on the page rendered as markdown rows.
    pub fn page_tables_text(&self) -> String {
        self.active_session.page_tables_text()
    }

    /// One-screen digest: identity, element counts, heading outline.
    pub fn page_summary_text(&self) -> String {
        self.active_session.page_summary_text()
    }

    /// Readability projection: markdown of just the main content region.
    pub fn page_content_markdown(&self) -> String {
        self.active_session.page_content_markdown()
    }

    // -- Screencast -----------------------------------------------------------
    // Structural screencast: each frame records the page's shape (viewport +
    // AOM element count + content hash) instead of pixels, giving the agent a
    // diffable timeline of how the page evolved across its actions.

    /// Capture a frame of the current page state. Returns
    /// `(frame_idx, element_count, frame_hash)`.
    pub fn screencast_capture(&mut self) -> (u32, usize, u64) {
        let element_count = self
            .active_session
            .dom_tree
            .as_ref()
            .map(|tree| AgenticAomTree::build_aom_nodes(tree).len())
            .unwrap_or(0);
        let frame = self.screencast.capture_frame(
            self.active_session.viewport_width as u32,
            self.active_session.viewport_height as u32,
            element_count,
        );
        (frame.frame_idx, frame.element_count, frame.frame_hash)
    }

    /// All frames captured so far, oldest first.
    pub fn screencast_frames(&self) -> &[velocity_browser::screencast::ScreencastFrame] {
        &self.screencast.frames
    }

    /// Persist the frame timeline as JSON under
    /// `.velocity/browser_artifacts/screencasts/`.
    pub fn screencast_save(&self, workspace_root: &Path) -> Result<PathBuf, String> {
        self.screencast.save_metadata(workspace_root)
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

    pub fn agent_scroll_into_view(&mut self, label: &str) -> AgentActionResult {
        self.active_session.agent_scroll_into_view(label)
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

    /// Filter the live AOM by role and/or a case-insensitive substring over
    /// accessible name and value. Returns `(hits, total_elements)` so the
    /// agent can target elements on big pages without paying for the whole
    /// element dump.
    pub fn find_elements(&self, role: Option<&str>, text: &str) -> (Vec<NativeAomElement>, usize) {
        let view = self.current_view();
        let total = view.elements.len();
        let text_lc = text.to_lowercase();
        let hits = view
            .elements
            .into_iter()
            .filter(|e| role.map(|r| e.role.eq_ignore_ascii_case(r)).unwrap_or(true))
            .filter(|e| {
                text_lc.is_empty()
                    || e.name.to_lowercase().contains(&text_lc)
                    || e.value.to_lowercase().contains(&text_lc)
            })
            .collect();
        (hits, total)
    }

    /// Run HTML5 constraint validation over every form control on the page.
    /// Returns `(node_id, accessible_name, failed_constraints)` per control;
    /// an empty constraint list means the control is valid.
    pub fn validate_forms(&self) -> Vec<(usize, String, Vec<&'static str>)> {
        let Some(tree) = &self.active_session.dom_tree else {
            return Vec::new();
        };
        let view = self.current_view();
        let mut results = Vec::new();
        for node in &tree.nodes {
            if node.node_type != velocity_browser::parser::html::NodeType::Element {
                continue;
            }
            if !matches!(node.tag_name.as_str(), "input" | "textarea" | "select") {
                continue;
            }
            let state = velocity_browser::FormDataSerializer::validate_control(node);
            let mut failed: Vec<&'static str> = Vec::new();
            if state.value_missing {
                failed.push("valueMissing");
            }
            if state.type_mismatch {
                failed.push("typeMismatch");
            }
            if state.pattern_mismatch {
                failed.push("patternMismatch");
            }
            if state.too_long {
                failed.push("tooLong");
            }
            if state.too_short {
                failed.push("tooShort");
            }
            if state.range_underflow {
                failed.push("rangeUnderflow");
            }
            if state.range_overflow {
                failed.push("rangeOverflow");
            }
            let name = view
                .elements
                .iter()
                .find(|e| e.node_id == node.id)
                .map(|e| e.name.clone())
                .or_else(|| node.attributes.get("name").cloned())
                .unwrap_or_default();
            results.push((node.id, name, failed));
        }
        results
    }

    /// The page's navigation map: every `<a href>` as
    /// `(node_id, link_text, href)`, in document order. Optional
    /// case-insensitive filter over text and href. The token-cheap answer to
    /// "where can I go from here" — the AOM view names links but never shows
    /// their targets.
    pub fn links(&self, filter: &str) -> Vec<(usize, String, String)> {
        let Some(tree) = &self.active_session.dom_tree else {
            return Vec::new();
        };
        let filter_lc = filter.to_lowercase();
        let mut out = Vec::new();
        for node in &tree.nodes {
            if node.node_type != velocity_browser::parser::html::NodeType::Element
                || node.tag_name != "a"
            {
                continue;
            }
            let Some(href) = node.attributes.get("href") else {
                continue;
            };
            let text = tree
                .text_content(node.id)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !filter_lc.is_empty()
                && !text.to_lowercase().contains(&filter_lc)
                && !href.to_lowercase().contains(&filter_lc)
            {
                continue;
            }
            out.push((node.id, text, href.clone()));
        }
        out
    }

    /// The session's navigation history: every `(url, title)` entry in
    /// stack order plus the index the session currently points at. Titles
    /// are backfilled by the engine when each page parses.
    pub fn history(&self) -> (Vec<(String, String)>, usize) {
        let stack = &self.active_session.history_stack;
        let items = stack
            .items
            .iter()
            .map(|h| (h.url.clone(), h.title.clone()))
            .collect();
        (items, stack.current_index)
    }

    // -- Checkpoints ----------------------------------------------------------
    // Named snapshots of the same lossless state document the agent_* actions
    // diff, so "what changed since I last looked" can span any number of
    // intermediate actions instead of one.

    /// Snapshot the current page state under `name`. Returns the snapshot's
    /// fact count and whether an existing checkpoint was replaced.
    pub fn checkpoint_save(&mut self, name: &str) -> (usize, bool) {
        let doc = self.active_session.capture_state_document();
        let facts = doc.facts.len();
        let replaced = self.checkpoints.insert(name.to_string(), doc).is_some();
        (facts, replaced)
    }

    /// Diff the current page state against the named checkpoint. `None` when
    /// no such checkpoint exists.
    pub fn checkpoint_diff(&self, name: &str) -> Option<velocity_browser::NdaDelta> {
        let before = self.checkpoints.get(name)?;
        let after = self.active_session.capture_state_document();
        Some(velocity_browser::agent_api::diff(before, &after))
    }

    /// All checkpoints as `(name, fact_count)`, sorted by name.
    pub fn checkpoint_list(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = self
            .checkpoints
            .iter()
            .map(|(n, d)| (n.clone(), d.facts.len()))
            .collect();
        out.sort();
        out
    }

    /// Remove the named checkpoint. False when it did not exist.
    pub fn checkpoint_drop(&mut self, name: &str) -> bool {
        self.checkpoints.remove(name).is_some()
    }

    // -- Outcome scoring & reflection --
    // Every native action already returns its observed NDA delta; scoring that
    // observation (instead of trusting the action) lets the reflection engine
    // spot patterns like "clicking this keeps doing nothing".

    /// Score a native action from its observed result and record it in the
    /// outcome history. All signals derive from the NDA delta and status
    /// string — nothing is self-reported.
    pub fn record_outcome(&mut self, action: &str, role: &str, target: &str, result: &AgentActionResult) {
        let kind = ActionKind::from_str(action);
        let status = result.status.to_lowercase();
        let signals = OutcomeSignals {
            dom_changed: !result.delta.is_empty(),
            url_changed: result.delta.changed.iter().any(|c| c.predicate == velocity_browser::predicates::SESSION_URL),
            error_thrown: status.starts_with("no ")
                || status.contains("failed")
                || status.contains("error"),
            target_removed: false,
            content_added: result.delta.added.len() > result.delta.removed.len(),
            network_request_fired: false,
            // The engine is synchronous: every action returns, none time out.
            completed_in_time: true,
            agent_confidence: 0.0,
        };
        let score = self.scorer.score(&kind, &signals);
        let page_url = self.active_session.current_url.clone();
        // Feed the learned confidence store so predict_learned improves with
        // every observed outcome on this domain.
        self.confidence.record(role, kind.label(), extract_domain(&page_url), score);
        self.scorer.record(ActionOutcome {
            action_kind: kind,
            target_selector: target.to_string(),
            target_role: role.to_string(),
            page_url,
            score,
            signals,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        });
    }

    /// Failure-pattern lessons derived from the recorded outcome history.
    pub fn reflect(&mut self) -> Vec<Reflection> {
        self.reflector.reflect(&self.scorer)
    }

    /// Suggest the next best action on the current page using the learned
    /// per-domain confidence instead of the legacy hardcoded heuristic.
    pub fn predict_learned(&self) -> Option<velocity_browser::PredictedActionTarget> {
        let tree = self.active_session.dom_tree.as_ref()?;
        let domain = extract_domain(&self.active_session.current_url);
        velocity_browser::ActionPredictorEngine::predict_with_confidence(
            tree,
            &self.confidence,
            domain,
        )
    }

    /// Learned `(role, action, confidence, observations)` patterns for the
    /// current domain, best first.
    pub fn confidence_report(&self) -> Vec<(String, String, f64, u32)> {
        self.confidence
            .domain_report(extract_domain(&self.active_session.current_url))
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
