use crate::parser::html::DomNode;

/// A parsed CSS declaration (property: value pair).
#[derive(Debug, Clone)]
pub struct CssDeclaration {
    pub property: String,
    pub value: String,
    pub important: bool,
}

/// A parsed CSS rule with selector, declarations, and specificity.
#[derive(Debug, Clone)]
pub struct FastCssRuleBitmask {
    pub selector_hash: u64,
    pub tag_name_hash: u64,
    pub class_hash: u64,
    pub specificity_score: u32,
    /// Parsed selector text for matching.
    pub selector_text: String,
    /// Parsed declarations.
    pub declarations: Vec<CssDeclaration>,
    /// Specificity components: (inline, ids, classes/attrs/pseudo-classes, elements/pseudo-elements).
    pub specificity: (u32, u32, u32, u32),
}

/// Fast CSS parser that extracts rules, selectors, and declarations from CSS text.
pub struct FastCssParser;

impl FastCssParser {
    /// Parse CSS text into a list of rule bitmasks with full selector and declaration data.
    pub fn parse_rules_fast(css: &str) -> Vec<FastCssRuleBitmask> {
        let mut rules = Vec::new();
        let cleaned = Self::strip_comments(css);
        let mut pos = 0;
        let bytes = cleaned.as_bytes();
        let len = bytes.len();

        while pos < len {
            // Skip whitespace
            while pos < len && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos >= len {
                break;
            }

            // Skip @-rules (media queries, keyframes, etc.) — collect but don't recurse
            if bytes[pos] == b'@' {
                let at_start = pos;
                // Find the opening brace or semicolon
                while pos < len && bytes[pos] != b'{' && bytes[pos] != b';' {
                    pos += 1;
                }
                if pos < len && bytes[pos] == b'{' {
                    // Skip the block (one level of nesting)
                    let mut depth = 1;
                    pos += 1;
                    while pos < len && depth > 0 {
                        if bytes[pos] == b'{' {
                            depth += 1;
                        }
                        if bytes[pos] == b'}' {
                            depth -= 1;
                        }
                        pos += 1;
                    }
                } else if pos < len {
                    pos += 1; // skip ';'
                }
                // Store @-rule as a rule with empty selector for tracking
                let at_text = &cleaned[at_start..pos];
                let selector_hash = crate::nda::hash_str(at_text);
                rules.push(FastCssRuleBitmask {
                    selector_hash,
                    tag_name_hash: 0,
                    class_hash: 0,
                    specificity_score: 0,
                    selector_text: at_text.to_string(),
                    declarations: Vec::new(),
                    specificity: (0, 0, 0, 0),
                });
                continue;
            }

            // Find selector (everything before '{')
            let selector_start = pos;
            while pos < len && bytes[pos] != b'{' {
                pos += 1;
            }
            if pos >= len {
                break;
            }
            let selector_text = cleaned[selector_start..pos].trim().to_string();
            pos += 1; // skip '{'

            // Find declarations (everything before '}')
            let decl_start = pos;
            let mut depth = 1;
            while pos < len && depth > 0 {
                if bytes[pos] == b'{' {
                    depth += 1;
                }
                if bytes[pos] == b'}' {
                    depth -= 1;
                }
                pos += 1;
            }
            let decl_text = &cleaned[decl_start..pos.saturating_sub(1)];

            if selector_text.is_empty() {
                continue;
            }

            // Parse declarations
            let declarations = Self::parse_declarations(decl_text);

            // Compute specificity for the selector
            let specificity = Self::compute_specificity(&selector_text);
            let specificity_score =
                specificity.0 * 1000 + specificity.1 * 100 + specificity.2 * 10 + specificity.3;

            // Compute hashes
            let selector_hash = crate::nda::hash_str(&selector_text);
            let tag_name_hash = Self::extract_tag_hash(&selector_text);
            let class_hash = Self::extract_class_hash(&selector_text);

            rules.push(FastCssRuleBitmask {
                selector_hash,
                tag_name_hash,
                class_hash,
                specificity_score,
                selector_text,
                declarations,
                specificity,
            });
        }

        rules
    }

    /// Match a DOM node against a parsed CSS rule selector.
    pub fn matches_bitmask(node: &DomNode, rule: &FastCssRuleBitmask) -> bool {
        if node.tag_name.is_empty() {
            return false;
        }
        if rule.selector_text.is_empty() {
            return false;
        }

        // Universal selector
        if rule.selector_text == "*" {
            return true;
        }

        // Type selector (tag name)
        if rule.selector_text == node.tag_name.to_lowercase() {
            return true;
        }

        // ID selector
        if rule.selector_text.starts_with('#') {
            let id_sel = rule.selector_text.trim_start_matches('#');
            if let Some(node_id) = node.attributes.get("id") {
                if node_id == id_sel {
                    return true;
                }
            }
            return false;
        }

        // Class selector
        if rule.selector_text.starts_with('.') {
            let class_sel = rule.selector_text.trim_start_matches('.');
            if let Some(node_class) = node.attributes.get("class") {
                return node_class.split_whitespace().any(|c| c == class_sel);
            }
            return false;
        }

        // Attribute selector [attr] or [attr=value]
        if rule.selector_text.starts_with('[') && rule.selector_text.ends_with(']') {
            let inner = &rule.selector_text[1..rule.selector_text.len() - 1];
            if let Some(eq_pos) = inner.find('=') {
                let attr_name = inner[..eq_pos].trim();
                let attr_val = inner[eq_pos + 1..]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');
                return node
                    .attributes
                    .get(attr_name)
                    .map(|v| v == attr_val)
                    .unwrap_or(false);
            } else {
                return node.attributes.contains_key(inner.trim());
            }
        }

        // Compound selector: tag.class or tag#id
        if let Some(dot_pos) = rule.selector_text.find('.') {
            let tag_part = &rule.selector_text[..dot_pos];
            let class_part = &rule.selector_text[dot_pos + 1..];
            let tag_match = tag_part.is_empty() || node.tag_name.to_lowercase() == *tag_part;
            let class_match = node
                .attributes
                .get("class")
                .map(|c| c.split_whitespace().any(|cls| cls == class_part))
                .unwrap_or(false);
            return tag_match && class_match;
        }

        if let Some(hash_pos) = rule.selector_text.find('#') {
            let tag_part = &rule.selector_text[..hash_pos];
            let id_part = &rule.selector_text[hash_pos + 1..];
            let tag_match = tag_part.is_empty() || node.tag_name.to_lowercase() == *tag_part;
            let id_match = node
                .attributes
                .get("id")
                .map(|i| i == id_part)
                .unwrap_or(false);
            return tag_match && id_match;
        }

        // Descendant combinator (space-separated)
        if rule.selector_text.contains(' ') {
            let parts: Vec<&str> = rule.selector_text.split_whitespace().collect();
            // Match the last part against this node
            if let Some(last) = parts.last() {
                return Self::matches_simple_selector(node, last);
            }
        }

        false
    }

    /// Match a simple selector (no combinators) against a node.
    fn matches_simple_selector(node: &DomNode, selector: &str) -> bool {
        if selector == "*" {
            return true;
        }
        if let Some(id_sel) = selector.strip_prefix('#') {
            return node
                .attributes
                .get("id")
                .map(|i| i == id_sel)
                .unwrap_or(false);
        }
        if let Some(class_sel) = selector.strip_prefix('.') {
            return node
                .attributes
                .get("class")
                .map(|c| c.split_whitespace().any(|cls| cls == class_sel))
                .unwrap_or(false);
        }
        if selector.starts_with('[') && selector.ends_with(']') {
            let inner = &selector[1..selector.len() - 1];
            if let Some(eq_pos) = inner.find('=') {
                let attr_name = &inner[..eq_pos];
                let attr_val = inner[eq_pos + 1..].trim_matches('"').trim_matches('\'');
                return node
                    .attributes
                    .get(attr_name)
                    .map(|v| v == attr_val)
                    .unwrap_or(false);
            }
            return node.attributes.contains_key(inner);
        }
        // Tag name
        node.tag_name.to_lowercase() == selector.to_lowercase()
    }

    /// Strip CSS comments (/* ... */).
    fn strip_comments(css: &str) -> String {
        let mut result = String::with_capacity(css.len());
        let mut chars = css.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '/' && chars.peek() == Some(&'*') {
                chars.next(); // consume '*'
                              // Skip until '*/'
                loop {
                    match chars.next() {
                        Some('*') if chars.peek() == Some(&'/') => {
                            chars.next();
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
            } else {
                result.push(ch);
            }
        }
        result
    }

    /// Parse declaration block text into declarations.
    fn parse_declarations(text: &str) -> Vec<CssDeclaration> {
        let mut decls = Vec::new();
        for part in text.split(';') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(colon) = trimmed.find(':') {
                let property = trimmed[..colon].trim().to_lowercase();
                let value_raw = trimmed[colon + 1..].trim();
                let (value, important) = if value_raw.ends_with("!important") {
                    (value_raw[..value_raw.len() - 10].trim().to_string(), true)
                } else {
                    (value_raw.to_string(), false)
                };
                if !property.is_empty() && !value.is_empty() {
                    decls.push(CssDeclaration {
                        property,
                        value,
                        important,
                    });
                }
            }
        }
        decls
    }

    /// Compute CSS specificity as (inline, ids, classes, elements).
    fn compute_specificity(selector: &str) -> (u32, u32, u32, u32) {
        let mut ids = 0u32;
        let mut classes = 0u32;
        let mut elements = 0u32;

        // Split by combinators to get compound selectors
        for compound in
            selector.split(|c: char| c.is_whitespace() || c == '>' || c == '+' || c == '~')
        {
            let compound = compound.trim();
            if compound.is_empty() {
                continue;
            }
            let mut rest = compound;
            while !rest.is_empty() {
                if rest.starts_with('#') {
                    ids += 1;
                    let end = rest[1..]
                        .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                        .map(|p| p + 1)
                        .unwrap_or(rest.len());
                    rest = &rest[end..];
                } else if rest.starts_with("::") {
                    elements += 1;
                    let end = rest[2..]
                        .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                        .map(|p| p + 2)
                        .unwrap_or(rest.len());
                    rest = &rest[end..];
                } else if rest.starts_with(':') {
                    classes += 1;
                    let end = rest[1..]
                        .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                        .map(|p| p + 1)
                        .unwrap_or(rest.len());
                    rest = &rest[end..];
                } else if rest.starts_with('[') {
                    classes += 1;
                    if let Some(close) = rest.find(']') {
                        rest = &rest[close + 1..];
                    } else {
                        break;
                    }
                } else if rest.starts_with('.') {
                    classes += 1;
                    let end = rest[1..]
                        .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                        .map(|p| p + 1)
                        .unwrap_or(rest.len());
                    rest = &rest[end..];
                } else if rest.starts_with('*') {
                    rest = &rest[1..];
                } else {
                    // Tag name. Unknown leading characters (e.g. stray '<'
                    // when the fast scanner runs over raw page HTML around a
                    // <style> block) are skipped one at a time so this loop
                    // always advances instead of spinning forever.
                    let end = rest
                        .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                        .unwrap_or(rest.len());
                    if end > 0 {
                        elements += 1;
                        rest = &rest[end..];
                    } else {
                        let skip = rest.chars().next().map_or(1, |c| c.len_utf8());
                        rest = &rest[skip..];
                    }
                }
            }
        }

        (0, ids, classes, elements)
    }

    /// Extract tag name hash from selector.
    fn extract_tag_hash(selector: &str) -> u64 {
        let tag = selector
            .split(|c: char| c == '.' || c == '#' || c == '[' || c.is_whitespace())
            .next()
            .unwrap_or("");
        if tag.is_empty() || tag.starts_with('#') || tag.starts_with('.') || tag.starts_with('[') {
            0
        } else {
            crate::nda::hash_str(&tag.to_lowercase())
        }
    }

    /// Extract class hash from selector.
    fn extract_class_hash(selector: &str) -> u64 {
        if let Some(dot_pos) = selector.find('.') {
            let rest = &selector[dot_pos + 1..];
            let class_name = rest
                .split(|c: char| c.is_whitespace() || c == '.' || c == '#' || c == '[')
                .next()
                .unwrap_or("");
            if !class_name.is_empty() {
                return crate::nda::hash_str(class_name);
            }
        }
        0
    }

    /// Resolve cascaded declarations for a node given multiple matching rules.
    /// Returns a map of property -> value, respecting specificity and !important.
    pub fn cascade_rules_for_node(
        node: &DomNode,
        rules: &[FastCssRuleBitmask],
    ) -> std::collections::HashMap<String, String> {
        let mut matched: Vec<(u32, usize, &CssDeclaration)> = Vec::new();

        for (rule_idx, rule) in rules.iter().enumerate() {
            if Self::matches_bitmask(node, rule) {
                for decl in &rule.declarations {
                    matched.push((rule.specificity_score, rule_idx, decl));
                }
            }
        }

        // Sort by specificity ascending (lowest first); at same specificity, earlier source order first
        matched.sort_by(|a, b| {
            let ord = a.0.cmp(&b.0); // lower specificity first
            if ord == std::cmp::Ordering::Equal {
                a.1.cmp(&b.1)
            } else {
                ord
            } // earlier rule first at tie
        });

        // Separate normal and !important declarations
        let mut result = std::collections::HashMap::new();
        let mut important_decls: Vec<&CssDeclaration> = Vec::new();
        for (_, _, decl) in &matched {
            if decl.important {
                important_decls.push(decl);
            } else {
                // Last write wins = highest specificity wins (ascending order)
                result.insert(decl.property.clone(), decl.value.clone());
            }
        }
        // !important overrides normal (also ascending, so highest !important wins)
        for decl in &important_decls {
            result.insert(decl.property.clone(), decl.value.clone());
        }

        result
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
    fn test_parse_simple_rule() {
        let css = "div { color: red; font-size: 14px; }";
        let rules = FastCssParser::parse_rules_fast(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector_text, "div");
        assert_eq!(rules[0].declarations.len(), 2);
        assert_eq!(rules[0].declarations[0].property, "color");
        assert_eq!(rules[0].declarations[0].value, "red");
        assert_eq!(rules[0].declarations[1].property, "font-size");
        assert_eq!(rules[0].declarations[1].value, "14px");
    }

    /// parse_rules_fast is fed whole HTML documents by the session loader, so
    /// the "selector" preceding a brace can be raw markup full of '<' and '>'.
    /// compute_specificity used to spin forever on such garbage (regression).
    #[test]
    fn test_parse_terminates_on_raw_html_input() {
        let html = "<html><head><title>T</title><style>.x { color: red; }</style></head>\
                    <body><h1>Plans</h1></body></html>";
        let rules = FastCssParser::parse_rules_fast(html);
        assert!(!rules.is_empty());
    }

    #[test]
    fn test_parse_multiple_rules() {
        let css = "h1 { color: blue; } .active { background: green; } #main { width: 100%; }";
        let rules = FastCssParser::parse_rules_fast(css);
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].selector_text, "h1");
        assert_eq!(rules[1].selector_text, ".active");
        assert_eq!(rules[2].selector_text, "#main");
    }

    #[test]
    fn test_strip_comments() {
        let css = "/* comment */ div { color: red; /* inline */ }";
        let rules = FastCssParser::parse_rules_fast(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].declarations.len(), 1);
    }

    #[test]
    fn test_important() {
        let css = "div { color: red !important; }";
        let rules = FastCssParser::parse_rules_fast(css);
        assert!(rules[0].declarations[0].important);
    }

    #[test]
    fn test_matches_tag() {
        let node = make_node("div", vec![]);
        let rules = FastCssParser::parse_rules_fast("div { color: red; }");
        assert!(FastCssParser::matches_bitmask(&node, &rules[0]));
    }

    #[test]
    fn test_matches_class() {
        let node = make_node("div", vec![("class", "active highlight")]);
        let rules = FastCssParser::parse_rules_fast(".active { color: green; }");
        assert!(FastCssParser::matches_bitmask(&node, &rules[0]));
        let rules2 = FastCssParser::parse_rules_fast(".missing { color: red; }");
        assert!(!FastCssParser::matches_bitmask(&node, &rules2[0]));
    }

    #[test]
    fn test_matches_id() {
        let node = make_node("div", vec![("id", "main")]);
        let rules = FastCssParser::parse_rules_fast("#main { width: 100%; }");
        assert!(FastCssParser::matches_bitmask(&node, &rules[0]));
    }

    #[test]
    fn test_matches_attribute() {
        let node = make_node("input", vec![("type", "text")]);
        let rules = FastCssParser::parse_rules_fast("[type=text] { border: 1px; }");
        assert!(FastCssParser::matches_bitmask(&node, &rules[0]));
    }

    #[test]
    fn test_matches_compound() {
        let node = make_node("div", vec![("class", "active")]);
        let rules = FastCssParser::parse_rules_fast("div.active { color: red; }");
        assert!(FastCssParser::matches_bitmask(&node, &rules[0]));
    }

    #[test]
    fn test_matches_universal() {
        let node = make_node("span", vec![]);
        let rules = FastCssParser::parse_rules_fast("* { margin: 0; }");
        assert!(FastCssParser::matches_bitmask(&node, &rules[0]));
    }

    #[test]
    fn test_specificity_ordering() {
        let css = "div { color: red; } .cls { color: blue; } #id { color: green; }";
        let rules = FastCssParser::parse_rules_fast(css);
        assert!(rules[0].specificity_score < rules[1].specificity_score);
        assert!(rules[1].specificity_score < rules[2].specificity_score);
    }

    #[test]
    fn test_cascade_for_node() {
        let node = make_node("div", vec![("class", "active")]);
        let css = "div { color: red; } .active { color: blue; font-size: 14px; }";
        let rules = FastCssParser::parse_rules_fast(css);
        let cascaded = FastCssParser::cascade_rules_for_node(&node, &rules);
        assert_eq!(cascaded.get("color").unwrap(), "blue"); // .active wins over div
        assert_eq!(cascaded.get("font-size").unwrap(), "14px");
    }

    #[test]
    fn test_no_match_wrong_tag() {
        let node = make_node("span", vec![]);
        let rules = FastCssParser::parse_rules_fast("div { color: red; }");
        assert!(!FastCssParser::matches_bitmask(&node, &rules[0]));
    }

    #[test]
    fn test_at_rule_skipped() {
        let css = "@media (max-width: 600px) { div { color: red; } } h1 { color: blue; }";
        let rules = FastCssParser::parse_rules_fast(css);
        // @media is captured as one rule, h1 as another
        assert_eq!(rules.len(), 2);
        assert!(rules[0].selector_text.starts_with("@media"));
        assert_eq!(rules[1].selector_text, "h1");
    }

    #[test]
    fn test_empty_css() {
        let rules = FastCssParser::parse_rules_fast("");
        assert!(rules.is_empty());
    }

    #[test]
    fn test_comment_only_css() {
        let rules = FastCssParser::parse_rules_fast("/* just a comment */");
        assert!(rules.is_empty());
    }

    #[test]
    fn test_important_overrides_normal() {
        let node = make_node("div", vec![]);
        let css = "div { color: red; } div { color: blue !important; }";
        let rules = FastCssParser::parse_rules_fast(css);
        let cascaded = FastCssParser::cascade_rules_for_node(&node, &rules);
        assert_eq!(cascaded.get("color").unwrap(), "blue");
    }

    #[test]
    fn test_matches_tag_hash_id() {
        let node = make_node("div", vec![("id", "header")]);
        let rules = FastCssParser::parse_rules_fast("#header { font-size: 20px; }");
        assert!(FastCssParser::matches_bitmask(&node, &rules[0]));
        let wrong_node = make_node("div", vec![("id", "footer")]);
        assert!(!FastCssParser::matches_bitmask(&wrong_node, &rules[0]));
    }

    #[test]
    fn test_declaration_property_lowercased() {
        let css = "div { Color: Red; Font-Size: 14px; }";
        let rules = FastCssParser::parse_rules_fast(css);
        assert_eq!(rules[0].declarations[0].property, "color");
        assert_eq!(rules[0].declarations[1].property, "font-size");
    }

    #[test]
    fn test_multiple_declarations_semicolons() {
        let css = "p { margin: 0; padding: 5px; border: none; }";
        let rules = FastCssParser::parse_rules_fast(css);
        assert_eq!(rules[0].declarations.len(), 3);
    }

    #[test]
    fn test_specificity_star_is_zero() {
        let css = "* { margin: 0; }";
        let rules = FastCssParser::parse_rules_fast(css);
        assert_eq!(rules[0].specificity, (0, 0, 0, 0));
    }

    #[test]
    fn test_cascade_no_matching_rules() {
        let node = make_node("span", vec![]);
        let css = "div { color: red; }";
        let rules = FastCssParser::parse_rules_fast(css);
        let cascaded = FastCssParser::cascade_rules_for_node(&node, &rules);
        assert!(cascaded.is_empty());
    }

    #[test]
    fn test_attribute_presence_selector() {
        let node = make_node("input", vec![("disabled", "")]);
        let rules = FastCssParser::parse_rules_fast("[disabled] { opacity: 0.5; }");
        assert!(FastCssParser::matches_bitmask(&node, &rules[0]));
        let node2 = make_node("input", vec![]);
        assert!(!FastCssParser::matches_bitmask(&node2, &rules[0]));
    }
}
