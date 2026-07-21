use crate::dom::DomTree;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Vec<JsValue>),
    Object(HashMap<String, JsValue>),
}

#[derive(Debug, Clone)]
pub struct JsEventListener {
    pub target_selector: String,
    pub event_type: String,
    pub handler_script: String,
}

pub struct JsVirtualMachine {
    pub global_scope: HashMap<String, JsValue>,
    pub listeners: Vec<JsEventListener>,
}

impl JsVirtualMachine {
    pub fn new() -> Self {
        let mut global_scope = HashMap::new();
        global_scope.insert("window".to_string(), JsValue::Object(HashMap::new()));
        global_scope.insert("document".to_string(), JsValue::Object(HashMap::new()));
        Self {
            global_scope,
            listeners: Vec::new(),
        }
    }

    pub fn add_event_listener(&mut self, selector: &str, event: &str, script: &str) {
        self.listeners.push(JsEventListener {
            target_selector: selector.to_string(),
            event_type: event.to_string(),
            handler_script: script.to_string(),
        });
    }

    pub fn dispatch_event(&mut self, tree: &mut DomTree, selector: &str, event: &str) -> Result<String, String> {
        let mut triggered = 0;
        for listener in self.listeners.clone() {
            if listener.target_selector == selector && listener.event_type == event {
                let _ = self.eval_statement(tree, &listener.handler_script)?;
                triggered += 1;
            }
        }
        Ok(format!("Dispatched {} event to '{}' (triggered {} listeners)", event, selector, triggered))
    }

    pub fn eval_statement(&mut self, tree: &mut DomTree, statement: &str) -> Result<JsValue, String> {
        let stmt = statement.trim();

        // Variable assignment: var x = 123; or let y = 'hello';
        if stmt.starts_with("var ") || stmt.starts_with("let ") || stmt.starts_with("const ") {
            let body = stmt.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
            if let Some((var_name, expr)) = body.split_once('=') {
                let val = self.eval_expression(tree, expr.trim())?;
                self.global_scope.insert(var_name.trim().to_string(), val.clone());
                return Ok(val);
            }
        }

        self.eval_expression(tree, stmt)
    }

    fn eval_expression(&mut self, tree: &mut DomTree, expr: &str) -> Result<JsValue, String> {
        let trimmed = expr.trim().trim_end_matches(';');

        if trimmed == "undefined" {
            return Ok(JsValue::Undefined);
        } else if trimmed == "null" {
            return Ok(JsValue::Null);
        } else if trimmed == "true" {
            return Ok(JsValue::Boolean(true));
        } else if trimmed == "false" {
            return Ok(JsValue::Boolean(false));
        } else if let Ok(num) = trimmed.parse::<f64>() {
            return Ok(JsValue::Number(num));
        } else if (trimmed.starts_with('"') && trimmed.ends_with('"')) || (trimmed.starts_with('\'') && trimmed.ends_with('\'')) {
            return Ok(JsValue::String(trimmed[1..trimmed.len() - 1].to_string()));
        }

        // DOM mutation expression
        if trimmed.contains(".setAttribute(") {
            if let Some(start) = trimmed.find(".setAttribute(") {
                let target = trimmed[..start].trim().trim_start_matches("document.querySelector('").trim_end_matches("')");
                let args = &trimmed[start + 14..trimmed.len() - 1];
                if let Some((attr, val)) = args.split_once(',') {
                    let clean_attr = attr.trim().trim_matches('"').trim_matches('\'');
                    let clean_val = val.trim().trim_matches('"').trim_matches('\'');

                    for node in &mut tree.nodes {
                        if node.attributes.get("id").map(|s| s.as_str()) == Some(target) || node.tag_name == target {
                            node.attributes.insert(clean_attr.to_string(), clean_val.to_string());
                            return Ok(JsValue::String(format!("Set attribute '{}'='{}'", clean_attr, clean_val)));
                        }
                    }
                }
            }
        }

        if let Some(val) = self.global_scope.get(trimmed) {
            return Ok(val.clone());
        }

        Ok(JsValue::String(trimmed.to_string()))
    }
}
