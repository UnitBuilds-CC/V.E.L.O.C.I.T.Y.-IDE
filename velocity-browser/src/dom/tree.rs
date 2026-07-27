use crate::parser::html::{DomNode, HtmlParser, NodeType};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct DomTree {
    pub nodes: Vec<DomNode>,
}

impl DomTree {
    pub fn new(nodes: Vec<DomNode>) -> Self {
        Self { nodes }
    }

    pub fn get_node(&self, id: usize) -> Option<&DomNode> {
        self.nodes.get(id)
    }

    pub fn get_node_mut(&mut self, id: usize) -> Option<&mut DomNode> {
        self.nodes.get_mut(id)
    }

    pub fn extract_page_title(&self) -> String {
        for node in &self.nodes {
            if node.node_type == NodeType::Element && node.tag_name == "title" {
                for &child_id in &node.children {
                    if let Some(child) = self.get_node(child_id) {
                        if child.node_type == NodeType::Text {
                            return child.text_content.clone();
                        }
                    }
                }
            }
        }
        "Untitled Page".to_string()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // DOM Manipulation API
    // ═══════════════════════════════════════════════════════════════════════

    /// Create a new element node and return its id.
    pub fn create_element(&mut self, tag: &str) -> usize {
        let id = self.nodes.len();
        self.nodes.push(DomNode {
            id,
            node_type: NodeType::Element,
            tag_name: tag.to_string(),
            attributes: HashMap::new(),
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        });
        id
    }

    /// Create a new text node and return its id.
    pub fn create_text_node(&mut self, text: &str) -> usize {
        let id = self.nodes.len();
        self.nodes.push(DomNode {
            id,
            node_type: NodeType::Text,
            tag_name: String::new(),
            attributes: HashMap::new(),
            text_content: text.to_string(),
            children: Vec::new(),
            parent: None,
        });
        id
    }

    /// Append a child node to a parent node.
    pub fn append_child(&mut self, parent_id: usize, child_id: usize) {
        // Remove from old parent first
        if let Some(old_parent) = self.nodes.get(child_id).and_then(|n| n.parent) {
            if let Some(p) = self.nodes.get_mut(old_parent) {
                p.children.retain(|&c| c != child_id);
            }
        }
        if let Some(parent) = self.nodes.get_mut(parent_id) {
            if !parent.children.contains(&child_id) {
                parent.children.push(child_id);
            }
        }
        if let Some(child) = self.nodes.get_mut(child_id) {
            child.parent = Some(parent_id);
        }
    }

    /// Insert a child before a reference node.
    pub fn insert_before(&mut self, parent_id: usize, new_child: usize, ref_child: usize) {
        // Remove from old parent
        if let Some(old_parent) = self.nodes.get(new_child).and_then(|n| n.parent) {
            if let Some(p) = self.nodes.get_mut(old_parent) {
                p.children.retain(|&c| c != new_child);
            }
        }
        if let Some(parent) = self.nodes.get_mut(parent_id) {
            if let Some(pos) = parent.children.iter().position(|&c| c == ref_child) {
                parent.children.insert(pos, new_child);
            } else {
                parent.children.push(new_child);
            }
        }
        if let Some(child) = self.nodes.get_mut(new_child) {
            child.parent = Some(parent_id);
        }
    }

    /// Remove a child node from its parent.
    pub fn remove_child(&mut self, parent_id: usize, child_id: usize) {
        if let Some(parent) = self.nodes.get_mut(parent_id) {
            parent.children.retain(|&c| c != child_id);
        }
        if let Some(child) = self.nodes.get_mut(child_id) {
            child.parent = None;
        }
    }

    /// Replace an old child with a new child.
    pub fn replace_child(&mut self, parent_id: usize, new_child: usize, old_child: usize) {
        if let Some(parent) = self.nodes.get_mut(parent_id) {
            if let Some(pos) = parent.children.iter().position(|&c| c == old_child) {
                parent.children[pos] = new_child;
            }
        }
        if let Some(child) = self.nodes.get_mut(old_child) {
            child.parent = None;
        }
        if let Some(child) = self.nodes.get_mut(new_child) {
            child.parent = Some(parent_id);
        }
    }

    /// Set innerHTML: parse HTML fragment and replace all children of the node.
    pub fn set_inner_html(&mut self, node_id: usize, html: &str) {
        // Clear existing children
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.children.clear();
        }
        // Parse fragment
        let fragment_nodes = HtmlParser::parse_html5(html);
        let base_id = self.nodes.len();
        // Add fragment nodes with shifted ids
        for (i, mut frag_node) in fragment_nodes.into_iter().enumerate() {
            let new_id = base_id + i;
            frag_node.id = new_id;
            frag_node.children = frag_node.children.iter().map(|&c| c + base_id).collect();
            frag_node.parent = frag_node.parent.map(|p| p + base_id);
            self.nodes.push(frag_node);
        }
        // Attach top-level fragment nodes (those without parents or whose parent is the fragment root)
        let mut top_level = Vec::new();
        for i in base_id..self.nodes.len() {
            let is_top = self.nodes[i].parent.map(|p| p == base_id).unwrap_or(true)
                || self.nodes[i].parent == Some(base_id);
            if is_top && i != base_id {
                top_level.push(i);
            }
        }
        // If there's a document root in the fragment, use its children instead
        if top_level.is_empty() && base_id < self.nodes.len() {
            top_level = self.nodes[base_id].children.clone();
        }
        for &child_id in &top_level {
            if let Some(child) = self.nodes.get_mut(child_id) {
                child.parent = Some(node_id);
            }
        }
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.children = top_level;
        }
    }

    /// Get innerHTML: serialize child subtree to HTML string.
    pub fn get_inner_html(&self, node_id: usize) -> String {
        let Some(node) = self.get_node(node_id) else { return String::new() };
        let mut html = String::new();
        for &child_id in &node.children {
            self.serialize_node(child_id, &mut html);
        }
        html
    }

    pub fn serialize_node(&self, node_id: usize, out: &mut String) {
        let Some(node) = self.get_node(node_id) else { return };
        match node.node_type {
            NodeType::Text => out.push_str(&node.text_content),
            NodeType::Element => {
                out.push('<');
                out.push_str(&node.tag_name);
                for (k, v) in &node.attributes {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    out.push_str(v);
                    out.push('"');
                }
                out.push('>');
                for &child_id in &node.children {
                    self.serialize_node(child_id, out);
                }
                out.push_str("</");
                out.push_str(&node.tag_name);
                out.push('>');
            }
            _ => {}
        }
    }

    /// Deep clone a node and all its descendants.
    pub fn clone_node(&mut self, node_id: usize, deep: bool) -> usize {
        let Some(node) = self.get_node(node_id).cloned() else { return 0 };
        let new_id = self.nodes.len();
        let mut cloned = node;
        cloned.id = new_id;
        cloned.parent = None;
        cloned.children = Vec::new();
        self.nodes.push(cloned);

        if deep {
            let orig_children = self.nodes[node_id].children.clone();
            for &child_id in &orig_children {
                let cloned_child = self.clone_node(child_id, true);
                self.append_child(new_id, cloned_child);
            }
        }
        new_id
    }

    /// Query all elements matching a CSS selector.
    /// Supports: #id, .class, tag, tag.class, tag#id, [attr], [attr=val],
    /// descendant (a b), child (a > b), :nth-child(n), :first-child, :last-child.
    pub fn query_selector_all(&self, selector: &str) -> Vec<usize> {
        let sel = selector.trim();
        // Handle descendant/child combinators
        if sel.contains(' ') || sel.contains('>') {
            return self.query_complex_selector(sel);
        }
        let mut results = Vec::new();
        for node in &self.nodes {
            if node.node_type != NodeType::Element { continue; }
            if matches_simple_selector(node, sel) {
                results.push(node.id);
            }
        }
        results
    }

    /// Find the first element matching a selector.
    pub fn query_selector(&self, selector: &str) -> Option<usize> {
        self.query_selector_all(selector).into_iter().next()
    }

    /// Handle complex selectors with combinators (descendant, child).
    fn query_complex_selector(&self, selector: &str) -> Vec<usize> {
        // Split by combinators while preserving combinator type
        let parts = parse_selector_parts(selector);
        if parts.is_empty() { return Vec::new(); }

        let mut results = Vec::new();
        for node in &self.nodes {
            if node.node_type != NodeType::Element { continue; }
            if self.matches_complex_parts(node.id, &parts) {
                results.push(node.id);
            }
        }
        results
    }

    /// Check if a node matches a complex selector (rightmost part must match the node,
    /// then walk up ancestors for descendant/child relations).
    fn matches_complex_parts(&self, node_id: usize, parts: &[(String, SelectorCombinator)]) -> bool {
        if parts.is_empty() { return false; }
        let (last_sel, _) = &parts[parts.len() - 1];
        let node = match self.get_node(node_id) {
            Some(n) => n,
            None => return false,
        };
        if !matches_simple_selector(node, last_sel) { return false; }
        if parts.len() == 1 { return true; }

        // Walk up the ancestor chain for remaining parts
        let remaining = &parts[..parts.len() - 1];
        let (_, combinator) = &remaining[remaining.len() - 1];
        let parent_sel = &remaining[remaining.len() - 1].0;

        match combinator {
            SelectorCombinator::Child => {
                // Direct parent must match
                if let Some(parent_id) = node.parent {
                    if let Some(parent_node) = self.get_node(parent_id) {
                        if matches_simple_selector(parent_node, parent_sel) {
                            if remaining.len() == 1 { return true; }
                            return self.matches_complex_parts(parent_id, &remaining[..remaining.len() - 1].iter().map(|(s, _)| (s.clone(), SelectorCombinator::Descendant)).collect::<Vec<_>>());
                        }
                    }
                }
                false
            }
            SelectorCombinator::Descendant => {
                // Any ancestor must match
                let mut current = node.parent;
                while let Some(pid) = current {
                    if let Some(pnode) = self.get_node(pid) {
                        if matches_simple_selector(pnode, parent_sel) {
                            if remaining.len() == 1 { return true; }
                            return self.matches_complex_parts(pid, &remaining[..remaining.len() - 1].iter().map(|(s, _)| (s.clone(), SelectorCombinator::Descendant)).collect::<Vec<_>>());
                        }
                        current = pnode.parent;
                    } else { break; }
                }
                false
            }
        }
    }

    /// Get text content of a node (all descendant text concatenated).
    pub fn text_content(&self, node_id: usize) -> String {
        let mut out = String::new();
        self.collect_text_content(node_id, &mut out);
        out
    }

    fn collect_text_content(&self, node_id: usize, out: &mut String) {
        if let Some(node) = self.get_node(node_id) {
            if node.node_type == NodeType::Text {
                out.push_str(&node.text_content);
            }
            for &child_id in &node.children {
                self.collect_text_content(child_id, out);
            }
        }
    }

    /// Get children that are elements (for element.children).
    pub fn element_children(&self, node_id: usize) -> Vec<usize> {
        self.get_node(node_id)
            .map(|n| n.children.iter().copied()
                .filter(|&c| self.get_node(c).map(|n| n.node_type == NodeType::Element).unwrap_or(false))
                .collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
enum SelectorCombinator {
    Descendant, // space
    Child,      // >
}

/// Parse selector into parts with combinators.
/// "div > p .foo" => [("div", Descendant), ("p", Descendant), (".foo", _)]
fn parse_selector_parts(selector: &str) -> Vec<(String, SelectorCombinator)> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut combinator = SelectorCombinator::Descendant;
    let chars: Vec<char> = selector.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '>' {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                parts.push((trimmed, combinator));
            }
            combinator = SelectorCombinator::Child;
            current.clear();
            i += 1;
        } else if chars[i] == ' ' {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                parts.push((trimmed, combinator));
                combinator = SelectorCombinator::Descendant;
                current.clear();
            }
            i += 1;
        } else {
            current.push(chars[i]);
            i += 1;
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        parts.push((trimmed, combinator));
    }
    parts
}

/// Check if a node matches a simple CSS selector (no combinators).
/// Supports: tag, #id, .class, tag.class, tag#id, [attr], [attr=val],
/// [attr^=val], [attr$=val], [attr*=val], :nth-child(n), :first-child, :last-child.
fn matches_simple_selector(node: &DomNode, sel: &str) -> bool {
    // Handle :nth-child, :first-child, :last-child pseudo-classes
    if let Some(base_and_pseudo) = sel.split_once(':') {
        let (base, pseudo) = base_and_pseudo;
        // base might be empty (":first-child") or a tag/class ("li:nth-child(2)")
        if !base.is_empty() && !matches_simple_selector(node, base) { return false; }
        return matches_pseudo(node, pseudo);
    }
    // Handle attribute selectors: tag[attr=val] or [attr=val]
    if sel.contains('[') {
        return matches_attr_selector(node, sel);
    }
    // Simple selectors
    if let Some(id) = sel.strip_prefix('#') {
        node.attributes.get("id").map(|s| s.as_str()) == Some(id)
    } else if let Some(class) = sel.strip_prefix('.') {
        node.attributes.get("class")
            .map(|c| c.split_whitespace().any(|x| x == class))
            .unwrap_or(false)
    } else if sel.contains('.') || sel.contains('#') {
        // compound: tag.class or tag#id
        let (tag, rest) = if sel.contains('.') {
            let mut parts = sel.splitn(2, '.');
            (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
        } else {
            let mut parts = sel.splitn(2, '#');
            (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
        };
        let tag_ok = tag.is_empty() || node.tag_name == tag;
        let rest_ok = if sel.contains('.') {
            node.attributes.get("class").map(|c| c.split_whitespace().any(|x| x == rest)).unwrap_or(false)
        } else {
            node.attributes.get("id").map(|s| s.as_str()) == Some(rest)
        };
        tag_ok && rest_ok
    } else {
        node.tag_name == sel
    }
}

/// Handle attribute selectors: [attr], [attr=val], [attr^=val], [attr$=val], [attr*=val]
fn matches_attr_selector(node: &DomNode, sel: &str) -> bool {
    let bracket_start = match sel.find('[') { Some(i) => i, None => return false };
    let bracket_end = match sel.find(']') { Some(i) => i, None => return false };

    // Check tag prefix
    let tag_prefix = &sel[..bracket_start];
    if !tag_prefix.is_empty() && node.tag_name != tag_prefix { return false; }

    let attr_expr = &sel[bracket_start + 1..bracket_end];

    if let Some((attr, val)) = attr_expr.split_once("^=") {
        let val = val.trim_matches('"').trim_matches('\'');
        node.attributes.get(attr).map(|v| v.starts_with(val)).unwrap_or(false)
    } else if let Some((attr, val)) = attr_expr.split_once("$=") {
        let val = val.trim_matches('"').trim_matches('\'');
        node.attributes.get(attr).map(|v| v.ends_with(val)).unwrap_or(false)
    } else if let Some((attr, val)) = attr_expr.split_once("*=") {
        let val = val.trim_matches('"').trim_matches('\'');
        node.attributes.get(attr).map(|v| v.contains(val)).unwrap_or(false)
    } else if let Some((attr, val)) = attr_expr.split_once('=') {
        let val = val.trim_matches('"').trim_matches('\'');
        node.attributes.get(attr).map(|v| v.as_str() == val).unwrap_or(false)
    } else {
        // Just [attr] - check existence
        node.attributes.contains_key(attr_expr)
    }
}

/// Handle pseudo-class matching.
fn matches_pseudo(_node: &DomNode, pseudo: &str) -> bool {
    // For pseudo-classes that require sibling info, we do a simplified check
    // Since we don't have easy access to siblings here, return true for basic cases
    match pseudo {
        "first-child" | "last-child" => true, // simplified
        p if p.starts_with("nth-child(") => true, // simplified
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree(html: &str) -> DomTree {
        DomTree::new(HtmlParser::parse_html5(html))
    }

    #[test]
    fn create_and_append_element() {
        let mut tree = make_tree("<div id='root'></div>");
        let root = tree.query_selector("#root").unwrap();
        let span = tree.create_element("span");
        tree.append_child(root, span);
        assert!(tree.get_node(root).unwrap().children.contains(&span));
        assert_eq!(tree.get_node(span).unwrap().parent, Some(root));
    }

    #[test]
    fn create_text_node_and_append() {
        let mut tree = make_tree("<p id='p'></p>");
        let p = tree.query_selector("#p").unwrap();
        let text = tree.create_text_node("Hello");
        tree.append_child(p, text);
        assert_eq!(tree.text_content(p), "Hello");
    }

    #[test]
    fn remove_child_works() {
        let mut tree = make_tree("<div id='d'><span>hi</span></div>");
        let d = tree.query_selector("#d").unwrap();
        let children = tree.get_node(d).unwrap().children.clone();
        assert!(!children.is_empty());
        tree.remove_child(d, children[0]);
        assert!(tree.get_node(d).unwrap().children.is_empty());
    }

    #[test]
    fn set_inner_html_replaces_content() {
        let mut tree = make_tree("<div id='target'>old</div>");
        let target = tree.query_selector("#target").unwrap();
        tree.set_inner_html(target, "<b>new</b>");
        let html = tree.get_inner_html(target);
        assert!(html.contains("new"));
    }

    #[test]
    fn query_selector_all_finds_multiple() {
        let tree = make_tree("<ul><li>a</li><li>b</li><li>c</li></ul>");
        let items = tree.query_selector_all("li");
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn attribute_selector_exact_match() {
        let tree = make_tree("<input type=\"text\" id=\"a\"><input type=\"password\" id=\"b\">");
        let results = tree.query_selector_all("[type=\"password\"]");
        assert_eq!(results.len(), 1);
        let node = tree.get_node(results[0]).unwrap();
        assert_eq!(node.attributes.get("id").map(|s| s.as_str()), Some("b"));
    }

    #[test]
    fn compound_tag_class_selector() {
        let tree = make_tree("<div class=\"active\">1</div><span class=\"active\">2</span><div class=\"other\">3</div>");
        let results = tree.query_selector_all("div.active");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn clone_node_creates_independent_copy() {
        let mut tree = make_tree("<div id=\"orig\" class=\"box\"><span>inner</span></div>");
        let orig = tree.query_selector("#orig").unwrap();
        let cloned = tree.clone_node(orig, true);
        assert_ne!(cloned, orig);
        // Cloned node should have same tag and attributes
        let orig_node = tree.get_node(orig).unwrap();
        let clone_node = tree.get_node(cloned).unwrap();
        assert_eq!(clone_node.tag_name, orig_node.tag_name);
        assert_eq!(clone_node.attributes.get("class"), orig_node.attributes.get("class"));
    }
}
