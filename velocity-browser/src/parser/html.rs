use std::collections::HashMap;

use crate::parser::html5::{Html5Tokenizer, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    Document,
    Element,
    Text,
}

#[derive(Debug, Clone)]
pub struct DomNode {
    pub id: usize,
    pub node_type: NodeType,
    pub tag_name: String,
    pub attributes: HashMap<String, String>,
    pub text_content: String,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
}

pub struct HtmlParser;

impl HtmlParser {
    pub fn parse(html: &str) -> Vec<DomNode> {
        let mut nodes = Vec::new();

        // Root document node
        let root = DomNode {
            id: 0,
            node_type: NodeType::Document,
            tag_name: "#document".to_string(),
            attributes: HashMap::new(),
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        nodes.push(root);

        let mut current_id = 0;
        let mut idx = 0;
        let chars: Vec<char> = html.chars().collect();
        let len = chars.len();

        while idx < len {
            if chars[idx] == '<' {
                if idx + 1 < len && chars[idx + 1] == '/' {
                    // Closing tag
                    idx += 2;
                    while idx < len && chars[idx] != '>' {
                        idx += 1;
                    }
                    idx += 1;
                    if let Some(parent) = nodes[current_id].parent {
                        current_id = parent;
                    }
                } else if idx + 1 < len && chars[idx + 1] != '!' {
                    // Opening tag
                    idx += 1;
                    let mut tag = String::new();
                    while idx < len
                        && !chars[idx].is_whitespace()
                        && chars[idx] != '>'
                        && chars[idx] != '/'
                    {
                        tag.push(chars[idx]);
                        idx += 1;
                    }

                    let mut attrs = HashMap::new();
                    while idx < len && chars[idx] != '>' && chars[idx] != '/' {
                        if chars[idx].is_whitespace() {
                            idx += 1;
                            continue;
                        }
                        let mut key = String::new();
                        while idx < len
                            && chars[idx] != '='
                            && !chars[idx].is_whitespace()
                            && chars[idx] != '>'
                        {
                            key.push(chars[idx]);
                            idx += 1;
                        }
                        let mut val = String::new();
                        if idx < len && chars[idx] == '=' {
                            idx += 1;
                            if idx < len && (chars[idx] == '"' || chars[idx] == '\'') {
                                let quote = chars[idx];
                                idx += 1;
                                while idx < len && chars[idx] != quote {
                                    val.push(chars[idx]);
                                    idx += 1;
                                }
                                if idx < len {
                                    idx += 1;
                                }
                            } else {
                                while idx < len && !chars[idx].is_whitespace() && chars[idx] != '>'
                                {
                                    val.push(chars[idx]);
                                    idx += 1;
                                }
                            }
                        }
                        if !key.is_empty() {
                            attrs.insert(key, val);
                        }
                    }

                    let self_closing = idx < len && chars[idx] == '/';
                    while idx < len && chars[idx] != '>' {
                        idx += 1;
                    }
                    if idx < len {
                        idx += 1;
                    }

                    if !tag.is_empty() {
                        let node_id = nodes.len();
                        let node = DomNode {
                            id: node_id,
                            node_type: NodeType::Element,
                            tag_name: tag.to_lowercase(),
                            attributes: attrs,
                            text_content: String::new(),
                            children: Vec::new(),
                            parent: Some(current_id),
                        };
                        nodes.push(node);
                        nodes[current_id].children.push(node_id);

                        if !self_closing
                            && tag != "img"
                            && tag != "input"
                            && tag != "br"
                            && tag != "meta"
                            && tag != "link"
                        {
                            current_id = node_id;
                        }
                    }
                } else {
                    idx += 1;
                }
            } else {
                // Text node
                let mut text = String::new();
                while idx < len && chars[idx] != '<' {
                    text.push(chars[idx]);
                    idx += 1;
                }
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let node_id = nodes.len();
                    let text_node = DomNode {
                        id: node_id,
                        node_type: NodeType::Text,
                        tag_name: "#text".to_string(),
                        attributes: HashMap::new(),
                        text_content: trimmed.to_string(),
                        children: Vec::new(),
                        parent: Some(current_id),
                    };
                    nodes.push(text_node);
                    nodes[current_id].children.push(node_id);
                }
            }
        }

        nodes
    }
}

impl HtmlParser {
    /// Void elements: they never take children and have no end tag.
    fn is_void_element(tag: &str) -> bool {
        matches!(
            tag,
            "area"
                | "base"
                | "br"
                | "col"
                | "embed"
                | "hr"
                | "img"
                | "input"
                | "link"
                | "meta"
                | "param"
                | "source"
                | "track"
                | "wbr"
        )
    }

    /// Faithful DOM construction driven by the HTML5 tokenizer.
    ///
    /// Produces the same `Vec<DomNode>` shape as [`HtmlParser::parse`] but with
    /// correct nesting: it maintains a proper open-element stack, treats void
    /// and self-closing tags as leaves, and recovers from stray/mismatched end
    /// tags by popping to the nearest matching open element instead of blindly
    /// unwinding one level. This is what `load_html` uses.
    pub fn parse_html5(html: &str) -> Vec<DomNode> {
        let tokens = Html5Tokenizer::new(html).tokenize();

        let mut nodes = Vec::new();
        nodes.push(DomNode {
            id: 0,
            node_type: NodeType::Document,
            tag_name: "#document".to_string(),
            attributes: HashMap::new(),
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        });

        // Stack of open element ids; index 0 (the document) is never popped.
        let mut stack: Vec<usize> = vec![0];

        for tok in &tokens {
            match tok.kind {
                TokenKind::StartTag => {
                    let parent = *stack.last().unwrap();
                    let node_id = nodes.len();
                    nodes.push(DomNode {
                        id: node_id,
                        node_type: NodeType::Element,
                        tag_name: tok.name.clone(),
                        attributes: tok.attributes.clone(),
                        text_content: String::new(),
                        children: Vec::new(),
                        parent: Some(parent),
                    });
                    nodes[parent].children.push(node_id);
                    if !tok.self_closing && !Self::is_void_element(&tok.name) {
                        stack.push(node_id);
                    }
                }
                TokenKind::EndTag => {
                    // Close the nearest matching open element; ignore strays.
                    if let Some(pos) = stack.iter().rposition(|&id| nodes[id].tag_name == tok.name)
                    {
                        if pos != 0 {
                            stack.truncate(pos);
                        }
                    }
                }
                TokenKind::Character => {
                    let parent = *stack.last().unwrap();
                    let node_id = nodes.len();
                    nodes.push(DomNode {
                        id: node_id,
                        node_type: NodeType::Text,
                        tag_name: "#text".to_string(),
                        attributes: HashMap::new(),
                        text_content: tok.data.clone(),
                        children: Vec::new(),
                        parent: Some(parent),
                    });
                    nodes[parent].children.push(node_id);
                }
                // Comments, doctype, and EOF do not contribute element nodes.
                _ => {}
            }
        }

        nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_elements_via_open_element_stack() {
        let nodes = HtmlParser::parse_html5("<div><p>hi</p></div>");
        let div = nodes.iter().find(|n| n.tag_name == "div").unwrap();
        let p = nodes.iter().find(|n| n.tag_name == "p").unwrap();
        assert_eq!(p.parent, Some(div.id));
        let text = nodes
            .iter()
            .find(|n| n.node_type == NodeType::Text)
            .unwrap();
        assert_eq!(text.text_content, "hi");
        assert_eq!(text.parent, Some(p.id));
    }

    #[test]
    fn void_elements_do_not_capture_siblings() {
        let nodes = HtmlParser::parse_html5("<div><img src=\"a.png\"><span>x</span></div>");
        let div = nodes.iter().find(|n| n.tag_name == "div").unwrap();
        let img = nodes.iter().find(|n| n.tag_name == "img").unwrap();
        let span = nodes.iter().find(|n| n.tag_name == "span").unwrap();
        // img is a void leaf, so span is a sibling under div - not a child of img.
        assert_eq!(img.parent, Some(div.id));
        assert_eq!(span.parent, Some(div.id));
        assert!(img.children.is_empty());
    }

    #[test]
    fn stray_end_tag_is_ignored() {
        let nodes = HtmlParser::parse_html5("<div></span><p>ok</p></div>");
        let div = nodes.iter().find(|n| n.tag_name == "div").unwrap();
        let p = nodes.iter().find(|n| n.tag_name == "p").unwrap();
        // The stray </span> must not have popped <div>.
        assert_eq!(p.parent, Some(div.id));
    }

    // ── parse() tests ──────────────────────────────────────────────────────

    #[test]
    fn parse_empty_html_produces_root_only() {
        let nodes = HtmlParser::parse("");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_type, NodeType::Document);
    }

    #[test]
    fn parse_single_element() {
        let nodes = HtmlParser::parse("<div></div>");
        assert_eq!(nodes.len(), 2); // root + div
        assert_eq!(nodes[1].tag_name, "div");
        assert_eq!(nodes[1].node_type, NodeType::Element);
        assert_eq!(nodes[1].parent, Some(0));
    }

    #[test]
    fn parse_tag_name_is_lowercased() {
        let nodes = HtmlParser::parse("<DIV></DIV>");
        let div = nodes.iter().find(|n| n.tag_name == "div").unwrap();
        assert_eq!(div.tag_name, "div");
    }

    #[test]
    fn parse_text_node_content() {
        let nodes = HtmlParser::parse("<p>hello world</p>");
        let text = nodes
            .iter()
            .find(|n| n.node_type == NodeType::Text)
            .unwrap();
        assert_eq!(text.text_content, "hello world");
    }

    #[test]
    fn parse_whitespace_only_text_is_skipped() {
        let nodes = HtmlParser::parse("<div>   </div>");
        let text_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.node_type == NodeType::Text)
            .collect();
        assert!(
            text_nodes.is_empty(),
            "whitespace-only text should be skipped"
        );
    }

    #[test]
    fn parse_multiple_attributes() {
        let nodes = HtmlParser::parse(r#"<input type="text" name="email" value="a@b.com">"#);
        let input = nodes.iter().find(|n| n.tag_name == "input").unwrap();
        assert_eq!(input.attributes.get("type"), Some(&"text".to_string()));
        assert_eq!(input.attributes.get("name"), Some(&"email".to_string()));
        assert_eq!(input.attributes.get("value"), Some(&"a@b.com".to_string()));
    }

    #[test]
    fn parse_single_quoted_attributes() {
        let nodes = HtmlParser::parse("<div class='main'></div>");
        let div = nodes.iter().find(|n| n.tag_name == "div").unwrap();
        assert_eq!(div.attributes.get("class"), Some(&"main".to_string()));
    }

    #[test]
    fn parse_nested_elements() {
        let nodes = HtmlParser::parse("<div><ul><li>item</li></ul></div>");
        let li = nodes.iter().find(|n| n.tag_name == "li").unwrap();
        let ul = nodes.iter().find(|n| n.tag_name == "ul").unwrap();
        assert_eq!(li.parent, Some(ul.id));
    }

    #[test]
    fn parse_sibling_elements() {
        let nodes = HtmlParser::parse("<div><a>1</a><b>2</b><c>3</c></div>");
        let div = nodes.iter().find(|n| n.tag_name == "div").unwrap();
        assert_eq!(div.children.len(), 3);
    }

    #[test]
    fn parse_void_element_br() {
        let nodes = HtmlParser::parse("<p>line1<br>line2</p>");
        let br = nodes.iter().find(|n| n.tag_name == "br").unwrap();
        assert!(br.children.is_empty(), "br should have no children");
    }

    #[test]
    fn parse_self_closing_tag() {
        let nodes = HtmlParser::parse("<div><span/></div>");
        let span = nodes.iter().find(|n| n.tag_name == "span").unwrap();
        assert!(
            span.children.is_empty(),
            "self-closing span should have no children"
        );
    }

    // ── parse_html5() additional tests ─────────────────────────────────────

    #[test]
    fn html5_empty_html() {
        let nodes = HtmlParser::parse_html5("");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_type, NodeType::Document);
    }

    #[test]
    fn html5_preserves_attribute_values() {
        let nodes = HtmlParser::parse_html5(r#"<a href="/path" id="link">text</a>"#);
        let a = nodes.iter().find(|n| n.tag_name == "a").unwrap();
        assert_eq!(a.attributes.get("href"), Some(&"/path".to_string()));
        assert_eq!(a.attributes.get("id"), Some(&"link".to_string()));
    }

    #[test]
    fn html5_multiple_void_elements() {
        let nodes = HtmlParser::parse_html5("<div><hr><hr><hr></div>");
        let div = nodes.iter().find(|n| n.tag_name == "div").unwrap();
        let hrs: Vec<_> = nodes.iter().filter(|n| n.tag_name == "hr").collect();
        assert_eq!(hrs.len(), 3);
        for hr in &hrs {
            assert_eq!(hr.parent, Some(div.id));
            assert!(hr.children.is_empty());
        }
    }

    #[test]
    fn html5_deeply_nested() {
        let nodes = HtmlParser::parse_html5(
            "<div><div><div><div><span>deep</span></div></div></div></div>",
        );
        let span = nodes.iter().find(|n| n.tag_name == "span").unwrap();
        // Walk up parent chain to verify depth
        let mut depth = 0;
        let mut current = span.parent;
        while let Some(pid) = current {
            depth += 1;
            current = nodes[pid].parent;
        }
        assert!(
            depth >= 4,
            "span should be nested at least 4 deep, got {}",
            depth
        );
    }

    #[test]
    fn html5_text_content_preserved() {
        let nodes = HtmlParser::parse_html5("<p>  hello  </p>");
        let text = nodes
            .iter()
            .find(|n| n.node_type == NodeType::Text)
            .unwrap();
        // HTML5 tokenizer preserves data as-is
        assert!(text.text_content.contains("hello"));
    }

    #[test]
    fn html5_comment_nodes_ignored() {
        let nodes = HtmlParser::parse_html5("<div><!-- comment --><p>text</p></div>");
        let p = nodes.iter().find(|n| n.tag_name == "p").unwrap();
        let div = nodes.iter().find(|n| n.tag_name == "div").unwrap();
        assert_eq!(p.parent, Some(div.id));
    }

    #[test]
    fn html5_document_root_has_no_parent() {
        let nodes = HtmlParser::parse_html5("<div>x</div>");
        assert_eq!(nodes[0].parent, None);
        assert_eq!(nodes[0].node_type, NodeType::Document);
    }

    #[test]
    fn is_void_element_covers_all_void_tags() {
        let voids = [
            "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
            "source", "track", "wbr",
        ];
        for tag in &voids {
            assert!(HtmlParser::is_void_element(tag), "{} should be void", tag);
        }
        assert!(!HtmlParser::is_void_element("div"));
        assert!(!HtmlParser::is_void_element("span"));
    }
}
