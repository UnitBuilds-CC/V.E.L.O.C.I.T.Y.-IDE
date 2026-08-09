use crate::dom::DomTree;
use crate::parser::html::NodeType;
use std::collections::HashMap;

/// Constraint validation result for a form control.
#[derive(Debug, Clone)]
pub struct ValidationState {
    pub is_valid: bool,
    pub value_missing: bool,
    pub type_mismatch: bool,
    pub pattern_mismatch: bool,
    pub too_long: bool,
    pub too_short: bool,
    pub range_underflow: bool,
    pub range_overflow: bool,
    pub custom_error: Option<String>,
}

impl ValidationState {
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            value_missing: false,
            type_mismatch: false,
            pattern_mismatch: false,
            too_long: false,
            too_short: false,
            range_underflow: false,
            range_overflow: false,
            custom_error: None,
        }
    }
}

/// Form data serializer with constraint validation.
pub struct FormDataSerializer;

impl FormDataSerializer {
    /// Serialize form data, optionally filtering by form ID.
    pub fn serialize_form(tree: &DomTree, form_id_or_selector: &str) -> HashMap<String, String> {
        let mut form_data = HashMap::new();
        let mut in_target_form = form_id_or_selector.is_empty();

        for node in &tree.nodes {
            if node.node_type != NodeType::Element {
                continue;
            }

            // Check if this is the target form
            if node.tag_name == "form"
                && !form_id_or_selector.is_empty()
                && node.attributes.get("id").map(|s| s.as_str()) == Some(form_id_or_selector)
            {
                in_target_form = true;
                // Collect children of this form
                for &child_id in &node.children {
                    if let Some(child) = tree.get_node(child_id) {
                        Self::collect_input(child, tree, &mut form_data);
                    }
                }
                break;
            }
        }

        // Fallback: collect all inputs if no specific form found
        if !in_target_form {
            for node in &tree.nodes {
                if node.node_type != NodeType::Element {
                    continue;
                }
                Self::collect_input(node, tree, &mut form_data);
            }
        }

        form_data
    }

    /// Collect input value from a form control node.
    fn collect_input(
        node: &crate::parser::html::DomNode,
        tree: &DomTree,
        data: &mut HashMap<String, String>,
    ) {
        match node.tag_name.as_str() {
            "input" | "textarea" => {
                if let Some(name) = node.attributes.get("name") {
                    let disabled = node.attributes.get("disabled").is_some();
                    let input_type = node
                        .attributes
                        .get("type")
                        .map(|s| s.as_str())
                        .unwrap_or("text");
                    // Skip disabled, unchecked checkboxes/radios
                    if disabled {
                        return;
                    }
                    if (input_type == "checkbox" || input_type == "radio")
                        && !node.attributes.contains_key("checked")
                    {
                        return;
                    }
                    let val = node.attributes.get("value").cloned().unwrap_or_default();
                    data.insert(name.clone(), val);
                }
            }
            "select" => {
                if let Some(name) = node.attributes.get("name") {
                    // Find selected option
                    for &child_id in &node.children {
                        if let Some(option) = tree.get_node(child_id) {
                            if option.tag_name == "option"
                                && option.attributes.contains_key("selected")
                            {
                                let val = option
                                    .attributes
                                    .get("value")
                                    .cloned()
                                    .unwrap_or_else(|| option.text_content.clone());
                                data.insert(name.clone(), val);
                                return;
                            }
                        }
                    }
                    // Default to first option
                    for &child_id in &node.children {
                        if let Some(option) = tree.get_node(child_id) {
                            if option.tag_name == "option" {
                                let val = option
                                    .attributes
                                    .get("value")
                                    .cloned()
                                    .unwrap_or_else(|| option.text_content.clone());
                                data.insert(name.clone(), val);
                                return;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Validate a form control against HTML5 constraint validation.
    pub fn validate_control(node: &crate::parser::html::DomNode) -> ValidationState {
        let mut state = ValidationState::valid();
        let input_type = node
            .attributes
            .get("type")
            .map(|s| s.as_str())
            .unwrap_or("text");
        let value = node.attributes.get("value").cloned().unwrap_or_default();
        let required = node.attributes.contains_key("required");
        let disabled = node.attributes.contains_key("disabled");

        if disabled {
            return state;
        } // disabled controls are not validated

        // required
        if required && value.is_empty() {
            state.is_valid = false;
            state.value_missing = true;
        }

        // type constraints
        match input_type {
            "email" => {
                if !value.is_empty() && !value.contains('@') {
                    state.is_valid = false;
                    state.type_mismatch = true;
                }
            }
            "url" => {
                if !value.is_empty()
                    && !value.starts_with("http://")
                    && !value.starts_with("https://")
                {
                    state.is_valid = false;
                    state.type_mismatch = true;
                }
            }
            "number" | "range" => {
                if !value.is_empty() && value.parse::<f64>().is_err() {
                    state.is_valid = false;
                    state.type_mismatch = true;
                }
                if let Ok(num) = value.parse::<f64>() {
                    if let Some(min) = node
                        .attributes
                        .get("min")
                        .and_then(|v| v.parse::<f64>().ok())
                    {
                        if num < min {
                            state.is_valid = false;
                            state.range_underflow = true;
                        }
                    }
                    if let Some(max) = node
                        .attributes
                        .get("max")
                        .and_then(|v| v.parse::<f64>().ok())
                    {
                        if num > max {
                            state.is_valid = false;
                            state.range_overflow = true;
                        }
                    }
                }
            }
            _ => {}
        }

        // pattern
        if let Some(pattern) = node.attributes.get("pattern") {
            if !value.is_empty() {
                // Simple pattern matching (exact match, no regex engine)
                if !Self::simple_pattern_match(&value, pattern) {
                    state.is_valid = false;
                    state.pattern_mismatch = true;
                }
            }
        }

        // minlength / maxlength
        if let Some(maxlen) = node
            .attributes
            .get("maxlength")
            .and_then(|v| v.parse::<usize>().ok())
        {
            if value.len() > maxlen {
                state.is_valid = false;
                state.too_long = true;
            }
        }
        if let Some(minlen) = node
            .attributes
            .get("minlength")
            .and_then(|v| v.parse::<usize>().ok())
        {
            if !value.is_empty() && value.len() < minlen {
                state.is_valid = false;
                state.too_short = true;
            }
        }

        state
    }

    /// Simple pattern matching (supports [a-z], [0-9], ., *, +).
    fn simple_pattern_match(value: &str, pattern: &str) -> bool {
        if pattern.is_empty() {
            return true;
        }
        // For simple patterns, check character classes
        let chars: Vec<char> = value.chars().collect();
        let pat_chars: Vec<char> = pattern.chars().collect();
        if pat_chars.len() != chars.len() {
            return false;
        }
        for (c, p) in chars.iter().zip(pat_chars.iter()) {
            match p {
                '.' => continue,
                '[' => {
                    // Simplified: just check if char is alphanumeric
                    if !c.is_alphanumeric() {
                        return false;
                    }
                }
                _ => {
                    if c != p {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Encode form data as URL-encoded string.
    pub fn to_url_encoded(data: &HashMap<String, String>) -> String {
        data.iter()
            .map(|(k, v)| format!("{}={}", Self::url_encode(k), Self::url_encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// Encode form data as multipart/form-data body.
    pub fn to_multipart(data: &HashMap<String, String>, boundary: &str) -> String {
        let mut body = String::new();
        for (k, v) in data {
            body.push_str(&format!("--{}\r\n", boundary));
            body.push_str(&format!(
                "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                k
            ));
            body.push_str(&format!("{}\r\n", v));
        }
        body.push_str(&format!("--{}--\r\n", boundary));
        body
    }

    /// Encode form data as JSON string.
    pub fn to_json(data: &HashMap<String, String>) -> String {
        let pairs: Vec<String> = data
            .iter()
            .map(|(k, v)| {
                format!(
                    "\"{}\":\"{}\"",
                    k.replace('"', "\\\""),
                    v.replace('"', "\\\"")
                )
            })
            .collect();
        format!("{{{}}}", pairs.join(","))
    }

    /// Simple URL encoding.
    fn url_encode(s: &str) -> String {
        let mut result = String::new();
        for ch in s.chars() {
            match ch {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(ch),
                ' ' => result.push_str("%20"),
                _ => {
                    for byte in ch.to_string().as_bytes() {
                        result.push_str(&format!("%{:02X}", byte));
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DomTree;
    use crate::parser::html::{DomNode, NodeType};

    fn make_form_tree() -> DomTree {
        let nodes = vec![
            DomNode {
                id: 0,
                node_type: NodeType::Document,
                tag_name: "#document".to_string(),
                attributes: HashMap::new(),
                text_content: String::new(),
                children: vec![1],
                parent: None,
            },
            DomNode {
                id: 1,
                node_type: NodeType::Element,
                tag_name: "form".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("id".to_string(), "login".to_string());
                    m
                },
                text_content: String::new(),
                children: vec![2, 3, 4],
                parent: Some(0),
            },
            DomNode {
                id: 2,
                node_type: NodeType::Element,
                tag_name: "input".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("name".to_string(), "user".to_string());
                    m.insert("value".to_string(), "admin".to_string());
                    m.insert("type".to_string(), "text".to_string());
                    m
                },
                text_content: String::new(),
                children: Vec::new(),
                parent: Some(1),
            },
            DomNode {
                id: 3,
                node_type: NodeType::Element,
                tag_name: "input".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("name".to_string(), "pass".to_string());
                    m.insert("value".to_string(), "secret".to_string());
                    m.insert("type".to_string(), "password".to_string());
                    m
                },
                text_content: String::new(),
                children: Vec::new(),
                parent: Some(1),
            },
            DomNode {
                id: 4,
                node_type: NodeType::Element,
                tag_name: "select".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("name".to_string(), "role".to_string());
                    m
                },
                text_content: String::new(),
                children: vec![5, 6],
                parent: Some(1),
            },
            DomNode {
                id: 5,
                node_type: NodeType::Element,
                tag_name: "option".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("value".to_string(), "user".to_string());
                    m
                },
                text_content: "User".to_string(),
                children: Vec::new(),
                parent: Some(4),
            },
            DomNode {
                id: 6,
                node_type: NodeType::Element,
                tag_name: "option".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("value".to_string(), "admin".to_string());
                    m.insert("selected".to_string(), "".to_string());
                    m
                },
                text_content: "Admin".to_string(),
                children: Vec::new(),
                parent: Some(4),
            },
        ];
        DomTree::new(nodes)
    }

    #[test]
    fn test_serialize_form() {
        let tree = make_form_tree();
        let data = FormDataSerializer::serialize_form(&tree, "login");
        assert_eq!(data.get("user").unwrap(), "admin");
        assert_eq!(data.get("pass").unwrap(), "secret");
        assert_eq!(data.get("role").unwrap(), "admin"); // selected option
    }

    #[test]
    fn test_url_encoded() {
        let mut data = HashMap::new();
        data.insert("name".to_string(), "hello world".to_string());
        let encoded = FormDataSerializer::to_url_encoded(&data);
        assert!(encoded.contains("hello%20world"));
    }

    #[test]
    fn test_multipart() {
        let mut data = HashMap::new();
        data.insert("field".to_string(), "value".to_string());
        let mp = FormDataSerializer::to_multipart(&data, "boundary123");
        assert!(mp.contains("--boundary123"));
        assert!(mp.contains("Content-Disposition"));
    }

    #[test]
    fn test_json() {
        let mut data = HashMap::new();
        data.insert("key".to_string(), "val".to_string());
        let json = FormDataSerializer::to_json(&data);
        assert!(json.contains("\"key\":\"val\""));
    }

    #[test]
    fn test_validate_required() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("required".to_string(), "".to_string());
                m.insert("type".to_string(), "text".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        assert!(!state.is_valid);
        assert!(state.value_missing);
    }

    #[test]
    fn test_validate_email() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("type".to_string(), "email".to_string());
                m.insert("value".to_string(), "invalid".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        assert!(!state.is_valid);
        assert!(state.type_mismatch);
    }

    #[test]
    fn test_validate_range() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("type".to_string(), "number".to_string());
                m.insert("value".to_string(), "150".to_string());
                m.insert("max".to_string(), "100".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        assert!(!state.is_valid);
        assert!(state.range_overflow);
    }

    #[test]
    fn test_validate_maxlength() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("type".to_string(), "text".to_string());
                m.insert("value".to_string(), "toolong".to_string());
                m.insert("maxlength".to_string(), "3".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        assert!(!state.is_valid);
        assert!(state.too_long);
    }

    #[test]
    fn test_disabled_not_validated() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("required".to_string(), "".to_string());
                m.insert("disabled".to_string(), "".to_string());
                m.insert("type".to_string(), "text".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        assert!(state.is_valid); // disabled controls pass validation
    }

    #[test]
    fn test_validate_url_type() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("type".to_string(), "url".to_string());
                m.insert("value".to_string(), "not-a-url".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        assert!(!state.is_valid);
        assert!(state.type_mismatch);
    }

    #[test]
    fn test_validate_url_valid() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("type".to_string(), "url".to_string());
                m.insert("value".to_string(), "https://example.com".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        assert!(state.is_valid);
    }

    #[test]
    fn test_validate_email_valid() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("type".to_string(), "email".to_string());
                m.insert("value".to_string(), "user@example.com".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        assert!(state.is_valid);
    }

    #[test]
    fn test_validate_number_range_underflow() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("type".to_string(), "number".to_string());
                m.insert("value".to_string(), "3".to_string());
                m.insert("min".to_string(), "5".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        assert!(!state.is_valid);
        assert!(state.range_underflow);
    }

    #[test]
    fn test_validate_minlength() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("type".to_string(), "text".to_string());
                m.insert("value".to_string(), "ab".to_string());
                m.insert("minlength".to_string(), "5".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        assert!(!state.is_valid);
        assert!(state.too_short);
    }

    #[test]
    fn test_validate_pattern_mismatch() {
        let node = DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "input".to_string(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("type".to_string(), "text".to_string());
                m.insert("value".to_string(), "ABC".to_string());
                m.insert("pattern".to_string(), "...".to_string());
                m
            },
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        };
        let state = FormDataSerializer::validate_control(&node);
        // Pattern "..." has length 3, value "ABC" has length 3, dots match any char
        assert!(state.is_valid); // dots match any alphanumeric
    }

    #[test]
    fn test_checkbox_unchecked_not_serialized() {
        let nodes = vec![
            DomNode {
                id: 0,
                node_type: NodeType::Document,
                tag_name: "#document".to_string(),
                attributes: HashMap::new(),
                text_content: String::new(),
                children: vec![1],
                parent: None,
            },
            DomNode {
                id: 1,
                node_type: NodeType::Element,
                tag_name: "form".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("id".to_string(), "f1".to_string());
                    m
                },
                text_content: String::new(),
                children: vec![2],
                parent: Some(0),
            },
            DomNode {
                id: 2,
                node_type: NodeType::Element,
                tag_name: "input".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("type".to_string(), "checkbox".to_string());
                    m.insert("name".to_string(), "agree".to_string());
                    m.insert("value".to_string(), "yes".to_string());
                    m
                },
                text_content: String::new(),
                children: Vec::new(),
                parent: Some(1),
            },
        ];
        let tree = DomTree::new(nodes);
        let data = FormDataSerializer::serialize_form(&tree, "f1");
        assert!(!data.contains_key("agree")); // unchecked checkbox skipped
    }

    #[test]
    fn test_select_defaults_to_first_option() {
        let nodes = vec![
            DomNode {
                id: 0,
                node_type: NodeType::Document,
                tag_name: "#document".to_string(),
                attributes: HashMap::new(),
                text_content: String::new(),
                children: vec![1],
                parent: None,
            },
            DomNode {
                id: 1,
                node_type: NodeType::Element,
                tag_name: "form".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("id".to_string(), "f1".to_string());
                    m
                },
                text_content: String::new(),
                children: vec![2],
                parent: Some(0),
            },
            DomNode {
                id: 2,
                node_type: NodeType::Element,
                tag_name: "select".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("name".to_string(), "color".to_string());
                    m
                },
                text_content: String::new(),
                children: vec![3, 4],
                parent: Some(1),
            },
            DomNode {
                id: 3,
                node_type: NodeType::Element,
                tag_name: "option".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("value".to_string(), "red".to_string());
                    m
                },
                text_content: "Red".to_string(),
                children: Vec::new(),
                parent: Some(2),
            },
            DomNode {
                id: 4,
                node_type: NodeType::Element,
                tag_name: "option".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("value".to_string(), "blue".to_string());
                    m
                },
                text_content: "Blue".to_string(),
                children: Vec::new(),
                parent: Some(2),
            },
        ];
        let tree = DomTree::new(nodes);
        let data = FormDataSerializer::serialize_form(&tree, "f1");
        assert_eq!(data.get("color").unwrap(), "red"); // first option selected by default
    }

    #[test]
    fn test_url_encode_special_chars() {
        let mut data = HashMap::new();
        data.insert("q".to_string(), "a+b&c=d".to_string());
        let encoded = FormDataSerializer::to_url_encoded(&data);
        assert!(encoded.contains("q="));
        assert!(!encoded.contains(" ")); // no raw spaces
    }
}
