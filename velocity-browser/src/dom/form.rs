use crate::dom::DomTree;
use crate::parser::html::NodeType;
use std::collections::HashMap;

pub struct FormDataSerializer;

impl FormDataSerializer {
    pub fn serialize_form(tree: &DomTree, form_id_or_selector: &str) -> HashMap<String, String> {
        let mut form_data = HashMap::new();

        for node in &tree.nodes {
            if node.node_type != NodeType::Element {
                continue;
            }

            if node.tag_name == "input" || node.tag_name == "textarea" || node.tag_name == "select" {
                if let Some(name) = node.attributes.get("name") {
                    let val = node.attributes.get("value").cloned().unwrap_or_default();
                    form_data.insert(name.clone(), val);
                }
            }
        }

        form_data
    }

    pub fn to_url_encoded(data: &HashMap<String, String>) -> String {
        data.iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&")
    }

    pub fn to_multipart(data: &HashMap<String, String>, boundary: &str) -> String {
        let mut body = String::new();
        for (k, v) in data {
            body.push_str(&format!("--{}\r\n", boundary));
            body.push_str(&format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", k));
            body.push_str(&format!("{}\r\n", v));
        }
        body.push_str(&format!("--{}--\r\n", boundary));
        body
    }
}
