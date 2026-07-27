use crate::parser::html::DomNode;

/// Scoped CSS matcher for Shadow DOM encapsulation.
/// Handles :host, :host(), ::slotted(), :defined, and shadow-piercing combinators.
pub struct ScopedCssMatcher;

/// The kind of scoped CSS selector matched.
#[derive(Debug, Clone, PartialEq)]
pub enum ScopedSelectorKind {
    /// :host — matches the shadow host element
    Host,
    /// :host(selector) — matches the shadow host if it matches selector
    HostFunctional(String),
    /// ::slotted(selector) — matches slotted light DOM nodes
    Slotted(String),
    /// :defined — matches custom elements that have been registered
    Defined,
    /// Normal selector within shadow tree
    Normal(String),
}

impl ScopedCssMatcher {
    /// Parse a scoped CSS selector into its kind.
    pub fn parse_scoped_selector(selector: &str) -> ScopedSelectorKind {
        let trimmed = selector.trim();
        if trimmed == ":host" {
            return ScopedSelectorKind::Host;
        }
        if trimmed.starts_with(":host(") && trimmed.ends_with(')') {
            let inner = &trimmed[6..trimmed.len() - 1];
            return ScopedSelectorKind::HostFunctional(inner.to_string());
        }
        if trimmed.starts_with("::slotted(") && trimmed.ends_with(')') {
            let inner = &trimmed[10..trimmed.len() - 1];
            return ScopedSelectorKind::Slotted(inner.to_string());
        }
        if trimmed == ":defined" {
            return ScopedSelectorKind::Defined;
        }
        ScopedSelectorKind::Normal(trimmed.to_string())
    }

    /// Check if a DOM node matches a :host selector.
    pub fn matches_host_selector(node: &DomNode, selector: &str) -> bool {
        let kind = Self::parse_scoped_selector(selector);
        match kind {
            ScopedSelectorKind::Host => {
                node.attributes.contains_key("shadowroot")
                    || node.attributes.contains_key("shadow-host")
            }
            ScopedSelectorKind::HostFunctional(ref inner) => {
                let is_host = node.attributes.contains_key("shadowroot")
                    || node.attributes.contains_key("shadow-host");
                if !is_host { return false; }
                Self::matches_simple_selector(node, inner)
            }
            _ => false,
        }
    }

    /// Check if a light DOM node matches a ::slotted() selector.
    pub fn matches_slotted(node: &DomNode, selector: &str) -> bool {
        let kind = Self::parse_scoped_selector(selector);
        match kind {
            ScopedSelectorKind::Slotted(ref inner) => {
                // The node must be assigned to a slot
                let has_slot = node.attributes.contains_key("slot");
                if !has_slot { return false; }
                Self::matches_simple_selector(node, inner)
            }
            _ => false,
        }
    }

    /// Check if a custom element is :defined (registered in the custom elements registry).
    pub fn matches_defined(node: &DomNode, registered_names: &[String]) -> bool {
        // :defined matches built-in elements and registered custom elements
        if !node.tag_name.contains('-') {
            return true; // built-in elements are always defined
        }
        registered_names.contains(&node.tag_name.to_lowercase())
    }

    /// Match a simple CSS selector against a node (used by scoped selectors).
    fn matches_simple_selector(node: &DomNode, selector: &str) -> bool {
        let trimmed = selector.trim();
        if trimmed == "*" {
            return true;
        }
        if let Some(id_sel) = trimmed.strip_prefix('#') {
            return node.attributes.get("id").map(|i| i == id_sel).unwrap_or(false);
        }
        if let Some(class_sel) = trimmed.strip_prefix('.') {
            return node.attributes.get("class")
                .map(|c| c.split_whitespace().any(|cls| cls == class_sel))
                .unwrap_or(false);
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let inner = &trimmed[1..trimmed.len() - 1];
            if let Some(eq_pos) = inner.find('=') {
                let attr_name = inner[..eq_pos].trim();
                let attr_val = inner[eq_pos + 1..].trim().trim_matches('"').trim_matches('\'');
                return node.attributes.get(attr_name).map(|v| v == attr_val).unwrap_or(false);
            }
            return node.attributes.contains_key(inner.trim());
        }
        // Tag name match
        node.tag_name.to_lowercase() == trimmed.to_lowercase()
    }

    /// Resolve shadow-piercing combinator (>>>) or /deep/.
    /// Returns true if the selector matches any node in the shadow tree.
    pub fn matches_deep_combinator(
        node: &DomNode,
        selector: &str,
        all_nodes: &[DomNode],
    ) -> bool {
        // Check for >>> or /deep/ combinator
        let parts = if selector.contains(" >>> ") {
            selector.splitn(2, " >>> ").collect::<Vec<_>>()
        } else if selector.contains(" /deep/ ") {
            selector.splitn(2, " /deep/ ").collect::<Vec<_>>()
        } else {
            return Self::matches_simple_selector(node, selector);
        };

        if parts.len() != 2 {
            return false;
        }

        let host_selector = parts[0].trim();
        let inner_selector = parts[1].trim();

        // First part must match a shadow host
        if !Self::matches_simple_selector(node, host_selector) {
            return false;
        }

        // Second part can match any descendant in the shadow tree
        for candidate in all_nodes {
            if Self::matches_simple_selector(candidate, inner_selector) {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::{DomNode, NodeType};
    use std::collections::HashMap;

    fn make_node(tag: &str, attrs: Vec<(&str, &str)>) -> DomNode {
        let mut attributes = HashMap::new();
        for (k, v) in attrs {
            attributes.insert(k.to_string(), v.to_string());
        }
        DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: tag.to_string(),
            attributes,
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        }
    }

    #[test]
    fn test_parse_host() {
        assert_eq!(ScopedCssMatcher::parse_scoped_selector(":host"), ScopedSelectorKind::Host);
    }

    #[test]
    fn test_parse_host_functional() {
        let kind = ScopedCssMatcher::parse_scoped_selector(":host(.active)");
        assert_eq!(kind, ScopedSelectorKind::HostFunctional(".active".to_string()));
    }

    #[test]
    fn test_parse_slotted() {
        let kind = ScopedCssMatcher::parse_scoped_selector("::slotted(span)");
        assert_eq!(kind, ScopedSelectorKind::Slotted("span".to_string()));
    }

    #[test]
    fn test_parse_defined() {
        assert_eq!(ScopedCssMatcher::parse_scoped_selector(":defined"), ScopedSelectorKind::Defined);
    }

    #[test]
    fn test_matches_host() {
        let node = make_node("div", vec![("shadowroot", "open")]);
        assert!(ScopedCssMatcher::matches_host_selector(&node, ":host"));
    }

    #[test]
    fn test_matches_host_functional() {
        let node = make_node("div", vec![("shadowroot", "open"), ("class", "active")]);
        assert!(ScopedCssMatcher::matches_host_selector(&node, ":host(.active)"));
        assert!(!ScopedCssMatcher::matches_host_selector(&node, ":host(.missing)"));
    }

    #[test]
    fn test_matches_slotted() {
        let node = make_node("span", vec![("slot", "header")]);
        assert!(ScopedCssMatcher::matches_slotted(&node, "::slotted(span)"));
        assert!(!ScopedCssMatcher::matches_slotted(&node, "::slotted(div)"));
    }

    #[test]
    fn test_slotted_requires_slot_attr() {
        let node = make_node("span", vec![]);
        assert!(!ScopedCssMatcher::matches_slotted(&node, "::slotted(span)"));
    }

    #[test]
    fn test_matches_defined_builtin() {
        let node = make_node("div", vec![]);
        assert!(ScopedCssMatcher::matches_defined(&node, &[]));
    }

    #[test]
    fn test_matches_defined_custom() {
        let node = make_node("my-element", vec![]);
        let registered = vec!["my-element".to_string()];
        assert!(ScopedCssMatcher::matches_defined(&node, &registered));
        assert!(!ScopedCssMatcher::matches_defined(&node, &[]));
    }

    #[test]
    fn test_deep_combinator() {
        let host = make_node("div", vec![("class", "wrapper")]);
        let inner = make_node("span", vec![("class", "target")]);
        let nodes = vec![host.clone(), inner.clone()];
        assert!(ScopedCssMatcher::matches_deep_combinator(&host, "div >>> .target", &nodes));
    }

    #[test]
    fn test_normal_selector_passthrough() {
        let _node = make_node("p", vec![("class", "intro")]);
        let kind = ScopedCssMatcher::parse_scoped_selector("p.intro");
        assert!(matches!(kind, ScopedSelectorKind::Normal(_)));
    }
}
