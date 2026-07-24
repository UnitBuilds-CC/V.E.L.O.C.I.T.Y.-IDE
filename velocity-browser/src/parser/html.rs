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
                    while idx < len && !chars[idx].is_whitespace() && chars[idx] != '>' && chars[idx] != '/' {
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
                        while idx < len && chars[idx] != '=' && !chars[idx].is_whitespace() && chars[idx] != '>' {
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
                                if idx < len { idx += 1; }
                            } else {
                                while idx < len && !chars[idx].is_whitespace() && chars[idx] != '>' {
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
                    if idx < len { idx += 1; }

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

                        if !self_closing && tag != "img" && tag != "input" && tag != "br" && tag != "meta" && tag != "link" {
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
            "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link"
                | "meta" | "param" | "source" | "track" | "wbr"
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
                    if let Some(pos) = stack.iter().rposition(|&id| nodes[id].tag_name == tok.name) {
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
        let text = nodes.iter().find(|n| n.node_type == NodeType::Text).unwrap();
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
}
