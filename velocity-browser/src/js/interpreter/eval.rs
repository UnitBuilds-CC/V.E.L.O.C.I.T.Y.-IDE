use super::token::*;
use super::ast::*;
use super::signal::*;
use super::coercion::*;
use super::property::*;
use super::function::*;
use super::native::call_native;
use super::method_dispatch::call_method;
use super::collections::*;
use super::core_methods::*;
use super::web_apis::*;
use super::intl::*;
use super::module::apply_import;
use super::eval_script::eval_script;
use super::constructors::eval_new;
use crate::js::scope::{Scope, ScopeRef};
use crate::js::vm::JsValue;
use std::collections::HashMap;

pub(super) const MAX_PROXY_TRAP_DEPTH: u32 = 8;
thread_local! { pub(super) static PROXY_TRAP_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }
thread_local! { pub(super) static PROMISE_CAPTURE: std::cell::RefCell<Option<(bool, JsValue)>> = const { std::cell::RefCell::new(None) }; }

/// Evaluate a full program (list of statements) in the given scope.
pub fn eval_program(stmts: &[Stmt], scope: &ScopeRef) -> EvalResult {
    let mut last = JsValue::Undefined;
    for stmt in stmts {
        last = eval_stmt(stmt, scope)?;
    }
    Ok(last)
}

pub fn eval_stmt(stmt: &Stmt, scope: &ScopeRef) -> EvalResult {
    match stmt {
        Stmt::Expr(e) => eval_expr_node(e, scope),
        Stmt::VarDecl { kind, name, init } => {
            let val = match init { Some(e) => eval_expr_node(e, scope)?, None => JsValue::Undefined };
            match kind {
                VarKind::Var => Scope::declare_var(scope, name, val),
                VarKind::Let => Scope::declare(scope, name, val),
                VarKind::Const => Scope::declare_const(scope, name, val),
                VarKind::Using => {
                    Scope::declare(scope, name, val.clone());
                    Scope::add_disposable(scope, val);
                }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::DestructureDecl { pattern, init } => {
            let val = eval_expr_node(init, scope)?;
            match pattern {
                DestructurePattern::Object(props) => {
                    if let JsValue::Object(map) = &val {
                        for (key, alias) in props {
                            let var_name = alias.as_ref().unwrap_or(key);
                            let v = map.get(key).cloned().unwrap_or(JsValue::Undefined);
                            Scope::declare(scope, var_name, v);
                        }
                    }
                }
                DestructurePattern::Array(items) => {
                    if let JsValue::Array(arr) = &val {
                        for (i, item) in items.iter().enumerate() {
                            if let Some(name) = item {
                                let v = arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                                Scope::declare(scope, name, v);
                            }
                        }
                    }
                }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::Block(stmts) => {
            let child = Scope::new_child(scope);
            eval_program(stmts, &child)
        }
        Stmt::If { cond, then_branch, else_branch } => {
            if to_boolean(&eval_expr_node(cond, scope)?) { eval_stmt(then_branch, scope) }
            else if let Some(eb) = else_branch { eval_stmt(eb, scope) }
            else { Ok(JsValue::Undefined) }
        }
        Stmt::While { cond, body } => {
            let mut iterations = 0;
            while to_boolean(&eval_expr_node(cond, scope)?) {
                iterations += 1;
                if iterations > 100_000 { break; }
                match eval_stmt(body, scope) {
                    Ok(_) => {}
                    Err(Signal::Break) => break,
                    Err(Signal::Continue) => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::DoWhile { body, cond } => {
            let mut iterations = 0;
            loop {
                iterations += 1;
                if iterations > 100_000 { break; }
                match eval_stmt(body, scope) {
                    Ok(_) => {}
                    Err(Signal::Break) => break,
                    Err(Signal::Continue) => {}
                    Err(e) => return Err(e),
                }
                if !to_boolean(&eval_expr_node(cond, scope)?) { break; }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::For { init, cond, update, body } => {
            let for_scope = Scope::new_child(scope);
            if let Some(i) = init { eval_stmt(i, &for_scope)?; }
            let mut iterations = 0;
            loop {
                iterations += 1;
                if iterations > 100_000 { break; }
                if let Some(c) = cond { if !to_boolean(&eval_expr_node(c, &for_scope)?) { break; } }
                match eval_stmt(body, &for_scope) {
                    Ok(_) => {}
                    Err(Signal::Break) => break,
                    Err(Signal::Continue) => {}
                    Err(e) => return Err(e),
                }
                if let Some(u) = update { eval_expr_node(u, &for_scope)?; }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::ForIn { var_name, object, body } => {
            let obj = eval_expr_node(object, scope)?;
            match &obj {
                JsValue::Array(arr) => {
                    // for-of: iterate over values
                    for item in arr.iter() {
                        Scope::declare(scope, var_name, item.clone());
                        match eval_stmt(body, scope) {
                            Ok(_) => {}
                            Err(Signal::Break) => break,
                            Err(Signal::Continue) => continue,
                            Err(e) => return Err(e),
                        }
                    }
                }
                JsValue::Object(map) => {
                    if map.get("__type__").map(to_string).as_deref() == Some("Generator") {
                        // Generator iterator: iterate over __values__
                        if let Some(JsValue::Array(values)) = map.get("__values__") {
                            for item in values.iter() {
                                Scope::declare(scope, var_name, item.clone());
                                match eval_stmt(body, scope) {
                                    Ok(_) => {}
                                    Err(Signal::Break) => break,
                                    Err(Signal::Continue) => continue,
                                    Err(e) => return Err(e),
                                }
                            }
                        }
                    } else {
                        // for-in: iterate over enumerable own keys (internal `__x__`
                        // bookkeeping keys and non-enumerable accessors are hidden).
                        for key in enumerable_keys(map) {
                            Scope::declare(scope, var_name, JsValue::String(key));
                            match eval_stmt(body, scope) {
                                Ok(_) => {}
                                Err(Signal::Break) => break,
                                Err(Signal::Continue) => continue,
                                Err(e) => return Err(e),
                            }
                        }
                    }
                }
                _ => {}
            }
            Ok(JsValue::Undefined)
        }
        Stmt::ForOf { var_name, object, body } => {
            let iterable = eval_expr_node(object, scope)?;
            for item in iterate_values(&iterable, scope) {
                Scope::declare(scope, var_name, item);
                match eval_stmt(body, scope) {
                    Ok(_) => {}
                    Err(Signal::Break) => break,
                    Err(Signal::Continue) => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::ForAwaitOf { var_name, object, body } => {
            let iterable = eval_expr_node(object, scope)?;
            for item in iterate_values(&iterable, scope) {
                // Await each value: unwrap settled promises, re-throw rejections.
                let val = await_value(item)?;
                Scope::declare(scope, var_name, val);
                match eval_stmt(body, scope) {
                    Ok(_) => {}
                    Err(Signal::Break) => break,
                    Err(Signal::Continue) => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::Return(e) => {
            let val = match e { Some(ex) => eval_expr_node(ex, scope)?, None => JsValue::Undefined };
            Err(Signal::Return(val))
        }
        Stmt::Break => Err(Signal::Break),
        Stmt::Continue => Err(Signal::Continue),
        Stmt::Throw(e) => Err(Signal::Throw(eval_expr_node(e, scope)?)),
        Stmt::TryCatch { try_block, catch_var, catch_block, finally_block } => {
            let result = eval_stmt(try_block, scope);
            let outcome: EvalResult = match result {
                Err(Signal::Throw(thrown)) => {
                    if let Some(cb) = catch_block {
                        let catch_scope = Scope::new_child(scope);
                        if let Some(var) = catch_var { Scope::declare(&catch_scope, var, thrown); }
                        eval_stmt(cb, &catch_scope)
                    } else {
                        Err(Signal::Throw(thrown))
                    }
                }
                other => other,
            };
            if let Some(fb) = finally_block { let _ = eval_stmt(fb, scope); }
            outcome
        }
        Stmt::FunctionDecl { name, params, body } => {
            let func = JsValue::Function {
                name: Some(name.clone()),
                params: params.clone(),
                body: (**body).clone(),
                closure: scope.clone(),
            };
            Scope::declare(scope, name, func);
            Ok(JsValue::Undefined)
        }
        Stmt::AsyncFunctionDecl { name, params, body } => {
            let func = JsValue::Function {
                name: Some(name.clone()),
                params: params.clone(),
                body: (**body).clone(),
                closure: scope.clone(),
            };
            let mut wrapper_map = HashMap::new();
            wrapper_map.insert("__type__".to_string(), JsValue::String("AsyncFunction".to_string()));
            wrapper_map.insert("__inner__".to_string(), func);
            let async_fn = JsValue::Object(wrapper_map);
            Scope::declare(scope, name, async_fn);
            Ok(JsValue::Undefined)
        }
        Stmt::ClassDecl { name, parent, methods, fields } => {
            eval_class_decl(name, parent, methods, fields, scope);
            Ok(JsValue::Undefined)
        }
        Stmt::Import { specifiers, source } => {
            if let Ok(()) = apply_import(specifiers, source, scope) {
                // Successfully imported from registry
            } else {
                for spec in specifiers {
                    if Scope::resolve(scope, &spec.local).is_none() {
                        Scope::declare(scope, &spec.local, JsValue::Undefined);
                    }
                }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::Export { declaration, default_expr, .. } => {
            if let Some(decl) = declaration {
                eval_stmt(decl, scope)?;
            }
            if let Some(expr) = default_expr {
                let val = eval_expr_node(expr, scope)?;
                Scope::declare(scope, "__default_export__", val);
            }
            Ok(JsValue::Undefined)
        }
        Stmt::GeneratorDecl { name, params, body } => {
            let func = JsValue::Function {
                name: Some(format!("__generator__{}", name)),
                params: params.clone(),
                body: (**body).clone(),
                closure: scope.clone(),
            };
            Scope::declare(scope, name, func);
            Ok(JsValue::Undefined)
        }
        Stmt::Labeled { body, .. } => {
            eval_stmt(body, scope)
        }
        Stmt::Switch { discriminant, cases } => {
            let disc_val = eval_expr_node(discriminant, scope)?;
            let mut matched = false;
            let mut has_default = false;
            let mut default_index = 0;
            for (i, case) in cases.iter().enumerate() {
                if case.pattern.is_none() {
                    has_default = true;
                    default_index = i;
                }
            }
            'outer: for case in cases.iter() {
                if let Some(pattern) = &case.pattern {
                    let pat_val = eval_expr_node(pattern, scope)?;
                    if strict_eq(&disc_val, &pat_val) {
                        matched = true;
                    }
                }
                if matched || (case.pattern.is_none()) {
                    let switch_scope = Scope::new_child(scope);
                    for stmt in &case.body {
                        match eval_stmt(stmt, &switch_scope) {
                            Err(Signal::Break) => break 'outer,
                            Err(e) => return Err(e),
                            _ => {}
                        }
                    }
                    if case.pattern.is_none() && matched {
                        break;
                    }
                }
            }
            if !matched && has_default {
                let switch_scope = Scope::new_child(scope);
                for stmt in &cases[default_index].body {
                    match eval_stmt(stmt, &switch_scope) {
                        Err(Signal::Break) => break,
                        Err(e) => return Err(e),
                        _ => {}
                    }
                }
            }
            Ok(JsValue::Undefined)
        }
    }
}

pub fn eval_expr_node(expr: &Expr, scope: &ScopeRef) -> EvalResult {
    match expr {
        Expr::Number(n) => Ok(JsValue::Number(*n)),
        Expr::Str(s) => Ok(JsValue::String(s.clone())),
        Expr::Template(s) => eval_template_literal(s, scope),
        Expr::Bool(b) => Ok(JsValue::Boolean(*b)),
        Expr::Null => Ok(JsValue::Null),
        Expr::Undefined => Ok(JsValue::Undefined),
        Expr::This => Ok(Scope::resolve(scope, "this").unwrap_or(JsValue::Undefined)),
        Expr::Super => Ok(Scope::resolve(scope, "__super__").unwrap_or(JsValue::Undefined)),
        Expr::Ident(name) => Ok(match Scope::resolve(scope, name) {
            Some(v) => v,
            None => match name.as_str() {
                "Infinity" => JsValue::Number(f64::INFINITY),
                "NaN" => JsValue::Number(f64::NAN),
                // globalThis: pragmatic view of the global scope as an object.
                "globalThis" => JsValue::Object(HashMap::new()),
                // Browser environment globals.
                "window" => super::browser_env::make_window(),
                "navigator" => super::browser_env::make_navigator(),
                "location" => super::browser_env::make_location("https://localhost/"),
                "document" => super::browser_env::make_document(),
                "localStorage" => super::browser_env::make_local_storage(),
                "sessionStorage" => super::browser_env::make_session_storage(),
                "performance" => super::web_platform::make_performance(),
                "history" => super::web_platform::make_history(),
                "customElements" => {
                    let mut m = HashMap::new();
                    m.insert("__type__".to_string(), JsValue::String("CustomElementRegistry".to_string()));
                    JsValue::Object(m)
                }
                "caches" => {
                    let mut m = HashMap::new();
                    m.insert("__type__".to_string(), JsValue::String("CacheStorage".to_string()));
                    JsValue::Object(m)
                }
                "indexedDB" => super::web_platform::make_indexed_db(),
                "speechSynthesis" => {
                    let mut m = HashMap::new();
                    m.insert("__type__".to_string(), JsValue::String("SpeechSynthesis".to_string()));
                    m.insert("paused".to_string(), JsValue::Boolean(false));
                    m.insert("pending".to_string(), JsValue::Boolean(false));
                    m.insert("speaking".to_string(), JsValue::Boolean(false));
                    JsValue::Object(m)
                }
                "Notification" => {
                    let mut m = HashMap::new();
                    m.insert("__type__".to_string(), JsValue::String("Notification".to_string()));
                    m.insert("permission".to_string(), JsValue::String("default".to_string()));
                    JsValue::Object(m)
                }
                "CSS" => {
                    let mut m = HashMap::new();
                    m.insert("__type__".to_string(), JsValue::String("CSS".to_string()));
                    JsValue::Object(m)
                }
                "navigation" => {
                    let mut m = HashMap::new();
                    m.insert("__type__".to_string(), JsValue::String("Navigation".to_string()));
                    m.insert("canGoBack".to_string(), JsValue::Boolean(false));
                    m.insert("canGoForward".to_string(), JsValue::Boolean(false));
                    JsValue::Object(m)
                }
                "crypto" => {
                    let mut m = HashMap::new();
                    m.insert("__type__".to_string(), JsValue::String("Crypto".to_string()));
                    let mut subtle = HashMap::new();
                    subtle.insert("__type__".to_string(), JsValue::String("SubtleCrypto".to_string()));
                    m.insert("subtle".to_string(), JsValue::Object(subtle));
                    JsValue::Object(m)
                }
                "eval" | "structuredClone" | "parseInt" | "parseFloat" | "isNaN" | "isFinite" |
                "encodeURIComponent" | "decodeURIComponent" | "Symbol" | "queueMicrotask" |
                "Number" | "String" | "Boolean" | "requestAnimationFrame" | "requestIdleCallback" |
                "atob" | "btoa" | "getComputedStyle" | "matchMedia" | "createImageBitmap" |
                "setTimeout" | "setInterval" | "clearTimeout" | "clearInterval" | "flushTimers" |
                "fetch" => {
                    JsValue::NativeFunction(name.clone())
                }
                _ => JsValue::Undefined,
            },
        }),
        Expr::Array(elems) => {
            let mut arr = Vec::new();
            for e in elems {
                if let Expr::Spread(inner) = e {
                    if let JsValue::Array(items) = eval_expr_node(inner, scope)? { arr.extend(items); }
                } else { arr.push(eval_expr_node(e, scope)?); }
            }
            Ok(JsValue::Array(arr))
        }
        Expr::Object(props) => {
            let mut map = HashMap::new();
            for (k, v) in props { map.insert(k.clone(), eval_expr_node(v, scope)?); }
            Ok(JsValue::Object(map))
        }
        Expr::ObjectWithSpread(items) => {
            let mut map = HashMap::new();
            for item in items {
                match item {
                    ObjectProp::KeyValue(k, v) => { map.insert(k.clone(), eval_expr_node(v, scope)?); }
                    ObjectProp::Getter(k, func_expr) => {
                        let func = eval_expr_node(func_expr, scope)?;
                        install_literal_accessor(&mut map, k, "get", func);
                    }
                    ObjectProp::Setter(k, func_expr) => {
                        let func = eval_expr_node(func_expr, scope)?;
                        install_literal_accessor(&mut map, k, "set", func);
                    }
                    ObjectProp::Computed(key_expr, val_expr) => {
                        let key = to_string(&eval_expr_node(key_expr, scope)?);
                        map.insert(key, eval_expr_node(val_expr, scope)?);
                    }
                    ObjectProp::Spread(expr) => {
                        if let JsValue::Object(src) = eval_expr_node(expr, scope)? {
                            map.extend(src);
                        }
                    }
                }
            }
            Ok(JsValue::Object(map))
        }
        Expr::Unary(op, rhs) => eval_unary(op, rhs, scope),
        Expr::Binary(op, lhs, rhs) => eval_binary(op, lhs, rhs, scope),
        Expr::Assign(target, op, val) => eval_assign(target, op, val, scope),
        Expr::Ternary(cond, then_e, else_e) => {
            if to_boolean(&eval_expr_node(cond, scope)?) { eval_expr_node(then_e, scope) } else { eval_expr_node(else_e, scope) }
        }
        Expr::Member(obj, prop) => {
            if let Expr::Ident(ns) = obj.as_ref() {
                if Scope::resolve(scope, ns).is_none() {
                    if let Some(v) = super::method_dispatch::builtin_namespace_constant(ns, prop) { return Ok(v); }
                }
            }
            let obj_val = eval_expr_node(obj, scope)?;
            Ok(get_property(&obj_val, prop))
        }
        Expr::OptionalMember(obj, prop) => {
            let obj_val = eval_expr_node(obj, scope)?;
            if matches!(obj_val, JsValue::Null | JsValue::Undefined) { return Ok(JsValue::Undefined); }
            Ok(get_property(&obj_val, prop))
        }
        Expr::OptionalIndex(obj, idx) => {
            let obj_val = eval_expr_node(obj, scope)?;
            if matches!(obj_val, JsValue::Null | JsValue::Undefined) { return Ok(JsValue::Undefined); }
            let key = eval_expr_node(idx, scope)?;
            let key_str = to_string(&key);
            Ok(get_property(&obj_val, &key_str))
        }
        Expr::OptionalCall(callee, args) => {
            let func = eval_expr_node(callee, scope)?;
            if matches!(func, JsValue::Null | JsValue::Undefined) { return Ok(JsValue::Undefined); }
            let mut evaluated_args = Vec::new();
            for a in args { evaluated_args.push(eval_expr_node(a, scope)?); }
            call_function(&func, &evaluated_args, scope)
        }
        Expr::Index(obj, idx) => {
            let obj_val = eval_expr_node(obj, scope)?;
            let key = eval_expr_node(idx, scope)?;
            let key_str = to_string(&key);
            Ok(get_property(&obj_val, &key_str))
        }
        Expr::Call(callee, args) => eval_call(callee, args, scope),
                Expr::New(callee, args) => eval_new(callee, args, scope),
        Expr::Arrow(params, body) => {
            Ok(JsValue::Function { name: None, params: params.clone(), body: (**body).clone(), closure: scope.clone() })
        }
        Expr::Function(name, params, body) => {
            Ok(JsValue::Function { name: name.clone(), params: params.clone(), body: (**body).clone(), closure: scope.clone() })
        }
        Expr::Typeof(e) => {
            let val = eval_expr_node(e, scope)?;
            Ok(JsValue::String(typeof_str(&val).to_string()))
        }
        Expr::Void(_) => Ok(JsValue::Undefined),
        Expr::Spread(_) => Ok(JsValue::Undefined),
        Expr::Await(e) => {
            let mut val = eval_expr_node(e, scope)?;
            let mut depth = 0;
            loop {
                if depth >= 32 { break; }
                match &val {
                    JsValue::Object(map) if map.get("__type__").map(to_string).as_deref() == Some("Promise") => {
                        if let Some(reason) = map.get("__rejected__") {
                            if *reason != JsValue::Undefined {
                                return Err(Signal::Throw(reason.clone()));
                            }
                        }
                        let inner = map.get("__resolved__").cloned().unwrap_or(JsValue::Undefined);
                        val = inner;
                        depth += 1;
                    }
                    _ => break,
                }
            }
            Ok(val)
        }
        Expr::Sequence(exprs) => {
            let mut last = JsValue::Undefined;
            for e in exprs { last = eval_expr_node(e, scope)?; }
            Ok(last)
        }
        Expr::Yield(e) => {
            let val = eval_expr_node(e, scope)?;
            if let Some(JsValue::Array(mut arr)) = Scope::resolve(scope, "__yield_values__") {
                arr.push(val.clone());
                Scope::assign(scope, "__yield_values__", JsValue::Array(arr));
            }
            Ok(val)
        }
    }
}

fn eval_unary(op: &Token, rhs: &Expr, scope: &ScopeRef) -> EvalResult {
    if matches!(op, Token::Delete) {
        return eval_delete(rhs, scope);
    }
    let val = eval_expr_node(rhs, scope)?;
    Ok(match op {
        Token::Minus => { let p = to_primitive(&val); JsValue::Number(-to_number(&p)) }
        Token::Plus => { let p = to_primitive(&val); JsValue::Number(to_number(&p)) }
        Token::Bang => JsValue::Boolean(!to_boolean(&val)),
        Token::Tilde => { let p = to_primitive(&val); JsValue::Number(!(to_number(&p) as i32) as f64) }
        Token::PlusPlus => {
            let n = to_number(&val) + 1.0;
            if let Expr::Ident(name) = rhs { Scope::assign(scope, name, JsValue::Number(n)); }
            JsValue::Number(n)
        }
        Token::MinusMinus => {
            let n = to_number(&val) - 1.0;
            if let Expr::Ident(name) = rhs { Scope::assign(scope, name, JsValue::Number(n)); }
            JsValue::Number(n)
        }
        _ => JsValue::Undefined,
    })
}

fn eval_delete(rhs: &Expr, scope: &ScopeRef) -> EvalResult {
    match rhs {
        Expr::Member(obj, prop) => {
            let obj_name = match obj.as_ref() {
                Expr::Ident(name) => Some(name.as_str()),
                Expr::This => Some("this"),
                _ => None,
            };
            if let Some(name) = obj_name {
                if let Some(mut val) = Scope::resolve(scope, name) {
                    let ok = delete_property(&mut val, prop);
                    Scope::assign(scope, name, val);
                    return Ok(JsValue::Boolean(ok));
                }
            }
            Ok(JsValue::Boolean(true))
        }
        Expr::Index(obj, idx_expr) => {
            let obj_name = match obj.as_ref() {
                Expr::Ident(name) => Some(name.as_str()),
                Expr::This => Some("this"),
                _ => None,
            };
            if let Some(name) = obj_name {
                if let Some(mut target) = Scope::resolve(scope, name) {
                    let key = eval_expr_node(idx_expr, scope).map(|k| to_string(&k)).unwrap_or_default();
                    let ok = delete_property(&mut target, &key);
                    Scope::assign(scope, name, target);
                    return Ok(JsValue::Boolean(ok));
                }
            }
            Ok(JsValue::Boolean(true))
        }
        _ => Ok(JsValue::Boolean(true)),
    }
}

fn eval_binary(op: &Token, lhs: &Expr, rhs: &Expr, scope: &ScopeRef) -> EvalResult {
    if matches!(op, Token::AmpAmp) {
        let l = eval_expr_node(lhs, scope)?;
        return if to_boolean(&l) { eval_expr_node(rhs, scope) } else { Ok(l) };
    }
    if matches!(op, Token::PipePipe) {
        let l = eval_expr_node(lhs, scope)?;
        return if to_boolean(&l) { Ok(l) } else { eval_expr_node(rhs, scope) };
    }
    if matches!(op, Token::QuestionQuestion) {
        let l = eval_expr_node(lhs, scope)?;
        return if matches!(l, JsValue::Null | JsValue::Undefined) { eval_expr_node(rhs, scope) } else { Ok(l) };
    }
    let l = eval_expr_node(lhs, scope)?;
    let r = eval_expr_node(rhs, scope)?;
    Ok(match op {
        Token::Plus => {
            let lp = to_primitive(&l);
            let rp = to_primitive(&r);
            if matches!(lp, JsValue::String(_)) || matches!(rp, JsValue::String(_)) {
                JsValue::String(format!("{}{}", to_string(&lp), to_string(&rp)))
            } else { JsValue::Number(to_number(&lp) + to_number(&rp)) }
        }
        Token::Minus => JsValue::Number(to_number(&l) - to_number(&r)),
        Token::Star => JsValue::Number(to_number(&l) * to_number(&r)),
        Token::Slash => JsValue::Number(to_number(&l) / to_number(&r)),
        Token::Percent => JsValue::Number(to_number(&l) % to_number(&r)),
        Token::StarStar => JsValue::Number(to_number(&l).powf(to_number(&r))),
        Token::EqEq => JsValue::Boolean(loose_eq(&l, &r)),
        Token::BangEq => JsValue::Boolean(!loose_eq(&l, &r)),
        Token::EqEqEq => JsValue::Boolean(strict_eq(&l, &r)),
        Token::BangEqEq => JsValue::Boolean(!strict_eq(&l, &r)),
        Token::Lt => JsValue::Boolean(relational_cmp(&l, &r) == Some(std::cmp::Ordering::Less)),
        Token::Gt => JsValue::Boolean(relational_cmp(&l, &r) == Some(std::cmp::Ordering::Greater)),
        Token::LtEq => JsValue::Boolean(matches!(relational_cmp(&l, &r), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal))),
        Token::GtEq => JsValue::Boolean(matches!(relational_cmp(&l, &r), Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal))),
        Token::Amp => JsValue::Number(((to_number(&l) as i32) & (to_number(&r) as i32)) as f64),
        Token::Pipe => JsValue::Number(((to_number(&l) as i32) | (to_number(&r) as i32)) as f64),
        Token::Caret => JsValue::Number(((to_number(&l) as i32) ^ (to_number(&r) as i32)) as f64),
        Token::LtLt => JsValue::Number(((to_number(&l) as i32) << (to_number(&r) as u32 & 31)) as f64),
        Token::GtGt => JsValue::Number(((to_number(&l) as i32) >> (to_number(&r) as u32 & 31)) as f64),
        Token::GtGtGt => JsValue::Number(((to_number(&l) as u32) >> (to_number(&r) as u32 & 31)) as f64),
        Token::Instanceof => {
            let ctor_name: Option<String> = match &r {
                JsValue::Object(cm) if cm.get("__type__").map(to_string).as_deref() == Some("class") => {
                    cm.get("__name__").map(to_string)
                }
                JsValue::Function { name: Some(n), .. } => Some(n.clone()),
                _ => None,
            };
            match (&l, ctor_name) {
                (JsValue::Object(im), Some(name)) => {
                    let in_chain = match im.get("__instanceof__") {
                        Some(JsValue::Array(chain)) => chain.iter().any(|v| to_string(v) == name),
                        _ => false,
                    };
                    JsValue::Boolean(in_chain)
                }
                _ => JsValue::Boolean(false),
            }
        }
        Token::In => {
            let key = to_string(&l);
            JsValue::Boolean(has_property(&r, &key))
        }
        _ => JsValue::Undefined,
    })
}

fn eval_assign(target: &Expr, op: &Token, val: &Expr, scope: &ScopeRef) -> EvalResult {
    let rhs = eval_expr_node(val, scope)?;
    let final_val = match op {
        Token::Eq => rhs,
        Token::PlusEq => { let curr = eval_expr_node(target, scope)?; if matches!(curr, JsValue::String(_)) || matches!(rhs, JsValue::String(_)) { JsValue::String(format!("{}{}", to_string(&curr), to_string(&rhs))) } else { JsValue::Number(to_number(&curr) + to_number(&rhs)) } }
        Token::MinusEq => { let curr = eval_expr_node(target, scope)?; JsValue::Number(to_number(&curr) - to_number(&rhs)) }
        Token::StarEq => { let curr = eval_expr_node(target, scope)?; JsValue::Number(to_number(&curr) * to_number(&rhs)) }
        Token::SlashEq => { let curr = eval_expr_node(target, scope)?; JsValue::Number(to_number(&curr) / to_number(&rhs)) }
        Token::QuestionQuestionEq => {
            let curr = eval_expr_node(target, scope)?;
            if matches!(curr, JsValue::Null | JsValue::Undefined) { rhs } else { return Ok(curr); }
        }
        _ => rhs,
    };
    assign_to_target(target, final_val.clone(), scope);
    Ok(final_val)
}

pub(super) fn assign_to_target(target: &Expr, value: JsValue, scope: &ScopeRef) {
    match target {
        Expr::Ident(name) => {
            if Scope::is_const(scope, name) {
                // Const reassignment — silently ignore (permissive mode).
                return;
            }
            if !Scope::assign(scope, name, value.clone()) { Scope::declare(scope, name, value); }
        }
        Expr::Member(obj, prop) => {
            let obj_name = match obj.as_ref() {
                Expr::Ident(name) => Some(name.as_str()),
                Expr::This => Some("this"),
                _ => None,
            };
            if let Some(name) = obj_name {
                if let Some(mut obj_val) = Scope::resolve(scope, name) {
                    set_property(&mut obj_val, prop, value);
                    Scope::assign(scope, name, obj_val);
                } else {
                    // Global objects (document, window, etc.) resolved via identifier fallback.
                    if let Ok(mut obj_val) = eval_expr_node(obj, scope) {
                        set_property(&mut obj_val, prop, value);
                    }
                }
            } else {
                // Nested member expression (e.g. el.dataset.x = v):
                // evaluate the object, set the property on it.
                if let Ok(mut obj_val) = eval_expr_node(obj, scope) {
                    set_property(&mut obj_val, prop, value);
                }
            }
        }
        Expr::Index(obj, idx_expr) => {
            if let Expr::Ident(name) = obj.as_ref() {
                if let Some(mut arr_or_obj) = Scope::resolve(scope, name) {
                    let key = if let Ok(k) = eval_expr_node(idx_expr, scope) { to_string(&k) } else { return };
                    match &mut arr_or_obj {
                        JsValue::Array(arr) => { if let Ok(i) = key.parse::<usize>() { while arr.len() <= i { arr.push(JsValue::Undefined); } arr[i] = value; } }
                        JsValue::Object(map) => { map.insert(key, value); }
                        _ => {}
                    }
                    Scope::assign(scope, name, arr_or_obj);
                }
            }
        }
        _ => {}
    }
}

fn eval_call(callee: &Expr, args: &[Expr], scope: &ScopeRef) -> EvalResult {
    let mut evaluated_args = Vec::new();
    for a in args {
        if let Expr::Spread(inner) = a {
            if let JsValue::Array(items) = eval_expr_node(inner, scope)? { evaluated_args.extend(items); }
        } else { evaluated_args.push(eval_expr_node(a, scope)?); }
    }
    if let Expr::Member(obj_expr, method) = callee {
        if matches!(obj_expr.as_ref(), Expr::Super) {
            let parent = Scope::resolve(scope, "__super__").unwrap_or(JsValue::Undefined);
            let this_val = Scope::resolve(scope, "this").unwrap_or_else(|| JsValue::Object(HashMap::new()));
            return call_super_method(&parent, method, &evaluated_args, this_val, scope);
        }
        if let Expr::Ident(obj_name) = obj_expr.as_ref() {
            let native_name = format!("{}.{}", obj_name, method);
            match native_name.as_str() {
                "Reflect.set" => {
                    let prop = evaluated_args.get(1).map(to_string).unwrap_or_default();
                    let value = evaluated_args.get(2).cloned().unwrap_or(JsValue::Undefined);
                    let mut ok = false;
                    if let Some(Expr::Ident(var_name)) = args.first() {
                        if let Some(mut target) = Scope::resolve(scope, var_name) { ok = set_property(&mut target, &prop, value); Scope::assign(scope, var_name, target); }
                    }
                    return Ok(JsValue::Boolean(ok));
                }
                "Reflect.deleteProperty" => {
                    let prop = evaluated_args.get(1).map(to_string).unwrap_or_default();
                    if let Some(Expr::Ident(var_name)) = args.first() {
                        if let Some(mut target) = Scope::resolve(scope, var_name) { let ok = delete_property(&mut target, &prop); Scope::assign(scope, var_name, target); return Ok(JsValue::Boolean(ok)); }
                        return Ok(JsValue::Boolean(false));
                    }
                    let mut target = evaluated_args.first().cloned().unwrap_or(JsValue::Undefined);
                    return Ok(JsValue::Boolean(delete_property(&mut target, &prop)));
                }
                "Promise.resolve" | "Promise.reject" | "Promise.all" | "Promise.race" | "Promise.allSettled" |
                "Promise.withResolvers" | "Promise.try" |
                "Object.keys" | "Object.values" | "Object.entries" | "Object.fromEntries" | "Object.assign" | "Object.freeze" |
                "Object.is" | "Object.setPrototypeOf" | "Object.hasOwn" |
                "Object.create" | "Object.getPrototypeOf" | "Object.defineProperty" |
                "Object.defineProperties" | "Object.getOwnPropertyDescriptor" | "Object.getOwnPropertyDescriptors" | "Object.getOwnPropertyNames" |
                "Object.groupBy" | "Object.getOwnPropertySymbols" |
                "Array.isArray" | "Array.from" | "Array.of" | "Array.fromAsync" |
                "Map.groupBy" |
                "Error.isError" | "ArrayBuffer.isView" | "Proxy.revocable" |
                "URL.canParse" | "crypto.randomUUID" | "crypto.getRandomValues" |
                "Intl.getCanonicalLocales" |
                "JSON.parse" | "JSON.stringify" |
                "Math.floor" | "Math.ceil" | "Math.round" | "Math.abs" | "Math.sqrt" |
                "Math.trunc" | "Math.sign" | "Math.log" | "Math.pow" | "Math.max" | "Math.min" | "Math.random" |
                "Math.sin" | "Math.cos" | "Math.tan" | "Math.asin" | "Math.acos" | "Math.atan" | "Math.atan2" |
                "Math.sinh" | "Math.cosh" | "Math.tanh" | "Math.exp" | "Math.expm1" | "Math.log1p" |
                "Math.log2" | "Math.log10" | "Math.cbrt" | "Math.hypot" | "Math.fround" | "Math.clz32" |
                "Math.asinh" | "Math.acosh" | "Math.atanh" | "Math.imul" |
                "Number.parseInt" | "Number.parseFloat" | "Number.isNaN" | "Number.isFinite" |
                "Number.isInteger" | "Number.isSafeInteger" |
                "String.fromCharCode" | "String.fromCodePoint" | "Date.now" | "performance.now" | "console.log" | "console.warn" | "console.error" | "console.info" |
                "console.debug" | "console.assert" | "console.count" | "console.countReset" | "console.time" | "console.timeEnd" |
                "console.table" | "console.trace" | "console.group" | "console.groupEnd" | "console.clear" |
                "eval" | "structuredClone" | "queueMicrotask" | "requestAnimationFrame" | "requestIdleCallback" | "Symbol" | "Symbol.for" |
                "Reflect.get" | "Reflect.has" |
                "Reflect.ownKeys" | "Reflect.getOwnPropertyDescriptor" | "Reflect.apply" | "Reflect.construct" => {
                    let result = call_native(&native_name, &evaluated_args)?;
                    if matches!(native_name.as_str(), "Object.defineProperty" | "Object.defineProperties" | "Object.assign" | "Object.setPrototypeOf") {
                        if let Some(Expr::Ident(var_name)) = args.first() { Scope::assign(scope, var_name, result.clone()); }
                    }
                    return Ok(result);
                }
                _ => {}
            }
        }
        let obj = eval_expr_node(obj_expr, scope)?;
        if let JsValue::Object(map) = &obj {
            let type_tag = map.get("__type__").map(to_string);
            match type_tag.as_deref() {
                Some("Map") | Some("WeakMap") => { let mut m = map.clone(); let result = call_map_method(&mut m, method, &evaluated_args, scope); assign_to_target(obj_expr, JsValue::Object(m), scope); return result; }
                Some("Set") | Some("WeakSet") => { let mut m = map.clone(); let result = call_set_method(&mut m, method, &evaluated_args, scope); assign_to_target(obj_expr, JsValue::Object(m), scope); return result; }
                Some("Promise") => return call_promise_method(map, method, &evaluated_args, scope),
                Some("Date") => return call_date_method_enhanced(map, method, &evaluated_args),
                Some("Generator") => return call_generator_method(map, method),
                Some("RegExp") => return call_regexp_method(map, method, &evaluated_args),
                Some("AbortController") => { let result = call_abort_controller_method(map, method, &evaluated_args)?; if method == "abort" { assign_to_target(obj_expr, result.clone(), scope); } return Ok(result); }
                Some("AbortSignal") => return call_abort_signal_method(map, method, &evaluated_args),
                Some("TextEncoder") => return call_text_encoder_method(map, method, &evaluated_args),
                Some("TextDecoder") => return call_text_decoder_method(map, method, &evaluated_args),
                Some("Response") => return call_response_method(map, method, &evaluated_args),
                Some("Blob") => return call_blob_method(map, method, &evaluated_args),
                Some("Uint8Array") | Some("Int8Array") | Some("Uint16Array") | Some("Int16Array") |
                Some("Uint32Array") | Some("Int32Array") | Some("Float32Array") | Some("Float64Array") |
                Some("Uint8ClampedArray") => return call_typed_array_method(map, method, &evaluated_args),
                Some("DataView") => return call_dataview_method(map, method, &evaluated_args),
                Some("Intl.Segmenter") => return call_segmenter_method(map, method, &evaluated_args),
                Some("Intl.Collator") => return call_collator_method(map, method, &evaluated_args),
                Some("Intl.NumberFormat") => return call_number_format_method(map, method, &evaluated_args),
                Some("Intl.DateTimeFormat") => return call_datetime_format_method(map, method, &evaluated_args),
                Some("Intl.PluralRules") => return call_plural_rules_method(map, method, &evaluated_args),
                Some("Intl.RelativeTimeFormat") => return call_relative_time_format_method(map, method, &evaluated_args),
                Some("Intl.DurationFormat") => return call_duration_format_method(map, method, &evaluated_args),
                Some("Intl.ListFormat") => return call_list_format_method(map, method, &evaluated_args),
                Some("Intl.DisplayNames") => return call_display_names_method(map, method, &evaluated_args),
                Some("Intl.Locale") => return call_locale_method(map, method),
                Some("MessagePort") => return super::web_apis2::call_message_port_method(map, method, &evaluated_args, scope),
                Some("EventTarget") => { let mut m = map.clone(); let result = super::web_apis2::call_event_target_method(&mut m, method, &evaluated_args, scope); assign_to_target(obj_expr, JsValue::Object(m), scope); return result; }
                Some("WeakRef") => return super::web_apis2::call_weakref_method(map, method),
                Some("FinalizationRegistry") => return super::web_apis2::call_finalization_registry_method(method),
                Some("Storage") => return Ok(super::browser_env::call_storage_method(map, method, &evaluated_args)),
                Some("Headers") => { let result = super::browser_env::call_headers_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("FormData") => { let result = super::browser_env::call_form_data_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("Event") | Some("CustomEvent") => { let result = super::browser_env::call_event_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("URLSearchParams") => { let result = super::browser_env::call_url_search_params_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("Navigator") => {
                    // Navigator methods + property access.
                    let result = super::web_platform::call_navigator_method(method, &evaluated_args);
                    if matches!(result, JsValue::Undefined) {
                        return Ok(get_property(&obj, method));
                    }
                    return Ok(result);
                }
                Some("Location") => {
                    return Ok(get_property(&obj, method));
                }
                Some("Window") => {
                    // Window methods + property access.
                    let result = match method.as_str() {
                        "postMessage" | "scrollTo" | "scrollBy" | "scroll" | "print" | "stop" | "focus" | "blur" => JsValue::Undefined,
                        "open" => {
                            let mut w = HashMap::new();
                            w.insert("__type__".to_string(), JsValue::String("Window".to_string()));
                            w.insert("closed".to_string(), JsValue::Boolean(false));
                            JsValue::Object(w)
                        }
                        "close" => JsValue::Undefined,
                        "alert" | "confirm" => JsValue::Undefined,
                        "prompt" => JsValue::Null,
                        "requestAnimationFrame" => super::browser_env::set_timeout(&evaluated_args),
                        "cancelAnimationFrame" => super::browser_env::clear_timer(&evaluated_args),
                        // Window shares the page's lifecycle event target with
                        // `document` — load/DOMContentLoaded land in one place.
                        "addEventListener" => {
                            let ev = evaluated_args.first().map(super::coercion::to_string).unwrap_or_default();
                            let handler = evaluated_args.get(1).cloned().unwrap_or(JsValue::Undefined);
                            super::browser_env::add_lifecycle_listener(&ev, handler);
                            JsValue::Undefined
                        }
                        "removeEventListener" => {
                            let ev = evaluated_args.first().map(super::coercion::to_string).unwrap_or_default();
                            super::browser_env::remove_lifecycle_listeners(&ev);
                            JsValue::Undefined
                        }
                        "dispatchEvent" => {
                            let ev = evaluated_args.first().and_then(|v| {
                                if let JsValue::Object(m) = v { m.get("type").map(super::coercion::to_string) } else { None }
                            }).unwrap_or_default();
                            super::browser_env::fire_lifecycle_event(&ev);
                            JsValue::Boolean(true)
                        }
                        "getSelection" => super::dom_bridge::make_selection(),
                        "getComputedStyle" => super::web_platform::get_computed_style(evaluated_args.first().unwrap_or(&JsValue::Undefined), evaluated_args.get(1)),
                        "matchMedia" => super::web_platform::match_media(&evaluated_args.first().map(super::coercion::to_string).unwrap_or_default()),
                        "atob" | "btoa" => JsValue::String(String::new()),
                        // Self-referential window properties.
                        "frames" | "self" | "top" | "parent" | "window" => super::browser_env::make_window(),
                        "opener" => JsValue::Null,
                        "length" => JsValue::Number(0.0),
                        "showOpenFilePicker" | "showSaveFilePicker" | "showDirectoryPicker" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            p.insert("__resolved__".to_string(), JsValue::Array(Vec::new()));
                            JsValue::Object(p)
                        }
                        _ => get_property(&obj, method),
                    };
                    return Ok(result);
                }
                Some("Document") => return Ok(super::dom_bridge::call_document_method(method, &evaluated_args)),
                Some("Element") => {
                    let result = super::dom_bridge::call_element_method(map, method, &evaluated_args);
                    return Ok(result);
                }
                Some("DOMParser") => return Ok(super::browser_env::call_dom_parser_method(method, &evaluated_args)),
                Some("XMLHttpRequest") => { let result = super::browser_env::call_xhr_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("MutationObserver") => { let result = super::browser_env::call_mutation_observer_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("BroadcastChannel") => { let result = super::browser_env::call_broadcast_channel_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("Performance") => return Ok(super::web_platform::call_performance_method(method, &evaluated_args)),
                Some("History") => return Ok(super::web_platform::call_history_method(method, &evaluated_args)),
                Some("IntersectionObserver") => { let result = super::web_platform::call_intersection_observer_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("ResizeObserver") => { let result = super::web_platform::call_resize_observer_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("WebSocket") => { let result = super::web_platform::call_web_socket_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("CSSStyleDeclaration") => return Ok(super::web_platform::call_css_style_declaration_method(map, method, &evaluated_args)),
                Some("MediaQueryList") => return Ok(super::web_platform::call_media_query_list_method(map, method, &evaluated_args)),
                Some("FileReader") => { let result = super::web_platform::call_file_reader_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("SubtleCrypto") => return Ok(super::web_platform::call_subtle_method(method, &evaluated_args)),
                Some("Crypto") => {
                    // crypto.randomUUID() / crypto.getRandomValues() as methods.
                    let result = match method.as_str() {
                        "randomUUID" => super::web_apis2::call_native_extended("crypto.randomUUID", &evaluated_args)?,
                        "getRandomValues" => super::web_apis2::call_native_extended("crypto.getRandomValues", &evaluated_args)?,
                        _ => get_property(&obj, method),
                    };
                    return Ok(result);
                }
                Some("CSSStyleSheet") => { let result = super::web_platform::call_css_style_sheet_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("DOMRect") => return Ok(super::web_platform::call_dom_rect_method(map, method, &evaluated_args)),
                Some("DOMMatrix") => {
                    let result = match method.as_str() {
                        "translate" | "scale" | "rotate" | "skewX" | "skewY" | "multiply" | "flipX" | "flipY" | "inverse" => JsValue::Object(map.clone()),
                        "transformPoint" => super::web_platform::make_dom_rect(0.0, 0.0, 0.0, 0.0),
                        "toJSON" => JsValue::Object(map.clone()),
                        _ => get_property(&obj, method),
                    };
                    return Ok(result);
                }
                Some("DocumentFragment") => {
                    let result = match method.as_str() {
                        "querySelector" => JsValue::Null,
                        "querySelectorAll" => JsValue::Array(Vec::new()),
                        "getElementById" => JsValue::Null,
                        "append" | "prepend" | "replaceChildren" => JsValue::Undefined,
                        _ => JsValue::Undefined,
                    };
                    return Ok(result);
                }
                Some("CacheStorage") => return Ok(super::web_platform::call_caches_method(method, &evaluated_args)),
                Some("Cache") => return Ok(super::web_platform::call_cache_method(map, method, &evaluated_args)),
                Some("Clipboard") => return Ok(super::web_platform::call_clipboard_method(method, &evaluated_args)),
                Some("Permissions") => return Ok(super::web_platform::call_permissions_method(method, &evaluated_args)),
                Some("Geolocation") => return Ok(super::web_platform::call_geolocation_method(method, &evaluated_args)),
                Some("ReadableStream") => return Ok(super::streams::call_readable_stream_method(map, method, &evaluated_args)),
                Some("ReadableStreamDefaultReader") => return Ok(super::streams::call_reader_method(map, method, &evaluated_args)),
                Some("ReadableStreamDefaultController") => return Ok(super::streams::call_readable_controller_method(map, method, &evaluated_args)),
                Some("WritableStream") => return Ok(super::streams::call_writable_stream_method(map, method, &evaluated_args)),
                Some("WritableStreamDefaultWriter") => return Ok(super::streams::call_writer_method(map, method, &evaluated_args)),
                Some("TransformStream") => return Ok(super::streams::call_transform_stream_method(map, method, &evaluated_args)),
                Some("TransformStreamDefaultController") => return Ok(super::streams::call_transform_controller_method(map, method, &evaluated_args)),
                Some("CountQueuingStrategy") | Some("ByteLengthQueuingStrategy") => return Ok(super::streams::call_queuing_strategy_method(map, method, &evaluated_args)),
                Some("DOMTokenList") => { let result = super::dom_bridge::call_dom_token_list_method(map, method, &evaluated_args); return Ok(result); }
                Some("DOMStringMap") => {
                    // dataset.prop access handled via property; methods are no-ops.
                    return Ok(super::dom_bridge::get_dataset_property(map, method));
                }
                Some("TreeWalker") => return Ok(super::dom_bridge::call_tree_walker_method(map, method, &evaluated_args)),
                Some("NodeIterator") => return Ok(super::dom_bridge::call_node_iterator_method(map, method, &evaluated_args)),
                Some("Range") => { let result = super::dom_bridge::call_range_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("Selection") => { let result = super::dom_bridge::call_selection_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("Worker") => return Ok(super::web_platform::call_worker_method(map, method, &evaluated_args)),
                Some("ServiceWorkerContainer") => return Ok(super::web_platform::call_service_worker_container_method(method, &evaluated_args)),
                Some("HTMLCanvasElement") => return Ok(super::canvas::call_canvas_method(map, method, &evaluated_args)),
                Some("CanvasRenderingContext2D") => return Ok(super::canvas::call_context_2d_method(map, method, &evaluated_args)),
                Some("CanvasGradient") => { let result = super::canvas::call_canvas_gradient_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("Path2D") => { let result = super::canvas::call_path_2d_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("OffscreenCanvas") => return Ok(super::canvas::call_offscreen_canvas_method(map, method, &evaluated_args)),
                Some("ImageBitmap") => { let result = super::canvas::call_image_bitmap_method(map, method, &evaluated_args); assign_to_target(obj_expr, result.clone(), scope); return Ok(result); }
                Some("Animation") => {
                    // Web Animations API: play/pause/cancel/finish are no-ops, return self.
                    let result = match method.as_str() {
                        "play" | "pause" | "cancel" | "finish" | "reverse" | "updatePlaybackRate" => JsValue::Object(map.clone()),
                        "finished" | "ready" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            p.insert("__resolved__".to_string(), JsValue::Object(map.clone()));
                            JsValue::Object(p)
                        }
                        _ => get_property(&obj, method),
                    };
                    assign_to_target(obj_expr, result.clone(), scope);
                    return Ok(result);
                }
                Some("CustomElementRegistry") => {
                    let result = match method.as_str() {
                        "define" | "upgrade" => JsValue::Undefined,
                        "get" => JsValue::Undefined,
                        "whenDefined" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            p.insert("__resolved__".to_string(), JsValue::Undefined);
                            JsValue::Object(p)
                        }
                        _ => JsValue::Undefined,
                    };
                    return Ok(result);
                }
                Some("PerformanceObserver") => {
                    if method == "supportedEntryTypes" {
                        return Ok(super::web_platform::performance_observer_supported_entry_types());
                    }
                    let result = super::web_platform::call_performance_observer_method(map, method, &evaluated_args);
                    assign_to_target(obj_expr, result.clone(), scope);
                    return Ok(result);
                }
                Some("IDBFactory") => return Ok(super::web_platform::call_indexed_db_method(method, &evaluated_args)),
                Some("CSS") => {
                    let result = match method.as_str() {
                        "escape" => {
                            let s = evaluated_args.first().map(to_string).unwrap_or_default();
                            JsValue::String(s.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "\\").to_string())
                        }
                        "supports" => JsValue::Boolean(true),
                        _ => JsValue::Undefined,
                    };
                    return Ok(result);
                }
                Some("FontFaceSet") => {
                    let result = match method.as_str() {
                        "check" => JsValue::Boolean(true),
                        "load" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            p.insert("__resolved__".to_string(), JsValue::Array(Vec::new()));
                            JsValue::Object(p)
                        }
                        "add" | "delete" | "clear" => JsValue::Undefined,
                        "forEach" => JsValue::Undefined,
                        _ => JsValue::Undefined,
                    };
                    return Ok(result);
                }
                Some("Navigation") => {
                    let result = match method.as_str() {
                        "back" | "forward" | "reload" | "navigate" | "traverseTo" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            p.insert("__resolved__".to_string(), JsValue::Undefined);
                            JsValue::Object(p)
                        }
                        "entries" => JsValue::Array(Vec::new()),
                        "addEventListener" | "removeEventListener" => JsValue::Undefined,
                        _ => get_property(&obj, method),
                    };
                    return Ok(result);
                }
                Some("ViewTransition") => {
                    let result = match method.as_str() {
                        "skipTransition" => JsValue::Undefined,
                        _ => get_property(&obj, method),
                    };
                    return Ok(result);
                }
                Some("ScreenOrientation") => {
                    let result = match method.as_str() {
                        "lock" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            p.insert("__resolved__".to_string(), JsValue::Undefined);
                            JsValue::Object(p)
                        }
                        "unlock" => JsValue::Undefined,
                        "addEventListener" | "removeEventListener" => JsValue::Undefined,
                        _ => get_property(&obj, method),
                    };
                    return Ok(result);
                }
                Some("NetworkInformation") => {
                    return Ok(get_property(&obj, method));
                }
                Some("StorageManager") => {
                    let result = match method.as_str() {
                        "estimate" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            let mut est = HashMap::new();
                            est.insert("quota".to_string(), JsValue::Number(1_073_741_824.0));
                            est.insert("usage".to_string(), JsValue::Number(0.0));
                            p.insert("__resolved__".to_string(), JsValue::Object(est));
                            JsValue::Object(p)
                        }
                        "persist" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            p.insert("__resolved__".to_string(), JsValue::Boolean(true));
                            JsValue::Object(p)
                        }
                        "persisted" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            p.insert("__resolved__".to_string(), JsValue::Boolean(true));
                            JsValue::Object(p)
                        }
                        _ => JsValue::Undefined,
                    };
                    return Ok(result);
                }
                Some("DataTransfer") => {
                    let result = match method.as_str() {
                        "getData" => JsValue::String(String::new()),
                        "setData" | "clearData" => JsValue::Undefined,
                        "setDragImage" => JsValue::Undefined,
                        _ => get_property(&obj, method),
                    };
                    return Ok(result);
                }
                Some("MediaDevices") => {
                    let result = match method.as_str() {
                        "enumerateDevices" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            p.insert("__resolved__".to_string(), JsValue::Array(Vec::new()));
                            JsValue::Object(p)
                        }
                        "getUserMedia" | "getDisplayMedia" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            p.insert("__resolved__".to_string(), JsValue::Undefined);
                            JsValue::Object(p)
                        }
                        "addEventListener" | "removeEventListener" => JsValue::Undefined,
                        _ => JsValue::Undefined,
                    };
                    return Ok(result);
                }
                Some("WakeLock") => {
                    let result = match method.as_str() {
                        "request" => {
                            let mut p = HashMap::new();
                            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            let mut sentinel = HashMap::new();
                            sentinel.insert("__type__".to_string(), JsValue::String("WakeLockSentinel".to_string()));
                            sentinel.insert("released".to_string(), JsValue::Boolean(false));
                            p.insert("__resolved__".to_string(), JsValue::Object(sentinel));
                            JsValue::Object(p)
                        }
                        _ => JsValue::Undefined,
                    };
                    return Ok(result);
                }
                Some("class") => {
                    if let Some(func) = find_static_method(&obj, method) { let (result, _) = call_method_with_this_writeback(&func, &evaluated_args, scope, obj.clone()); return result; }
                    return Ok(JsValue::Undefined);
                }
                _ => {}
            }
            if let Some(func) = map.get(method).cloned() {
                let (result, updated_this) = call_method_with_this_writeback(&func, &evaluated_args, scope, obj.clone());
                if let Expr::Ident(var_name) = obj_expr.as_ref() { Scope::assign(scope, var_name, updated_this); }
                return result;
            }
            let proto_func = get_property(&obj, method);
            if let JsValue::Function { .. } = &proto_func {
                let (result, updated_this) = call_method_with_this_writeback(&proto_func, &evaluated_args, scope, obj.clone());
                if let Expr::Ident(var_name) = obj_expr.as_ref() { Scope::assign(scope, var_name, updated_this); }
                return result;
            }
            return call_object_method_enhanced(map, method, &evaluated_args);
        }
        if let JsValue::Array(arr) = &obj {
            let mut updated = arr.clone();
            let result = call_array_method(&mut updated, method, &evaluated_args, scope);
            assign_to_target(obj_expr, JsValue::Array(updated), scope);
            return result;
        }
        return call_method(&obj, method, &evaluated_args, scope);
    }
    if let Expr::OptionalMember(obj_expr, method) = callee {
        let obj = eval_expr_node(obj_expr, scope)?;
        if matches!(obj, JsValue::Null | JsValue::Undefined) { return Ok(JsValue::Undefined); }
        return call_method(&obj, method, &evaluated_args, scope);
    }
    if let Expr::Ident(name) = callee {
        match name.as_str() {
            "eval" | "structuredClone" | "parseInt" | "parseFloat" | "isNaN" | "isFinite" |
            "encodeURIComponent" | "decodeURIComponent" | "Symbol" | "queueMicrotask" |
            "Number" | "String" | "Boolean" | "requestAnimationFrame" | "requestIdleCallback" |
            "atob" | "btoa" |
            "setTimeout" | "setInterval" | "clearTimeout" | "clearInterval" | "flushTimers" |
            "fetch" => {
                return call_native(name, &evaluated_args);
            }
            _ => {}
        }
    }
    if matches!(callee, Expr::Super) {
        let parent = Scope::resolve(scope, "__super__").unwrap_or(JsValue::Undefined);
        let this_val = Scope::resolve(scope, "this").unwrap_or_else(|| JsValue::Object(HashMap::new()));
        return call_super_constructor(&parent, &evaluated_args, this_val, scope);
    }
    let func = eval_expr_node(callee, scope)?;
    call_function(&func, &evaluated_args, scope)
}

fn call_super_constructor(parent_class: &JsValue, args: &[JsValue], this_val: JsValue, scope: &ScopeRef) -> EvalResult {
    if let JsValue::Object(parent_map) = parent_class {
        if let Some(JsValue::Function { params, body, closure, .. }) = parent_map.get("__constructor__") {
            let ctor_scope = Scope::new_child(closure);
            Scope::declare(&ctor_scope, "this", this_val);
            if let Some(grandparent) = parent_map.get("__parent__") { Scope::declare(&ctor_scope, "__super__", grandparent.clone()); }
            for (i, p) in params.iter().enumerate() { Scope::declare(&ctor_scope, p, args.get(i).cloned().unwrap_or(JsValue::Undefined)); }
            Scope::declare(&ctor_scope, "arguments", JsValue::Array(args.to_vec()));
            let _ = eval_stmt(body, &ctor_scope);
            if let Some(updated) = Scope::resolve(&ctor_scope, "this") { Scope::assign(scope, "this", updated); }
        }
    }
    Ok(JsValue::Undefined)
}

fn call_super_method(parent_class: &JsValue, method: &str, args: &[JsValue], this_val: JsValue, scope: &ScopeRef) -> EvalResult {
    if let Some(func) = find_proto_method(parent_class, method) {
        let (result, updated_this) = call_method_with_this_writeback(&func, args, scope, this_val);
        Scope::assign(scope, "this", updated_this);
        return result;
    }
    Ok(JsValue::Undefined)
}

pub(super) fn find_proto_method(class_val: &JsValue, method: &str) -> Option<JsValue> {
    let mut current = class_val.clone();
    let mut depth = 0;
    loop {
        if depth > 64 { return None; }
        let JsValue::Object(cm) = &current else { return None };
        if let Some(JsValue::Object(proto)) = cm.get("__proto_methods__") { if let Some(func) = proto.get(method) { return Some(func.clone()); } }
        match cm.get("__parent__") { Some(parent) => { current = parent.clone(); depth += 1; } None => return None, }
    }
}

pub(super) fn find_static_method(class_val: &JsValue, method: &str) -> Option<JsValue> {
    let mut current = class_val.clone();
    let mut depth = 0;
    loop {
        if depth > 64 { return None; }
        let JsValue::Object(cm) = &current else { return None };
        if let Some(JsValue::Object(statics)) = cm.get("__static_methods__") { if let Some(func) = statics.get(method) { return Some(func.clone()); } }
        match cm.get("__parent__") { Some(parent) => { current = parent.clone(); depth += 1; } None => return None, }
    }
}

fn eval_template_literal(raw: &str, scope: &ScopeRef) -> EvalResult {
    let mut result = String::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '$' && chars[i + 1] == '{' {
            i += 2; let mut depth = 1; let mut expr_str = String::new();
            while i < chars.len() && depth > 0 { if chars[i] == '{' { depth += 1; } else if chars[i] == '}' { depth -= 1; if depth == 0 { i += 1; break; } } expr_str.push(chars[i]); i += 1; }
            match eval_script(&expr_str, scope) { Ok(val) => result.push_str(&to_string(&val)), Err(_) => result.push_str("undefined"), }
        } else { result.push(chars[i]); i += 1; }
    }
    Ok(JsValue::String(result))
}

// eval_new lives in constructors.rs (imported above) to keep this file under the LOC budget.

fn eval_class_decl(name: &str, parent: &Option<String>, methods: &[ClassMethod], fields: &[ClassField], scope: &ScopeRef) {
    let mut class_obj = HashMap::new();
    class_obj.insert("__type__".to_string(), JsValue::String("class".to_string()));
    class_obj.insert("__name__".to_string(), JsValue::String(name.to_string()));
    if let Some(parent_name) = parent { if let Some(parent_val) = Scope::resolve(scope, parent_name) { class_obj.insert("__parent__".to_string(), parent_val); } }
    let mut proto = HashMap::new();
    let mut statics = HashMap::new();
    for m in methods {
        let func = JsValue::Function { name: Some(m.name.clone()), params: m.params.clone(), body: m.body.clone(), closure: scope.clone() };
        if m.name == "constructor" { class_obj.insert("__constructor__".to_string(), func); }
        else if m.is_static { statics.insert(m.name.clone(), func); }
        else { match m.kind { ClassMemberKind::Getter => install_literal_accessor(&mut proto, &m.name, "get", func), ClassMemberKind::Setter => install_literal_accessor(&mut proto, &m.name, "set", func), ClassMemberKind::Method => { proto.insert(m.name.clone(), func); } } }
    }
    class_obj.insert("__proto_methods__".to_string(), JsValue::Object(proto));
    class_obj.insert("__static_methods__".to_string(), JsValue::Object(statics));
    let mut instance_fields: Vec<JsValue> = Vec::new();
    for f in fields {
        if f.is_static { let val = match &f.init { Some(expr) => eval_expr_node(expr, scope).unwrap_or(JsValue::Undefined), None => JsValue::Undefined }; class_obj.insert(f.name.clone(), val); }
        else { let init_func = JsValue::Function { name: None, params: Vec::new(), body: Stmt::Return(f.init.clone()), closure: scope.clone() }; instance_fields.push(JsValue::Array(vec![JsValue::String(f.name.clone()), init_func])); }
    }
    class_obj.insert("__instance_fields__".to_string(), JsValue::Array(instance_fields));
    Scope::declare(scope, name, JsValue::Object(class_obj));
}

pub(super) fn call_class_constructor(class_map: &HashMap<String, JsValue>, args: &[JsValue], _scope: &ScopeRef) -> EvalResult {
    let mut instance = HashMap::new();
    if let Some(JsValue::Object(proto)) = class_map.get("__proto_methods__") { instance.extend(proto.iter().map(|(k, v)| (k.clone(), v.clone()))); }
    if let Some(JsValue::Object(parent_class)) = class_map.get("__parent__") { if let Some(JsValue::Object(parent_proto)) = parent_class.get("__proto_methods__") { for (k, v) in parent_proto { instance.entry(k.clone()).or_insert_with(|| v.clone()); } } }
    if let Some(JsValue::String(class_name)) = class_map.get("__name__") { instance.insert("__class_name__".to_string(), JsValue::String(class_name.clone())); }
    instance.insert("__instanceof__".to_string(), JsValue::Array(class_ancestry_names(class_map)));
    let mut chain: Vec<&HashMap<String, JsValue>> = Vec::new();
    let mut cur = Some(class_map);
    let mut depth = 0;
    while let Some(cm) = cur { if depth > 64 { break; } chain.push(cm); cur = match cm.get("__parent__") { Some(JsValue::Object(p)) => Some(p), _ => None }; depth += 1; }
    for cm in chain.iter().rev() { if let Some(JsValue::Array(fields_arr)) = cm.get("__instance_fields__") { for entry in fields_arr { if let JsValue::Array(pair) = entry { if let (Some(JsValue::String(fname)), Some(func)) = (pair.first(), pair.get(1)) { let this_val = JsValue::Object(instance.clone()); let val = call_function_with_this(func, &[], &Scope::new_global(), Some(this_val)).unwrap_or(JsValue::Undefined); instance.insert(fname.clone(), val); } } } } }
    if let Some(ctor) = class_map.get("__constructor__") {
        if let JsValue::Function { params, body, closure, .. } = ctor {
            let ctor_scope = Scope::new_child(closure);
            Scope::declare(&ctor_scope, "this", JsValue::Object(instance.clone()));
            if let Some(parent) = class_map.get("__parent__") { Scope::declare(&ctor_scope, "__super__", parent.clone()); }
            for (i, p) in params.iter().enumerate() { Scope::declare(&ctor_scope, p, args.get(i).cloned().unwrap_or(JsValue::Undefined)); }
            Scope::declare(&ctor_scope, "arguments", JsValue::Array(args.to_vec()));
            let result = eval_stmt(body, &ctor_scope);
            let _ = result;
            if let Some(JsValue::Object(updated)) = Scope::resolve(&ctor_scope, "this") { instance = updated; }
        }
    }
    Ok(JsValue::Object(instance))
}

fn class_ancestry_names(class_map: &HashMap<String, JsValue>) -> Vec<JsValue> {
    let mut names = Vec::new();
    let mut current: Option<&HashMap<String, JsValue>> = Some(class_map);
    let mut depth = 0;
    while let Some(cm) = current {
        if depth > 64 { break; }
        if let Some(JsValue::String(n)) = cm.get("__name__") {
            names.push(JsValue::String(n.clone()));
        }
        current = match cm.get("__parent__") {
            Some(JsValue::Object(parent)) => Some(parent),
            _ => None,
        };
        depth += 1;
    }
    names
}
