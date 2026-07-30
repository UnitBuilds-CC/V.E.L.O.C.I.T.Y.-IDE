//! Agent empowerment layer — zero-alloc primitives that turn the DOM engine
//! into an LLM superpower.
//!
//! All functions operate on a [`DomElementSnapshot`] (taken once, no lock held)
//! and produce compact, semantic output — never raw DOM dumps.
//!
//! # Modules
//! - **Selector generation** — unique CSS selectors for any DOM node
//! - **Interactive elements** — buttons, inputs, links → compact NDA-ready list
//! - **Content extraction** — strip nav/footer/ads → main text only
//! - **Page summary** — title, headings, stats in a few hundred bytes
//! - **DOM diff** — snapshot comparison for wait-for-settlement
//! - **Table extraction** — tables → structured headers + rows
//! - **Page-to-Markdown** — densest page representation for LLMs
//! - **Bulk form fill** — one call fills N fields
//! - **Link map** — deduplicated navigation targets

use super::dom_bridge::{DomElementSnapshot, snapshot_dom};

// ── CSS Selector Generation ─────────────────────────────────────────────────

/// Generate a unique CSS selector for a node in the snapshot.
///
/// Strategy (cheapest first):
/// 1. `#id` if element has an id attribute
/// 2. `tag[attr=value]` if element has a unique attribute
/// 3. `tag:nth-child(n)` fallback
pub(super) fn generate_selector(snaps: &[DomElementSnapshot], node_id: usize) -> String {
    let Some(node) = snaps.get(node_id) else { return String::new() };
    if node.node_type != 1 { return String::new(); }

    // 1. ID selector (fastest, most specific).
    if let Some(id) = node.attributes.get("id") {
        if !id.is_empty() { return format!("#{}", id); }
    }

    // 2. Name attribute for form elements.
    if matches!(node.tag.as_str(), "input" | "select" | "textarea" | "button") {
        if let Some(name) = node.attributes.get("name") {
            if !name.is_empty() { return format!("{}[name=\"{}\"]", node.tag, name); }
        }
    }

    // 3. Build path from root.
    let mut path = Vec::new();
    let mut current = node_id;
    loop {
        let Some(n) = snaps.get(current) else { break };
        if n.node_type != 1 {
            if let Some(p) = n.parent { current = p; } else { break; }
            continue;
        }
        // Stop at body/html.
        if n.tag == "body" || n.tag == "html" {
            path.push(n.tag.clone());
            break;
        }
        let segment = nth_child_selector(snaps, current);
        path.push(segment);
        match n.parent {
            Some(p) => current = p,
            None => break,
        }
    }
    path.reverse();
    path.join(" > ")
}

/// Generate `tag:nth-child(n)` for a node among its siblings.
fn nth_child_selector(snaps: &[DomElementSnapshot], node_id: usize) -> String {
    let Some(node) = snaps.get(node_id) else { return String::new() };
    let Some(parent_id) = node.parent else { return node.tag.clone() };
    let Some(parent) = snaps.get(parent_id) else { return node.tag.clone() };

    // Count same-tag siblings before this node.
    let mut index = 1;
    for &child_id in &parent.children {
        if child_id == node_id { break; }
        if let Some(sibling) = snaps.get(child_id) {
            if sibling.node_type == 1 && sibling.tag == node.tag {
                index += 1;
            }
        }
    }

    // Count total same-tag siblings.
    let total = parent.children.iter().filter(|&&cid| {
        snaps.get(cid).map(|s| s.node_type == 1 && s.tag == node.tag).unwrap_or(false)
    }).count();

    if total == 1 {
        node.tag.clone()
    } else {
        format!("{}:nth-of-type({})", node.tag, index)
    }
}

// ── Interactive Elements Query ───────────────────────────────────────────────

/// An interactive element as seen by an agent — compact, semantic, actionable.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct InteractiveElement {
    pub node_id: usize,
    pub role: &'static str,
    pub name: String,
    pub value: String,
    pub selector: String,
    pub disabled: bool,
    pub visible: bool,
}

/// Tags that are inherently interactive.
const INTERACTIVE_TAGS: &[&str] = &[
    "a", "button", "input", "select", "textarea", "details", "summary",
    "label", "option", "optgroup",
];

/// Return all interactive elements in the DOM, sorted by actionability.
///
/// This is the primary API agents use to understand "what can I do on this page?"
/// Output is compact: role, accessible name, CSS selector, current value.
pub(super) fn get_interactive_elements() -> Vec<InteractiveElement> {
    let (snaps, _root) = snapshot_dom();
    let mut elements = Vec::new();

    for snap in &snaps {
        if snap.node_type != 1 { continue; }
        // Skip hidden elements.
        if snap.attributes.contains_key("hidden") { continue; }
        if snap.attributes.get("aria-hidden").map(|s| s.as_str()) == Some("true") { continue; }
        if snap.attributes.get("style").map(|s| s.contains("display:none") || s.contains("display: none")).unwrap_or(false) { continue; }

        let role = element_role(snap);
        if role.is_none() { continue; }
        let role = role.unwrap();

        let name = accessible_name(snap, &snaps);
        let value = snap.attributes.get("value").cloned().unwrap_or_default();
        let selector = generate_selector(&snaps, snap.id);
        let disabled = snap.attributes.contains_key("disabled")
            || snap.attributes.get("aria-disabled").map(|s| s.as_str()) == Some("true");

        elements.push(InteractiveElement {
            node_id: snap.id,
            role,
            name,
            value,
            selector,
            disabled,
            visible: true,
        });
    }

    // Sort by actionability: buttons/links first, then inputs, then others.
    elements.sort_by(|a, b| {
        let score_a = actionability_score(a.role);
        let score_b = actionability_score(b.role);
        score_b.cmp(&score_a)
    });

    elements
}

/// Map a DOM element to its ARIA role (static lifetime strings for zero-alloc).
fn element_role(snap: &DomElementSnapshot) -> Option<&'static str> {
    // Explicit role attribute takes priority.
    if let Some(role) = snap.attributes.get("role") {
        return match role.as_str() {
            "button" => Some("button"),
            "link" => Some("link"),
            "textbox" => Some("textbox"),
            "checkbox" => Some("checkbox"),
            "radio" => Some("radio"),
            "combobox" => Some("combobox"),
            "tab" => Some("tab"),
            "menuitem" => Some("menuitem"),
            "switch" => Some("switch"),
            "slider" => Some("slider"),
            "searchbox" => Some("searchbox"),
            "spinbutton" => Some("spinbutton"),
            _ => None,
        };
    }

    match snap.tag.as_str() {
        "a" if snap.attributes.contains_key("href") => Some("link"),
        "button" => Some("button"),
        "input" => {
            let t = snap.attributes.get("type").map(|s| s.as_str()).unwrap_or("text");
            Some(match t {
                "button" | "submit" | "reset" | "image" => "button",
                "checkbox" => "checkbox",
                "radio" => "radio",
                "search" => "searchbox",
                "number" => "spinbutton",
                "range" => "slider",
                _ => "textbox",
            })
        }
        "select" => Some("combobox"),
        "textarea" => Some("textbox"),
        "details" => Some("disclosure"),
        "summary" => Some("button"),
        _ if INTERACTIVE_TAGS.contains(&snap.tag.as_str()) => Some("interactive"),
        _ => None,
    }
}

/// Compute the accessible name for an element.
fn accessible_name(snap: &DomElementSnapshot, snaps: &[DomElementSnapshot]) -> String {
    // Priority: aria-label > placeholder > title > text content > id
    if let Some(label) = snap.attributes.get("aria-label") {
        if !label.is_empty() { return label.clone(); }
    }
    if let Some(placeholder) = snap.attributes.get("placeholder") {
        if !placeholder.is_empty() { return placeholder.clone(); }
    }
    if let Some(title) = snap.attributes.get("title") {
        if !title.is_empty() { return title.clone(); }
    }
    // For links/buttons, use text content.
    if matches!(snap.tag.as_str(), "a" | "button" | "summary") {
        let text = collect_text(snap.id, snaps);
        let trimmed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !trimmed.is_empty() { return trimmed; }
    }
    // For inputs, use associated label element.
    if let Some(id) = snap.attributes.get("id") {
        for s in snaps {
            if s.tag == "label" && s.attributes.get("for").map(|v| v.as_str()) == Some(id.as_str()) {
                let text = collect_text(s.id, snaps);
                let trimmed = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !trimmed.is_empty() { return trimmed; }
            }
        }
    }
    if let Some(name) = snap.attributes.get("name") {
        if !name.is_empty() { return name.clone(); }
    }
    String::new()
}

/// Collect text content from a node and its descendants.
fn collect_text(node_id: usize, snaps: &[DomElementSnapshot]) -> String {
    let mut buf = String::new();
    collect_text_walk(node_id, snaps, &mut buf);
    buf
}

fn collect_text_walk(id: usize, snaps: &[DomElementSnapshot], out: &mut String) {
    let Some(node) = snaps.get(id) else { return };
    if node.node_type == 3 {
        out.push_str(&node.text_content);
        return;
    }
    for &child in &node.children {
        collect_text_walk(child, snaps, out);
    }
}

fn actionability_score(role: &str) -> u8 {
    match role {
        "button" | "link" => 100,
        "textbox" | "searchbox" | "checkbox" | "radio" | "combobox" => 90,
        "switch" | "slider" | "spinbutton" => 85,
        "tab" | "menuitem" => 80,
        "disclosure" | "interactive" => 70,
        _ => 10,
    }
}

// ── Content Extraction ───────────────────────────────────────────────────────

/// Tags considered boilerplate (stripped during content extraction).
const BOILERPLATE_TAGS: &[&str] = &[
    "nav", "footer", "header", "aside", "noscript", "script", "style",
    "svg", "iframe", "object", "embed",
];

/// Class/id patterns that indicate boilerplate content.
const BOILERPLATE_PATTERNS: &[&str] = &[
    "sidebar", "footer", "nav", "menu", "ad", "advert", "banner",
    "cookie", "popup", "modal", "social", "share", "related",
];

/// Extract the main content from the DOM, stripping boilerplate.
///
/// Returns a list of content blocks, each with a heading (if any) and text.
/// This is what agents actually want to read — not the raw HTML.
pub(super) fn extract_main_content() -> Vec<ContentBlock> {
    let (snaps, root) = snapshot_dom();
    let mut blocks = Vec::new();

    // Find the body element.
    let body_id = snaps.iter()
        .find(|s| s.tag == "body" && s.node_type == 1)
        .map(|s| s.id)
        .unwrap_or(root);

    // Find <main> if it exists, otherwise use body.
    let content_root = snaps.iter()
        .find(|s| s.tag == "main" && s.node_type == 1)
        .map(|s| s.id)
        .unwrap_or(body_id);

    extract_blocks(content_root, &snaps, &mut blocks, &mut None);
    blocks
}

/// A block of extracted content.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct ContentBlock {
    pub heading: String,
    pub text: String,
    pub depth: u8,
}

fn extract_blocks(
    node_id: usize,
    snaps: &[DomElementSnapshot],
    blocks: &mut Vec<ContentBlock>,
    current_heading: &mut Option<(String, u8)>,
) {
    let Some(node) = snaps.get(node_id) else { return };
    if node.node_type != 1 { return; }

    // Skip boilerplate.
    if is_boilerplate(node) { return; }

    // Check if this is a heading.
    if let Some(depth) = heading_depth(&node.tag) {
        // Flush previous heading's content.
        if let Some((heading, d)) = current_heading.take() {
            // Already flushed below.
            let _ = (heading, d);
        }
        let text = collect_text(node_id, snaps);
        let trimmed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !trimmed.is_empty() {
            *current_heading = Some((trimmed, depth));
        }
        return;
    }

    // If this is a content container (p, li, td, div with text), extract it.
    if is_content_container(&node.tag) {
        let text = collect_text(node_id, snaps);
        let trimmed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.len() > 20 {
            let heading = current_heading.take().map(|(h, _)| h).unwrap_or_default();
            blocks.push(ContentBlock {
                heading,
                text: trimmed,
                depth: 0,
            });
            return;
        }
    }

    // Recurse into children.
    for &child in &node.children {
        extract_blocks(child, snaps, blocks, current_heading);
    }
}

fn is_boilerplate(node: &DomElementSnapshot) -> bool {
    if BOILERPLATE_TAGS.contains(&node.tag.as_str()) { return true; }
    // Check class/id for boilerplate patterns.
    let class = node.attributes.get("class").cloned().unwrap_or_default();
    let id = node.attributes.get("id").cloned().unwrap_or_default();
    let haystack = format!(" {} {} ", class.to_lowercase(), id.to_lowercase());
    BOILERPLATE_PATTERNS.iter().any(|p| haystack.contains(p))
}

fn heading_depth(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn is_content_container(tag: &str) -> bool {
    matches!(tag, "p" | "li" | "td" | "th" | "blockquote" | "pre" | "code")
}

// ── Page Summary ─────────────────────────────────────────────────────────────

/// Compact page summary — title, headings, stats.
///
/// This is the first thing an agent should see when loading a page.
/// Typically ~200-500 bytes, fitting easily in context.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(super) struct PageSummary {
    pub title: String,
    pub url: String,
    pub headings: Vec<(u8, String)>,  // (depth, text)
    pub interactive_count: usize,
    pub form_count: usize,
    pub link_count: usize,
    pub image_count: usize,
    pub total_text_length: usize,
}

/// Generate a compact page summary.
pub(super) fn summarize_page() -> PageSummary {
    let (snaps, _root) = snapshot_dom();
    let mut summary = PageSummary::default();

    for snap in &snaps {
        if snap.node_type != 1 { continue; }
        match snap.tag.as_str() {
            "title" => {
                summary.title = collect_text(snap.id, &snaps)
                    .split_whitespace().collect::<Vec<_>>().join(" ");
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if let Some(depth) = heading_depth(&snap.tag) {
                    let text = collect_text(snap.id, &snaps)
                        .split_whitespace().collect::<Vec<_>>().join(" ");
                    if !text.is_empty() && summary.headings.len() < 50 {
                        summary.headings.push((depth, text));
                    }
                }
            }
            "a" if snap.attributes.contains_key("href") => summary.link_count += 1,
            "img" => summary.image_count += 1,
            "form" => summary.form_count += 1,
            "input" | "select" | "textarea" | "button" => summary.interactive_count += 1,
            "a" if snap.attributes.contains_key("href") => summary.interactive_count += 1,
            "body" => {
                let text = collect_text(snap.id, &snaps);
                summary.total_text_length = text.len();
            }
            _ => {}
        }
    }

    summary
}

/// Serialize a page summary to a compact string (for LLM consumption).
pub(super) fn summary_to_text(summary: &PageSummary) -> String {
    let mut out = String::with_capacity(512);
    if !summary.title.is_empty() {
        out.push_str("Title: ");
        out.push_str(&summary.title);
        out.push('\n');
    }
    out.push_str(&format!(
        "Links: {} | Forms: {} | Interactive: {} | Images: {} | Text: {} chars\n",
        summary.link_count, summary.form_count, summary.interactive_count,
        summary.image_count, summary.total_text_length,
    ));
    if !summary.headings.is_empty() {
        out.push_str("Headings:\n");
        for (depth, text) in &summary.headings {
            let indent = "  ".repeat(*depth as usize);
            out.push_str(&indent);
            out.push_str(text);
            out.push('\n');
        }
    }
    out
}

/// Serialize interactive elements to a compact string (for LLM consumption).
pub(super) fn interactive_elements_to_text(elements: &[InteractiveElement]) -> String {
    let mut out = String::with_capacity(elements.len() * 80);
    for (i, el) in elements.iter().enumerate() {
        let disabled_mark = if el.disabled { " [disabled]" } else { "" };
        let value_part = if !el.value.is_empty() {
            format!(" value=\"{}\"", el.value)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "[{}] <{}> {}{}{} → {}\n",
            i, el.role, el.name, value_part, disabled_mark, el.selector,
        ));
    }
    out
}

// ── DOM Diff ─────────────────────────────────────────────────────────────────

/// A minimal snapshot for diffing (just node count + interactive element count).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DomState {
    pub node_count: usize,
    pub interactive_count: usize,
    pub body_text_hash: u64,
}

/// Capture the current DOM state for later comparison.
pub(super) fn capture_dom_state() -> DomState {
    let (snaps, _root) = snapshot_dom();
    let node_count = snaps.len();
    let interactive_count = snaps.iter()
        .filter(|s| s.node_type == 1 && element_role(s).is_some())
        .count();
    // Hash body text for change detection.
    let body_text = snaps.iter()
        .find(|s| s.tag == "body" && s.node_type == 1)
        .map(|s| collect_text(s.id, &snaps))
        .unwrap_or_default();
    let body_text_hash = simple_hash(&body_text);

    DomState { node_count, interactive_count, body_text_hash }
}

/// Check if two DOM states are meaningfully different.
#[allow(dead_code)]
pub(super) fn dom_states_differ(a: &DomState, b: &DomState) -> bool {
    a.node_count != b.node_count
        || a.interactive_count != b.interactive_count
        || a.body_text_hash != b.body_text_hash
}

/// Simple FNV-1a hash for change detection (zero-alloc, fast).
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ── Table Extraction ─────────────────────────────────────────────────────────

/// A table extracted as structured data — headers + rows, no markup.
#[derive(Debug, Clone, Default)]
pub(super) struct TableData {
    pub caption: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Extract all tables in the DOM as structured data.
///
/// Raw HTML tables cost thousands of tokens; this yields headers + rows only.
pub(super) fn extract_tables() -> Vec<TableData> {
    let (snaps, _root) = snapshot_dom();
    let mut tables = Vec::new();

    for snap in &snaps {
        if snap.node_type != 1 || snap.tag != "table" { continue; }
        let mut table = TableData::default();

        // Caption.
        if let Some(cap_id) = find_descendant_by_tag(snap.id, "caption", &snaps) {
            table.caption = normalized_text(cap_id, &snaps);
        }

        // Walk all <tr> descendants in document order.
        let mut row_ids = Vec::new();
        collect_descendants_by_tag(snap.id, "tr", &snaps, &mut row_ids);
        for row_id in row_ids {
            let Some(row) = snaps.get(row_id) else { continue };
            let mut cells = Vec::new();
            let mut is_header_row = false;
            for &cell_id in &row.children {
                let Some(cell) = snaps.get(cell_id) else { continue };
                match cell.tag.as_str() {
                    "th" => {
                        is_header_row = true;
                        cells.push(normalized_text(cell_id, &snaps));
                    }
                    "td" => cells.push(normalized_text(cell_id, &snaps)),
                    _ => {}
                }
            }
            if cells.is_empty() { continue; }
            if is_header_row && table.headers.is_empty() {
                table.headers = cells;
            } else {
                table.rows.push(cells);
            }
        }

        if !table.headers.is_empty() || !table.rows.is_empty() {
            tables.push(table);
        }
    }
    tables
}

/// Serialize tables as Markdown — the most token-efficient tabular format.
pub(super) fn tables_to_text(tables: &[TableData]) -> String {
    let mut out = String::new();
    for (i, table) in tables.iter().enumerate() {
        if i > 0 { out.push('\n'); }
        if !table.caption.is_empty() {
            out.push_str("### ");
            out.push_str(&table.caption);
            out.push('\n');
        }
        if !table.headers.is_empty() {
            out.push_str("| ");
            out.push_str(&table.headers.join(" | "));
            out.push_str(" |\n|");
            for _ in &table.headers { out.push_str(" --- |"); }
            out.push('\n');
        }
        for row in &table.rows {
            out.push_str("| ");
            out.push_str(&row.join(" | "));
            out.push_str(" |\n");
        }
    }
    out
}

/// Find the first descendant with the given tag (depth-first).
fn find_descendant_by_tag(node_id: usize, tag: &str, snaps: &[DomElementSnapshot]) -> Option<usize> {
    let node = snaps.get(node_id)?;
    for &child in &node.children {
        if let Some(c) = snaps.get(child) {
            if c.node_type == 1 && c.tag == tag { return Some(child); }
            if let Some(found) = find_descendant_by_tag(child, tag, snaps) {
                return Some(found);
            }
        }
    }
    None
}

/// Collect all descendants with the given tag in document order.
fn collect_descendants_by_tag(node_id: usize, tag: &str, snaps: &[DomElementSnapshot], out: &mut Vec<usize>) {
    let Some(node) = snaps.get(node_id) else { return };
    for &child in &node.children {
        if let Some(c) = snaps.get(child) {
            if c.node_type == 1 && c.tag == tag { out.push(child); }
            collect_descendants_by_tag(child, tag, snaps, out);
        }
    }
}

/// Whitespace-normalized text content of a node.
fn normalized_text(node_id: usize, snaps: &[DomElementSnapshot]) -> String {
    collect_text(node_id, snaps).split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Page-to-Markdown ─────────────────────────────────────────────────────────

/// Convert the page's main content to Markdown.
///
/// Markdown is the densest page representation an LLM can consume:
/// structure is preserved (headings, lists, links, tables) at a fraction
/// of the token cost of HTML.
pub(super) fn page_to_markdown() -> String {
    let (snaps, root) = snapshot_dom();
    let body_id = snaps.iter()
        .find(|s| s.tag == "body" && s.node_type == 1)
        .map(|s| s.id)
        .unwrap_or(root);
    let content_root = snaps.iter()
        .find(|s| s.tag == "main" && s.node_type == 1)
        .map(|s| s.id)
        .unwrap_or(body_id);

    let mut out = String::with_capacity(1024);
    // Title first.
    if let Some(title) = snaps.iter().find(|s| s.tag == "title" && s.node_type == 1) {
        let t = normalized_text(title.id, &snaps);
        if !t.is_empty() {
            out.push_str("# ");
            out.push_str(&t);
            out.push_str("\n\n");
        }
    }
    markdown_walk(content_root, &snaps, &mut out, 0);
    // Collapse runs of 3+ newlines.
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out.trim_end().to_string() + "\n"
}

fn markdown_walk(node_id: usize, snaps: &[DomElementSnapshot], out: &mut String, list_depth: usize) {
    let Some(node) = snaps.get(node_id) else { return };
    if node.node_type == 3 { return; } // handled by inline collection
    if node.node_type != 1 { return; }
    if is_boilerplate(node) { return; }

    match node.tag.as_str() {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let depth = heading_depth(&node.tag).unwrap_or(1) as usize;
            let text = markdown_inline(node_id, snaps);
            if !text.is_empty() {
                out.push_str(&"#".repeat(depth));
                out.push(' ');
                out.push_str(&text);
                out.push_str("\n\n");
            }
        }
        "p" | "blockquote" => {
            let text = markdown_inline(node_id, snaps);
            if !text.is_empty() {
                if node.tag == "blockquote" { out.push_str("> "); }
                out.push_str(&text);
                out.push_str("\n\n");
            }
        }
        "pre" => {
            let text = collect_text(node_id, snaps);
            if !text.trim().is_empty() {
                out.push_str("```\n");
                out.push_str(text.trim_end());
                out.push_str("\n```\n\n");
            }
        }
        "ul" | "ol" => {
            let ordered = node.tag == "ol";
            let mut index = 1;
            for &child in &node.children {
                if let Some(c) = snaps.get(child) {
                    if c.node_type == 1 && c.tag == "li" {
                        out.push_str(&"  ".repeat(list_depth));
                        if ordered {
                            out.push_str(&format!("{}. ", index));
                            index += 1;
                        } else {
                            out.push_str("- ");
                        }
                        let text = markdown_inline(child, snaps);
                        out.push_str(&text);
                        out.push('\n');
                        // Nested lists inside this <li>.
                        for &gc in &c.children {
                            if let Some(g) = snaps.get(gc) {
                                if g.node_type == 1 && (g.tag == "ul" || g.tag == "ol") {
                                    markdown_walk(gc, snaps, out, list_depth + 1);
                                }
                            }
                        }
                    }
                }
            }
            if list_depth == 0 { out.push('\n'); }
        }
        "table" => {
            let mut row_ids = Vec::new();
            collect_descendants_by_tag(node_id, "tr", snaps, &mut row_ids);
            let mut header_done = false;
            for row_id in row_ids {
                let Some(row) = snaps.get(row_id) else { continue };
                let mut cells = Vec::new();
                let mut is_header = false;
                for &cell_id in &row.children {
                    if let Some(cell) = snaps.get(cell_id) {
                        match cell.tag.as_str() {
                            "th" => { is_header = true; cells.push(normalized_text(cell_id, snaps)); }
                            "td" => cells.push(normalized_text(cell_id, snaps)),
                            _ => {}
                        }
                    }
                }
                if cells.is_empty() { continue; }
                out.push_str("| ");
                out.push_str(&cells.join(" | "));
                out.push_str(" |\n");
                if is_header && !header_done {
                    out.push('|');
                    for _ in &cells { out.push_str(" --- |"); }
                    out.push('\n');
                    header_done = true;
                }
            }
            out.push('\n');
        }
        "hr" => out.push_str("---\n\n"),
        "img" => {
            let alt = node.attributes.get("alt").cloned().unwrap_or_default();
            let src = node.attributes.get("src").cloned().unwrap_or_default();
            if !alt.is_empty() || !src.is_empty() {
                out.push_str(&format!("![{}]({})\n\n", alt, src));
            }
        }
        _ => {
            for &child in &node.children {
                markdown_walk(child, snaps, out, list_depth);
            }
        }
    }
}

/// Collect inline Markdown for a node: text with links/emphasis/code preserved.
fn markdown_inline(node_id: usize, snaps: &[DomElementSnapshot]) -> String {
    let mut buf = String::new();
    markdown_inline_walk(node_id, snaps, &mut buf);
    buf.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn markdown_inline_walk(node_id: usize, snaps: &[DomElementSnapshot], out: &mut String) {
    let Some(node) = snaps.get(node_id) else { return };
    if node.node_type == 3 {
        out.push_str(&node.text_content);
        return;
    }
    if node.node_type != 1 { return; }
    match node.tag.as_str() {
        "a" => {
            let text = normalized_text(node_id, snaps);
            match node.attributes.get("href") {
                Some(href) if !href.is_empty() && !text.is_empty() => {
                    out.push_str(&format!("[{}]({})", text, href));
                }
                _ => out.push_str(&text),
            }
            out.push(' ');
        }
        "strong" | "b" => {
            let text = normalized_text(node_id, snaps);
            if !text.is_empty() { out.push_str(&format!("**{}** ", text)); }
        }
        "em" | "i" => {
            let text = normalized_text(node_id, snaps);
            if !text.is_empty() { out.push_str(&format!("*{}* ", text)); }
        }
        "code" => {
            let text = collect_text(node_id, snaps);
            if !text.is_empty() { out.push_str(&format!("`{}` ", text.trim())); }
        }
        "br" => out.push('\n'),
        _ => {
            for &child in &node.children {
                markdown_inline_walk(child, snaps, out);
            }
        }
    }
}

// ── Bulk Form Fill ───────────────────────────────────────────────────────────

/// Result of filling a single field.
#[derive(Debug, Clone)]
pub(super) struct FillResult {
    pub field: String,
    pub ok: bool,
    pub reason: &'static str,
}

/// Fill multiple form fields in a single call.
///
/// Each `(field, value)` pair is matched against `name`, `id`, or
/// `placeholder` of input/textarea/select elements. Checkboxes and radios
/// treat "true"/"checked" as checked. One call replaces N agent round-trips.
pub(super) fn fill_form(values: &[(String, String)]) -> Vec<FillResult> {
    let (snaps, _root) = snapshot_dom();
    let mut results = Vec::with_capacity(values.len());

    for (field, value) in values {
        let target = snaps.iter().find(|s| {
            s.node_type == 1
                && matches!(s.tag.as_str(), "input" | "textarea" | "select")
                && (s.attributes.get("name").map(|v| v == field).unwrap_or(false)
                    || s.attributes.get("id").map(|v| v == field).unwrap_or(false)
                    || s.attributes.get("placeholder").map(|v| v == field).unwrap_or(false))
        });
        let Some(target) = target else {
            results.push(FillResult { field: field.clone(), ok: false, reason: "not found" });
            continue;
        };
        if target.attributes.contains_key("disabled") {
            results.push(FillResult { field: field.clone(), ok: false, reason: "disabled" });
            continue;
        }
        let input_type = target.attributes.get("type").map(|s| s.as_str()).unwrap_or("text");
        match input_type {
            "checkbox" | "radio" => {
                let checked = matches!(value.as_str(), "true" | "checked" | "1" | "on");
                if checked {
                    super::dom_bridge::set_node_attr(target.id, "checked", "checked");
                } else {
                    super::dom_bridge::remove_node_attr(target.id, "checked");
                }
                super::dom_bridge::fire_event(target.id, "change");
            }
            _ => {
                super::dom_bridge::set_node_attr(target.id, "value", value);
                // Real pages react to input/change — fire both so listeners run.
                super::dom_bridge::fire_event(target.id, "input");
                super::dom_bridge::fire_event(target.id, "change");
            }
        }
        results.push(FillResult { field: field.clone(), ok: true, reason: "" });
    }
    results
}

// ── Link Map ─────────────────────────────────────────────────────────────────

/// A link as seen by an agent: text + destination.
#[derive(Debug, Clone)]
pub(super) struct LinkInfo {
    pub text: String,
    pub href: String,
}

/// Return all links on the page (text + href), deduplicated by href.
///
/// This is the agent's navigation map — where can I go from here?
pub(super) fn get_links() -> Vec<LinkInfo> {
    let (snaps, _root) = snapshot_dom();
    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for snap in &snaps {
        if snap.node_type != 1 || snap.tag != "a" { continue; }
        let Some(href) = snap.attributes.get("href") else { continue };
        if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") { continue; }
        if !seen.insert(href.clone()) { continue; }
        let text = normalized_text(snap.id, &snaps);
        links.push(LinkInfo { text, href: href.clone() });
    }
    links
}

/// Serialize links as compact text lines: `[i] text → href`.
pub(super) fn links_to_text(links: &[LinkInfo]) -> String {
    let mut out = String::with_capacity(links.len() * 60);
    for (i, link) in links.iter().enumerate() {
        out.push_str(&format!("[{}] {} → {}\n", i, link.text, link.href));
    }
    out
}

// ── Semantic Element Finding ─────────────────────────────────────────────────

/// A text-matched element with ranking metadata.
#[derive(Debug, Clone)]
pub(super) struct TextMatch {
    pub node_id: usize,
    pub selector: String,
    pub exact: bool,
    pub interactive: bool,
}

/// Find elements by their visible text (case-insensitive).
///
/// Agents think in labels ("the Login button"), not selectors. Matches are
/// deepest-first (innermost element containing the text), ranked exact >
/// interactive > shortest text.
pub(super) fn find_by_text(query: &str) -> Vec<TextMatch> {
    let (snaps, _root) = snapshot_dom();
    let needle = query.trim().to_lowercase();
    if needle.is_empty() { return Vec::new(); }

    // Pass 1: all elements whose text contains the needle.
    let mut candidates: Vec<usize> = Vec::new();
    for snap in &snaps {
        if snap.node_type != 1 { continue; }
        if matches!(snap.tag.as_str(), "html" | "head" | "script" | "style" | "title") { continue; }
        let text = normalized_text(snap.id, &snaps).to_lowercase();
        if text.contains(&needle) {
            candidates.push(snap.id);
        }
    }

    // Pass 2: keep only deepest matches (no descendant also matches).
    let candidate_set: std::collections::HashSet<usize> = candidates.iter().copied().collect();
    let mut matches: Vec<TextMatch> = Vec::new();
    for &id in &candidates {
        let Some(snap) = snaps.get(id) else { continue };
        let has_matching_child = snap.children.iter().any(|c| {
            descendant_in_set(*c, &candidate_set, &snaps)
        });
        if has_matching_child { continue; }
        let text = normalized_text(id, &snaps).to_lowercase();
        matches.push(TextMatch {
            node_id: id,
            selector: generate_selector(&snaps, id),
            exact: text == needle,
            interactive: element_role(snap).is_some(),
        });
    }

    // Rank: exact > interactive > shortest text.
    matches.sort_by(|a, b| {
        b.exact.cmp(&a.exact)
            .then(b.interactive.cmp(&a.interactive))
            .then(a.node_id.cmp(&b.node_id))
    });
    matches
}

fn descendant_in_set(id: usize, set: &std::collections::HashSet<usize>, snaps: &[DomElementSnapshot]) -> bool {
    if set.contains(&id) { return true; }
    let Some(node) = snaps.get(id) else { return false };
    node.children.iter().any(|c| descendant_in_set(*c, set, snaps))
}

/// Resolve a text query to a clickable node: the best match itself if
/// interactive, otherwise its nearest interactive ancestor.
pub(super) fn resolve_click_target(query: &str) -> Option<usize> {
    let matches = find_by_text(query);
    let (snaps, _root) = snapshot_dom();
    for m in &matches {
        if m.interactive { return Some(m.node_id); }
        // Walk up looking for an interactive ancestor.
        let mut current = snaps.get(m.node_id).and_then(|n| n.parent);
        while let Some(id) = current {
            let Some(node) = snaps.get(id) else { break };
            if element_role(node).is_some() { return Some(id); }
            current = node.parent;
        }
    }
    matches.first().map(|m| m.node_id)
}

// ── NDA Export ───────────────────────────────────────────────────────────────

/// Export the current agent-visible page state as a lossless [`NdaDocument`].
///
/// This is the zero-JSON path: page summary + every interactive element as
/// dictionary-interned facts using the central predicate registry. The session
/// layer serializes the document with `to_binary_stream()` (or seals it) —
/// no serde, no JSON, and repeated roles/names cost one dictionary entry.
pub fn export_agent_state_nda() -> crate::nda::NdaDocument {
    use crate::predicates::{
        AOM_ACTIONABILITY, AOM_DISABLED, AOM_NAME, AOM_ROLE, AOM_SELECTOR, AOM_VALUE,
        SESSION_FORM_COUNT, SESSION_HEADING, SESSION_INTERACTIVE_COUNT, SESSION_LINK_COUNT,
        SESSION_TEXT_LENGTH, SESSION_TITLE,
    };

    let mut doc = crate::nda::NdaDocument::new();

    // Page-level facts.
    let summary = summarize_page();
    if !summary.title.is_empty() {
        doc.push_str("page", SESSION_TITLE, &summary.title);
    }
    doc.push_int("page", SESSION_LINK_COUNT, summary.link_count as i64);
    doc.push_int("page", SESSION_FORM_COUNT, summary.form_count as i64);
    doc.push_int("page", SESSION_INTERACTIVE_COUNT, summary.interactive_count as i64);
    doc.push_int("page", SESSION_TEXT_LENGTH, summary.total_text_length as i64);
    for (depth, text) in &summary.headings {
        doc.push_str("page", SESSION_HEADING, &format!("h{}:{}", depth, text));
    }

    // One subject per interactive element, in actionability order.
    for el in get_interactive_elements() {
        let subject = format!("el{}", el.node_id);
        doc.push_str(&subject, AOM_ROLE, el.role);
        if !el.name.is_empty() {
            doc.push_str(&subject, AOM_NAME, &el.name);
        }
        if !el.value.is_empty() {
            doc.push_str(&subject, AOM_VALUE, &el.value);
        }
        if !el.selector.is_empty() {
            doc.push_str(&subject, AOM_SELECTOR, &el.selector);
        }
        doc.push_int(&subject, AOM_ACTIONABILITY, actionability_score(el.role) as i64);
        if el.disabled {
            doc.push_int(&subject, AOM_DISABLED, 1);
        }
    }

    // Network activity the page triggered — one subject per fetch call.
    for entry in super::browser_env::fetch_log() {
        doc.push_str(&entry.url, crate::predicates::NET_METHOD, &entry.method);
        doc.push_int(&entry.url, crate::predicates::NET_STATUS, entry.status as i64);
    }
    doc
}

/// Render an NDA document's facts as compact `subject|predicate|object` lines.
pub(super) fn nda_facts_to_text(doc: &crate::nda::NdaDocument) -> String {
    doc.facts_text()
}

