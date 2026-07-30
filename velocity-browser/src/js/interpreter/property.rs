use super::coercion::*;
use super::function::{call_function, call_function_with_this};
use super::eval::{eval_stmt, PROXY_TRAP_DEPTH, MAX_PROXY_TRAP_DEPTH};
use crate::js::scope::{Scope, ScopeRef};
use crate::js::vm::JsValue;
use std::collections::HashMap;

/// Materialise the value sequence produced by a `for...of` loop for the given
/// iterable, honouring the iterator protocol for custom iterables.
pub(super) fn iterate_values(val: &JsValue, scope: &ScopeRef) -> Vec<JsValue> {
    match val {
        JsValue::Array(arr) => arr.clone(),
        JsValue::String(s) => s.chars().map(|c| JsValue::String(c.to_string())).collect(),
        JsValue::Object(map) => match map.get("__type__").map(to_string).as_deref() {
            Some("Generator") => match map.get("__values__") {
                Some(JsValue::Array(values)) => values.clone(),
                _ => Vec::new(),
            },
            Some("Map") | Some("WeakMap") | Some("URLSearchParams") => match map.get("__entries__") {
                Some(JsValue::Array(entries)) => entries.clone(),
                _ => Vec::new(),
            },
            Some("Set") => match map.get("__items__") {
                Some(JsValue::Array(items)) => items.clone(),
                _ => Vec::new(),
            },
            _ => {
                // Custom iterable: a `Symbol.iterator`/`Symbol.asyncIterator`/`__iterator__`
                // method returns an iterator; otherwise treat the object itself as an
                // iterator (has `next`).
                let iterator = map
                    .get("Symbol.iterator")
                    .or_else(|| map.get("Symbol.asyncIterator"))
                    .or_else(|| map.get("__iterator__"))
                    .and_then(|mk| call_function(mk, &[val.clone()], scope).ok())
                    .unwrap_or_else(|| val.clone());
                drain_iterator(&iterator, scope)
            }
        },
        _ => Vec::new(),
    }
}

/// Drain an iterator object by repeatedly calling its `next()` method until it
/// reports `done`, collecting the yielded `value`s.
pub(super) fn drain_iterator(iter: &JsValue, scope: &ScopeRef) -> Vec<JsValue> {
    let mut out = Vec::new();
    for _ in 0..100_000 {
        let next_fn = get_property(iter, "next");
        if matches!(next_fn, JsValue::Undefined | JsValue::Null) {
            break;
        }
        let step = match call_function(&next_fn, &[], scope) {
            Ok(v) => v,
            Err(_) => break,
        };
        let (value, done) = match &step {
            JsValue::Object(m) => (
                m.get("value").cloned().unwrap_or(JsValue::Undefined),
                matches!(m.get("done"), Some(JsValue::Boolean(true))),
            ),
            _ => (step, false),
        };
        if done {
            break;
        }
        out.push(value);
    }
    out
}

/// Membership test for the `in` operator, respecting Proxy `has` traps and the
/// prototype chain (matching JS semantics where `in` sees inherited properties).
pub fn has_property(obj: &JsValue, prop: &str) -> bool {
    match obj {
        // Native Proxy variant: consult handler.has(target, prop) when present.
        JsValue::Proxy { target, handler } => {
            if let JsValue::Object(h_map) = handler.as_ref() {
                if let Some(has_trap) = h_map.get("has") {
                    if !matches!(has_trap, JsValue::NativeFunction(_)) {
                        let depth = PROXY_TRAP_DEPTH.with(|d| {
                            let cur = d.get();
                            if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                            d.set(cur + 1);
                            cur
                        });
                        if depth < MAX_PROXY_TRAP_DEPTH {
                            let prop_val = JsValue::String(prop.to_string());
                            let result = call_function(
                                has_trap,
                                &[(**target).clone(), prop_val],
                                &Scope::new_global(),
                            );
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            if let Ok(val) = result {
                                return to_boolean(&val);
                            }
                        }
                    }
                }
            }
            has_property(target, prop)
        }
        JsValue::Object(map) => {
            // Object-based proxy variant.
            if map.get("__type__").map(to_string).as_deref() == Some("Proxy") {
                if let (Some(t), Some(JsValue::Object(h_map))) =
                    (map.get("__proxy_target__"), map.get("__proxy_handler__"))
                {
                    if let Some(has_trap) = h_map.get("has") {
                        if !matches!(has_trap, JsValue::NativeFunction(_)) {
                            let depth = PROXY_TRAP_DEPTH.with(|d| {
                                let cur = d.get();
                                if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                                d.set(cur + 1);
                                cur
                            });
                            if depth < MAX_PROXY_TRAP_DEPTH {
                                let prop_val = JsValue::String(prop.to_string());
                                let result = call_function(
                                    has_trap,
                                    &[t.clone(), prop_val],
                                    &Scope::new_global(),
                                );
                                PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                                if let Ok(val) = result {
                                    return to_boolean(&val);
                                }
                            }
                        }
                    }
                    return has_property(t, prop);
                }
            }
            if map.contains_key(prop) {
                return true;
            }
            // Walk the prototype chain so inherited members are visible to `in`.
            let mut proto = map.get("__proto__");
            let mut depth = 0;
            while let Some(JsValue::Object(proto_map)) = proto {
                if depth >= 64 { break; }
                if proto_map.contains_key(prop) {
                    return true;
                }
                proto = proto_map.get("__proto__");
                depth += 1;
            }
            false
        }
        JsValue::Array(arr) => {
            if prop == "length" {
                return true;
            }
            prop.parse::<usize>().map(|i| i < arr.len()).unwrap_or(false)
        }
        JsValue::String(s) => {
            if prop == "length" {
                return true;
            }
            prop.parse::<usize>()
                .map(|i| i < s.chars().count())
                .unwrap_or(false)
        }
        _ => false,
    }
}

/// Delete a property, respecting Proxy `deleteProperty` traps. Returns the
/// boolean result of the delete (per JS, `delete` yields true in non-strict mode).
pub fn delete_property(obj: &mut JsValue, prop: &str) -> bool {
    match obj {
        // Native Proxy variant: consult handler.deleteProperty(target, prop).
        JsValue::Proxy { target, handler } => {
            if let JsValue::Object(h_map) = handler.as_ref() {
                if let Some(trap) = h_map.get("deleteProperty") {
                    if !matches!(trap, JsValue::NativeFunction(_)) {
                        let depth = PROXY_TRAP_DEPTH.with(|d| {
                            let cur = d.get();
                            if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                            d.set(cur + 1);
                            cur
                        });
                        if depth < MAX_PROXY_TRAP_DEPTH {
                            let prop_val = JsValue::String(prop.to_string());
                            let result = call_function(
                                trap,
                                &[(**target).clone(), prop_val],
                                &Scope::new_global(),
                            );
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            if let Ok(val) = result {
                                return to_boolean(&val);
                            }
                        }
                    }
                }
            }
            delete_property(target, prop)
        }
        JsValue::Object(map) => {
            // Object-based proxy variant.
            if map.get("__type__").map(to_string).as_deref() == Some("Proxy") {
                let target_clone = map.get("__proxy_target__").cloned();
                let handler_clone = map.get("__proxy_handler__").cloned();
                if let (Some(t), Some(JsValue::Object(h_map))) = (&target_clone, &handler_clone) {
                    if let Some(trap) = h_map.get("deleteProperty") {
                        if !matches!(trap, JsValue::NativeFunction(_)) {
                            let depth = PROXY_TRAP_DEPTH.with(|d| {
                                let cur = d.get();
                                if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                                d.set(cur + 1);
                                cur
                            });
                            if depth < MAX_PROXY_TRAP_DEPTH {
                                let prop_val = JsValue::String(prop.to_string());
                                let result = call_function(
                                    trap,
                                    &[t.clone(), prop_val],
                                    &Scope::new_global(),
                                );
                                PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                                if let Ok(val) = result {
                                    return to_boolean(&val);
                                }
                            }
                        }
                    }
                    if let Some(inner) = map.get_mut("__proxy_target__") {
                        return delete_property(inner, prop);
                    }
                }
            }
            map.remove(prop);
            true
        }
        JsValue::Array(arr) => {
            // Deleting an array element leaves a hole (undefined), per JS.
            if let Ok(i) = prop.parse::<usize>() {
                if i < arr.len() {
                    arr[i] = JsValue::Undefined;
                }
            }
            true
        }
        _ => true,
    }
}

/// Enumerable own keys for `Object.keys/values/entries`, respecting a Proxy
/// `ownKeys` trap and falling back to the target for proxies.
pub fn own_keys_of(obj: &JsValue) -> Vec<String> {
    match obj {
        JsValue::Proxy { target, handler } => {
            if let JsValue::Object(h_map) = handler.as_ref() {
                if let Some(trap) = h_map.get("ownKeys") {
                    if !matches!(trap, JsValue::NativeFunction(_)) {
                        let depth = PROXY_TRAP_DEPTH.with(|d| {
                            let cur = d.get();
                            if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                            d.set(cur + 1);
                            cur
                        });
                        if depth < MAX_PROXY_TRAP_DEPTH {
                            let result = call_function(trap, &[(**target).clone()], &Scope::new_global());
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            if let Ok(JsValue::Array(arr)) = result {
                                return arr.iter().map(to_string).collect();
                            }
                        }
                    }
                }
            }
            own_keys_of(target)
        }
        JsValue::Object(map) => {
            if map.get("__type__").map(to_string).as_deref() == Some("Proxy") {
                if let (Some(t), Some(JsValue::Object(h_map))) =
                    (map.get("__proxy_target__"), map.get("__proxy_handler__"))
                {
                    if let Some(trap) = h_map.get("ownKeys") {
                        if !matches!(trap, JsValue::NativeFunction(_)) {
                            let depth = PROXY_TRAP_DEPTH.with(|d| {
                                let cur = d.get();
                                if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                                d.set(cur + 1);
                                cur
                            });
                            if depth < MAX_PROXY_TRAP_DEPTH {
                                let result = call_function(trap, &[t.clone()], &Scope::new_global());
                                PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                                if let Ok(JsValue::Array(arr)) = result {
                                    return arr.iter().map(to_string).collect();
                                }
                            }
                        }
                    }
                    return own_keys_of(t);
                }
            }
            enumerable_keys(map)
        }
        JsValue::Array(arr) => (0..arr.len()).map(|i| i.to_string()).collect(),
        JsValue::String(s) => (0..s.chars().count()).map(|i| i.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// All own property names for `Reflect.ownKeys` / `Object.getOwnPropertyNames`.
/// Reports every non-internal own key regardless of enumerability (matching JS
/// semantics where these APIs ignore enumerability), consults a Proxy `ownKeys`
/// trap when present, and yields indices plus `length` for arrays.
pub fn own_property_names(obj: &JsValue) -> Vec<String> {
    match obj {
        // Proxy targets (native or object-based) consult the ownKeys trap.
        JsValue::Proxy { .. } => own_keys_of(obj),
        JsValue::Object(map) if map.get("__type__").map(to_string).as_deref() == Some("Proxy") => {
            own_keys_of(obj)
        }
        JsValue::Object(map) => map.keys().filter(|k| !is_internal_key(k)).cloned().collect(),
        JsValue::Array(arr) => {
            let mut keys: Vec<String> = (0..arr.len()).map(|i| i.to_string()).collect();
            keys.push("length".to_string());
            keys
        }
        _ => Vec::new(),
    }
}

pub fn get_property(obj: &JsValue, prop: &str) -> JsValue {
    match obj {
        JsValue::Object(map) => {
            // Proxy get trap: forward property access through handler
            if map.get("__type__").map(to_string).as_deref() == Some("Proxy") {
                let target = map.get("__proxy_target__");
                let handler = map.get("__proxy_handler__");
                if let (Some(t), Some(h)) = (target, handler) {
                    if let JsValue::Object(h_map) = h {
                        // Check for get trap in handler
                        if let Some(get_trap) = h_map.get("get") {
                            // Guard against infinite recursion
                            let depth = PROXY_TRAP_DEPTH.with(|d| {
                                let cur = d.get();
                                if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                                d.set(cur + 1);
                                cur
                            });
                            if depth >= MAX_PROXY_TRAP_DEPTH {
                                // Recursion limit: fall through to target
                                return get_property(t, prop);
                            }
                            // Invoke the trap: handler.get(target, prop)
                            let prop_val = JsValue::String(prop.to_string());
                            // For native function traps, just return the target property
                            if matches!(get_trap, JsValue::NativeFunction(_)) {
                                PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                                return get_property(t, prop);
                            }
                            let result = call_function(get_trap, &[t.clone(), prop_val], &Scope::new_global());
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            if let Ok(val) = result {
                                return val;
                            }
                        }
                        // Check for has trap (for "in" operator support)
                        if prop == "__has__" {
                            if let Some(has_trap) = h_map.get("has") {
                                if matches!(has_trap, JsValue::NativeFunction(_)) {
                                    return JsValue::Boolean(true);
                                }
                            }
                        }
                    }
                    // Fallback: forward to target
                    return get_property(t, prop);
                }
            }
            if let Some(val) = map.get(prop) { return resolve_accessor(val, obj); }
            // Computed properties for typed objects (Set.size, Map.size, etc.)
            if prop == "size" {
                match map.get("__type__").map(to_string).as_deref() {
                    Some("Set") | Some("WeakSet") => {
                        if let Some(JsValue::Array(items)) = map.get("__items__") { return JsValue::Number(items.len() as f64); }
                    }
                    Some("Map") | Some("WeakMap") => {
                        if let Some(JsValue::Array(entries)) = map.get("__entries__") { return JsValue::Number(entries.len() as f64); }
                    }
                    _ => {}
                }
            }
            // Storage.length computed property.
            if prop == "length" && map.get("__type__").map(to_string).as_deref() == Some("Storage") {
                return super::browser_env::storage_length(map);
            }
            // History computed properties (length, state).
            if map.get("__type__").map(to_string).as_deref() == Some("History") {
                match prop {
                    "length" => return super::web_platform::history_length(),
                    "state" => return super::web_platform::history_state(),
                    _ => {}
                }
            }
            // DOMTokenList.length computed property.
            if prop == "length" && map.get("__type__").map(to_string).as_deref() == Some("DOMTokenList") {
                return super::dom_bridge::dom_token_list_length(map);
            }
            // DOMStringMap (dataset) property access.
            if map.get("__type__").map(to_string).as_deref() == Some("DOMStringMap") {
                let val = super::dom_bridge::get_dataset_property(map, prop);
                if !matches!(val, JsValue::Undefined) { return val; }
            }
            // Document computed properties (body, head, documentElement).
            if map.get("__type__").map(to_string).as_deref() == Some("Document") {
                let doc_prop = super::dom_bridge::get_document_property(prop);
                if !matches!(doc_prop, JsValue::Undefined) { return doc_prop; }
            }
            // Element computed properties (textContent, innerHTML, children, etc.)
            if map.get("__type__").map(to_string).as_deref() == Some("Element") {
                return super::dom_bridge::get_element_property(map, prop);
            }
            // Walk __proto__ chain
            let mut proto = map.get("__proto__");
            while let Some(p) = proto {
                if let JsValue::Object(proto_map) = p {
                    if let Some(val) = proto_map.get(prop) { return resolve_accessor(val, obj); }
                    proto = proto_map.get("__proto__");
                } else { break; }
            }
            JsValue::Undefined
        }
        JsValue::Array(arr) => {
            if prop == "length" { return JsValue::Number(arr.len() as f64); }
            if let Ok(i) = prop.parse::<usize>() { return arr.get(i).cloned().unwrap_or(JsValue::Undefined); }
            JsValue::Undefined
        }
        JsValue::Proxy { target, handler } => {
            // Check if this proxy has been revoked (Proxy.revocable).
            if super::web_apis2::check_revoked(obj).is_err() { return JsValue::Undefined; }
            // Phase 7: Native Proxy variant — intercept property access via handler.get trap
            let depth = PROXY_TRAP_DEPTH.with(|d| {
                let cur = d.get();
                if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                d.set(cur + 1);
                cur
            });
            if depth >= MAX_PROXY_TRAP_DEPTH {
                return get_property(target, prop);
            }
            if let JsValue::Object(h_map) = handler.as_ref() {
                if let Some(get_trap) = h_map.get("get") {
                    if !matches!(get_trap, JsValue::NativeFunction(_)) {
                        let prop_val = JsValue::String(prop.to_string());
                        let result = call_function(get_trap, &[(**target).clone(), prop_val], &Scope::new_global());
                        PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                        if let Ok(val) = result {
                            return val;
                        }
                    }
                }
            }
            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
            get_property(target, prop)
        }
        JsValue::String(s) => {
            // Length counts Unicode scalar values, matching char-based indexing below.
            if prop == "length" { return JsValue::Number(s.chars().count() as f64); }
            if let Ok(i) = prop.parse::<usize>() { return s.chars().nth(i).map(|c| JsValue::String(c.to_string())).unwrap_or(JsValue::Undefined); }
            JsValue::Undefined
        }
        _ => JsValue::Undefined,
    }
}

/// Set a property on an object, respecting Proxy set traps.
pub fn set_property(obj: &mut JsValue, prop: &str, value: JsValue) -> bool {
    if let JsValue::Object(map) = obj {
        // Proxy set trap
        if map.get("__type__").map(to_string).as_deref() == Some("Proxy") {
            if let Some(JsValue::Object(h_map)) = map.get("__proxy_handler__") {
                if let Some(set_trap) = h_map.get("set") {
                    if let Some(target) = map.get("__proxy_target__").cloned() {
                        // Guard against infinite recursion
                        let depth_ok = PROXY_TRAP_DEPTH.with(|d| {
                            let cur = d.get();
                            if cur >= MAX_PROXY_TRAP_DEPTH { return false; }
                            d.set(cur + 1);
                            true
                        });
                        if depth_ok {
                            let prop_val = JsValue::String(prop.to_string());
                            let ok = call_function(set_trap, &[target, prop_val, value.clone()], &Scope::new_global()).is_ok();
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            if ok { return true; }
                        }
                    }
                }
            }
            // Forward to target
            if let Some(target) = map.get_mut("__proxy_target__") {
                return set_property(target, prop, value);
            }
        }
        // Accessor property: invoke the setter rather than overwriting the descriptor.
        // We snapshot the object, run the setter with `this` bound to that snapshot, then
        // merge any mutations the setter made to `this` back into the real object so that
        // `set x(v) { this._v = v; }` actually persists.
        let accessor_info = match map.get(prop) {
            Some(JsValue::Object(desc)) if desc.get("__accessor__") == Some(&JsValue::Boolean(true)) => {
                Some((desc.get("set").cloned(), desc.clone(), map.clone()))
            }
            _ => None,
        };
        if let Some((setter, descriptor, this_snapshot)) = accessor_info {
            if let Some(setter) = setter {
                if !matches!(setter, JsValue::NativeFunction(_)) {
                    let updated = invoke_setter_readback(&setter, &value, &this_snapshot);
                    for (k, v) in updated {
                        map.insert(k, v);
                    }
                    // Preserve the accessor descriptor itself (the setter must not clobber it).
                    map.insert(prop.to_string(), JsValue::Object(descriptor));
                }
            }
            return true;
        }
        // Document property setting (cookie, title).
        if map.get("__type__").map(to_string).as_deref() == Some("Document") {
            super::dom_bridge::set_document_property(prop, &value);
            return true;
        }
        // Element property setting (textContent, innerHTML, id, className, etc.)
        if map.get("__type__").map(to_string).as_deref() == Some("Element") {
            super::dom_bridge::set_element_property(map, prop, &value);
            map.insert(prop.to_string(), value);
            return true;
        }
        // DOMStringMap (dataset) property setting.
        if map.get("__type__").map(to_string).as_deref() == Some("DOMStringMap") {
            super::dom_bridge::set_dataset_property(map, prop, &value);
            return true;
        }
        // MessagePort.onmessage assignment: register in the shared handler registry.
        if prop == "onmessage" && map.get("__type__").map(to_string).as_deref() == Some("MessagePort") {
            super::web_apis2::register_port_handler(map, &value);
        }
        map.insert(prop.to_string(), value);
        true
    } else {
        false
    }
}

/// Invoke an accessor setter with `this` bound to a snapshot of the owning object, then
/// read back the (possibly mutated) `this` so the caller can merge the changes into the
/// real object. Returns the updated object map (or the snapshot unchanged if the setter
/// did not mutate `this`).
fn invoke_setter_readback(setter: &JsValue, value: &JsValue, this_map: &HashMap<String, JsValue>) -> HashMap<String, JsValue> {
    if let JsValue::Function { params, body, closure, .. } = setter {
        let call_scope = Scope::new_child(closure);
        Scope::declare(&call_scope, "this", JsValue::Object(this_map.clone()));
        for (i, p) in params.iter().enumerate() {
            let val = if i == 0 { value.clone() } else { JsValue::Undefined };
            Scope::declare(&call_scope, p, val);
        }
        Scope::declare(&call_scope, "arguments", JsValue::Array(vec![value.clone()]));
        let _ = eval_stmt(body, &call_scope);
        if let Some(JsValue::Object(updated)) = Scope::resolve(&call_scope, "this") {
            return updated;
        }
    }
    this_map.clone()
}

/// Apply a single property descriptor (data or accessor) to `target` under `prop`.
pub(super) fn apply_descriptor(target: &mut HashMap<String, JsValue>, prop: &str, desc: &HashMap<String, JsValue>) {
    if desc.contains_key("get") || desc.contains_key("set") {
        let mut accessor = HashMap::new();
        accessor.insert("__accessor__".to_string(), JsValue::Boolean(true));
        if let Some(g) = desc.get("get") { accessor.insert("get".to_string(), g.clone()); }
        if let Some(s) = desc.get("set") { accessor.insert("set".to_string(), s.clone()); }
        accessor.insert("enumerable".to_string(), desc.get("enumerable").cloned().unwrap_or(JsValue::Boolean(false)));
        accessor.insert("configurable".to_string(), desc.get("configurable").cloned().unwrap_or(JsValue::Boolean(false)));
        target.insert(prop.to_string(), JsValue::Object(accessor));
    } else {
        target.insert(prop.to_string(), desc.get("value").cloned().unwrap_or(JsValue::Undefined));
    }
}

/// Install a getter or setter coming from object-literal syntax (`{ get x() {}, set x(v) {} }`).
/// A getter and setter for the same key arrive as separate props, so we merge them into a
/// single `__accessor__` descriptor. Object-literal accessors are enumerable+configurable
/// by default (unlike Object.defineProperty, which defaults to false).
pub(super) fn install_literal_accessor(target: &mut HashMap<String, JsValue>, prop: &str, kind: &str, func: JsValue) {
    let mut accessor = match target.get(prop) {
        Some(JsValue::Object(existing)) if existing.get("__accessor__") == Some(&JsValue::Boolean(true)) => existing.clone(),
        _ => {
            let mut a = HashMap::new();
            a.insert("__accessor__".to_string(), JsValue::Boolean(true));
            a.insert("enumerable".to_string(), JsValue::Boolean(true));
            a.insert("configurable".to_string(), JsValue::Boolean(true));
            a
        }
    };
    accessor.insert(kind.to_string(), func);
    target.insert(prop.to_string(), JsValue::Object(accessor));
}

/// If `val` is an accessor property descriptor (installed via Object.defineProperty
/// with a `get` function), invoke the getter with `this` bound to `this_obj` and
/// return its result. Data values are returned unchanged.
fn resolve_accessor(val: &JsValue, this_obj: &JsValue) -> JsValue {
    if let JsValue::Object(desc) = val {
        if desc.get("__accessor__") == Some(&JsValue::Boolean(true)) {
            if let Some(getter) = desc.get("get") {
                if !matches!(getter, JsValue::NativeFunction(_)) {
                    if let Ok(result) = call_function_with_this(getter, &[], &Scope::new_global(), Some(this_obj.clone())) {
                        return result;
                    }
                }
            }
            return JsValue::Undefined;
        }
    }
    val.clone()
}

/// Internal bookkeeping keys are double-underscore delimited (`__type__`, `__proto__`,
/// `__instanceof__`, `__accessor__`, ...). They must never leak into user-visible
/// enumeration (`for...in`, `Object.keys/values/entries`). A key like `__foo` (no
/// trailing delimiter) is a legitimate user key and is NOT internal.
pub(super) fn is_internal_key(key: &str) -> bool {
    key.len() >= 4 && key.starts_with("__") && key.ends_with("__")
}

/// Whether an accessor descriptor should appear in enumeration. Data properties are
/// always enumerable; accessors honor their `enumerable` flag (default true for
/// object-literal accessors, false for Object.defineProperty unless set).
fn accessor_is_enumerable(desc: &HashMap<String, JsValue>) -> bool {
    match desc.get("enumerable") {
        Some(JsValue::Boolean(b)) => *b,
        _ => true,
    }
}

/// Enumerable own keys of an object: excludes internal `__x__` keys and non-enumerable
/// accessors. Order is unspecified (HashMap), matching the engine's existing semantics.
pub fn enumerable_keys(map: &HashMap<String, JsValue>) -> Vec<String> {
    map.iter()
        .filter(|(k, v)| {
            if is_internal_key(k) {
                return false;
            }
            if let JsValue::Object(desc) = v {
                if desc.get("__accessor__") == Some(&JsValue::Boolean(true)) {
                    return accessor_is_enumerable(desc);
                }
            }
            true
        })
        .map(|(k, _)| k.clone())
        .collect()
}
