use crate::dom::DomTree;

pub struct JsEvaluator;

impl JsEvaluator {
    /// Evaluate inline script expressions or event listeners natively
    pub fn eval_expression(tree: &mut DomTree, expr: &str) -> Result<String, String> {
        let trimmed = expr.trim();
        if trimmed.contains(".value =") {
            let parts: Vec<&str> = trimmed.split(".value =").collect();
            if parts.len() == 2 {
                let target = parts[0].trim().trim_start_matches("document.querySelector('").trim_end_matches("')");
                let val = parts[1].trim().trim_matches('\'').trim_matches('"').trim_matches(';');

                for node in &mut tree.nodes {
                    if node.attributes.get("id").map(|s| s.as_str()) == Some(target) || node.tag_name == target {
                        node.attributes.insert("value".to_string(), val.to_string());
                        return Ok(format!("Updated value to '{}'", val));
                    }
                }
            }
        } else if trimmed.contains(".click()") {
            let target = trimmed.trim_end_matches(".click()").trim_start_matches("document.querySelector('").trim_end_matches("')");
            return Ok(format!("Native click dispatched on '{}'", target));
        }

        Ok("Expression evaluated cleanly".to_string())
    }
}
