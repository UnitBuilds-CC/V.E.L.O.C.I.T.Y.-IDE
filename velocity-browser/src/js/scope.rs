//! Lexical scope chain for the JS evaluator.
//!
//! Each scope holds a local variable map and an optional reference to its
//! parent scope. Variable resolution walks up the chain until a binding is
//! found. Closures capture a reference to the scope in which they were
//! defined, enabling lexical scoping.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::js::vm::JsValue;

/// A single scope level in the chain.
#[derive(Debug, Clone)]
pub struct Scope {
    pub locals: HashMap<String, JsValue>,
    pub parent: Option<ScopeRef>,
}

/// Shared reference-counted scope pointer (allows closures to reference enclosing scopes).
pub type ScopeRef = Arc<Mutex<Scope>>;

impl Scope {
    /// Create a new root (global) scope.
    pub fn new_global() -> ScopeRef {
        Arc::new(Mutex::new(Scope {
            locals: HashMap::new(),
            parent: None,
        }))
    }

    /// Create a child scope with a parent link.
    pub fn new_child(parent: &ScopeRef) -> ScopeRef {
        Arc::new(Mutex::new(Scope {
            locals: HashMap::new(),
            parent: Some(Arc::clone(parent)),
        }))
    }

    /// Resolve a variable by walking up the scope chain.
    pub fn resolve(scope: &ScopeRef, name: &str) -> Option<JsValue> {
        let s = scope.lock().unwrap();
        if let Some(val) = s.locals.get(name) {
            return Some(val.clone());
        }
        if let Some(ref parent) = s.parent {
            return Scope::resolve(parent, name);
        }
        None
    }

    /// Assign to an existing variable anywhere in the chain. Returns true if found.
    pub fn assign(scope: &ScopeRef, name: &str, value: JsValue) -> bool {
        let mut s = scope.lock().unwrap();
        if s.locals.contains_key(name) {
            s.locals.insert(name.to_string(), value);
            return true;
        }
        if let Some(ref parent) = s.parent {
            let parent = Arc::clone(parent);
            drop(s);
            return Scope::assign(&parent, name, value);
        }
        false
    }

    /// Declare a new variable in the current (innermost) scope.
    pub fn declare(scope: &ScopeRef, name: &str, value: JsValue) {
        let mut s = scope.lock().unwrap();
        s.locals.insert(name.to_string(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_scope_declare_and_resolve() {
        let global = Scope::new_global();
        Scope::declare(&global, "x", JsValue::Number(42.0));
        assert_eq!(Scope::resolve(&global, "x"), Some(JsValue::Number(42.0)));
        assert_eq!(Scope::resolve(&global, "y"), None);
    }

    #[test]
    fn child_scope_shadows_parent() {
        let global = Scope::new_global();
        Scope::declare(&global, "x", JsValue::Number(1.0));
        let child = Scope::new_child(&global);
        Scope::declare(&child, "x", JsValue::Number(2.0));
        assert_eq!(Scope::resolve(&child, "x"), Some(JsValue::Number(2.0)));
        assert_eq!(Scope::resolve(&global, "x"), Some(JsValue::Number(1.0)));
    }

    #[test]
    fn child_resolves_from_parent() {
        let global = Scope::new_global();
        Scope::declare(&global, "y", JsValue::String("hello".into()));
        let child = Scope::new_child(&global);
        assert_eq!(
            Scope::resolve(&child, "y"),
            Some(JsValue::String("hello".into()))
        );
    }

    #[test]
    fn assign_updates_parent_binding() {
        let global = Scope::new_global();
        Scope::declare(&global, "count", JsValue::Number(0.0));
        let child = Scope::new_child(&global);
        Scope::assign(&child, "count", JsValue::Number(5.0));
        assert_eq!(
            Scope::resolve(&global, "count"),
            Some(JsValue::Number(5.0))
        );
    }
}
