use std::collections::HashMap;

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
