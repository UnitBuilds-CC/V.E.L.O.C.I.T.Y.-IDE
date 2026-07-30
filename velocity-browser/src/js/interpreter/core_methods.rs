use super::signal::*;
use super::coercion::*;
use super::function::call_function;
use crate::js::scope::ScopeRef;
use crate::js::vm::JsValue;
use std::collections::HashMap;

#[allow(dead_code)]
pub(super) fn call_object_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "hasOwnProperty" => {
            let key = _args.first().map(to_string).unwrap_or_default();
            JsValue::Boolean(map.contains_key(&key))
        }
        "keys" => JsValue::Array(map.keys().map(|k| JsValue::String(k.clone())).collect()),
        "values" => JsValue::Array(map.values().cloned().collect()),
        "toString" | "toLocaleString" => JsValue::String("[object Object]".to_string()),
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_map_method(map: &mut HashMap<String, JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    let mut entries = if let Some(JsValue::Array(e)) = map.get("__entries__") { e.clone() } else { Vec::new() };
    Ok(match method {
        "get" => {
            let key = args.first().cloned().unwrap_or(JsValue::Undefined);
            let key_str = to_string(&key);
            entries.iter().find_map(|entry| {
                if let JsValue::Array(kv) = entry {
                    if kv.len() >= 2 && to_string(&kv[0]) == key_str { return Some(kv[1].clone()); }
                }
                None
            }).unwrap_or(JsValue::Undefined)
        }
        "has" => {
            let key = args.first().cloned().unwrap_or(JsValue::Undefined);
            let key_str = to_string(&key);
            JsValue::Boolean(entries.iter().any(|entry| {
                if let JsValue::Array(kv) = entry { kv.first().map(to_string).as_deref() == Some(&key_str) } else { false }
            }))
        }
        "set" => {
            let key = args.first().cloned().unwrap_or(JsValue::Undefined);
            let key_str = to_string(&key);
            let value = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            let mut replaced = false;
            for entry in entries.iter_mut() {
                if let JsValue::Array(kv) = entry {
                    if kv.first().map(to_string).as_deref() == Some(&key_str) {
                        if kv.len() >= 2 { kv[1] = value.clone(); } else { kv.push(value.clone()); }
                        replaced = true;
                        break;
                    }
                }
            }
            if !replaced { entries.push(JsValue::Array(vec![key, value])); }
            map.insert("__entries__".to_string(), JsValue::Array(entries));
            JsValue::Object(map.clone())
        }
        "delete" => {
            let key = args.first().cloned().unwrap_or(JsValue::Undefined);
            let key_str = to_string(&key);
            let before = entries.len();
            entries.retain(|entry| {
                if let JsValue::Array(kv) = entry { kv.first().map(to_string).as_deref() != Some(&key_str) } else { true }
            });
            let removed = entries.len() != before;
            map.insert("__entries__".to_string(), JsValue::Array(entries));
            JsValue::Boolean(removed)
        }
        "size" => JsValue::Number(entries.len() as f64),
        "keys" => JsValue::Array(entries.iter().filter_map(|e| if let JsValue::Array(kv) = e { kv.first().cloned() } else { None }).collect()),
        "values" => JsValue::Array(entries.iter().filter_map(|e| if let JsValue::Array(kv) = e { kv.get(1).cloned() } else { None }).collect()),
        "entries" => JsValue::Array(entries),
        "forEach" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            for entry in &entries {
                if let JsValue::Array(kv) = entry {
                    let key = kv.first().cloned().unwrap_or(JsValue::Undefined);
                    let value = kv.get(1).cloned().unwrap_or(JsValue::Undefined);
                    call_function(&callback, &[value, key], scope)?;
                }
            }
            JsValue::Undefined
        }
        "clear" => {
            map.insert("__entries__".to_string(), JsValue::Array(Vec::new()));
            JsValue::Undefined
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_set_method(map: &mut HashMap<String, JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    let mut items = if let Some(JsValue::Array(i)) = map.get("__items__") { i.clone() } else { Vec::new() };
    Ok(match method {
        "has" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::Boolean(items.iter().any(|x| strict_eq(x, &val)))
        }
        "add" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            if !items.iter().any(|x| strict_eq(x, &val)) { items.push(val); }
            map.insert("__items__".to_string(), JsValue::Array(items));
            JsValue::Object(map.clone())
        }
        "delete" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            let before = items.len();
            items.retain(|x| !strict_eq(x, &val));
            let removed = items.len() != before;
            map.insert("__items__".to_string(), JsValue::Array(items));
            JsValue::Boolean(removed)
        }
        "size" => JsValue::Number(items.len() as f64),
        "values" | "keys" => JsValue::Array(items),
        "forEach" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            for item in &items {
                call_function(&callback, &[item.clone(), item.clone()], scope)?;
            }
            JsValue::Undefined
        }
        "clear" => {
            map.insert("__items__".to_string(), JsValue::Array(Vec::new()));
            JsValue::Undefined
        }
        // ── ES2025 set operations ── each takes another Set (or array) argument.
        "union" => {
            let other = set_items_of(args.first());
            for v in other {
                if !items.iter().any(|x| strict_eq(x, &v)) { items.push(v); }
            }
            make_set(items)
        }
        "intersection" => {
            let other = set_items_of(args.first());
            items.retain(|x| other.iter().any(|y| strict_eq(x, y)));
            make_set(items)
        }
        "difference" => {
            let other = set_items_of(args.first());
            items.retain(|x| !other.iter().any(|y| strict_eq(x, y)));
            make_set(items)
        }
        "symmetricDifference" => {
            let other = set_items_of(args.first());
            let mut out: Vec<JsValue> = items.iter().filter(|x| !other.iter().any(|y| strict_eq(x, y))).cloned().collect();
            for v in other {
                if !items.iter().any(|x| strict_eq(x, &v)) { out.push(v); }
            }
            make_set(out)
        }
        "isSubsetOf" => {
            let other = set_items_of(args.first());
            JsValue::Boolean(items.iter().all(|x| other.iter().any(|y| strict_eq(x, y))))
        }
        "isSupersetOf" => {
            let other = set_items_of(args.first());
            JsValue::Boolean(other.iter().all(|y| items.iter().any(|x| strict_eq(x, y))))
        }
        "isDisjointFrom" => {
            let other = set_items_of(args.first());
            JsValue::Boolean(!items.iter().any(|x| other.iter().any(|y| strict_eq(x, y))))
        }
        _ => JsValue::Undefined,
    })
}

/// Extract the element list from a Set-like argument (Set object or array).
fn set_items_of(v: Option<&JsValue>) -> Vec<JsValue> {
    match v {
        Some(JsValue::Object(m)) => {
            if let Some(JsValue::Array(items)) = m.get("__items__") { items.clone() } else { Vec::new() }
        }
        Some(JsValue::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

/// Wrap an element list in a new Set object.
fn make_set(items: Vec<JsValue>) -> JsValue {
    let mut m = HashMap::new();
    m.insert("__type__".to_string(), JsValue::String("Set".to_string()));
    m.insert("__items__".to_string(), JsValue::Array(items));
    JsValue::Object(m)
}

pub(super) fn call_promise_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    match method {
        "then" => {
            if let Some(rejected) = map.get("__rejected__") {
                if *rejected != JsValue::Undefined {
                    let mut new_promise = HashMap::new();
                    new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                    new_promise.insert("__rejected__".to_string(), rejected.clone());
                    return Ok(JsValue::Object(new_promise));
                }
            }
            let resolved = map.get("__resolved__").cloned().unwrap_or(JsValue::Undefined);
            if let Some(callback) = args.first() {
                match call_function(callback, &[resolved], scope) {
                    Ok(result) => {
                        let mut new_promise = HashMap::new();
                        new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                        match &result {
                            JsValue::Object(inner_map) if inner_map.get("__type__").map(to_string).as_deref() == Some("Promise") => {
                                if let Some(rej) = inner_map.get("__rejected__") {
                                    if *rej != JsValue::Undefined {
                                        new_promise.insert("__rejected__".to_string(), rej.clone());
                                    } else {
                                        new_promise.insert("__resolved__".to_string(), inner_map.get("__resolved__").cloned().unwrap_or(JsValue::Undefined));
                                    }
                                } else {
                                    new_promise.insert("__resolved__".to_string(), inner_map.get("__resolved__").cloned().unwrap_or(JsValue::Undefined));
                                }
                            }
                            _ => {
                                new_promise.insert("__resolved__".to_string(), result);
                            }
                        }
                        Ok(JsValue::Object(new_promise))
                    }
                    Err(Signal::Throw(reason)) => {
                        let mut new_promise = HashMap::new();
                        new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                        new_promise.insert("__rejected__".to_string(), reason);
                        Ok(JsValue::Object(new_promise))
                    }
                    Err(other) => Err(other),
                }
            } else {
                Ok(JsValue::Object(map.clone()))
            }
        }
        "catch" => {
            if let Some(rejected) = map.get("__rejected__") {
                if *rejected != JsValue::Undefined {
                    if let Some(callback) = args.first() {
                        match call_function(callback, &[rejected.clone()], scope) {
                            Ok(result) => {
                                let mut new_promise = HashMap::new();
                                new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                                new_promise.insert("__resolved__".to_string(), result);
                                return Ok(JsValue::Object(new_promise));
                            }
                            Err(Signal::Throw(reason)) => {
                                let mut new_promise = HashMap::new();
                                new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                                new_promise.insert("__rejected__".to_string(), reason);
                                return Ok(JsValue::Object(new_promise));
                            }
                            Err(other) => return Err(other),
                        }
                    }
                }
            }
            Ok(JsValue::Object(map.clone()))
        }
        "finally" => {
            if let Some(callback) = args.first() {
                let _ = call_function(callback, &[], scope);
            }
            Ok(JsValue::Object(map.clone()))
        }
        _ => Ok(JsValue::Undefined),
    }
}

#[allow(dead_code)]
pub(super) fn call_date_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    let ts = if let Some(JsValue::Number(n)) = map.get("__value__") { *n } else { 0.0 };
    Ok(match method {
        "getTime" | "valueOf" => JsValue::Number(ts),
        "toISOString" | "toJSON" => JsValue::String("1970-01-01T00:00:00.000Z".to_string()),
        "toString" => JsValue::String(format!("Date({})", ts)),
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_generator_method(map: &HashMap<String, JsValue>, method: &str) -> EvalResult {
    match method {
        "next" => {
            let values = match map.get("__values__") {
                Some(JsValue::Array(arr)) => arr.clone(),
                _ => Vec::new(),
            };
            let index = match map.get("__index__") {
                Some(JsValue::Number(n)) => *n as usize,
                _ => 0,
            };
            if index < values.len() {
                let value = values[index].clone();
                let mut result = HashMap::new();
                result.insert("value".to_string(), value);
                result.insert("done".to_string(), JsValue::Boolean(false));
                Ok(JsValue::Object(result))
            } else {
                let mut result = HashMap::new();
                result.insert("value".to_string(), JsValue::Undefined);
                result.insert("done".to_string(), JsValue::Boolean(true));
                Ok(JsValue::Object(result))
            }
        }
        "return" => {
            let mut result = HashMap::new();
            result.insert("value".to_string(), JsValue::Undefined);
            result.insert("done".to_string(), JsValue::Boolean(true));
            Ok(JsValue::Object(result))
        }
        _ => Ok(JsValue::Undefined),
    }
}

// ── Enhanced Date methods ────────────────────────────────────────────────────

pub(super) fn call_date_method_enhanced(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    let ts = if let Some(JsValue::Number(n)) = map.get("__value__") { *n } else { 0.0 };
    
    // Decompose epoch millis into date components
    let secs = (ts / 1000.0).floor() as i64;
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;
    let millis = (ts % 1000.0).floor() as i64;
    
    // Calculate year, month, day from days since 1970-01-01
    let mut y = 1970;
    let mut remaining_days = days;
    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year { break; }
        remaining_days -= days_in_year;
        y += 1;
    }
    let month_days = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md { m = i; break; }
        remaining_days -= md;
    }
    let d = remaining_days + 1;
    let dow = ((days % 7) + 4) % 7; // Jan 1 1970 = Thursday (4)
    
    Ok(match method {
        "getTime" | "valueOf" => JsValue::Number(ts),
        "getFullYear" => JsValue::Number(y as f64),
        "getMonth" => JsValue::Number(m as f64),
        "getDate" => JsValue::Number(d as f64),
        "getDay" => JsValue::Number(dow as f64),
        "getHours" => JsValue::Number(hours as f64),
        "getMinutes" => JsValue::Number(minutes as f64),
        "getSeconds" => JsValue::Number(seconds as f64),
        "getMilliseconds" => JsValue::Number(millis as f64),
        "getTimezoneOffset" => JsValue::Number(0.0), // UTC
        "toISOString" | "toJSON" => {
            JsValue::String(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                y, m + 1, d, hours, minutes, seconds, millis))
        }
        "toDateString" => {
            let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            JsValue::String(format!("{} {} {:02} {}", day_names[dow as usize], month_names[m], d, y))
        }
        "toTimeString" => {
            JsValue::String(format!("{:02}:{:02}:{:02} GMT+0000 (UTC)", hours, minutes, seconds))
        }
        "toUTCString" => {
            let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            JsValue::String(format!("{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT", day_names[dow as usize], d, month_names[m], y, hours, minutes, seconds))
        }
        "toString" => JsValue::String(format!("Date({})", ts)),
        _ => JsValue::Undefined,
    })
}

pub(super) fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

// ── Object.prototype enhanced methods ────────────────────────────────────────

pub(super) fn call_object_method_enhanced(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "hasOwnProperty" => {
            let key = _args.first().map(to_string).unwrap_or_default();
            JsValue::Boolean(map.contains_key(&key))
        }
        "valueOf" => JsValue::Object(map.clone()),
        "isPrototypeOf" => {
            let obj = _args.first().cloned().unwrap_or(JsValue::Undefined);
            if let JsValue::Object(obj_map) = obj {
                let mut proto = obj_map.get("__proto__").cloned();
                let mut depth = 0;
                while let Some(p) = proto {
                    if depth > 64 { break; }
                    if let JsValue::Object(pm) = &p {
                        if std::ptr::eq(pm, map) { return Ok(JsValue::Boolean(true)); }
                        proto = pm.get("__proto__").cloned();
                    } else {
                        break;
                    }
                    depth += 1;
                }
            }
            JsValue::Boolean(false)
        }
        "propertyIsEnumerable" => {
            let key = _args.first().map(to_string).unwrap_or_default();
            JsValue::Boolean(map.contains_key(&key) && !key.starts_with("__"))
        }
        "keys" => JsValue::Array(map.keys().map(|k| JsValue::String(k.clone())).collect()),
        "values" => JsValue::Array(map.values().cloned().collect()),
        "toString" | "toLocaleString" => JsValue::String("[object Object]".to_string()),
        _ => JsValue::Undefined,
    })
}

// ── Boolean methods ──────────────────────────────────────────────────────────

pub(super) fn call_boolean_method(b: bool, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "toString" => JsValue::String(b.to_string()),
        "valueOf" => JsValue::Boolean(b),
        _ => JsValue::Undefined,
    })
}

// ── NativeFunction methods ───────────────────────────────────────────────────

pub(super) fn call_native_function_method(name: &str, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "toString" => JsValue::String(format!("function {}() {{ [native code] }}", name)),
        "name" => JsValue::String(name.to_string()),
        "call" | "apply" | "bind" => JsValue::Undefined, // These are handled in method_dispatch
        _ => JsValue::Undefined,
    })
}

// ── Newer static builtins (ES2023-ES2025) ─────────────────────────────

/// Build a settled Promise object (`resolved == false` means rejected).
pub(super) fn make_promise(rejected: bool, val: JsValue) -> JsValue {
    let mut m = HashMap::new();
    m.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
    m.insert(if rejected { "__rejected__" } else { "__resolved__" }.to_string(), val);
    JsValue::Object(m)
}

/// Unwrap a (possibly nested) settled promise like `await` does: resolved
/// values pass through, a rejection is re-thrown.
pub(super) fn await_value(mut val: JsValue) -> EvalResult {
    let mut depth = 0;
    while depth < 32 {
        match &val {
            JsValue::Object(map) if map.get("__type__").map(to_string).as_deref() == Some("Promise") => {
                if let Some(reason) = map.get("__rejected__") {
                    if *reason != JsValue::Undefined {
                        return Err(Signal::Throw(reason.clone()));
                    }
                }
                val = map.get("__resolved__").cloned().unwrap_or(JsValue::Undefined);
                depth += 1;
            }
            _ => break,
        }
    }
    Ok(val)
}

/// Promise.withResolvers() → { promise, resolve, reject }. Pragmatic under
/// this synchronous promise model: the promise starts pre-resolved with
/// undefined and the returned resolve/reject reuse the executor capture
/// natives (they cannot retroactively settle the already-created promise).
pub(super) fn promise_with_resolvers() -> JsValue {
    let mut out = HashMap::new();
    out.insert("promise".to_string(), make_promise(false, JsValue::Undefined));
    out.insert("resolve".to_string(), JsValue::NativeFunction("__promise_resolve__".to_string()));
    out.insert("reject".to_string(), JsValue::NativeFunction("__promise_reject__".to_string()));
    JsValue::Object(out)
}

/// Promise.try(fn, ...args): call fn synchronously; the result (or thrown
/// error) becomes a resolved (or rejected) promise.
pub(super) fn promise_try(args: &[JsValue]) -> EvalResult {
    let f = args.first().cloned().unwrap_or(JsValue::Undefined);
    let call_args = if args.len() > 1 { &args[1..] } else { &[] };
    match call_function(&f, call_args, &crate::js::scope::Scope::new_global()) {
        Ok(v) => Ok(await_value(v).map_or_else(
            |sig| match sig { Signal::Throw(reason) => make_promise(true, reason), _ => make_promise(false, JsValue::Undefined) },
            |v| make_promise(false, v),
        )),
        Err(Signal::Throw(reason)) => Ok(make_promise(true, reason)),
        Err(other) => Err(other),
    }
}

/// Shared grouping loop: returns (key, group) pairs preserving first-seen order.
fn group_pairs(args: &[JsValue]) -> Result<Vec<(String, Vec<JsValue>)>, Signal> {
    let items = match args.first() {
        Some(JsValue::Array(a)) => a.clone(),
        Some(other) => set_items_of(Some(other)),
        None => Vec::new(),
    };
    let callback = args.get(1).cloned().unwrap_or(JsValue::Undefined);
    let scope = crate::js::scope::Scope::new_global();
    let mut groups: Vec<(String, Vec<JsValue>)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let key = to_string(&call_function(&callback, &[item.clone(), JsValue::Number(i as f64)], &scope)?);
        if let Some((_, g)) = groups.iter_mut().find(|(k, _)| *k == key) {
            g.push(item.clone());
        } else {
            groups.push((key, vec![item.clone()]));
        }
    }
    Ok(groups)
}

/// Object.groupBy(items, cb) → plain object of arrays.
pub(super) fn object_group_by(args: &[JsValue]) -> EvalResult {
    let mut out = HashMap::new();
    for (key, group) in group_pairs(args)? {
        out.insert(key, JsValue::Array(group));
    }
    Ok(JsValue::Object(out))
}

/// Map.groupBy(items, cb) → Map of arrays.
pub(super) fn map_group_by(args: &[JsValue]) -> EvalResult {
    let entries: Vec<JsValue> = group_pairs(args)?
        .into_iter()
        .map(|(key, group)| JsValue::Array(vec![JsValue::String(key), JsValue::Array(group)]))
        .collect();
    let mut m = HashMap::new();
    m.insert("__type__".to_string(), JsValue::String("Map".to_string()));
    m.insert("__entries__".to_string(), JsValue::Array(entries));
    Ok(JsValue::Object(m))
}

/// Array.fromAsync(iterable[, mapFn]) — synchronous model: like Array.from
/// but each element is awaited (settled promises unwrap; a rejection rejects
/// the returned promise). Returns a resolved promise wrapping the array.
pub(super) fn array_from_async(args: &[JsValue]) -> EvalResult {
    let items = match args.first() {
        Some(JsValue::Array(a)) => a.clone(),
        Some(JsValue::String(s)) => s.chars().map(|c| JsValue::String(c.to_string())).collect(),
        Some(other) => set_items_of(Some(other)),
        None => Vec::new(),
    };
    let map_fn = args.get(1).cloned();
    let scope = crate::js::scope::Scope::new_global();
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        let v = match await_value(item) {
            Ok(v) => v,
            Err(Signal::Throw(reason)) => return Ok(make_promise(true, reason)),
            Err(other) => return Err(other),
        };
        let v = match &map_fn {
            Some(f) if !matches!(f, JsValue::Undefined) => call_function(f, &[v, JsValue::Number(i as f64)], &scope)?,
            _ => v,
        };
        out.push(v);
    }
    Ok(make_promise(false, JsValue::Array(out)))
}

/// Error.isError(v) — pragmatic: errors in this engine are plain objects with
/// a `message` key and a `name` string ending in "Error".
pub(super) fn error_is_error(v: Option<&JsValue>) -> JsValue {
    let is_err = matches!(v, Some(JsValue::Object(m))
        if m.contains_key("message")
            && matches!(m.get("name"), Some(JsValue::String(n)) if n.ends_with("Error") || n == "Error"));
    JsValue::Boolean(is_err)
}

/// Object.getOwnPropertySymbols(obj) — symbols are modeled as string keys of
/// the form `__symbol_{desc}_{id}__` (see the Symbol() native), so we return
/// exactly those keys.
pub(super) fn get_own_property_symbols(v: Option<&JsValue>) -> JsValue {
    match v {
        Some(JsValue::Object(m)) => JsValue::Array(
            m.keys()
                .filter(|k| k.starts_with("__symbol_"))
                .map(|k| JsValue::String(k.clone()))
                .collect(),
        ),
        _ => JsValue::Array(Vec::new()),
    }
}

/// ArrayBuffer.isView(v) — true for TypedArray and DataView typed objects.
pub(super) fn is_array_buffer_view(v: Option<&JsValue>) -> bool {
    matches!(v, Some(JsValue::Object(m)) if matches!(
        m.get("__type__").map(to_string).as_deref(),
        Some("Uint8Array" | "Int8Array" | "Uint16Array" | "Int16Array" | "Uint32Array"
            | "Int32Array" | "Float32Array" | "Float64Array" | "Uint8ClampedArray" | "DataView")
    ))
}
