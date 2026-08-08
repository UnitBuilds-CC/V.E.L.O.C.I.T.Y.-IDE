use super::ast::*;
use super::lexer::lex;
use super::parser::Parser;
use super::eval::{eval_stmt, eval_expr_node};
use crate::js::scope::{Scope, ScopeRef};
use crate::js::vm::JsValue;
use std::collections::HashMap;
use std::sync::Mutex;

/// Global module registry for ES module imports.
/// Maps module specifier -> exported bindings (name -> value).
static MODULE_REGISTRY: Mutex<Option<HashMap<String, HashMap<String, JsValue>>>> = Mutex::new(None);

// Resolver callback: given a module specifier, returns the module source code.
type ModuleResolverFn = dyn Fn(&str) -> Option<String> + Send + Sync;
static MODULE_RESOLVER: Mutex<Option<Box<ModuleResolverFn>>> = Mutex::new(None);

/// Serialization lock for tests that mutate the global module resolver /
/// registry. These statics are process-wide, so tests touching them must hold
/// this lock to avoid racing.
#[cfg(test)]
pub static MODULE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Set a module resolver callback.
pub fn set_module_resolver(resolver: impl Fn(&str) -> Option<String> + Send + Sync + 'static) {
    *MODULE_RESOLVER.lock().unwrap() = Some(Box::new(resolver));
}

/// Clear the module resolver.
pub fn clear_module_resolver() {
    *MODULE_RESOLVER.lock().unwrap() = None;
}

/// Register a module's exports in the global registry.
pub fn register_module(specifier: &str, exports: HashMap<String, JsValue>) {
    let mut registry = MODULE_REGISTRY.lock().unwrap();
    let map = registry.get_or_insert_with(HashMap::new);
    map.insert(specifier.to_string(), exports);
}

/// Clear all registered modules.
pub fn clear_module_registry() {
    *MODULE_REGISTRY.lock().unwrap() = None;
}

/// Resolve a module import. Returns the module's exports or None if not registered.
pub fn resolve_module(specifier: &str) -> Option<HashMap<String, JsValue>> {
    let registry = MODULE_REGISTRY.lock().unwrap();
    registry.as_ref().and_then(|map| map.get(specifier).cloned())
}

/// Evaluate a module source and register its exports.
pub fn evaluate_module(specifier: &str, source: &str) -> Result<HashMap<String, JsValue>, String> {
    let tokens = lex(source)?;
    let mut parser = Parser::new(tokens);
    let stmts = parser.parse_program()?;
    let scope = Scope::new_global();
    let mut exports = HashMap::new();

    for stmt in &stmts {
        match stmt {
            Stmt::Export { declaration, default_expr, named } => {
                if let Some(decl) = declaration {
                    let _ = eval_stmt(decl, &scope);
                    match decl.as_ref() {
                        Stmt::VarDecl { name, .. } => {
                            if let Some(val) = Scope::resolve(&scope, name) {
                                exports.insert(name.clone(), val);
                            }
                        }
                        Stmt::FunctionDecl { name, .. } | Stmt::AsyncFunctionDecl { name, .. } => {
                            if let Some(val) = Scope::resolve(&scope, name) {
                                exports.insert(name.clone(), val);
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(expr) = default_expr {
                    if let Ok(val) = eval_expr_node(expr, &scope) {
                        exports.insert("default".to_string(), val);
                    }
                }
                for name in named {
                    if let Some(val) = Scope::resolve(&scope, name) {
                        exports.insert(name.clone(), val);
                    }
                }
            }
            _ => { let _ = eval_stmt(stmt, &scope); }
        }
    }

    register_module(specifier, exports.clone());
    Ok(exports)
}

/// Apply an import statement: resolve the module and bind specifiers into scope.
pub fn apply_import(
    specifiers: &[ImportSpecifier],
    source: &str,
    scope: &ScopeRef,
) -> Result<(), String> {
    let module_exports = match resolve_module(source) {
        Some(exports) => exports,
        None => {
            let fetched = MODULE_RESOLVER.lock().unwrap()
                .as_ref()
                .and_then(|resolver| resolver(source));
            match fetched {
                Some(src) => evaluate_module(source, &src)?,
                None => return Ok(()),
            }
        }
    };
    for spec in specifiers {
        let value = if spec.imported == "*" {
            JsValue::Object(module_exports.clone())
        } else {
            module_exports.get(&spec.imported).cloned().unwrap_or(JsValue::Undefined)
        };
        Scope::declare(scope, &spec.local, value);
    }
    Ok(())
}

