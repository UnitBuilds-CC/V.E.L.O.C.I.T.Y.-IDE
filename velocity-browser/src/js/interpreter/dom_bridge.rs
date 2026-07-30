//! Lightweight in-memory DOM bridge for the JS interpreter.
//!
//! Provides `document.*` methods and Element APIs so that page scripts can
//! create, query, and manipulate DOM nodes entirely within the interpreter.
//! The DOM state is thread-local, giving each interpreter instance its own
//! isolated document — matching browser per-origin isolation.

use crate::js::vm::JsValue;
use std::cell::RefCell;
use std::collections::HashMap;

// ── DOM Node Storage ─────────────────────────────────────────────────────────

thread_local! {
    static DOM_NODES: RefCell<Vec<DomNode>> = const { RefCell::new(Vec::new()) };
    static DOM_ROOT: RefCell<Option<usize>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone)]
struct DomNode {
    tag: String,
    attributes: HashMap<String, String>,
    children: Vec<usize>,
    parent: Option<usize>,
    text_content: String,
    node_type: u8, // 1=Element, 3=Text, 11=Fragment
    event_listeners: HashMap<String, Vec<JsValue>>,
}

impl DomNode {
    fn new_element(tag: &str) -> Self {
        Self {
            tag: tag.to_lowercase(),
            attributes: HashMap::new(),
            children: Vec::new(),
            parent: None,
            text_content: String::new(),
            node_type: 1,
            event_listeners: HashMap::new(),
        }
    }

    fn new_text(text: &str) -> Self {
        Self {
            tag: "#text".to_string(),
            attributes: HashMap::new(),
            children: Vec::new(),
            parent: None,
            text_content: text.to_string(),
            node_type: 3,
            event_listeners: HashMap::new(),
        }
    }

    fn new_fragment() -> Self {
        Self {
            tag: "#document-fragment".to_string(),
            attributes: HashMap::new(),
            children: Vec::new(),
            parent: None,
            text_content: String::new(),
            node_type: 11,
            event_listeners: HashMap::new(),
        }
    }
}

fn alloc_node(node: DomNode) -> usize {
    DOM_NODES.with(|nodes| {
        let mut nodes = nodes.borrow_mut();
        let id = nodes.len();
        nodes.push(node);
        id
    })
}

fn ensure_root() -> usize {
    DOM_ROOT.with(|root| {
        let mut root = root.borrow_mut();
        if let Some(id) = *root { return id; }
        let html = alloc_node(DomNode::new_element("html"));
        let head = alloc_node(DomNode::new_element("head"));
        let body = alloc_node(DomNode::new_element("body"));
        DOM_NODES.with(|nodes| {
            let mut nodes = nodes.borrow_mut();
            nodes[html].children.push(head);
            nodes[html].children.push(body);
            nodes[head].parent = Some(html);
            nodes[body].parent = Some(html);
        });
        *root = Some(html);
        html
    })
}

// ── Public DOM accessors for agent layer ─────────────────────────────────────

/// Snapshot of a single DOM element for zero-lock traversal.
#[derive(Debug, Clone)]
pub(super) struct DomElementSnapshot {
    pub id: usize,
    pub tag: String,
    pub attributes: HashMap<String, String>,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub text_content: String,
    pub node_type: u8,
}

/// Take a snapshot of all DOM nodes (releases the borrow immediately).
pub(super) fn snapshot_dom() -> (Vec<DomElementSnapshot>, usize) {
    let root = ensure_root();
    let snaps = DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        nodes.iter().enumerate().map(|(id, n)| DomElementSnapshot {
            id,
            tag: n.tag.clone(),
            attributes: n.attributes.clone(),
            children: n.children.clone(),
            parent: n.parent,
            text_content: n.text_content.clone(),
            node_type: n.node_type,
        }).collect()
    });
    (snaps, root)
}

/// Get the root element ID.
#[allow(dead_code)]
pub(super) fn get_root_id() -> usize { ensure_root() }

/// Get the total number of DOM nodes.
#[allow(dead_code)]
pub(super) fn dom_node_count() -> usize {
    DOM_NODES.with(|nodes| nodes.borrow().len())
}

/// Reset all DOM state (for test isolation).
#[cfg(test)]
pub fn reset_dom() {
    DOM_NODES.with(|nodes| nodes.borrow_mut().clear());
    DOM_ROOT.with(|root| *root.borrow_mut() = None);
}

// ── Element Handle ───────────────────────────────────────────────────────────

fn element_handle(id: usize) -> JsValue {
    let mut obj = HashMap::new();
    obj.insert("__type__".to_string(), JsValue::String("Element".to_string()));
    obj.insert("__node_id__".to_string(), JsValue::Number(id as f64));
    // Expose common properties directly on the handle for fast access.
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        if let Some(node) = nodes.get(id) {
            obj.insert("tagName".to_string(), JsValue::String(node.tag.to_uppercase()));
            obj.insert("nodeName".to_string(), JsValue::String(node.tag.to_uppercase()));
            obj.insert("nodeType".to_string(), JsValue::Number(node.node_type as f64));
            if let Some(id_attr) = node.attributes.get("id") {
                obj.insert("id".to_string(), JsValue::String(id_attr.clone()));
            }
            if let Some(class) = node.attributes.get("class") {
                obj.insert("className".to_string(), JsValue::String(class.clone()));
            }
        }
    });
    JsValue::Object(obj)
}

fn node_id_from_handle(val: &JsValue) -> Option<usize> {
    if let JsValue::Object(map) = val {
        if let Some(JsValue::Number(id)) = map.get("__node_id__") {
            return Some(*id as usize);
        }
    }
    None
}

// ── Document Methods ─────────────────────────────────────────────────────────

/// Dispatch a method call on the `document` object.
pub(super) fn call_document_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "createElement" => {
            let tag = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("div");
            if tag.eq_ignore_ascii_case("canvas") {
                return super::canvas::make_canvas_element(300, 150);
            }
            let id = alloc_node(DomNode::new_element(tag));
            element_handle(id)
        }
        "createTextNode" => {
            let text = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            let id = alloc_node(DomNode::new_text(text));
            element_handle(id)
        }
        "createDocumentFragment" => {
            let id = alloc_node(DomNode::new_fragment());
            element_handle(id)
        }
        "getElementById" => {
            let target_id = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            ensure_root();
            let found = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                nodes.iter().position(|n| n.node_type == 1 && n.attributes.get("id").map(|s| s.as_str()) == Some(target_id))
            });
            found.map(element_handle).unwrap_or(JsValue::Null)
        }
        "querySelector" => {
            let selector = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            ensure_root();
            let found = query_first(selector);
            found.map(element_handle).unwrap_or(JsValue::Null)
        }
        "querySelectorAll" => {
            let selector = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            ensure_root();
            let matches = query_all(selector);
            JsValue::Array(matches.into_iter().map(element_handle).collect())
        }
        "getElementsByClassName" => {
            let class = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            ensure_root();
            let matches = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                nodes.iter().enumerate()
                    .filter(|(_, n)| n.node_type == 1 && n.attributes.get("class").map(|c| c.split_whitespace().any(|x| x == class)).unwrap_or(false))
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>()
            });
            JsValue::Array(matches.into_iter().map(element_handle).collect())
        }
        "getElementsByTagName" => {
            let tag = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            ensure_root();
            let tag_lower = tag.to_lowercase();
            let matches = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                nodes.iter().enumerate()
                    .filter(|(_, n)| n.node_type == 1 && (n.tag == tag_lower || tag == "*"))
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>()
            });
            JsValue::Array(matches.into_iter().map(element_handle).collect())
        }
        // Traversal APIs.
        "createTreeWalker" | "createNodeIterator" | "createRange" => {
            call_document_traversal_method(method, args).unwrap_or(JsValue::Undefined)
        }
        "getSelection" => make_selection(),
        "elementFromPoint" | "elementsFromPoint" => {
            if method == "elementFromPoint" { JsValue::Null } else { JsValue::Array(Vec::new()) }
        }
        "createComment" => {
            let data = args.first().map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            let id = alloc_node(DomNode::new_text(&data));
            element_handle(id)
        }
        "hasFocus" => JsValue::Boolean(true),
        "write" | "writeln" | "open" | "close" => JsValue::Undefined,
        "execCommand" | "queryCommandSupported" | "queryCommandEnabled" => JsValue::Boolean(false),
        "getAnimations" => JsValue::Array(vec![]),
        "startViewTransition" => {
            let mut vt = HashMap::new();
            vt.insert("__type__".to_string(), JsValue::String("ViewTransition".to_string()));
            let mut p = HashMap::new();
            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            p.insert("__resolved__".to_string(), JsValue::Undefined);
            vt.insert("finished".to_string(), JsValue::Object(p.clone()));
            vt.insert("ready".to_string(), JsValue::Object(p.clone()));
            vt.insert("updateCallbackDone".to_string(), JsValue::Object(p));
            JsValue::Object(vt)
        }
        // ── Agent empowerment APIs ────────────────────────────────────────────
        "getInteractiveElements" => {
            let elements = super::agent_layer::get_interactive_elements();
            let arr: Vec<JsValue> = elements.into_iter().map(|el| {
                let mut obj = HashMap::new();
                obj.insert("__type__".to_string(), JsValue::String("InteractiveElement".to_string()));
                obj.insert("nodeId".to_string(), JsValue::Number(el.node_id as f64));
                obj.insert("role".to_string(), JsValue::String(el.role.to_string()));
                obj.insert("name".to_string(), JsValue::String(el.name));
                obj.insert("value".to_string(), JsValue::String(el.value));
                obj.insert("selector".to_string(), JsValue::String(el.selector));
                obj.insert("disabled".to_string(), JsValue::Boolean(el.disabled));
                JsValue::Object(obj)
            }).collect();
            JsValue::Array(arr)
        }
        "getInteractiveElementsText" => {
            let elements = super::agent_layer::get_interactive_elements();
            JsValue::String(super::agent_layer::interactive_elements_to_text(&elements))
        }
        "extractContent" => {
            let blocks = super::agent_layer::extract_main_content();
            let arr: Vec<JsValue> = blocks.into_iter().map(|b| {
                let mut obj = HashMap::new();
                obj.insert("heading".to_string(), JsValue::String(b.heading));
                obj.insert("text".to_string(), JsValue::String(b.text));
                JsValue::Object(obj)
            }).collect();
            JsValue::Array(arr)
        }
        "summarizePage" => {
            let summary = super::agent_layer::summarize_page();
            let mut obj = HashMap::new();
            obj.insert("__type__".to_string(), JsValue::String("PageSummary".to_string()));
            obj.insert("title".to_string(), JsValue::String(summary.title));
            obj.insert("links".to_string(), JsValue::Number(summary.link_count as f64));
            obj.insert("forms".to_string(), JsValue::Number(summary.form_count as f64));
            obj.insert("interactive".to_string(), JsValue::Number(summary.interactive_count as f64));
            obj.insert("images".to_string(), JsValue::Number(summary.image_count as f64));
            obj.insert("textLength".to_string(), JsValue::Number(summary.total_text_length as f64));
            let headings: Vec<JsValue> = summary.headings.into_iter().map(|(d, t)| {
                let mut h = HashMap::new();
                h.insert("depth".to_string(), JsValue::Number(d as f64));
                h.insert("text".to_string(), JsValue::String(t));
                JsValue::Object(h)
            }).collect();
            obj.insert("headings".to_string(), JsValue::Array(headings));
            JsValue::Object(obj)
        }
        "summarizePageText" => {
            let summary = super::agent_layer::summarize_page();
            JsValue::String(super::agent_layer::summary_to_text(&summary))
        }
        "captureState" => {
            let state = super::agent_layer::capture_dom_state();
            let mut obj = HashMap::new();
            obj.insert("__type__".to_string(), JsValue::String("DomState".to_string()));
            obj.insert("nodeCount".to_string(), JsValue::Number(state.node_count as f64));
            obj.insert("interactiveCount".to_string(), JsValue::Number(state.interactive_count as f64));
            obj.insert("textHash".to_string(), JsValue::Number(hash_to_js(state.body_text_hash)));
            JsValue::Object(obj)
        }
        "extractTables" => {
            let tables = super::agent_layer::extract_tables();
            let arr: Vec<JsValue> = tables.into_iter().map(|t| {
                let mut obj = HashMap::new();
                obj.insert("caption".to_string(), JsValue::String(t.caption));
                obj.insert("headers".to_string(), JsValue::Array(
                    t.headers.into_iter().map(JsValue::String).collect()));
                obj.insert("rows".to_string(), JsValue::Array(
                    t.rows.into_iter().map(|r| JsValue::Array(
                        r.into_iter().map(JsValue::String).collect())).collect()));
                JsValue::Object(obj)
            }).collect();
            JsValue::Array(arr)
        }
        "extractTablesText" => {
            let tables = super::agent_layer::extract_tables();
            JsValue::String(super::agent_layer::tables_to_text(&tables))
        }
        "toMarkdown" => {
            JsValue::String(super::agent_layer::page_to_markdown())
        }
        "fillForm" => {
            // Accepts an object: { fieldName: value, ... }
            let mut pairs = Vec::new();
            if let Some(JsValue::Object(map)) = args.first() {
                for (k, v) in map {
                    if k.starts_with("__") { continue; }
                    pairs.push((k.clone(), super::coercion::to_string(v)));
                }
            }
            let results = super::agent_layer::fill_form(&pairs);
            let arr: Vec<JsValue> = results.into_iter().map(|r| {
                let mut obj = HashMap::new();
                obj.insert("field".to_string(), JsValue::String(r.field));
                obj.insert("ok".to_string(), JsValue::Boolean(r.ok));
                obj.insert("reason".to_string(), JsValue::String(r.reason.to_string()));
                JsValue::Object(obj)
            }).collect();
            JsValue::Array(arr)
        }
        "getLinks" => {
            let links = super::agent_layer::get_links();
            let arr: Vec<JsValue> = links.into_iter().map(|l| {
                let mut obj = HashMap::new();
                obj.insert("text".to_string(), JsValue::String(l.text));
                obj.insert("href".to_string(), JsValue::String(l.href));
                JsValue::Object(obj)
            }).collect();
            JsValue::Array(arr)
        }
        "getLinksText" => {
            let links = super::agent_layer::get_links();
            JsValue::String(super::agent_layer::links_to_text(&links))
        }
        "findByText" => {
            let query = args.first().map(super::coercion::to_string).unwrap_or_default();
            let matches = super::agent_layer::find_by_text(&query);
            let arr: Vec<JsValue> = matches.into_iter().map(|m| {
                let mut obj = HashMap::new();
                if let JsValue::Object(handle) = element_handle(m.node_id) {
                    obj = handle;
                }
                obj.insert("selector".to_string(), JsValue::String(m.selector));
                obj.insert("exact".to_string(), JsValue::Boolean(m.exact));
                obj.insert("interactive".to_string(), JsValue::Boolean(m.interactive));
                JsValue::Object(obj)
            }).collect();
            JsValue::Array(arr)
        }
        "clickByText" => {
            let query = args.first().map(super::coercion::to_string).unwrap_or_default();
            match super::agent_layer::resolve_click_target(&query) {
                Some(id) => {
                    fire_event(id, "click");
                    JsValue::Boolean(true)
                }
                None => JsValue::Boolean(false),
            }
        }
        "exportNdaText" => {
            let doc = super::agent_layer::export_agent_state_nda();
            JsValue::String(super::agent_layer::nda_facts_to_text(&doc))
        }
        "exportNdaBytes" => {
            // Binary NDA stream as a byte array — the wire format for sessions.
            let doc = super::agent_layer::export_agent_state_nda();
            let bytes = doc.to_binary_stream();
            JsValue::Array(bytes.into_iter().map(|b| JsValue::Number(b as f64)).collect())
        }
        "diffState" => {
            // Compare a previously captured state against the current DOM.
            let current = super::agent_layer::capture_dom_state();
            let (prev_nodes, prev_interactive, prev_hash) = match args.first() {
                Some(JsValue::Object(m)) => (
                    m.get("nodeCount").map(super::coercion::to_number).unwrap_or(0.0) as usize,
                    m.get("interactiveCount").map(super::coercion::to_number).unwrap_or(0.0) as usize,
                    m.get("textHash").map(super::coercion::to_number).unwrap_or(0.0),
                ),
                _ => (0, 0, 0.0),
            };
            let text_changed = hash_to_js(current.body_text_hash) != prev_hash;
            let node_delta = current.node_count as f64 - prev_nodes as f64;
            let interactive_delta = current.interactive_count as f64 - prev_interactive as f64;
            let mut obj = HashMap::new();
            obj.insert("changed".to_string(), JsValue::Boolean(
                text_changed || node_delta != 0.0 || interactive_delta != 0.0));
            obj.insert("nodeDelta".to_string(), JsValue::Number(node_delta));
            obj.insert("interactiveDelta".to_string(), JsValue::Number(interactive_delta));
            obj.insert("textChanged".to_string(), JsValue::Boolean(text_changed));
            JsValue::Object(obj)
        }
        "waitForSettlement" => {
            // Deterministically pump the timer queue until the page stops
            // scheduling work (or the round cap trips), then report the final
            // DOM state — the agent's "page is quiet, act now" primitive.
            let mut timers_run = 0u32;
            let mut rounds = 0u32;
            while rounds < 10 {
                let ran = super::browser_env::flush_timers();
                if ran == 0 { break; }
                timers_run += ran;
                rounds += 1;
            }
            let state = super::agent_layer::capture_dom_state();
            let mut obj = HashMap::new();
            obj.insert("__type__".to_string(), JsValue::String("Settlement".to_string()));
            obj.insert("settled".to_string(), JsValue::Boolean(rounds < 10));
            obj.insert("timersRun".to_string(), JsValue::Number(timers_run as f64));
            obj.insert("nodeCount".to_string(), JsValue::Number(state.node_count as f64));
            obj.insert("interactiveCount".to_string(), JsValue::Number(state.interactive_count as f64));
            obj.insert("textHash".to_string(), JsValue::Number(hash_to_js(state.body_text_hash)));
            JsValue::Object(obj)
        }
        "getConsoleText" => {
            JsValue::String(super::console::console_output_text())
        }
        "getNetworkLog" => {
            let entries = super::browser_env::fetch_log();
            let arr: Vec<JsValue> = entries.into_iter().map(|e| {
                let mut obj = HashMap::new();
                obj.insert("url".to_string(), JsValue::String(e.url));
                obj.insert("method".to_string(), JsValue::String(e.method));
                obj.insert("status".to_string(), JsValue::Number(e.status as f64));
                obj.insert("mocked".to_string(), JsValue::Boolean(e.mocked));
                JsValue::Object(obj)
            }).collect();
            JsValue::Array(arr)
        }
        "getNetworkLogText" => {
            let entries = super::browser_env::fetch_log();
            let mut out = String::with_capacity(entries.len() * 48);
            for e in &entries {
                out.push_str(&format!("{} {} -> {}{}\n",
                    e.method, e.url, e.status, if e.mocked { " (mocked)" } else { "" }));
            }
            JsValue::String(out)
        }
        _ => JsValue::Undefined,
    }
}

/// Get a document property (body, head, documentElement, etc.)
pub(super) fn get_document_property(prop: &str) -> JsValue {
    match prop {
        "body" => {
            let root = ensure_root();
            let body = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                nodes[root].children.get(1).copied()
            });
            body.map(element_handle).unwrap_or(JsValue::Null)
        }
        "head" => {
            let root = ensure_root();
            let head = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                nodes[root].children.first().copied()
            });
            head.map(element_handle).unwrap_or(JsValue::Null)
        }
        "documentElement" => {
            let root = ensure_root();
            element_handle(root)
        }
        "cookie" => get_cookie_string(),
        "activeElement" => JsValue::Null,
        "hasFocus" => JsValue::Boolean(true),
        // Document collections (empty in the in-memory DOM unless populated).
        "forms" | "images" | "links" | "scripts" | "embeds" | "plugins" | "styleSheets" => JsValue::Array(Vec::new()),
        "all" => JsValue::Array(Vec::new()),
        // State properties.
        "fullscreenElement" | "pointerLockElement" | "pictureInPictureElement" | "currentScript" => JsValue::Null,
        "scrollingElement" => {
            let root = ensure_root();
            element_handle(root)
        }
        "designMode" => JsValue::String("off".to_string()),
        "compatMode" => JsValue::String("CSS1Compat".to_string()),
        "doctype" => JsValue::Null,
        "implementation" => {
            let mut dom_impl = HashMap::new();
            dom_impl.insert("__type__".to_string(), JsValue::String("DOMImplementation".to_string()));
            JsValue::Object(dom_impl)
        }
        "timeline" => {
            let mut timeline = HashMap::new();
            timeline.insert("__type__".to_string(), JsValue::String("DocumentTimeline".to_string()));
            timeline.insert("currentTime".to_string(), JsValue::Number(0.0));
            JsValue::Object(timeline)
        }
        "fonts" => {
            let mut fonts = HashMap::new();
            fonts.insert("__type__".to_string(), JsValue::String("FontFaceSet".to_string()));
            fonts.insert("status".to_string(), JsValue::String("loaded".to_string()));
            fonts.insert("size".to_string(), JsValue::Number(0.0));
            JsValue::Object(fonts)
        }
        _ => JsValue::Undefined,
    }
}

/// Set a document property (cookie, title).
pub(super) fn set_document_property(prop: &str, value: &JsValue) {
    if prop == "cookie" {
        let cookie = crate::js::interpreter::coercion::to_string(value);
        set_cookie(&cookie);
    }
}

// ── Cookie jar ───────────────────────────────────────────────────────────────

thread_local! {
    static COOKIE_JAR: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

fn get_cookie_string() -> JsValue {
    COOKIE_JAR.with(|jar| {
        let jar = jar.borrow();
        let s: Vec<String> = jar.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        JsValue::String(s.join("; "))
    })
}

fn set_cookie(cookie_str: &str) {
    // Parse "name=value; path=/; ..." — only store name=value.
    let pair = cookie_str.split(';').next().unwrap_or("").trim();
    if let Some(eq) = pair.find('=') {
        let name = pair[..eq].trim().to_string();
        let value = pair[eq + 1..].trim().to_string();
        COOKIE_JAR.with(|jar| {
            let mut jar = jar.borrow_mut();
            if let Some(entry) = jar.iter_mut().find(|(k, _)| *k == name) {
                entry.1 = value;
            } else {
                jar.push((name, value));
            }
        });
    }
}

// ── Element Methods ──────────────────────────────────────────────────────────

/// Dispatch a method call on an Element object.
pub(super) fn call_element_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    let Some(id) = node_id_from_handle(&JsValue::Object(map.clone())) else {
        return JsValue::Undefined;
    };

    match method {
        "getAttribute" => {
            let key = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get(key))
                    .map(|v| JsValue::String(v.clone()))
                    .unwrap_or(JsValue::Null)
            })
        }
        "setAttribute" => {
            let key = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
            let val = args.get(1).map(|v| if let JsValue::String(s) = v { s.clone() } else { super::coercion::to_string(v) }).unwrap_or_default();
            DOM_NODES.with(|nodes| {
                if let Some(node) = nodes.borrow_mut().get_mut(id) {
                    node.attributes.insert(key, val);
                }
            });
            JsValue::Undefined
        }
        "removeAttribute" => {
            let key = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            DOM_NODES.with(|nodes| {
                if let Some(node) = nodes.borrow_mut().get_mut(id) {
                    node.attributes.remove(key);
                }
            });
            JsValue::Undefined
        }
        "hasAttribute" => {
            let key = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            let has = DOM_NODES.with(|nodes| {
                nodes.borrow().get(id).map(|n| n.attributes.contains_key(key)).unwrap_or(false)
            });
            JsValue::Boolean(has)
        }
        "appendChild" => {
            if let Some(child_id) = args.first().and_then(node_id_from_handle) {
                DOM_NODES.with(|nodes| {
                    let mut nodes = nodes.borrow_mut();
                    // Remove from old parent.
                    if let Some(old_parent) = nodes.get(child_id).and_then(|n| n.parent) {
                        if let Some(p) = nodes.get_mut(old_parent) {
                            p.children.retain(|&c| c != child_id);
                        }
                    }
                    if let Some(parent) = nodes.get_mut(id) {
                        parent.children.push(child_id);
                    }
                    if let Some(child) = nodes.get_mut(child_id) {
                        child.parent = Some(id);
                    }
                });
                return args.first().cloned().unwrap_or(JsValue::Undefined);
            }
            JsValue::Undefined
        }
        "removeChild" => {
            if let Some(child_id) = args.first().and_then(node_id_from_handle) {
                DOM_NODES.with(|nodes| {
                    let mut nodes = nodes.borrow_mut();
                    if let Some(parent) = nodes.get_mut(id) {
                        parent.children.retain(|&c| c != child_id);
                    }
                    if let Some(child) = nodes.get_mut(child_id) {
                        child.parent = None;
                    }
                });
                return args.first().cloned().unwrap_or(JsValue::Undefined);
            }
            JsValue::Undefined
        }
        "insertBefore" => {
            let new_id = args.first().and_then(node_id_from_handle);
            let ref_id = args.get(1).and_then(node_id_from_handle);
            if let Some(new_id) = new_id {
                DOM_NODES.with(|nodes| {
                    let mut nodes = nodes.borrow_mut();
                    // Remove from old parent.
                    if let Some(old_parent) = nodes.get(new_id).and_then(|n| n.parent) {
                        if let Some(p) = nodes.get_mut(old_parent) {
                            p.children.retain(|&c| c != new_id);
                        }
                    }
                    if let Some(parent) = nodes.get_mut(id) {
                        let pos = ref_id.and_then(|rid| parent.children.iter().position(|&c| c == rid)).unwrap_or(parent.children.len());
                        parent.children.insert(pos, new_id);
                    }
                    if let Some(child) = nodes.get_mut(new_id) {
                        child.parent = Some(id);
                    }
                });
                return args.first().cloned().unwrap_or(JsValue::Undefined);
            }
            JsValue::Undefined
        }
        "remove" => {
            DOM_NODES.with(|nodes| {
                let mut nodes = nodes.borrow_mut();
                if let Some(parent_id) = nodes.get(id).and_then(|n| n.parent) {
                    if let Some(parent) = nodes.get_mut(parent_id) {
                        parent.children.retain(|&c| c != id);
                    }
                }
                if let Some(node) = nodes.get_mut(id) {
                    node.parent = None;
                }
            });
            JsValue::Undefined
        }
        "querySelector" => {
            let selector = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            let found = query_first_within(selector, id);
            found.map(element_handle).unwrap_or(JsValue::Null)
        }
        "querySelectorAll" => {
            let selector = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            let matches = query_all_within(selector, id);
            JsValue::Array(matches.into_iter().map(element_handle).collect())
        }
        "addEventListener" => {
            let event_type = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
            let handler = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            DOM_NODES.with(|nodes| {
                if let Some(node) = nodes.borrow_mut().get_mut(id) {
                    node.event_listeners.entry(event_type).or_default().push(handler);
                }
            });
            JsValue::Undefined
        }
        "removeEventListener" => {
            let event_type = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            DOM_NODES.with(|nodes| {
                if let Some(node) = nodes.borrow_mut().get_mut(id) {
                    node.event_listeners.remove(event_type);
                }
            });
            JsValue::Undefined
        }
        "dispatchEvent" => {
            // Fire listeners for the event type.
            let event_type = args.first().and_then(|v| {
                if let JsValue::Object(m) = v { m.get("type").and_then(|t| if let JsValue::String(s) = t { Some(s.clone()) } else { None }) } else { None }
            }).unwrap_or_default();
            let listeners = DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.event_listeners.get(&event_type))
                    .cloned()
                    .unwrap_or_default()
            });
            for listener in listeners {
                let _ = super::function::call_function(&listener, &[args.first().cloned().unwrap_or(JsValue::Undefined)], &crate::js::scope::Scope::new_global());
            }
            JsValue::Boolean(true)
        }
        "cloneNode" => {
            let deep = args.first().map(super::coercion::to_boolean).unwrap_or(false);
            let new_id = clone_node(id, deep);
            element_handle(new_id)
        }
        "contains" => {
            if let Some(other_id) = args.first().and_then(node_id_from_handle) {
                JsValue::Boolean(is_descendant_of(other_id, id))
            } else {
                JsValue::Boolean(false)
            }
        }
        "closest" => {
            let selector = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            let mut current = Some(id);
            while let Some(cid) = current {
                if matches_selector(cid, selector) {
                    return element_handle(cid);
                }
                current = DOM_NODES.with(|nodes| nodes.borrow().get(cid).and_then(|n| n.parent));
            }
            JsValue::Null
        }
        "matches" => {
            let selector = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            JsValue::Boolean(matches_selector(id, selector))
        }
        "click" => {
            fire_event(id, "click");
            JsValue::Undefined
        }
        "focus" | "blur" | "submit" | "reset" | "select" => JsValue::Undefined,
        // Scrolling.
        "scrollIntoView" | "scrollTo" | "scrollBy" | "scroll" => JsValue::Undefined,
        // Insertion.
        "insertAdjacentHTML" => {
            let position = args.first().map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            let html = args.get(1).map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            insert_adjacent_html(id, &position, &html);
            JsValue::Undefined
        }
        "insertAdjacentText" => {
            let position = args.first().map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            let text = args.get(1).map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            let text_id = alloc_node(DomNode::new_text(&text));
            insert_adjacent_node(id, &position, text_id);
            JsValue::Undefined
        }
        "insertAdjacentElement" => {
            let position = args.first().map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            if let Some(child_id) = args.get(1).and_then(node_id_from_handle) {
                insert_adjacent_node(id, &position, child_id);
                args.get(1).cloned().unwrap_or(JsValue::Null)
            } else {
                JsValue::Null
            }
        }
        "before" | "after" | "replaceWith" | "prepend" | "append" => {
            for arg in args {
                if let Some(child_id) = node_id_from_handle(arg) {
                    match method {
                        "before" => insert_adjacent_node(id, "beforebegin", child_id),
                        "after" => insert_adjacent_node(id, "afterend", child_id),
                        "prepend" => { DOM_NODES.with(|n| { let mut n = n.borrow_mut(); if let Some(node) = n.get_mut(id) { node.children.insert(0, child_id); } let c = n.get_mut(child_id); if let Some(c) = c { c.parent = Some(id); } }); }
                        "append" => { DOM_NODES.with(|n| { let mut n = n.borrow_mut(); if let Some(node) = n.get_mut(id) { node.children.push(child_id); } let c = n.get_mut(child_id); if let Some(c) = c { c.parent = Some(id); } }); }
                        "replaceWith" => { insert_adjacent_node(id, "beforebegin", child_id); remove_node_from_parent(id); }
                        _ => {}
                    }
                } else if let JsValue::String(s) = arg {
                    let text_id = alloc_node(DomNode::new_text(s));
                    match method {
                        "before" => insert_adjacent_node(id, "beforebegin", text_id),
                        "after" => insert_adjacent_node(id, "afterend", text_id),
                        "prepend" => { DOM_NODES.with(|n| { let mut n = n.borrow_mut(); if let Some(node) = n.get_mut(id) { node.children.insert(0, text_id); } let c = n.get_mut(text_id); if let Some(c) = c { c.parent = Some(id); } }); }
                        "append" => { DOM_NODES.with(|n| { let mut n = n.borrow_mut(); if let Some(node) = n.get_mut(id) { node.children.push(text_id); } let c = n.get_mut(text_id); if let Some(c) = c { c.parent = Some(id); } }); }
                        _ => {}
                    }
                }
            }
            JsValue::Undefined
        }
        // Attribute helpers.
        "getAttributeNames" => {
            let names = DOM_NODES.with(|nodes| {
                nodes.borrow().get(id).map(|n| n.attributes.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()
            });
            JsValue::Array(names.into_iter().map(JsValue::String).collect())
        }
        "toggleAttribute" => {
            let name = args.first().map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            let force = args.get(1).map(crate::js::interpreter::coercion::to_boolean);
            let has = DOM_NODES.with(|nodes| nodes.borrow().get(id).map(|n| n.attributes.contains_key(&name)).unwrap_or(false));
            let should_have = force.unwrap_or(!has);
            DOM_NODES.with(|nodes| {
                let mut nodes = nodes.borrow_mut();
                if let Some(node) = nodes.get_mut(id) {
                    if should_have { node.attributes.entry(name.clone()).or_insert_with(String::new); }
                    else { node.attributes.remove(&name); }
                }
            });
            JsValue::Boolean(should_have)
        }
        // Shadow DOM.
        "attachShadow" => {
            let mode = args.first().and_then(|v| if let JsValue::Object(m) = v { m.get("mode").map(crate::js::interpreter::coercion::to_string) } else { None }).unwrap_or_else(|| "open".into());
            let shadow_id = alloc_node(DomNode::new_element("shadow-root"));
            DOM_NODES.with(|nodes| {
                let mut nodes = nodes.borrow_mut();
                if let Some(node) = nodes.get_mut(id) {
                    node.attributes.insert("__shadow_root__".to_string(), shadow_id.to_string());
                    node.attributes.insert("__shadow_mode__".to_string(), mode.clone());
                }
                if let Some(shadow) = nodes.get_mut(shadow_id) {
                    shadow.parent = Some(id);
                }
            });
            element_handle(shadow_id)
        }
        // Web Animations API stub.
        "animate" => {
            let mut anim = HashMap::new();
            anim.insert("__type__".to_string(), JsValue::String("Animation".to_string()));
            anim.insert("playState".to_string(), JsValue::String("running".to_string()));
            anim.insert("currentTime".to_string(), JsValue::Number(0.0));
            anim.insert("playbackRate".to_string(), JsValue::Number(1.0));
            JsValue::Object(anim)
        }
        "getClientRects" => {
            JsValue::Array(vec![super::web_platform::make_dom_rect(0.0, 0.0, 0.0, 0.0)])
        }
        "getBoundingClientRect" => super::web_platform::make_dom_rect(0.0, 0.0, 0.0, 0.0),
        // Node methods.
        "hasChildNodes" => {
            let has = DOM_NODES.with(|nodes| nodes.borrow().get(id).map(|n| !n.children.is_empty()).unwrap_or(false));
            JsValue::Boolean(has)
        }
        "normalize" => {
            // Merge adjacent text nodes (simplified: no-op for in-memory DOM).
            JsValue::Undefined
        }
        "getRootNode" => {
            // Walk up to find root.
            let mut current = id;
            loop {
                let parent = DOM_NODES.with(|nodes| nodes.borrow().get(current).and_then(|n| n.parent));
                match parent {
                    Some(p) => current = p,
                    None => break,
                }
            }
            element_handle(current)
        }
        "replaceChildren" => {
            DOM_NODES.with(|nodes| {
                let mut nodes = nodes.borrow_mut();
                if let Some(node) = nodes.get_mut(id) {
                    node.children.clear();
                    node.text_content.clear();
                }
            });
            for arg in args {
                if let Some(child_id) = node_id_from_handle(arg) {
                    DOM_NODES.with(|nodes| {
                        let mut nodes = nodes.borrow_mut();
                        if let Some(node) = nodes.get_mut(id) { node.children.push(child_id); }
                        if let Some(c) = nodes.get_mut(child_id) { c.parent = Some(id); }
                    });
                }
            }
            JsValue::Undefined
        }
        "compareDocumentPosition" => JsValue::Number(0.0),
        "isSameNode" => {
            let other_id = args.first().and_then(node_id_from_handle);
            JsValue::Boolean(other_id == Some(id))
        }
        "isEqualNode" => {
            let other_id = args.first().and_then(node_id_from_handle);
            JsValue::Boolean(other_id == Some(id))
        }
        "lookupPrefix" | "lookupNamespaceURI" | "isDefaultNamespace" => JsValue::Null,
        // Fullscreen / Pointer Lock.
        "requestFullscreen" | "requestPointerLock" => {
            let mut p = HashMap::new();
            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            p.insert("__resolved__".to_string(), JsValue::Undefined);
            JsValue::Object(p)
        }
        "exitFullscreen" | "exitPointerLock" => JsValue::Undefined,
        // Pointer capture.
        "setPointerCapture" | "releasePointerCapture" => JsValue::Undefined,
        "hasPointerCapture" => JsValue::Boolean(false),
        // Animations.
        "getAnimations" => JsValue::Array(vec![]),
        // Dialog element.
        "showModal" | "show" => {
            set_node_attr(id, "open", "");
            JsValue::Undefined
        }
        "close" => {
            remove_node_attr(id, "open");
            JsValue::Undefined
        }
        // Form validation.
        "checkValidity" | "reportValidity" => JsValue::Boolean(true),
        "setCustomValidity" => JsValue::Undefined,
        // Input methods.
        "setSelectionRange" | "setRangeText" | "stepUp" | "stepDown" => JsValue::Undefined,
        // Form methods.
        "requestSubmit" => JsValue::Undefined,
        // Select methods.
        "add" | "item" | "namedItem" => JsValue::Null,
        // Image.
        "decode" => {
            let mut p = HashMap::new();
            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            p.insert("__resolved__".to_string(), JsValue::Undefined);
            JsValue::Object(p)
        }
        // Popover API.
        "showPopover" => {
            set_node_attr(id, "open", "");
            JsValue::Undefined
        }
        "hidePopover" => {
            remove_node_attr(id, "open");
            JsValue::Undefined
        }
        "togglePopover" => {
            let has = get_node_attr(id, "open").is_some();
            if has { remove_node_attr(id, "open"); } else { set_node_attr(id, "open", ""); }
            JsValue::Undefined
        }
        _ => JsValue::Undefined,
    }
}

/// Get a computed property of an Element.
pub(super) fn get_element_property(map: &HashMap<String, JsValue>, prop: &str) -> JsValue {
    let Some(id) = node_id_from_handle(&JsValue::Object(map.clone())) else {
        return JsValue::Undefined;
    };

    match prop {
        "textContent" | "innerText" => {
            let text = collect_text_content(id);
            JsValue::String(text)
        }
        "innerHTML" => {
            let html = serialize_children(id);
            JsValue::String(html)
        }
        "outerHTML" => {
            let html = serialize_node(id);
            JsValue::String(html)
        }
        "children" => {
            let children = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                nodes.get(id).map(|n| {
                    n.children.iter()
                        .filter(|&&c| nodes.get(c).map(|cn| cn.node_type == 1).unwrap_or(false))
                        .copied()
                        .collect::<Vec<_>>()
                }).unwrap_or_default()
            });
            JsValue::Array(children.into_iter().map(element_handle).collect())
        }
        "childNodes" => {
            let children = DOM_NODES.with(|nodes| {
                nodes.borrow().get(id).map(|n| n.children.clone()).unwrap_or_default()
            });
            JsValue::Array(children.into_iter().map(element_handle).collect())
        }
        "parentNode" | "parentElement" => {
            let parent = DOM_NODES.with(|nodes| nodes.borrow().get(id).and_then(|n| n.parent));
            parent.map(element_handle).unwrap_or(JsValue::Null)
        }
        "firstChild" | "firstElementChild" => {
            let first = DOM_NODES.with(|nodes| nodes.borrow().get(id).and_then(|n| n.children.first().copied()));
            first.map(element_handle).unwrap_or(JsValue::Null)
        }
        "lastChild" | "lastElementChild" => {
            let last = DOM_NODES.with(|nodes| nodes.borrow().get(id).and_then(|n| n.children.last().copied()));
            last.map(element_handle).unwrap_or(JsValue::Null)
        }
        "nextSibling" | "nextElementSibling" => {
            let sibling = get_sibling_node(id, 1);
            sibling.map(element_handle).unwrap_or(JsValue::Null)
        }
        "previousSibling" | "previousElementSibling" => {
            let sibling = get_sibling_node(id, -1);
            sibling.map(element_handle).unwrap_or(JsValue::Null)
        }
        "childElementCount" => {
            let count = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                nodes.get(id).map(|n| {
                    n.children.iter().filter(|&&c| nodes.get(c).map(|cn| cn.node_type == 1).unwrap_or(false)).count()
                }).unwrap_or(0)
            });
            JsValue::Number(count as f64)
        }
        "value" | "checked" | "disabled" | "href" | "src" | "alt" | "title" | "type" | "name" | "placeholder" => {
            DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get(prop))
                    .map(|v| JsValue::String(v.clone()))
                    .unwrap_or(JsValue::String(String::new()))
            })
        }
        "classList" => {
            // Return a classList-like object.
            let mut obj = HashMap::new();
            obj.insert("__type__".to_string(), JsValue::String("DOMTokenList".to_string()));
            obj.insert("__node_id__".to_string(), JsValue::Number(id as f64));
            JsValue::Object(obj)
        }
        "style" => {
            let mut obj = HashMap::new();
            obj.insert("__type__".to_string(), JsValue::String("CSSStyleDeclaration".to_string()));
            obj.insert("__node_id__".to_string(), JsValue::Number(id as f64));
            JsValue::Object(obj)
        }
        "dataset" => {
            let mut obj = HashMap::new();
            obj.insert("__type__".to_string(), JsValue::String("DOMStringMap".to_string()));
            obj.insert("__node_id__".to_string(), JsValue::Number(id as f64));
            JsValue::Object(obj)
        }
        "shadowRoot" => {
            let shadow_id = DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get("__shadow_root__"))
                    .and_then(|s| s.parse::<usize>().ok())
            });
            match shadow_id {
                Some(sid) => element_handle(sid),
                None => JsValue::Null,
            }
        }
        "offsetWidth" | "offsetHeight" | "offsetTop" | "offsetLeft" => JsValue::Number(0.0),
        "scrollWidth" | "scrollHeight" | "scrollTop" | "scrollLeft" | "clientWidth" | "clientHeight" => JsValue::Number(0.0),
        "isConnected" => JsValue::Boolean(true),
        // Boolean properties (attribute-backed).
        "hidden" | "inert" | "draggable" | "spellcheck" | "contentEditable" => {
            let has = DOM_NODES.with(|nodes| nodes.borrow().get(id).map(|n| n.attributes.contains_key(prop)).unwrap_or(false));
            JsValue::Boolean(has)
        }
        "tabIndex" => {
            let val = DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get("tabindex"))
                    .and_then(|v| v.parse::<f64>().ok())
            });
            JsValue::Number(val.unwrap_or(-1.0))
        }
        "slot" | "accessKey" | "dir" | "lang" | "role" | "ariaLabel" => {
            DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get(prop))
                    .map(|v| JsValue::String(v.clone()))
                    .unwrap_or(JsValue::String(String::new()))
            })
        }
        "part" => {
            let mut obj = HashMap::new();
            obj.insert("__type__".to_string(), JsValue::String("DOMTokenList".to_string()));
            obj.insert("__node_id__".to_string(), JsValue::Number(id as f64));
            obj.insert("__attr__".to_string(), JsValue::String("part".to_string()));
            JsValue::Object(obj)
        }
        "assignedSlot" => JsValue::Null,
        // Dialog properties.
        "open" => {
            let has = DOM_NODES.with(|nodes| nodes.borrow().get(id).map(|n| n.attributes.contains_key("open")).unwrap_or(false));
            JsValue::Boolean(has)
        }
        "returnValue" => {
            DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get("data-returnvalue"))
                    .map(|v| JsValue::String(v.clone()))
                    .unwrap_or(JsValue::String(String::new()))
            })
        }
        // Popover.
        "popover" => {
            DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get("popover"))
                    .map(|v| JsValue::String(v.clone()))
                    .unwrap_or(JsValue::Null)
            })
        }
        // Form properties.
        "validity" => {
            let mut v = HashMap::new();
            v.insert("__type__".to_string(), JsValue::String("ValidityState".to_string()));
            v.insert("valid".to_string(), JsValue::Boolean(true));
            v.insert("valueMissing".to_string(), JsValue::Boolean(false));
            v.insert("typeMismatch".to_string(), JsValue::Boolean(false));
            v.insert("patternMismatch".to_string(), JsValue::Boolean(false));
            v.insert("tooLong".to_string(), JsValue::Boolean(false));
            v.insert("tooShort".to_string(), JsValue::Boolean(false));
            v.insert("rangeUnderflow".to_string(), JsValue::Boolean(false));
            v.insert("rangeOverflow".to_string(), JsValue::Boolean(false));
            v.insert("stepMismatch".to_string(), JsValue::Boolean(false));
            v.insert("badInput".to_string(), JsValue::Boolean(false));
            v.insert("customError".to_string(), JsValue::Boolean(false));
            JsValue::Object(v)
        }
        "validationMessage" => JsValue::String(String::new()),
        "willValidate" => JsValue::Boolean(true),
        // Input-specific properties.
        "selectionStart" | "selectionEnd" => JsValue::Number(0.0),
        "selectionDirection" => JsValue::String("none".to_string()),
        "files" => JsValue::Array(Vec::new()),
        "indeterminate" => JsValue::Boolean(false),
        "required" | "readOnly" | "multiple" | "autofocus" | "noValidate" => {
            let attr = prop.to_lowercase();
            let has = DOM_NODES.with(|nodes| nodes.borrow().get(id).map(|n| n.attributes.contains_key(&attr)).unwrap_or(false));
            JsValue::Boolean(has)
        }
        "min" | "max" | "step" | "pattern" | "accept" | "autocomplete" | "inputMode" | "enterKeyHint" => {
            DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get(prop))
                    .map(|v| JsValue::String(v.clone()))
                    .unwrap_or(JsValue::String(String::new()))
            })
        }
        "valueAsNumber" => {
            let val = DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get("value"))
                    .and_then(|v| v.parse::<f64>().ok())
            });
            JsValue::Number(val.unwrap_or(f64::NAN))
        }
        "valueAsDate" => JsValue::Null,
        "labels" => JsValue::Array(Vec::new()),
        "form" => JsValue::Null,
        // Select-specific properties.
        "selectedIndex" => JsValue::Number(-1.0),
        "selectedOptions" | "options" => JsValue::Array(Vec::new()),
        "length" => JsValue::Number(0.0),
        "size" => JsValue::Number(0.0),
        // Form-specific properties.
        "elements" => JsValue::Array(Vec::new()),
        "action" | "method" | "enctype" | "target" | "encoding" => {
            DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get(prop))
                    .map(|v| JsValue::String(v.clone()))
                    .unwrap_or(JsValue::String(String::new()))
            })
        }
        // Image-specific.
        "naturalWidth" | "naturalHeight" => JsValue::Number(0.0),
        "complete" => JsValue::Boolean(true),
        "currentSrc" => JsValue::String(String::new()),
        // Anchor-specific (URL decomposition).
        "protocol" | "host" | "hostname" | "port" | "pathname" | "search" | "hash" | "origin" => {
            let href = DOM_NODES.with(|nodes| {
                nodes.borrow().get(id)
                    .and_then(|n| n.attributes.get("href"))
                    .cloned()
            }).unwrap_or_default();
            JsValue::String(href)
        }
        _ => {
            // Fall through to stored properties on the handle.
            map.get(prop).cloned().unwrap_or(JsValue::Undefined)
        }
    }
}

/// Set a property on an Element (textContent, innerHTML, value, etc.)
pub(super) fn set_element_property(map: &HashMap<String, JsValue>, prop: &str, value: &JsValue) {
    let Some(id) = node_id_from_handle(&JsValue::Object(map.clone())) else { return; };

    match prop {
        "textContent" | "innerText" => {
            let text = super::coercion::to_string(value);
            DOM_NODES.with(|nodes| {
                let mut nodes = nodes.borrow_mut();
                if let Some(node) = nodes.get_mut(id) {
                    node.children.clear();
                }
                let text_id = nodes.len();
                nodes.push(DomNode::new_text(&text));
                if let Some(text_node) = nodes.get_mut(text_id) {
                    text_node.parent = Some(id);
                }
                if let Some(node) = nodes.get_mut(id) {
                    node.children.push(text_id);
                }
            });
        }
        "innerHTML" => {
            let html = super::coercion::to_string(value);
            set_inner_html(id, &html);
        }
        "id" | "className" | "value" | "href" | "src" | "alt" | "title" | "type" | "name" | "placeholder" | "disabled" | "checked" => {
            let val = super::coercion::to_string(value);
            let attr_name = if prop == "className" { "class" } else { prop };
            DOM_NODES.with(|nodes| {
                if let Some(node) = nodes.borrow_mut().get_mut(id) {
                    node.attributes.insert(attr_name.to_string(), val);
                }
            });
        }
        _ => {}
    }
}

// ── Selector Engine (minimal) ────────────────────────────────────────────────

fn query_first(selector: &str) -> Option<usize> {
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        nodes.iter().position(|n| n.node_type == 1 && matches_selector_raw(n, selector))
    })
}

fn query_all(selector: &str) -> Vec<usize> {
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        nodes.iter().enumerate()
            .filter(|(_, n)| n.node_type == 1 && matches_selector_raw(n, selector))
            .map(|(i, _)| i)
            .collect()
    })
}

fn query_first_within(selector: &str, ancestor: usize) -> Option<usize> {
    let descendants = get_descendants(ancestor);
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        descendants.into_iter().find(|&id| {
            nodes.get(id).map(|n| n.node_type == 1 && matches_selector_raw(n, selector)).unwrap_or(false)
        })
    })
}

fn query_all_within(selector: &str, ancestor: usize) -> Vec<usize> {
    let descendants = get_descendants(ancestor);
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        descendants.into_iter()
            .filter(|&id| nodes.get(id).map(|n| n.node_type == 1 && matches_selector_raw(n, selector)).unwrap_or(false))
            .collect()
    })
}

fn matches_selector(id: usize, selector: &str) -> bool {
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        nodes.get(id).map(|n| matches_selector_raw(n, selector)).unwrap_or(false)
    })
}

/// Minimal selector matching: `#id`, `.class`, `tag`, `[attr]`, `[attr=val]`,
/// and comma-separated lists.
fn matches_selector_raw(node: &DomNode, selector: &str) -> bool {
    // Handle comma-separated selectors.
    if selector.contains(',') {
        return selector.split(',').any(|s| matches_selector_raw(node, s.trim()));
    }
    let sel = selector.trim();
    if sel.is_empty() || sel == "*" { return true; }

    // Compound selectors (e.g., "div.foo#bar").
    // For simplicity, handle single-part selectors and basic compounds.
    if sel.starts_with('#') {
        return node.attributes.get("id").map(|s| s.as_str()) == Some(&sel[1..]);
    }
    if sel.starts_with('.') {
        let class = &sel[1..];
        return node.attributes.get("class").map(|c| c.split_whitespace().any(|x| x == class)).unwrap_or(false);
    }
    if sel.starts_with('[') && sel.ends_with(']') {
        let inner = &sel[1..sel.len()-1];
        if let Some((attr, val)) = inner.split_once('=') {
            let val = val.trim_matches('"').trim_matches('\'');
            return node.attributes.get(attr).map(|v| v.as_str()) == Some(val);
        }
        return node.attributes.contains_key(inner);
    }
    // Bare tag name.
    node.tag == sel.to_lowercase()
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn get_descendants(root: usize) -> Vec<usize> {
    let mut result = Vec::new();
    let mut stack = vec![root];
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        while let Some(id) = stack.pop() {
            if let Some(node) = nodes.get(id) {
                for &child in node.children.iter().rev() {
                    result.push(child);
                    stack.push(child);
                }
            }
        }
    });
    result
}

fn is_descendant_of(child: usize, ancestor: usize) -> bool {
    let mut current = DOM_NODES.with(|nodes| nodes.borrow().get(child).and_then(|n| n.parent));
    while let Some(id) = current {
        if id == ancestor { return true; }
        current = DOM_NODES.with(|nodes| nodes.borrow().get(id).and_then(|n| n.parent));
    }
    false
}

fn get_sibling_node(id: usize, offset: i32) -> Option<usize> {
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        let parent_id = nodes.get(id)?.parent?;
        let parent = nodes.get(parent_id)?;
        let pos = parent.children.iter().position(|&c| c == id)? as i32;
        let target = pos + offset;
        if target >= 0 && (target as usize) < parent.children.len() {
            Some(parent.children[target as usize])
        } else { None }
    })
}

fn clone_node(id: usize, deep: bool) -> usize {
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        let Some(node) = nodes.get(id) else {
            return alloc_node(DomNode::new_element("div"));
        };
        let mut new_node = node.clone();
        new_node.children.clear();
        new_node.parent = None;
        drop(nodes);
        let new_id = alloc_node(new_node);
        if deep {
            let children = DOM_NODES.with(|nodes| {
                nodes.borrow().get(id).map(|n| n.children.clone()).unwrap_or_default()
            });
            for child in children {
                let cloned_child = clone_node(child, true);
                DOM_NODES.with(|nodes| {
                    let mut nodes = nodes.borrow_mut();
                    if let Some(parent) = nodes.get_mut(new_id) {
                        parent.children.push(cloned_child);
                    }
                    if let Some(child_node) = nodes.get_mut(cloned_child) {
                        child_node.parent = Some(new_id);
                    }
                });
            }
        }
        new_id
    })
}

fn collect_text_content(id: usize) -> String {
    let mut out = String::new();
    collect_text_recursive(id, &mut out);
    out
}

fn collect_text_recursive(id: usize, out: &mut String) {
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        if let Some(node) = nodes.get(id) {
            if node.node_type == 3 {
                out.push_str(&node.text_content);
            }
            for &child in &node.children {
                collect_text_recursive(child, out);
            }
        }
    });
}

fn serialize_children(id: usize) -> String {
    let mut out = String::new();
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        if let Some(node) = nodes.get(id) {
            for &child in &node.children {
                serialize_node_inner(child, &nodes, &mut out);
            }
        }
    });
    out
}

fn serialize_node(id: usize) -> String {
    let mut out = String::new();
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        serialize_node_inner(id, &nodes, &mut out);
    });
    out
}

fn serialize_node_inner(id: usize, nodes: &[DomNode], out: &mut String) {
    if let Some(node) = nodes.get(id) {
        if node.node_type == 3 {
            out.push_str(&node.text_content);
            return;
        }
        out.push('<');
        out.push_str(&node.tag);
        for (k, v) in &node.attributes {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            out.push_str(v);
            out.push('"');
        }
        out.push('>');
        for &child in &node.children {
            serialize_node_inner(child, nodes, out);
        }
        out.push_str("</");
        out.push_str(&node.tag);
        out.push('>');
    }
}

// ── DOMTokenList (classList) ─────────────────────────────────────────────────

fn get_classes(node_id: usize) -> Vec<String> {
    DOM_NODES.with(|nodes| {
        nodes.borrow().get(node_id)
            .and_then(|n| n.attributes.get("class"))
            .map(|c| c.split_whitespace().map(String::from).collect())
            .unwrap_or_default()
    })
}

fn set_classes(node_id: usize, classes: &[String]) {
    let val = classes.join(" ");
    DOM_NODES.with(|nodes| {
        if let Some(node) = nodes.borrow_mut().get_mut(node_id) {
            if val.is_empty() {
                node.attributes.remove("class");
            } else {
                node.attributes.insert("class".to_string(), val);
            }
        }
    });
}

pub(super) fn call_dom_token_list_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    let node_id = map.get("__node_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
    match method {
        "add" => {
            let mut classes = get_classes(node_id);
            for arg in args {
                let cls = super::coercion::to_string(arg);
                if !cls.is_empty() && !classes.contains(&cls) {
                    classes.push(cls);
                }
            }
            set_classes(node_id, &classes);
            JsValue::Undefined
        }
        "remove" => {
            let mut classes = get_classes(node_id);
            for arg in args {
                let cls = super::coercion::to_string(arg);
                classes.retain(|c| c != &cls);
            }
            set_classes(node_id, &classes);
            JsValue::Undefined
        }
        "toggle" => {
            let cls = args.first().map(super::coercion::to_string).unwrap_or_default();
            let force = args.get(1).and_then(|v| if let JsValue::Boolean(b) = v { Some(*b) } else { None });
            let mut classes = get_classes(node_id);
            let has = classes.contains(&cls);
            let result = match force {
                Some(true) => { if !has { classes.push(cls.clone()); } true }
                Some(false) => { classes.retain(|c| c != &cls); false }
                None => {
                    if has { classes.retain(|c| c != &cls); false }
                    else { classes.push(cls.clone()); true }
                }
            };
            set_classes(node_id, &classes);
            JsValue::Boolean(result)
        }
        "contains" => {
            let cls = args.first().map(super::coercion::to_string).unwrap_or_default();
            JsValue::Boolean(get_classes(node_id).contains(&cls))
        }
        "replace" => {
            let old = args.first().map(super::coercion::to_string).unwrap_or_default();
            let new = args.get(1).map(super::coercion::to_string).unwrap_or_default();
            let mut classes = get_classes(node_id);
            let found = classes.contains(&old);
            if found {
                for c in classes.iter_mut() {
                    if c == &old { *c = new.clone(); }
                }
                set_classes(node_id, &classes);
            }
            JsValue::Boolean(found)
        }
        "supports" => JsValue::Boolean(true),
        "item" => {
            let idx = args.first().map(super::coercion::to_number).unwrap_or(0.0) as usize;
            let classes = get_classes(node_id);
            classes.get(idx).map(|c| JsValue::String(c.clone())).unwrap_or(JsValue::Null)
        }
        "forEach" => JsValue::Undefined,
        "entries" | "keys" | "values" => {
            let classes = get_classes(node_id);
            let items: Vec<JsValue> = classes.into_iter().map(JsValue::String).collect();
            JsValue::Array(items)
        }
        "toString" => {
            let classes = get_classes(node_id);
            JsValue::String(classes.join(" "))
        }
        _ => JsValue::Undefined,
    }
}

pub(super) fn dom_token_list_length(map: &HashMap<String, JsValue>) -> JsValue {
    let node_id = map.get("__node_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
    JsValue::Number(get_classes(node_id).len() as f64)
}

// ── DOMStringMap (dataset) ───────────────────────────────────────────────────

pub(super) fn get_dataset_property(map: &HashMap<String, JsValue>, prop: &str) -> JsValue {
    let node_id = map.get("__node_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
    // Convert camelCase to kebab-case for data-* attribute lookup.
    let attr = format!("data-{}", camel_to_kebab(prop));
    DOM_NODES.with(|nodes| {
        nodes.borrow().get(node_id)
            .and_then(|n| n.attributes.get(&attr))
            .map(|v| JsValue::String(v.clone()))
            .unwrap_or(JsValue::Undefined)
    })
}

pub(super) fn set_dataset_property(map: &HashMap<String, JsValue>, prop: &str, value: &JsValue) {
    let node_id = map.get("__node_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
    let attr = format!("data-{}", camel_to_kebab(prop));
    let val = super::coercion::to_string(value);
    DOM_NODES.with(|nodes| {
        if let Some(node) = nodes.borrow_mut().get_mut(node_id) {
            node.attributes.insert(attr, val);
        }
    });
}

fn camel_to_kebab(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

// ── innerHTML parsing ────────────────────────────────────────────────────────

/// Parse an HTML string and replace the node's children with the parsed tree.
pub(super) fn set_inner_html(node_id: usize, html: &str) {
    use crate::parser::html::HtmlParser;
    let parsed = HtmlParser::parse(html);
    DOM_NODES.with(|nodes| {
        let mut nodes = nodes.borrow_mut();
        // Clear existing children.
        if let Some(node) = nodes.get_mut(node_id) {
            node.children.clear();
            node.text_content.clear();
        }
        // Convert parsed nodes (skip root document node at index 0).
        // Build a mapping from parser IDs to our IDs.
        let mut id_map: HashMap<usize, usize> = HashMap::new();
        for pnode in parsed.iter().skip(1) {
            let our_id = nodes.len();
            id_map.insert(pnode.id, our_id);
            let mut dom_node = if pnode.node_type == crate::parser::html::NodeType::Text {
                DomNode::new_text(&pnode.text_content)
            } else {
                DomNode::new_element(&pnode.tag_name)
            };
            for (k, v) in &pnode.attributes {
                dom_node.attributes.insert(k.clone(), v.clone());
            }
            nodes.push(dom_node);
        }
        // Wire parent-child relationships using the parser's parent field only.
        for pnode in parsed.iter().skip(1) {
            if let Some(&our_id) = id_map.get(&pnode.id) {
                if let Some(parent_parser_id) = pnode.parent {
                    if parent_parser_id == 0 {
                        // Parent is the document root → attach to our target node.
                        nodes[our_id].parent = Some(node_id);
                        if let Some(target) = nodes.get_mut(node_id) {
                            target.children.push(our_id);
                        }
                    } else if let Some(&our_parent_id) = id_map.get(&parent_parser_id) {
                        nodes[our_id].parent = Some(our_parent_id);
                        if let Some(parent) = nodes.get_mut(our_parent_id) {
                            parent.children.push(our_id);
                        }
                    }
                } else {
                    // No parent in parser → attach to target node.
                    nodes[our_id].parent = Some(node_id);
                    if let Some(target) = nodes.get_mut(node_id) {
                        target.children.push(our_id);
                    }
                }
            }
        }
    });
}

// ── TreeWalker ───────────────────────────────────────────────────────────────

pub(super) fn make_tree_walker(root_id: usize, _what_to_show: u32) -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("TreeWalker".to_string()));
    map.insert("__root_id__".to_string(), JsValue::Number(root_id as f64));
    map.insert("__current_id__".to_string(), JsValue::Number(root_id as f64));
    map.insert("currentNode".to_string(), element_handle(root_id));
    JsValue::Object(map)
}

pub(super) fn call_tree_walker_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> JsValue {
    let current = map.get("__current_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
    let root = map.get("__root_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
    match method {
        "nextNode" => {
            // DFS next: first child, then next sibling, then ancestor's next sibling.
            let next = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                fn find_next(id: usize, root: usize, nodes: &[DomNode]) -> Option<usize> {
                    if let Some(node) = nodes.get(id) {
                        if let Some(&first) = node.children.first() { return Some(first); }
                        let mut cur = id;
                        while cur != root {
                            if let Some(n) = nodes.get(cur) {
                                if let Some(parent_id) = n.parent {
                                    if let Some(parent) = nodes.get(parent_id) {
                                        if let Some(pos) = parent.children.iter().position(|&c| c == cur) {
                                            if pos + 1 < parent.children.len() {
                                                return Some(parent.children[pos + 1]);
                                            }
                                        }
                                    }
                                    cur = parent_id;
                                } else { break; }
                            } else { break; }
                        }
                    }
                    None
                }
                find_next(current, root, &nodes)
            });
            match next {
                Some(id) => element_handle(id),
                None => JsValue::Null,
            }
        }
        "previousNode" => {
            let prev = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                if let Some(node) = nodes.get(current) {
                    if let Some(parent_id) = node.parent {
                        if let Some(parent) = nodes.get(parent_id) {
                            if let Some(pos) = parent.children.iter().position(|&c| c == current) {
                                if pos > 0 {
                                    // Go to last descendant of previous sibling.
                                    let mut target = parent.children[pos - 1];
                                    loop {
                                        let children = nodes.get(target).map(|n| n.children.clone()).unwrap_or_default();
                                        if let Some(&last) = children.last() { target = last; } else { break; }
                                    }
                                    return Some(target);
                                }
                            }
                        }
                        return Some(parent_id);
                    }
                }
                None
            });
            match prev {
                Some(id) => element_handle(id),
                None => JsValue::Null,
            }
        }
        "parentNode" => {
            let parent = DOM_NODES.with(|nodes| {
                nodes.borrow().get(current).and_then(|n| n.parent)
            });
            match parent {
                Some(id) if id != root => element_handle(id),
                _ => JsValue::Null,
            }
        }
        "firstChild" => {
            let first = DOM_NODES.with(|nodes| {
                nodes.borrow().get(current).and_then(|n| n.children.first().copied())
            });
            match first {
                Some(id) => element_handle(id),
                None => JsValue::Null,
            }
        }
        "lastChild" => {
            let last = DOM_NODES.with(|nodes| {
                nodes.borrow().get(current).and_then(|n| n.children.last().copied())
            });
            match last {
                Some(id) => element_handle(id),
                None => JsValue::Null,
            }
        }
        "nextSibling" => {
            let sibling = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                if let Some(node) = nodes.get(current) {
                    if let Some(parent_id) = node.parent {
                        if let Some(parent) = nodes.get(parent_id) {
                            if let Some(pos) = parent.children.iter().position(|&c| c == current) {
                                if pos + 1 < parent.children.len() {
                                    return Some(parent.children[pos + 1]);
                                }
                            }
                        }
                    }
                }
                None
            });
            match sibling {
                Some(id) => element_handle(id),
                None => JsValue::Null,
            }
        }
        "previousSibling" => {
            let sibling = DOM_NODES.with(|nodes| {
                let nodes = nodes.borrow();
                if let Some(node) = nodes.get(current) {
                    if let Some(parent_id) = node.parent {
                        if let Some(parent) = nodes.get(parent_id) {
                            if let Some(pos) = parent.children.iter().position(|&c| c == current) {
                                if pos > 0 {
                                    return Some(parent.children[pos - 1]);
                                }
                            }
                        }
                    }
                }
                None
            });
            match sibling {
                Some(id) => element_handle(id),
                None => JsValue::Null,
            }
        }
        _ => JsValue::Undefined,
    }
}

// ── NodeIterator ─────────────────────────────────────────────────────────────

pub(super) fn make_node_iterator(root_id: usize) -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("NodeIterator".to_string()));
    map.insert("__root_id__".to_string(), JsValue::Number(root_id as f64));
    map.insert("__index__".to_string(), JsValue::Number(0.0));
    map.insert("referenceNode".to_string(), element_handle(root_id));
    JsValue::Object(map)
}

pub(super) fn call_node_iterator_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> JsValue {
    let root = map.get("__root_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
    match method {
        "nextNode" => {
            // Collect all nodes in DFS order from root, return next one.
            let all = collect_dfs(root);
            let idx = map.get("__index__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
            if idx < all.len() {
                let id = all[idx];
                element_handle(id)
            } else {
                JsValue::Null
            }
        }
        "previousNode" => {
            let all = collect_dfs(root);
            let idx = map.get("__index__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
            if idx > 0 && idx - 1 < all.len() {
                let id = all[idx - 1];
                element_handle(id)
            } else {
                JsValue::Null
            }
        }
        "detach" => JsValue::Undefined,
        _ => JsValue::Undefined,
    }
}

fn collect_dfs(root: usize) -> Vec<usize> {
    DOM_NODES.with(|nodes| {
        let nodes = nodes.borrow();
        let mut result = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            result.push(id);
            if let Some(node) = nodes.get(id) {
                for &child in node.children.iter().rev() {
                    stack.push(child);
                }
            }
        }
        result
    })
}

// ── Range ────────────────────────────────────────────────────────────────────

pub(super) fn make_range() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Range".to_string()));
    map.insert("collapsed".to_string(), JsValue::Boolean(true));
    map.insert("startOffset".to_string(), JsValue::Number(0.0));
    map.insert("endOffset".to_string(), JsValue::Number(0.0));
    JsValue::Object(map)
}

pub(super) fn call_range_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "setStart" | "setEnd" => {
            let mut m = map.clone();
            let node = args.first().cloned().unwrap_or(JsValue::Undefined);
            let offset = args.get(1).map(super::coercion::to_number).unwrap_or(0.0);
            if method == "setStart" {
                m.insert("startContainer".to_string(), node);
                m.insert("startOffset".to_string(), JsValue::Number(offset));
            } else {
                m.insert("endContainer".to_string(), node);
                m.insert("endOffset".to_string(), JsValue::Number(offset));
                m.insert("collapsed".to_string(), JsValue::Boolean(false));
            }
            JsValue::Object(m)
        }
        "selectNode" | "selectNodeContents" => {
            let mut m = map.clone();
            let node = args.first().cloned().unwrap_or(JsValue::Undefined);
            m.insert("startContainer".to_string(), node.clone());
            m.insert("endContainer".to_string(), node);
            m.insert("collapsed".to_string(), JsValue::Boolean(false));
            JsValue::Object(m)
        }
        "collapse" => {
            let mut m = map.clone();
            m.insert("collapsed".to_string(), JsValue::Boolean(true));
            JsValue::Object(m)
        }
        "cloneRange" => JsValue::Object(map.clone()),
        "deleteContents" | "extractContents" => {
            let mut frag = HashMap::new();
            frag.insert("__type__".to_string(), JsValue::String("DocumentFragment".to_string()));
            JsValue::Object(frag)
        }
        "cloneContents" => {
            let mut frag = HashMap::new();
            frag.insert("__type__".to_string(), JsValue::String("DocumentFragment".to_string()));
            JsValue::Object(frag)
        }
        "insertNode" => JsValue::Undefined,
        "createContextualFragment" => {
            let html = args.first().map(super::coercion::to_string).unwrap_or_default();
            let mut frag = HashMap::new();
            frag.insert("__type__".to_string(), JsValue::String("DocumentFragment".to_string()));
            frag.insert("__html__".to_string(), JsValue::String(html));
            JsValue::Object(frag)
        }
        "toString" => JsValue::String(String::new()),
        "getBoundingClientRect" => super::web_platform::make_dom_rect(0.0, 0.0, 0.0, 0.0),
        "getClientRects" => JsValue::Array(Vec::new()),
        "detach" => JsValue::Undefined,
        "intersectsNode" => JsValue::Boolean(true),
        "compareBoundaryPoints" => JsValue::Number(0.0),
        "surroundContents" => JsValue::Undefined,
        _ => JsValue::Undefined,
    }
}

// ── Selection ────────────────────────────────────────────────────────────────

pub(super) fn make_selection() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Selection".to_string()));
    map.insert("anchorOffset".to_string(), JsValue::Number(0.0));
    map.insert("focusOffset".to_string(), JsValue::Number(0.0));
    map.insert("isCollapsed".to_string(), JsValue::Boolean(true));
    map.insert("rangeCount".to_string(), JsValue::Number(0.0));
    map.insert("type".to_string(), JsValue::String("None".to_string()));
    JsValue::Object(map)
}

pub(super) fn call_selection_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> JsValue {
    match method {
        "getRangeAt" => make_range(),
        "addRange" => {
            let mut m = map.clone();
            m.insert("rangeCount".to_string(), JsValue::Number(1.0));
            m.insert("type".to_string(), JsValue::String("Range".to_string()));
            JsValue::Object(m)
        }
        "removeAllRanges" | "empty" => {
            let mut m = map.clone();
            m.insert("rangeCount".to_string(), JsValue::Number(0.0));
            m.insert("type".to_string(), JsValue::String("None".to_string()));
            JsValue::Object(m)
        }
        "removeRange" => JsValue::Object(map.clone()),
        "collapse" | "collapseToStart" | "collapseToEnd" => {
            let mut m = map.clone();
            m.insert("isCollapsed".to_string(), JsValue::Boolean(true));
            JsValue::Object(m)
        }
        "extend" | "selectAllChildren" | "setBaseAndExtent" | "setPosition" => JsValue::Object(map.clone()),
        "toString" => JsValue::String(String::new()),
        "containsNode" => JsValue::Boolean(false),
        "deleteFromDocument" => JsValue::Undefined,
        "modify" => JsValue::Undefined,
        _ => JsValue::Undefined,
    }
}

// ── document.createTreeWalker / createNodeIterator / createRange ─────────────

pub(super) fn call_document_traversal_method(method: &str, args: &[JsValue]) -> Option<JsValue> {
    match method {
        "createTreeWalker" => {
            let root_id = args.first().and_then(node_id_from_handle).unwrap_or(0);
            let what_to_show = args.get(1).map(super::coercion::to_number).unwrap_or(0xFFFF_FFFF_u32 as f64) as u32;
            Some(make_tree_walker(root_id, what_to_show))
        }
        "createNodeIterator" => {
            let root_id = args.first().and_then(node_id_from_handle).unwrap_or(0);
            Some(make_node_iterator(root_id))
        }
        "createRange" => Some(make_range()),
        _ => None,
    }
}

// ── Attribute helpers ────────────────────────────────────────────────────────

/// Truncate a 64-bit hash to the 53-bit range exactly representable in f64,
/// so round-tripping through a JS number stays lossless.
fn hash_to_js(hash: u64) -> f64 {
    (hash & ((1u64 << 53) - 1)) as f64
}

pub(super) fn set_node_attr(id: usize, key: &str, val: &str) {
    DOM_NODES.with(|nodes| {
        if let Some(node) = nodes.borrow_mut().get_mut(id) {
            node.attributes.insert(key.to_string(), val.to_string());
        }
    });
}

// ── Event firing ─────────────────────────────────────────────────────────────

/// Fire an event on a node, bubbling up through ancestors.
///
/// Runs all registered listeners for `event_type` on the target and each
/// ancestor (capture phase is not modeled — agents care about effects).
pub(super) fn fire_event(target_id: usize, event_type: &str) {
    // Build the event object once.
    let mut event = HashMap::new();
    event.insert("__type__".to_string(), JsValue::String("Event".to_string()));
    event.insert("type".to_string(), JsValue::String(event_type.to_string()));
    event.insert("target".to_string(), element_handle(target_id));
    event.insert("bubbles".to_string(), JsValue::Boolean(true));
    let event_val = JsValue::Object(event);

    // Collect the bubble path (target → root) and each node's listeners
    // up-front so no borrow is held while listeners run.
    let mut path = Vec::new();
    let mut current = Some(target_id);
    while let Some(id) = current {
        let (listeners, parent) = DOM_NODES.with(|nodes| {
            let nodes = nodes.borrow();
            match nodes.get(id) {
                Some(n) => (
                    n.event_listeners.get(event_type).cloned().unwrap_or_default(),
                    n.parent,
                ),
                None => (Vec::new(), None),
            }
        });
        path.push(listeners);
        current = parent;
    }

    for listeners in path {
        for listener in listeners {
            let _ = super::function::call_function(
                &listener,
                &[event_val.clone()],
                &crate::js::scope::Scope::new_global(),
            );
        }
    }
}

pub(super) fn remove_node_attr(id: usize, key: &str) {
    DOM_NODES.with(|nodes| {
        if let Some(node) = nodes.borrow_mut().get_mut(id) {
            node.attributes.remove(key);
        }
    });
}

fn get_node_attr(id: usize, key: &str) -> Option<String> {
    DOM_NODES.with(|nodes| {
        nodes.borrow().get(id).and_then(|n| n.attributes.get(key).cloned())
    })
}

// ── Insertion helpers ────────────────────────────────────────────────────────

fn insert_adjacent_node(target_id: usize, position: &str, new_id: usize) {
    DOM_NODES.with(|nodes| {
        let mut nodes = nodes.borrow_mut();
        let parent_id = nodes.get(target_id).and_then(|n| n.parent);
        if let Some(pid) = parent_id {
            if let Some(parent) = nodes.get_mut(pid) {
                let pos = parent.children.iter().position(|&c| c == target_id);
                match position {
                    "beforebegin" => {
                        if let Some(idx) = pos { parent.children.insert(idx, new_id); }
                    }
                    "afterend" => {
                        if let Some(idx) = pos { parent.children.insert(idx + 1, new_id); }
                    }
                    _ => {}
                }
            }
            if let Some(new_node) = nodes.get_mut(new_id) {
                new_node.parent = Some(pid);
            }
        }
    });
}

fn insert_adjacent_html(target_id: usize, position: &str, html: &str) {
    use crate::parser::html::HtmlParser;
    let parsed = HtmlParser::parse(html);
    // Create nodes from parsed tree (skip root document node).
    let mut new_ids: Vec<usize> = Vec::new();
    DOM_NODES.with(|nodes| {
        let mut nodes = nodes.borrow_mut();
        for pnode in parsed.iter().skip(1) {
            let our_id = nodes.len();
            let dom_node = if pnode.node_type == crate::parser::html::NodeType::Text {
                DomNode::new_text(&pnode.text_content)
            } else {
                let mut n = DomNode::new_element(&pnode.tag_name);
                for (k, v) in &pnode.attributes {
                    n.attributes.insert(k.clone(), v.clone());
                }
                n
            };
            nodes.push(dom_node);
            new_ids.push(our_id);
        }
    });
    // Insert top-level nodes at the appropriate position.
    for &nid in &new_ids {
        match position {
            "beforebegin" => insert_adjacent_node(target_id, "beforebegin", nid),
            "afterend" => insert_adjacent_node(target_id, "afterend", nid),
            "afterbegin" => {
                DOM_NODES.with(|nodes| {
                    let mut nodes = nodes.borrow_mut();
                    if let Some(node) = nodes.get_mut(target_id) {
                        node.children.insert(0, nid);
                    }
                    if let Some(n) = nodes.get_mut(nid) { n.parent = Some(target_id); }
                });
            }
            "beforeend" => {
                DOM_NODES.with(|nodes| {
                    let mut nodes = nodes.borrow_mut();
                    if let Some(node) = nodes.get_mut(target_id) {
                        node.children.push(nid);
                    }
                    if let Some(n) = nodes.get_mut(nid) { n.parent = Some(target_id); }
                });
            }
            _ => {}
        }
    }
}

fn remove_node_from_parent(node_id: usize) {
    DOM_NODES.with(|nodes| {
        let mut nodes = nodes.borrow_mut();
        if let Some(node) = nodes.get(node_id) {
            if let Some(pid) = node.parent {
                if let Some(parent) = nodes.get_mut(pid) {
                    parent.children.retain(|&c| c != node_id);
                }
            }
        }
        if let Some(node) = nodes.get_mut(node_id) {
            node.parent = None;
        }
    });
}
