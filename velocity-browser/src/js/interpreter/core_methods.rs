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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::scope::Scope;

    fn dummy_scope() -> ScopeRef { Scope::new_global() }

    fn make_map_obj(entries: Vec<(&str, JsValue)>) -> HashMap<String, JsValue> {
        entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    fn make_set_obj(items: Vec<JsValue>) -> HashMap<String, JsValue> {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("Set".to_string()));
        m.insert("__items__".to_string(), JsValue::Array(items));
        m
    }

    fn make_map_with_entries(kvs: Vec<(JsValue, JsValue)>) -> HashMap<String, JsValue> {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("Map".to_string()));
        let entries: Vec<JsValue> = kvs.into_iter()
            .map(|(k, v)| JsValue::Array(vec![k, v]))
            .collect();
        m.insert("__entries__".to_string(), JsValue::Array(entries));
        m
    }

    fn make_date(ts: f64) -> HashMap<String, JsValue> {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("Date".to_string()));
        m.insert("__value__".to_string(), JsValue::Number(ts));
        m
    }

    fn make_generator(values: Vec<JsValue>, index: f64) -> HashMap<String, JsValue> {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("Generator".to_string()));
        m.insert("__values__".to_string(), JsValue::Array(values));
        m.insert("__index__".to_string(), JsValue::Number(index));
        m
    }

    // ── call_object_method ─────────────────────────────────────────────

    #[test]
    fn object_has_own_property_true() {
        let m = make_map_obj(vec![("foo", JsValue::Number(1.0))]);
        let r = call_object_method(&m, "hasOwnProperty", &[JsValue::String("foo".into())]).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
    }

    #[test]
    fn object_has_own_property_false() {
        let m = make_map_obj(vec![("foo", JsValue::Number(1.0))]);
        let r = call_object_method(&m, "hasOwnProperty", &[JsValue::String("bar".into())]).unwrap();
        assert_eq!(r, JsValue::Boolean(false));
    }

    #[test]
    fn object_keys() {
        let m = make_map_obj(vec![("a", JsValue::Number(1.0)), ("b", JsValue::Number(2.0))]);
        let r = call_object_method(&m, "keys", &[]).unwrap();
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 2);
            let mut keys: Vec<String> = Vec::new();
            for v in &arr { keys.push(to_string(v)); }
            keys.sort();
            assert_eq!(keys, vec!["a".to_string(), "b".to_string()]);
        } else { panic!("expected Array"); }
    }

    #[test]
    fn object_values() {
        let m = make_map_obj(vec![("x", JsValue::Number(42.0))]);
        let r = call_object_method(&m, "values", &[]).unwrap();
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0], JsValue::Number(42.0));
        } else { panic!("expected Array"); }
    }

    #[test]
    fn object_to_string() {
        let m = make_map_obj(vec![]);
        let r = call_object_method(&m, "toString", &[]).unwrap();
        assert_eq!(r, JsValue::String("[object Object]".into()));
    }

    #[test]
    fn object_to_locale_string() {
        let m = make_map_obj(vec![]);
        let r = call_object_method(&m, "toLocaleString", &[]).unwrap();
        assert_eq!(r, JsValue::String("[object Object]".into()));
    }

    #[test]
    fn object_unknown_method() {
        let m = make_map_obj(vec![]);
        let r = call_object_method(&m, "nope", &[]).unwrap();
        assert_eq!(r, JsValue::Undefined);
    }

    // ── call_map_method ────────────────────────────────────────────────

    #[test]
    fn map_get_existing() {
        let mut m = make_map_with_entries(vec![
            (JsValue::String("a".into()), JsValue::Number(10.0)),
        ]);
        let sc = dummy_scope();
        let r = call_map_method(&mut m, "get", &[JsValue::String("a".into())], &sc).unwrap();
        assert_eq!(r, JsValue::Number(10.0));
    }

    #[test]
    fn map_get_missing() {
        let mut m = make_map_with_entries(vec![]);
        let sc = dummy_scope();
        let r = call_map_method(&mut m, "get", &[JsValue::String("z".into())], &sc).unwrap();
        assert_eq!(r, JsValue::Undefined);
    }

    #[test]
    fn map_has_true() {
        let mut m = make_map_with_entries(vec![
            (JsValue::String("key".into()), JsValue::Boolean(true)),
        ]);
        let sc = dummy_scope();
        let r = call_map_method(&mut m, "has", &[JsValue::String("key".into())], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
    }

    #[test]
    fn map_has_false() {
        let mut m = make_map_with_entries(vec![]);
        let sc = dummy_scope();
        let r = call_map_method(&mut m, "has", &[JsValue::String("nope".into())], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(false));
    }

    #[test]
    fn map_set_new_entry() {
        let mut m = make_map_with_entries(vec![]);
        let sc = dummy_scope();
        let _ = call_map_method(&mut m, "set", &[JsValue::String("k".into()), JsValue::Number(5.0)], &sc).unwrap();
        // Verify via get
        let r = call_map_method(&mut m, "get", &[JsValue::String("k".into())], &sc).unwrap();
        assert_eq!(r, JsValue::Number(5.0));
    }

    #[test]
    fn map_set_replace_existing() {
        let mut m = make_map_with_entries(vec![
            (JsValue::String("k".into()), JsValue::Number(1.0)),
        ]);
        let sc = dummy_scope();
        let _ = call_map_method(&mut m, "set", &[JsValue::String("k".into()), JsValue::Number(99.0)], &sc).unwrap();
        let r = call_map_method(&mut m, "get", &[JsValue::String("k".into())], &sc).unwrap();
        assert_eq!(r, JsValue::Number(99.0));
        // Size should still be 1
        let sz = call_map_method(&mut m, "size", &[], &sc).unwrap();
        assert_eq!(sz, JsValue::Number(1.0));
    }

    #[test]
    fn map_delete_existing() {
        let mut m = make_map_with_entries(vec![
            (JsValue::String("a".into()), JsValue::Number(1.0)),
            (JsValue::String("b".into()), JsValue::Number(2.0)),
        ]);
        let sc = dummy_scope();
        let r = call_map_method(&mut m, "delete", &[JsValue::String("a".into())], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
        let sz = call_map_method(&mut m, "size", &[], &sc).unwrap();
        assert_eq!(sz, JsValue::Number(1.0));
    }

    #[test]
    fn map_delete_missing() {
        let mut m = make_map_with_entries(vec![
            (JsValue::String("a".into()), JsValue::Number(1.0)),
        ]);
        let sc = dummy_scope();
        let r = call_map_method(&mut m, "delete", &[JsValue::String("z".into())], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(false));
    }

    #[test]
    fn map_size() {
        let mut m = make_map_with_entries(vec![
            (JsValue::String("a".into()), JsValue::Number(1.0)),
            (JsValue::String("b".into()), JsValue::Number(2.0)),
            (JsValue::String("c".into()), JsValue::Number(3.0)),
        ]);
        let sc = dummy_scope();
        let r = call_map_method(&mut m, "size", &[], &sc).unwrap();
        assert_eq!(r, JsValue::Number(3.0));
    }

    #[test]
    fn map_keys() {
        let mut m = make_map_with_entries(vec![
            (JsValue::String("x".into()), JsValue::Number(1.0)),
            (JsValue::String("y".into()), JsValue::Number(2.0)),
        ]);
        let sc = dummy_scope();
        let r = call_map_method(&mut m, "keys", &[], &sc).unwrap();
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 2);
        } else { panic!("expected Array"); }
    }

    #[test]
    fn map_values() {
        let mut m = make_map_with_entries(vec![
            (JsValue::String("x".into()), JsValue::Number(10.0)),
        ]);
        let sc = dummy_scope();
        let r = call_map_method(&mut m, "values", &[], &sc).unwrap();
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0], JsValue::Number(10.0));
        } else { panic!("expected Array"); }
    }

    #[test]
    fn map_entries() {
        let mut m = make_map_with_entries(vec![
            (JsValue::String("a".into()), JsValue::Number(1.0)),
        ]);
        let sc = dummy_scope();
        let r = call_map_method(&mut m, "entries", &[], &sc).unwrap();
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 1);
            if let JsValue::Array(kv) = &arr[0] {
                assert_eq!(kv.len(), 2);
            } else { panic!("expected inner Array"); }
        } else { panic!("expected Array"); }
    }

    #[test]
    fn map_clear() {
        let mut m = make_map_with_entries(vec![
            (JsValue::String("a".into()), JsValue::Number(1.0)),
            (JsValue::String("b".into()), JsValue::Number(2.0)),
        ]);
        let sc = dummy_scope();
        let _ = call_map_method(&mut m, "clear", &[], &sc).unwrap();
        let sz = call_map_method(&mut m, "size", &[], &sc).unwrap();
        assert_eq!(sz, JsValue::Number(0.0));
    }

    // ── call_set_method ────────────────────────────────────────────────

    #[test]
    fn set_has_existing() {
        let mut s = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s, "has", &[JsValue::Number(1.0)], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
    }

    #[test]
    fn set_has_missing() {
        let mut s = make_set_obj(vec![JsValue::Number(1.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s, "has", &[JsValue::Number(99.0)], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(false));
    }

    #[test]
    fn set_add_new() {
        let mut s = make_set_obj(vec![]);
        let sc = dummy_scope();
        let _ = call_set_method(&mut s, "add", &[JsValue::String("hello".into())], &sc).unwrap();
        let r = call_set_method(&mut s, "has", &[JsValue::String("hello".into())], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
    }

    #[test]
    fn set_add_duplicate_ignored() {
        let mut s = make_set_obj(vec![JsValue::Number(1.0)]);
        let sc = dummy_scope();
        let _ = call_set_method(&mut s, "add", &[JsValue::Number(1.0)], &sc).unwrap();
        let sz = call_set_method(&mut s, "size", &[], &sc).unwrap();
        assert_eq!(sz, JsValue::Number(1.0));
    }

    #[test]
    fn set_delete_existing() {
        let mut s = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s, "delete", &[JsValue::Number(1.0)], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
        let sz = call_set_method(&mut s, "size", &[], &sc).unwrap();
        assert_eq!(sz, JsValue::Number(1.0));
    }

    #[test]
    fn set_delete_missing() {
        let mut s = make_set_obj(vec![JsValue::Number(1.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s, "delete", &[JsValue::Number(99.0)], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(false));
    }

    #[test]
    fn set_size() {
        let mut s = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0), JsValue::Number(3.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s, "size", &[], &sc).unwrap();
        assert_eq!(r, JsValue::Number(3.0));
    }

    #[test]
    fn set_values() {
        let mut s = make_set_obj(vec![JsValue::String("a".into()), JsValue::String("b".into())]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s, "values", &[], &sc).unwrap();
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 2);
        } else { panic!("expected Array"); }
    }

    #[test]
    fn set_clear() {
        let mut s = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0)]);
        let sc = dummy_scope();
        let _ = call_set_method(&mut s, "clear", &[], &sc).unwrap();
        let sz = call_set_method(&mut s, "size", &[], &sc).unwrap();
        assert_eq!(sz, JsValue::Number(0.0));
    }

    #[test]
    fn set_union() {
        let mut s1 = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0)]);
        let s2 = make_set_obj(vec![JsValue::Number(2.0), JsValue::Number(3.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s1, "union", &[JsValue::Object(s2)], &sc).unwrap();
        if let JsValue::Object(m) = r {
            let items = m.get("__items__").unwrap();
            if let JsValue::Array(arr) = items {
                assert_eq!(arr.len(), 3); // 1, 2, 3
            } else { panic!("expected Array"); }
        } else { panic!("expected Object"); }
    }

    #[test]
    fn set_intersection() {
        let mut s1 = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0), JsValue::Number(3.0)]);
        let s2 = make_set_obj(vec![JsValue::Number(2.0), JsValue::Number(3.0), JsValue::Number(4.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s1, "intersection", &[JsValue::Object(s2)], &sc).unwrap();
        if let JsValue::Object(m) = r {
            if let JsValue::Array(arr) = m.get("__items__").unwrap() {
                assert_eq!(arr.len(), 2); // 2, 3
            } else { panic!("expected Array"); }
        } else { panic!("expected Object"); }
    }

    #[test]
    fn set_difference() {
        let mut s1 = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0), JsValue::Number(3.0)]);
        let s2 = make_set_obj(vec![JsValue::Number(2.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s1, "difference", &[JsValue::Object(s2)], &sc).unwrap();
        if let JsValue::Object(m) = r {
            if let JsValue::Array(arr) = m.get("__items__").unwrap() {
                assert_eq!(arr.len(), 2); // 1, 3
            } else { panic!("expected Array"); }
        } else { panic!("expected Object"); }
    }

    #[test]
    fn set_symmetric_difference() {
        let mut s1 = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0)]);
        let s2 = make_set_obj(vec![JsValue::Number(2.0), JsValue::Number(3.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s1, "symmetricDifference", &[JsValue::Object(s2)], &sc).unwrap();
        if let JsValue::Object(m) = r {
            if let JsValue::Array(arr) = m.get("__items__").unwrap() {
                assert_eq!(arr.len(), 2); // 1, 3
            } else { panic!("expected Array"); }
        } else { panic!("expected Object"); }
    }

    #[test]
    fn set_is_subset_of_true() {
        let mut s1 = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0)]);
        let s2 = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0), JsValue::Number(3.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s1, "isSubsetOf", &[JsValue::Object(s2)], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
    }

    #[test]
    fn set_is_subset_of_false() {
        let mut s1 = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(4.0)]);
        let s2 = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s1, "isSubsetOf", &[JsValue::Object(s2)], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(false));
    }

    #[test]
    fn set_is_superset_of_true() {
        let mut s1 = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0), JsValue::Number(3.0)]);
        let s2 = make_set_obj(vec![JsValue::Number(1.0), JsValue::Number(2.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s1, "isSupersetOf", &[JsValue::Object(s2)], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
    }

    #[test]
    fn set_is_disjoint_from_true() {
        let mut s1 = make_set_obj(vec![JsValue::Number(1.0)]);
        let s2 = make_set_obj(vec![JsValue::Number(2.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s1, "isDisjointFrom", &[JsValue::Object(s2)], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
    }

    #[test]
    fn set_is_disjoint_from_false() {
        let mut s1 = make_set_obj(vec![JsValue::Number(1.0)]);
        let s2 = make_set_obj(vec![JsValue::Number(1.0)]);
        let sc = dummy_scope();
        let r = call_set_method(&mut s1, "isDisjointFrom", &[JsValue::Object(s2)], &sc).unwrap();
        assert_eq!(r, JsValue::Boolean(false));
    }

    // ── call_date_method (basic) ───────────────────────────────────────

    #[test]
    fn date_get_time() {
        let d = make_date(1000.0);
        let r = call_date_method(&d, "getTime", &[]).unwrap();
        assert_eq!(r, JsValue::Number(1000.0));
    }

    #[test]
    fn date_value_of() {
        let d = make_date(500.0);
        let r = call_date_method(&d, "valueOf", &[]).unwrap();
        assert_eq!(r, JsValue::Number(500.0));
    }

    #[test]
    fn date_to_iso_string() {
        let d = make_date(0.0);
        let r = call_date_method(&d, "toISOString", &[]).unwrap();
        assert_eq!(r, JsValue::String("1970-01-01T00:00:00.000Z".into()));
    }

    #[test]
    fn date_to_string_basic() {
        let d = make_date(0.0);
        let r = call_date_method(&d, "toString", &[]).unwrap();
        assert_eq!(r, JsValue::String("Date(0)".into()));
    }

    #[test]
    fn date_unknown_method() {
        let d = make_date(0.0);
        let r = call_date_method(&d, "nope", &[]).unwrap();
        assert_eq!(r, JsValue::Undefined);
    }

    // ── call_generator_method ──────────────────────────────────────────

    #[test]
    fn generator_next_first_value() {
        let g = make_generator(vec![JsValue::Number(10.0), JsValue::Number(20.0)], 0.0);
        let r = call_generator_method(&g, "next").unwrap();
        if let JsValue::Object(m) = r {
            assert_eq!(m.get("value").unwrap(), &JsValue::Number(10.0));
            assert_eq!(m.get("done").unwrap(), &JsValue::Boolean(false));
        } else { panic!("expected Object"); }
    }

    #[test]
    fn generator_next_second_value() {
        let g = make_generator(vec![JsValue::Number(10.0), JsValue::Number(20.0)], 1.0);
        let r = call_generator_method(&g, "next").unwrap();
        if let JsValue::Object(m) = r {
            assert_eq!(m.get("value").unwrap(), &JsValue::Number(20.0));
            assert_eq!(m.get("done").unwrap(), &JsValue::Boolean(false));
        } else { panic!("expected Object"); }
    }

    #[test]
    fn generator_next_past_end() {
        let g = make_generator(vec![JsValue::Number(10.0)], 1.0);
        let r = call_generator_method(&g, "next").unwrap();
        if let JsValue::Object(m) = r {
            assert_eq!(m.get("value").unwrap(), &JsValue::Undefined);
            assert_eq!(m.get("done").unwrap(), &JsValue::Boolean(true));
        } else { panic!("expected Object"); }
    }

    #[test]
    fn generator_return() {
        let g = make_generator(vec![], 0.0);
        let r = call_generator_method(&g, "return").unwrap();
        if let JsValue::Object(m) = r {
            assert_eq!(m.get("value").unwrap(), &JsValue::Undefined);
            assert_eq!(m.get("done").unwrap(), &JsValue::Boolean(true));
        } else { panic!("expected Object"); }
    }

    #[test]
    fn generator_empty_values() {
        let g = make_generator(vec![], 0.0);
        let r = call_generator_method(&g, "next").unwrap();
        if let JsValue::Object(m) = r {
            assert_eq!(m.get("done").unwrap(), &JsValue::Boolean(true));
        } else { panic!("expected Object"); }
    }

    // ── is_leap_year ───────────────────────────────────────────────────

    #[test]
    fn leap_year_divisible_by_4_not_100() {
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2000));
    }

    #[test]
    fn leap_year_divisible_by_100_not_400() {
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2100));
    }

    #[test]
    fn leap_year_non_divisible() {
        assert!(!is_leap_year(2023));
        assert!(!is_leap_year(2025));
    }

    #[test]
    fn leap_year_divisible_by_400() {
        assert!(is_leap_year(1600));
        assert!(is_leap_year(2400));
    }

    // ── call_date_method_enhanced ──────────────────────────────────────

    #[test]
    fn date_enhanced_epoch_zero() {
        let d = make_date(0.0);
        assert_eq!(call_date_method_enhanced(&d, "getFullYear", &[]).unwrap(), JsValue::Number(1970.0));
        assert_eq!(call_date_method_enhanced(&d, "getMonth", &[]).unwrap(), JsValue::Number(0.0));
        assert_eq!(call_date_method_enhanced(&d, "getDate", &[]).unwrap(), JsValue::Number(1.0));
        assert_eq!(call_date_method_enhanced(&d, "getHours", &[]).unwrap(), JsValue::Number(0.0));
        assert_eq!(call_date_method_enhanced(&d, "getMinutes", &[]).unwrap(), JsValue::Number(0.0));
        assert_eq!(call_date_method_enhanced(&d, "getSeconds", &[]).unwrap(), JsValue::Number(0.0));
    }

    #[test]
    fn date_enhanced_known_timestamp() {
        // 2000-01-01T00:00:00.000Z = 946684800000
        let d = make_date(946684800000.0);
        assert_eq!(call_date_method_enhanced(&d, "getFullYear", &[]).unwrap(), JsValue::Number(2000.0));
        assert_eq!(call_date_method_enhanced(&d, "getMonth", &[]).unwrap(), JsValue::Number(0.0));
        assert_eq!(call_date_method_enhanced(&d, "getDate", &[]).unwrap(), JsValue::Number(1.0));
    }

    #[test]
    fn date_enhanced_to_iso_string() {
        let d = make_date(0.0);
        let r = call_date_method_enhanced(&d, "toISOString", &[]).unwrap();
        assert_eq!(r, JsValue::String("1970-01-01T00:00:00.000Z".into()));
    }

    #[test]
    fn date_enhanced_to_date_string() {
        let d = make_date(0.0);
        let r = call_date_method_enhanced(&d, "toDateString", &[]).unwrap();
        // Jan 1 1970 was Thursday
        assert_eq!(r, JsValue::String("Thu Jan 01 1970".into()));
    }

    #[test]
    fn date_enhanced_to_time_string() {
        let d = make_date(0.0);
        let r = call_date_method_enhanced(&d, "toTimeString", &[]).unwrap();
        assert_eq!(r, JsValue::String("00:00:00 GMT+0000 (UTC)".into()));
    }

    #[test]
    fn date_enhanced_to_utc_string() {
        let d = make_date(0.0);
        let r = call_date_method_enhanced(&d, "toUTCString", &[]).unwrap();
        assert_eq!(r, JsValue::String("Thu, 01 Jan 1970 00:00:00 GMT".into()));
    }

    #[test]
    fn date_enhanced_get_timezone_offset() {
        let d = make_date(0.0);
        let r = call_date_method_enhanced(&d, "getTimezoneOffset", &[]).unwrap();
        assert_eq!(r, JsValue::Number(0.0));
    }

    #[test]
    fn date_enhanced_get_milliseconds() {
        let d = make_date(123.0);
        let r = call_date_method_enhanced(&d, "getMilliseconds", &[]).unwrap();
        assert_eq!(r, JsValue::Number(123.0));
    }

    // ── call_object_method_enhanced ────────────────────────────────────

    #[test]
    fn object_enhanced_has_own_property() {
        let m = make_map_obj(vec![("a", JsValue::Number(1.0))]);
        let r = call_object_method_enhanced(&m, "hasOwnProperty", &[JsValue::String("a".into())]).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
    }

    #[test]
    fn object_enhanced_value_of() {
        let m = make_map_obj(vec![("a", JsValue::Number(1.0))]);
        let r = call_object_method_enhanced(&m, "valueOf", &[]).unwrap();
        if let JsValue::Object(_) = r { /* ok */ } else { panic!("expected Object"); }
    }

    #[test]
    fn object_enhanced_property_is_enumerable_true() {
        let m = make_map_obj(vec![("foo", JsValue::Number(1.0))]);
        let r = call_object_method_enhanced(&m, "propertyIsEnumerable", &[JsValue::String("foo".into())]).unwrap();
        assert_eq!(r, JsValue::Boolean(true));
    }

    #[test]
    fn object_enhanced_property_is_enumerable_dunder_false() {
        let m = make_map_obj(vec![("__internal__", JsValue::Number(1.0))]);
        let r = call_object_method_enhanced(&m, "propertyIsEnumerable", &[JsValue::String("__internal__".into())]).unwrap();
        assert_eq!(r, JsValue::Boolean(false));
    }

    #[test]
    fn object_enhanced_property_is_enumerable_missing() {
        let m = make_map_obj(vec![]);
        let r = call_object_method_enhanced(&m, "propertyIsEnumerable", &[JsValue::String("nope".into())]).unwrap();
        assert_eq!(r, JsValue::Boolean(false));
    }

    // ── call_boolean_method ────────────────────────────────────────────

    #[test]
    fn boolean_to_string_true() {
        let r = call_boolean_method(true, "toString", &[]).unwrap();
        assert_eq!(r, JsValue::String("true".into()));
    }

    #[test]
    fn boolean_to_string_false() {
        let r = call_boolean_method(false, "toString", &[]).unwrap();
        assert_eq!(r, JsValue::String("false".into()));
    }

    #[test]
    fn boolean_value_of() {
        assert_eq!(call_boolean_method(true, "valueOf", &[]).unwrap(), JsValue::Boolean(true));
        assert_eq!(call_boolean_method(false, "valueOf", &[]).unwrap(), JsValue::Boolean(false));
    }

    #[test]
    fn boolean_unknown_method() {
        assert_eq!(call_boolean_method(true, "nope", &[]).unwrap(), JsValue::Undefined);
    }

    // ── call_native_function_method ────────────────────────────────────

    #[test]
    fn native_function_to_string() {
        let r = call_native_function_method("parseInt", "toString", &[]).unwrap();
        assert_eq!(r, JsValue::String("function parseInt() { [native code] }".into()));
    }

    #[test]
    fn native_function_name() {
        let r = call_native_function_method("Math.max", "name", &[]).unwrap();
        assert_eq!(r, JsValue::String("Math.max".into()));
    }

    #[test]
    fn native_function_call_returns_undefined() {
        let r = call_native_function_method("fn", "call", &[]).unwrap();
        assert_eq!(r, JsValue::Undefined);
    }

    // ── make_promise ───────────────────────────────────────────────────

    #[test]
    fn make_promise_resolved() {
        let p = make_promise(false, JsValue::Number(42.0));
        if let JsValue::Object(m) = p {
            assert_eq!(m.get("__type__").unwrap(), &JsValue::String("Promise".into()));
            assert_eq!(m.get("__resolved__").unwrap(), &JsValue::Number(42.0));
            assert!(m.get("__rejected__").is_none());
        } else { panic!("expected Object"); }
    }

    #[test]
    fn make_promise_rejected() {
        let p = make_promise(true, JsValue::String("err".into()));
        if let JsValue::Object(m) = p {
            assert_eq!(m.get("__rejected__").unwrap(), &JsValue::String("err".into()));
        } else { panic!("expected Object"); }
    }

    // ── await_value ────────────────────────────────────────────────────

    #[test]
    fn await_plain_value() {
        let r = await_value(JsValue::Number(42.0)).unwrap();
        assert_eq!(r, JsValue::Number(42.0));
    }

    #[test]
    fn await_resolved_promise() {
        let p = make_promise(false, JsValue::String("ok".into()));
        let r = await_value(p).unwrap();
        assert_eq!(r, JsValue::String("ok".into()));
    }

    #[test]
    fn await_rejected_promise() {
        let p = make_promise(true, JsValue::String("fail".into()));
        let r = await_value(p);
        assert!(r.is_err());
    }

    #[test]
    fn await_nested_resolved_promise() {
        let inner = make_promise(false, JsValue::Number(7.0));
        let outer = make_promise(false, inner);
        let r = await_value(outer).unwrap();
        assert_eq!(r, JsValue::Number(7.0));
    }

    // ── error_is_error ─────────────────────────────────────────────────

    #[test]
    fn error_is_error_true() {
        let mut m = HashMap::new();
        m.insert("message".to_string(), JsValue::String("oops".into()));
        m.insert("name".to_string(), JsValue::String("TypeError".into()));
        assert_eq!(error_is_error(Some(&JsValue::Object(m))), JsValue::Boolean(true));
    }

    #[test]
    fn error_is_error_plain_error() {
        let mut m = HashMap::new();
        m.insert("message".to_string(), JsValue::String("oops".into()));
        m.insert("name".to_string(), JsValue::String("Error".into()));
        assert_eq!(error_is_error(Some(&JsValue::Object(m))), JsValue::Boolean(true));
    }

    #[test]
    fn error_is_error_false_no_message() {
        let mut m = HashMap::new();
        m.insert("name".to_string(), JsValue::String("Error".into()));
        assert_eq!(error_is_error(Some(&JsValue::Object(m))), JsValue::Boolean(false));
    }

    #[test]
    fn error_is_error_false_no_name() {
        let mut m = HashMap::new();
        m.insert("message".to_string(), JsValue::String("oops".into()));
        assert_eq!(error_is_error(Some(&JsValue::Object(m))), JsValue::Boolean(false));
    }

    #[test]
    fn error_is_error_false_non_object() {
        assert_eq!(error_is_error(Some(&JsValue::Number(1.0))), JsValue::Boolean(false));
        assert_eq!(error_is_error(None), JsValue::Boolean(false));
    }

    // ── get_own_property_symbols ───────────────────────────────────────

    #[test]
    fn get_own_property_symbols_finds_symbols() {
        let mut m = HashMap::new();
        m.insert("foo".to_string(), JsValue::Number(1.0));
        m.insert("__symbol_desc_1__".to_string(), JsValue::String("sym".into()));
        m.insert("__symbol_other_2__".to_string(), JsValue::String("sym2".into()));
        let r = get_own_property_symbols(Some(&JsValue::Object(m)));
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 2);
        } else { panic!("expected Array"); }
    }

    #[test]
    fn get_own_property_symbols_none() {
        let m = make_map_obj(vec![("a", JsValue::Number(1.0))]);
        let r = get_own_property_symbols(Some(&JsValue::Object(m)));
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 0);
        } else { panic!("expected Array"); }
    }

    #[test]
    fn get_own_property_symbols_non_object() {
        let r = get_own_property_symbols(Some(&JsValue::Number(1.0)));
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 0);
        } else { panic!("expected Array"); }
    }

    // ── is_array_buffer_view ───────────────────────────────────────────

    #[test]
    fn is_view_uint8array() {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("Uint8Array".into()));
        assert!(is_array_buffer_view(Some(&JsValue::Object(m))));
    }

    #[test]
    fn is_view_dataview() {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("DataView".into()));
        assert!(is_array_buffer_view(Some(&JsValue::Object(m))));
    }

    #[test]
    fn is_view_float64array() {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("Float64Array".into()));
        assert!(is_array_buffer_view(Some(&JsValue::Object(m))));
    }

    #[test]
    fn is_view_not_a_view() {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("ArrayBuffer".into()));
        assert!(!is_array_buffer_view(Some(&JsValue::Object(m))));
    }

    #[test]
    fn is_view_none() {
        assert!(!is_array_buffer_view(None));
    }

    #[test]
    fn is_view_plain_object() {
        let m = HashMap::new();
        assert!(!is_array_buffer_view(Some(&JsValue::Object(m))));
    }

    // ── promise_with_resolvers ─────────────────────────────────────────

    #[test]
    fn promise_with_resolvers_structure() {
        let r = promise_with_resolvers();
        if let JsValue::Object(m) = r {
            assert!(m.contains_key("promise"));
            assert!(m.contains_key("resolve"));
            assert!(m.contains_key("reject"));
            // promise should be a Promise object
            if let Some(JsValue::Object(pm)) = m.get("promise") {
                assert_eq!(pm.get("__type__").unwrap(), &JsValue::String("Promise".into()));
            } else { panic!("expected promise to be Object"); }
            // resolve and reject should be native functions
            assert!(matches!(m.get("resolve"), Some(JsValue::NativeFunction(_))));
            assert!(matches!(m.get("reject"), Some(JsValue::NativeFunction(_))));
        } else { panic!("expected Object"); }
    }
}
