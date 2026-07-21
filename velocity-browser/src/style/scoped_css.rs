use crate::dom::DomTree;
use crate::parser::html::DomNode;

pub struct ScopedCssMatcher;

impl ScopedCssMatcher {
    pub fn matches_host_selector(node: &DomNode, selector: &str) -> bool {
        if selector == ":host" {
            return node.attributes.contains_key("shadowroot");
        }
        if selector.starts_with(":host(") {
            let inner = selector.trim_start_matches(":host(").trim_end_matches(')');
            if let Some(id) = node.attributes.get("id") {
                if format!("#{}", id) == inner {
                    return true;
                }
            }
        }
        false
    }
}
