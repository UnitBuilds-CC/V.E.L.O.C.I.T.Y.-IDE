//! Lexical scope chain for the JS evaluator.
//!
//! Each scope holds a local variable map and an optional reference to its
//! parent scope. Variable resolution walks up the chain until a binding is
//! found. Closures capture a reference to the scope in which they were
//! defined, enabling lexical scoping.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use velocity_ide::safety::SafeMutex;

use crate::js::vm::JsValue;

/// A single scope level in the chain.
#[derive(Debug, Clone)]
pub struct Scope {
    pub locals: HashMap<String, JsValue>,
    pub parent: Option<ScopeRef>,
    /// True for function-level and global scopes. `var` declarations hoist
    /// up to the nearest scope with this flag set.
    pub is_function_scope: bool,
    /// Variables declared with `const` — reassignment should throw.
    pub consts: std::collections::HashSet<String>,
    /// Resources declared with `using` — disposed (Symbol.dispose called) when
    /// the enclosing block scope exits. Stored in declaration order; disposed
    /// in reverse (LIFO) per spec.
    pub disposables: Vec<JsValue>,
}

/// Shared reference-counted scope pointer (allows closures to reference enclosing scopes).
pub type ScopeRef = Arc<Mutex<Scope>>;

impl Scope {
    /// Create a new root (global) scope.
    pub fn new_global() -> ScopeRef {
        Arc::new(Mutex::new(Scope {
            locals: HashMap::new(),
            parent: None,
            is_function_scope: true,
            consts: std::collections::HashSet::new(),
            disposables: Vec::new(),
        }))
    }

    /// Create a child scope with a parent link.
    pub fn new_child(parent: &ScopeRef) -> ScopeRef {
        Arc::new(Mutex::new(Scope {
            locals: HashMap::new(),
            parent: Some(Arc::clone(parent)),
            is_function_scope: false,
            consts: std::collections::HashSet::new(),
            disposables: Vec::new(),
        }))
    }

    /// Create a child scope that acts as a function boundary (var hoisting target).
    pub fn new_function_scope(parent: &ScopeRef) -> ScopeRef {
        Arc::new(Mutex::new(Scope {
            locals: HashMap::new(),
            parent: Some(Arc::clone(parent)),
            is_function_scope: true,
            consts: std::collections::HashSet::new(),
            disposables: Vec::new(),
        }))
    }

    /// Register a disposable resource in this scope.
    pub fn add_disposable(scope: &ScopeRef, resource: JsValue) {
        let mut s = scope.lock_safe();
        s.disposables.push(resource);
    }

    /// Drain all disposables from this scope (returns them in LIFO order for disposal).
    pub fn take_disposables(scope: &ScopeRef) -> Vec<JsValue> {
        let mut s = scope.lock_safe();
        let mut items = std::mem::take(&mut s.disposables);
        items.reverse(); // LIFO disposal per spec
        items
    }

    /// Resolve a variable by walking up the scope chain.
    pub fn resolve(scope: &ScopeRef, name: &str) -> Option<JsValue> {
        let s = scope.lock_safe();
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
        let mut s = scope.lock_safe();
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
        let mut s = scope.lock_safe();
        s.locals.insert(name.to_string(), value);
    }

    /// Declare a `var` variable — hoists to the nearest function/global scope.
    pub fn declare_var(scope: &ScopeRef, name: &str, value: JsValue) {
        let is_fn = { scope.lock_safe().is_function_scope };
        if is_fn {
            let mut s = scope.lock_safe();
            s.locals.insert(name.to_string(), value);
        } else {
            let parent = { scope.lock_safe().parent.clone() };
            match parent {
                Some(p) => Scope::declare_var(&p, name, value),
                None => {
                    let mut s = scope.lock_safe();
                    s.locals.insert(name.to_string(), value);
                }
            }
        }
    }

    /// Declare a `const` variable in the current scope and mark it immutable.
    pub fn declare_const(scope: &ScopeRef, name: &str, value: JsValue) {
        let mut s = scope.lock_safe();
        s.locals.insert(name.to_string(), value);
        s.consts.insert(name.to_string());
    }

    /// Check if a variable is a const anywhere in the chain.
    pub fn is_const(scope: &ScopeRef, name: &str) -> bool {
        let s = scope.lock_safe();
        if s.consts.contains(name) {
            return true;
        }
        if let Some(ref parent) = s.parent {
            return Scope::is_const(parent, name);
        }
        false
    }

    /// Agent-first: snapshot all visible bindings from this scope up the chain.
    /// Returns a flat map of name → value for every variable visible at this point.
    /// Inner scopes shadow outer ones (inner wins).
    pub fn snapshot(scope: &ScopeRef) -> HashMap<String, JsValue> {
        let mut result = HashMap::new();
        Scope::collect_bindings(scope, &mut result);
        result
    }

    fn collect_bindings(scope: &ScopeRef, out: &mut HashMap<String, JsValue>) {
        let s = scope.lock_safe();
        // Walk parent first so inner scopes can shadow
        if let Some(ref parent) = s.parent {
            Scope::collect_bindings(parent, out);
        }
        for (k, v) in &s.locals {
            out.insert(k.clone(), v.clone());
        }
    }

    /// Agent-first: list only the bindings declared directly in this scope level.
    pub fn local_keys(scope: &ScopeRef) -> Vec<String> {
        let s = scope.lock_safe();
        s.locals.keys().cloned().collect()
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
        assert_eq!(Scope::resolve(&global, "count"), Some(JsValue::Number(5.0)));
    }

    #[test]
    fn assign_returns_false_for_nonexistent() {
        let global = Scope::new_global();
        let child = Scope::new_child(&global);
        assert!(!Scope::assign(&child, "nope", JsValue::Number(1.0)));
    }

    #[test]
    fn declare_var_hoists_to_function_scope() {
        let global = Scope::new_global();
        let block = Scope::new_child(&global); // not a function scope
        Scope::declare_var(&block, "x", JsValue::Number(99.0));
        // x should be hoisted to global (the nearest function scope)
        assert_eq!(Scope::resolve(&global, "x"), Some(JsValue::Number(99.0)));
    }

    #[test]
    fn declare_var_stays_in_function_scope() {
        let global = Scope::new_global();
        let fn_scope = Scope::new_function_scope(&global);
        let block = Scope::new_child(&fn_scope);
        Scope::declare_var(&block, "y", JsValue::String("hi".into()));
        // y should be in fn_scope, not global
        assert_eq!(
            Scope::resolve(&fn_scope, "y"),
            Some(JsValue::String("hi".into()))
        );
        assert_eq!(Scope::resolve(&global, "y"), None);
    }

    #[test]
    fn declare_const_and_is_const() {
        let global = Scope::new_global();
        Scope::declare_const(&global, "PI", JsValue::Number(3.0));
        assert!(Scope::is_const(&global, "PI"));
        assert!(!Scope::is_const(&global, "not_const"));
    }

    #[test]
    fn is_const_propagates_to_child() {
        let global = Scope::new_global();
        Scope::declare_const(&global, "X", JsValue::Number(1.0));
        let child = Scope::new_child(&global);
        assert!(Scope::is_const(&child, "X"));
    }

    #[test]
    fn snapshot_collects_all_bindings() {
        let global = Scope::new_global();
        Scope::declare(&global, "a", JsValue::Number(1.0));
        let child = Scope::new_child(&global);
        Scope::declare(&child, "b", JsValue::Number(2.0));
        let snap = Scope::snapshot(&child);
        assert_eq!(snap.get("a"), Some(&JsValue::Number(1.0)));
        assert_eq!(snap.get("b"), Some(&JsValue::Number(2.0)));
    }

    #[test]
    fn snapshot_inner_shadows_outer() {
        let global = Scope::new_global();
        Scope::declare(&global, "x", JsValue::Number(1.0));
        let child = Scope::new_child(&global);
        Scope::declare(&child, "x", JsValue::Number(2.0));
        let snap = Scope::snapshot(&child);
        assert_eq!(snap.get("x"), Some(&JsValue::Number(2.0)));
    }

    #[test]
    fn local_keys_only_current_scope() {
        let global = Scope::new_global();
        Scope::declare(&global, "a", JsValue::Number(1.0));
        Scope::declare(&global, "b", JsValue::Number(2.0));
        let child = Scope::new_child(&global);
        Scope::declare(&child, "c", JsValue::Number(3.0));
        let keys = Scope::local_keys(&child);
        assert_eq!(keys.len(), 1);
        assert!(keys.contains(&"c".to_string()));
    }

    #[test]
    fn disposables_lifo_order() {
        let global = Scope::new_global();
        Scope::add_disposable(&global, JsValue::Number(1.0));
        Scope::add_disposable(&global, JsValue::Number(2.0));
        Scope::add_disposable(&global, JsValue::Number(3.0));
        let items = Scope::take_disposables(&global);
        // LIFO: last added comes first
        assert_eq!(items[0], JsValue::Number(3.0));
        assert_eq!(items[1], JsValue::Number(2.0));
        assert_eq!(items[2], JsValue::Number(1.0));
    }

    #[test]
    fn take_disposables_clears_list() {
        let global = Scope::new_global();
        Scope::add_disposable(&global, JsValue::Number(1.0));
        let _ = Scope::take_disposables(&global);
        let items2 = Scope::take_disposables(&global);
        assert!(items2.is_empty());
    }
}
