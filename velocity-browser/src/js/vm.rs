use crate::dom::DomTree;
use crate::js::scope::{Scope, ScopeRef};
use std::collections::HashMap;
use velocity_ide::safety::SafeMutex;

#[derive(Debug, Clone)]
pub enum JsValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Vec<JsValue>),
    Object(HashMap<String, JsValue>),
    Function {
        name: Option<String>,
        params: Vec<String>,
        body: crate::js::interpreter::Stmt,
        closure: ScopeRef,
    },
    NativeFunction(String),
    /// ES6 Proxy: intercepts property access on `target` using `handler` traps.
    Proxy {
        target: Box<JsValue>,
        handler: Box<JsValue>,
    },
}

impl PartialEq for JsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsValue::Undefined, JsValue::Undefined) => true,
            (JsValue::Null, JsValue::Null) => true,
            (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
            (JsValue::Number(a), JsValue::Number(b)) => a == b,
            (JsValue::String(a), JsValue::String(b)) => a == b,
            (JsValue::Array(a), JsValue::Array(b)) => a == b,
            (JsValue::Object(a), JsValue::Object(b)) => a == b,
            (JsValue::NativeFunction(a), JsValue::NativeFunction(b)) => a == b,
            (JsValue::Proxy { target: a, .. }, JsValue::Proxy { target: b, .. }) => a == b,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JsEventListener {
    pub target_selector: String,
    pub event_type: String,
    pub handler_script: String,
}

/// Function-based event listener (for addEventListener with closure handlers).
#[derive(Debug, Clone)]
pub struct JsFunctionListener {
    pub node_id: usize,
    pub event_type: String,
    pub handler: JsValue,
}

pub struct JsVirtualMachine {
    pub global_scope: HashMap<String, JsValue>,
    pub scope_ref: ScopeRef,
    pub listeners: Vec<JsEventListener>,
    pub fn_listeners: Vec<JsFunctionListener>,
}

impl Default for JsVirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl JsVirtualMachine {
    pub fn new() -> Self {
        let scope_ref = Scope::new_global();
        let mut global_scope = HashMap::new();
        global_scope.insert("window".to_string(), JsValue::Object(HashMap::new()));
        global_scope.insert("document".to_string(), JsValue::Object(HashMap::new()));

        // Register built-in globals in the scope chain
        {
            let mut s = scope_ref.lock_safe();
            s.locals.insert("undefined".to_string(), JsValue::Undefined);
            s.locals.insert("null".to_string(), JsValue::Null);
            s.locals.insert("NaN".to_string(), JsValue::Number(f64::NAN));
            s.locals.insert("Infinity".to_string(), JsValue::Number(f64::INFINITY));
            s.locals.insert("parseInt".to_string(), JsValue::NativeFunction("parseInt".into()));
            s.locals.insert("parseFloat".to_string(), JsValue::NativeFunction("parseFloat".into()));
            s.locals.insert("isNaN".to_string(), JsValue::NativeFunction("isNaN".into()));
            s.locals.insert("isFinite".to_string(), JsValue::NativeFunction("isFinite".into()));
            s.locals.insert("eval".to_string(), JsValue::NativeFunction("eval".into()));
            s.locals.insert("encodeURIComponent".to_string(), JsValue::NativeFunction("encodeURIComponent".into()));
            s.locals.insert("decodeURIComponent".to_string(), JsValue::NativeFunction("decodeURIComponent".into()));

            // Math object
            let mut math = HashMap::new();
            math.insert("PI".to_string(), JsValue::Number(std::f64::consts::PI));
            math.insert("E".to_string(), JsValue::Number(std::f64::consts::E));
            math.insert("floor".to_string(), JsValue::NativeFunction("Math.floor".into()));
            math.insert("ceil".to_string(), JsValue::NativeFunction("Math.ceil".into()));
            math.insert("round".to_string(), JsValue::NativeFunction("Math.round".into()));
            math.insert("abs".to_string(), JsValue::NativeFunction("Math.abs".into()));
            math.insert("max".to_string(), JsValue::NativeFunction("Math.max".into()));
            math.insert("min".to_string(), JsValue::NativeFunction("Math.min".into()));
            math.insert("random".to_string(), JsValue::NativeFunction("Math.random".into()));
            math.insert("sqrt".to_string(), JsValue::NativeFunction("Math.sqrt".into()));
            math.insert("pow".to_string(), JsValue::NativeFunction("Math.pow".into()));
            math.insert("log".to_string(), JsValue::NativeFunction("Math.log".into()));
            math.insert("trunc".to_string(), JsValue::NativeFunction("Math.trunc".into()));
            math.insert("sign".to_string(), JsValue::NativeFunction("Math.sign".into()));
            s.locals.insert("Math".to_string(), JsValue::Object(math));

            // JSON object
            let mut json_obj = HashMap::new();
            json_obj.insert("parse".to_string(), JsValue::NativeFunction("JSON.parse".into()));
            json_obj.insert("stringify".to_string(), JsValue::NativeFunction("JSON.stringify".into()));
            s.locals.insert("JSON".to_string(), JsValue::Object(json_obj));

            // Object constructor
            let mut obj_ctor = HashMap::new();
            obj_ctor.insert("keys".to_string(), JsValue::NativeFunction("Object.keys".into()));
            obj_ctor.insert("values".to_string(), JsValue::NativeFunction("Object.values".into()));
            obj_ctor.insert("assign".to_string(), JsValue::NativeFunction("Object.assign".into()));
            obj_ctor.insert("entries".to_string(), JsValue::NativeFunction("Object.entries".into()));
            obj_ctor.insert("freeze".to_string(), JsValue::NativeFunction("Object.freeze".into()));
            s.locals.insert("Object".to_string(), JsValue::Object(obj_ctor));

            // Array constructor
            let mut arr_ctor = HashMap::new();
            arr_ctor.insert("isArray".to_string(), JsValue::NativeFunction("Array.isArray".into()));
            arr_ctor.insert("from".to_string(), JsValue::NativeFunction("Array.from".into()));
            s.locals.insert("Array".to_string(), JsValue::Object(arr_ctor));

            // Number constructor
            let mut num_ctor = HashMap::new();
            num_ctor.insert("parseInt".to_string(), JsValue::NativeFunction("parseInt".into()));
            num_ctor.insert("parseFloat".to_string(), JsValue::NativeFunction("parseFloat".into()));
            num_ctor.insert("isNaN".to_string(), JsValue::NativeFunction("isNaN".into()));
            num_ctor.insert("isFinite".to_string(), JsValue::NativeFunction("isFinite".into()));
            num_ctor.insert("MAX_SAFE_INTEGER".to_string(), JsValue::Number(9007199254740991.0));
            s.locals.insert("Number".to_string(), JsValue::Object(num_ctor));

            // String constructor
            let mut str_ctor = HashMap::new();
            str_ctor.insert("fromCharCode".to_string(), JsValue::NativeFunction("String.fromCharCode".into()));
            s.locals.insert("String".to_string(), JsValue::Object(str_ctor));

            // Date
            let mut date_ctor = HashMap::new();
            date_ctor.insert("now".to_string(), JsValue::NativeFunction("Date.now".into()));
            s.locals.insert("Date".to_string(), JsValue::Object(date_ctor));

            // console
            let mut console = HashMap::new();
            console.insert("log".to_string(), JsValue::NativeFunction("console.log".into()));
            console.insert("warn".to_string(), JsValue::NativeFunction("console.warn".into()));
            console.insert("error".to_string(), JsValue::NativeFunction("console.error".into()));
            console.insert("info".to_string(), JsValue::NativeFunction("console.info".into()));
            s.locals.insert("console".to_string(), JsValue::Object(console));

            // window and document will be populated per-session
            s.locals.insert("window".to_string(), JsValue::Object(HashMap::new()));
            s.locals.insert("document".to_string(), JsValue::Object(HashMap::new()));

            // Symbol constructor + well-known symbols
            let mut symbol_obj = HashMap::new();
            symbol_obj.insert("iterator".to_string(), JsValue::String("__symbol_iterator__".into()));
            symbol_obj.insert("toPrimitive".to_string(), JsValue::String("__symbol_toPrimitive__".into()));
            symbol_obj.insert("hasInstance".to_string(), JsValue::String("__symbol_hasInstance__".into()));
            symbol_obj.insert("for".to_string(), JsValue::NativeFunction("Symbol.for".into()));
            s.locals.insert("Symbol".to_string(), JsValue::Object(symbol_obj));

            // globalThis = window
            s.locals.insert("globalThis".to_string(), JsValue::Object(HashMap::new()));
            s.locals.insert("self".to_string(), JsValue::Object(HashMap::new()));

            // structuredClone
            s.locals.insert("structuredClone".to_string(), JsValue::NativeFunction("structuredClone".into()));
            s.locals.insert("queueMicrotask".to_string(), JsValue::NativeFunction("queueMicrotask".into()));
            s.locals.insert("requestAnimationFrame".to_string(), JsValue::NativeFunction("requestAnimationFrame".into()));
            s.locals.insert("requestIdleCallback".to_string(), JsValue::NativeFunction("requestIdleCallback".into()));

            // navigator
            let mut navigator = HashMap::new();
            navigator.insert("userAgent".to_string(), JsValue::String("Mozilla/5.0 (compatible; VelocityBrowser/1.0; AgentFirst)".into()));
            navigator.insert("language".to_string(), JsValue::String("en-US".into()));
            navigator.insert("languages".to_string(), JsValue::Array(vec![JsValue::String("en-US".into()), JsValue::String("en".into())]));
            navigator.insert("platform".to_string(), JsValue::String("AgentOS".into()));
            navigator.insert("cookieEnabled".to_string(), JsValue::Boolean(true));
            navigator.insert("onLine".to_string(), JsValue::Boolean(true));
            s.locals.insert("navigator".to_string(), JsValue::Object(navigator));

            // Reflect (methods dispatched via call_native)
            let mut reflect = HashMap::new();
            reflect.insert("get".to_string(), JsValue::NativeFunction("Reflect.get".into()));
            reflect.insert("set".to_string(), JsValue::NativeFunction("Reflect.set".into()));
            reflect.insert("has".to_string(), JsValue::NativeFunction("Reflect.has".into()));
            reflect.insert("deleteProperty".to_string(), JsValue::NativeFunction("Reflect.deleteProperty".into()));
            reflect.insert("ownKeys".to_string(), JsValue::NativeFunction("Reflect.ownKeys".into()));
            reflect.insert("getOwnPropertyDescriptor".to_string(), JsValue::NativeFunction("Reflect.getOwnPropertyDescriptor".into()));
            reflect.insert("apply".to_string(), JsValue::NativeFunction("Reflect.apply".into()));
            reflect.insert("construct".to_string(), JsValue::NativeFunction("Reflect.construct".into()));
            s.locals.insert("Reflect".to_string(), JsValue::Object(reflect));

            // history
            let mut history = HashMap::new();
            history.insert("length".to_string(), JsValue::Number(1.0));
            history.insert("state".to_string(), JsValue::Null);
            history.insert("pushState".to_string(), JsValue::NativeFunction("__noop__".into()));
            history.insert("replaceState".to_string(), JsValue::NativeFunction("__noop__".into()));
            history.insert("back".to_string(), JsValue::NativeFunction("__noop__".into()));
            history.insert("forward".to_string(), JsValue::NativeFunction("__noop__".into()));
            history.insert("go".to_string(), JsValue::NativeFunction("__noop__".into()));
            s.locals.insert("history".to_string(), JsValue::Object(history));

            // location (basic)
            let mut location = HashMap::new();
            location.insert("href".to_string(), JsValue::String("about:blank".into()));
            location.insert("protocol".to_string(), JsValue::String("https:".into()));
            location.insert("hostname".to_string(), JsValue::String(String::new()));
            location.insert("pathname".to_string(), JsValue::String("/".into()));
            location.insert("search".to_string(), JsValue::String(String::new()));
            location.insert("hash".to_string(), JsValue::String(String::new()));
            location.insert("origin".to_string(), JsValue::String(String::new()));
            s.locals.insert("location".to_string(), JsValue::Object(location));

            // crypto
            let mut crypto = HashMap::new();
            crypto.insert("randomUUID".to_string(), JsValue::NativeFunction("__noop__".into()));
            crypto.insert("getRandomValues".to_string(), JsValue::NativeFunction("__noop__".into()));
            s.locals.insert("crypto".to_string(), JsValue::Object(crypto));

            // performance
            let mut perf = HashMap::new();
            perf.insert("now".to_string(), JsValue::NativeFunction("Date.now".into()));
            s.locals.insert("performance".to_string(), JsValue::Object(perf));
        }

        Self {
            global_scope,
            scope_ref,
            listeners: Vec::new(),
            fn_listeners: Vec::new(),
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
        // Script-based listeners
        for listener in self.listeners.clone() {
            if listener.target_selector == selector && listener.event_type == event {
                let _ = self.eval_statement(tree, &listener.handler_script)?;
                triggered += 1;
            }
        }
        // Function-based listeners: find node by selector, dispatch to matching
        let node_id = crate::js::dom_api::eval_dom(tree, &format!("document.querySelector('{}')", selector))
            .and_then(|r| r.ok())
            .and_then(|v| if let JsValue::Object(m) = v { m.get("__node_id__").and_then(|n| if let JsValue::Number(id) = n { Some(*id as usize) } else { None }) } else { None });
        if let Some(nid) = node_id {
            let matching: Vec<_> = self.fn_listeners.iter()
                .filter(|l| l.node_id == nid && l.event_type == event)
                .cloned().collect();
            for listener in matching {
                let mut event_obj = HashMap::new();
                event_obj.insert("type".to_string(), JsValue::String(event.to_string()));
                event_obj.insert("target".to_string(), JsValue::Object({
                    let mut t = HashMap::new();
                    t.insert("__node_id__".to_string(), JsValue::Number(nid as f64));
                    t
                }));
                event_obj.insert("preventDefault".to_string(), JsValue::NativeFunction("__noop__".into()));
                event_obj.insert("stopPropagation".to_string(), JsValue::NativeFunction("__noop__".into()));
                let _ = crate::js::interpreter::call_function(&listener.handler, &[JsValue::Object(event_obj)], &self.scope_ref);
                triggered += 1;
            }
        }
        Ok(format!("Dispatched {} event to '{}' (triggered {} listeners)", event, selector, triggered))
    }

    /// Register a function-based event listener (from JS addEventListener).
    pub fn add_fn_listener(&mut self, node_id: usize, event_type: &str, handler: JsValue) {
        self.fn_listeners.push(JsFunctionListener {
            node_id,
            event_type: event_type.to_string(),
            handler,
        });
    }

    /// Remove a function-based event listener.
    pub fn remove_fn_listener(&mut self, node_id: usize, event_type: &str) {
        self.fn_listeners.retain(|l| !(l.node_id == node_id && l.event_type == event_type));
    }

    /// Dispatch event by node_id directly (used by agent actions).
    pub fn dispatch_event_by_node(&mut self, node_id: usize, event: &str) -> Vec<JsValue> {
        let matching: Vec<_> = self.fn_listeners.iter()
            .filter(|l| l.node_id == node_id && l.event_type == event)
            .cloned().collect();
        let mut results = Vec::new();
        for listener in matching {
            let mut event_obj = HashMap::new();
            event_obj.insert("type".to_string(), JsValue::String(event.to_string()));
            event_obj.insert("target".to_string(), JsValue::Object({
                let mut t = HashMap::new();
                t.insert("__node_id__".to_string(), JsValue::Number(node_id as f64));
                t
            }));
            event_obj.insert("preventDefault".to_string(), JsValue::NativeFunction("__noop__".into()));
            event_obj.insert("stopPropagation".to_string(), JsValue::NativeFunction("__noop__".into()));
            if let Ok(v) = crate::js::interpreter::call_function(&listener.handler, &[JsValue::Object(event_obj)], &self.scope_ref) {
                results.push(v);
            }
        }
        results
    }

    /// Evaluate JS code: tries DOM API first, then falls back to the full
    /// interpreter. This is the primary entry point for session.eval_js().
    pub fn eval_statement(&mut self, tree: &mut DomTree, statement: &str) -> Result<JsValue, String> {
        let statement = statement.trim();
        if statement.is_empty() {
            return Ok(JsValue::Undefined);
        }

        // DOM access/mutation is modeled natively in Rust:
        if let Some(result) = crate::js::dom_api::eval_dom(tree, statement) {
            return result;
        }

        // Sync scope_ref with global_scope for backward compat
        {
            let mut s = self.scope_ref.lock_safe();
            for (k, v) in &self.global_scope {
                if !s.locals.contains_key(k) {
                    s.locals.insert(k.clone(), v.clone());
                }
            }
        }

        // Use the full interpreter with scope chain (graceful degradation)
        let result = match crate::js::interpreter::eval_script(statement, &self.scope_ref) {
            Ok(v) => v,
            Err(e) => {
                // Graceful: log the error but don't propagate parse/eval failures
                // This prevents a single broken script from crashing the entire page
                if e.contains("unexpected token") || e.contains("expected ") {
                    // Parse error - return undefined, don't crash
                    return Ok(JsValue::Undefined);
                }
                return Err(e);
            }
        };

        // Sync back any new variables to global_scope for backward compat
        {
            let s = self.scope_ref.lock_safe();
            for (k, v) in &s.locals {
                self.global_scope.insert(k.clone(), v.clone());
            }
        }

        Ok(result)
    }

    /// Evaluate a script with access to the global scope but no DOM context.
    pub fn eval_script(&mut self, script: &str) -> Result<JsValue, String> {
        if script.trim().is_empty() {
            return Ok(JsValue::Undefined);
        }
        crate::js::interpreter::eval_script(script, &self.scope_ref)
    }

    /// Get the scope reference for direct manipulation by web APIs.
    pub fn scope(&self) -> &ScopeRef {
        &self.scope_ref
    }
}

/// Split `x = expr` into ("x", "expr") when the left side is a simple
/// identifier and the `=` is a real assignment (not part of ==/!=/<=/>=).
#[allow(dead_code)]
fn split_simple_assignment(stmt: &str) -> Option<(&str, &str)> {
    let bytes = stmt.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { 0 };
            let next = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
            if prev != b'!' && prev != b'<' && prev != b'>' && prev != b'=' && next != b'=' {
                let lhs = stmt[..i].trim();
                if is_identifier(lhs) {
                    return Some((lhs, &stmt[i + 1..]));
                }
                return None;
            }
        }
    }
    None
}

#[allow(dead_code)]
fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::HtmlParser;

    fn empty_tree() -> DomTree {
        DomTree::new(HtmlParser::parse_html5("<div></div>"))
    }

    #[test]
    fn evaluates_multiple_statements_returns_last() {
        let mut vm = JsVirtualMachine::new();
        let mut tree = empty_tree();
        let out = vm.eval_statement(&mut tree, "var a = 2; var b = 3; a + b").unwrap();
        assert_eq!(out, JsValue::Number(5.0));
    }

    #[test]
    fn bare_assignment_updates_scope() {
        let mut vm = JsVirtualMachine::new();
        let mut tree = empty_tree();
        vm.eval_statement(&mut tree, "let x = 10; x = x + 5").unwrap();
        assert_eq!(vm.global_scope.get("x"), Some(&JsValue::Number(15.0)));
    }

    #[test]
    fn comparison_is_not_treated_as_assignment() {
        let mut vm = JsVirtualMachine::new();
        let mut tree = empty_tree();
        let out = vm.eval_statement(&mut tree, "var y = 4; y == 4").unwrap();
        assert_eq!(out, JsValue::Boolean(true));
    }

    #[test]
    fn dom_mutation_flows_through_statement() {
        let mut vm = JsVirtualMachine::new();
        let mut tree = DomTree::new(HtmlParser::parse_html5("<input id=\"f\">"));
        vm.eval_statement(&mut tree, "document.getElementById('f').setAttribute('value','hi')").unwrap();
        let node = tree.nodes.iter().find(|n| n.attributes.get("id").map(|s| s.as_str()) == Some("f")).unwrap();
        assert_eq!(node.attributes.get("value").map(|s| s.as_str()), Some("hi"));
    }

    #[test]
    fn math_builtin_works() {
        let mut vm = JsVirtualMachine::new();
        let mut tree = empty_tree();
        let out = vm.eval_statement(&mut tree, "Math.floor(3.7)").unwrap();
        assert_eq!(out, JsValue::Number(3.0));
    }

    #[test]
    fn function_and_closure() {
        let mut vm = JsVirtualMachine::new();
        let mut tree = empty_tree();
        let out = vm.eval_statement(&mut tree, "function greet(name) { return 'Hello ' + name; } greet('World')").unwrap();
        assert_eq!(out, JsValue::String("Hello World".into()));
    }

    #[test]
    fn if_else_control_flow() {
        let mut vm = JsVirtualMachine::new();
        let mut tree = empty_tree();
        let out = vm.eval_statement(&mut tree, "var x = 10; if (x > 5) { x = x * 2; } x").unwrap();
        assert_eq!(out, JsValue::Number(20.0));
    }

    #[test]
    fn for_loop_with_accumulator() {
        let mut vm = JsVirtualMachine::new();
        let mut tree = empty_tree();
        let out = vm.eval_statement(&mut tree, "var sum = 0; for (var i = 1; i <= 10; i = i + 1) { sum = sum + i; } sum").unwrap();
        assert_eq!(out, JsValue::Number(55.0));
    }
}
