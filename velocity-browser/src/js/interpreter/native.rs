use super::signal::*;
use super::coercion::*;
use super::property::*;
use super::function::{call_function, call_function_with_this, parse_int_js, parse_float_js};
use super::eval::{eval_stmt, PROMISE_CAPTURE};
use super::eval::call_class_constructor;
use super::eval_script::eval_script_standalone;
use super::console::*;
use crate::js::scope::Scope;
use crate::js::vm::JsValue;
use std::collections::HashMap;

pub fn call_native(name: &str, args: &[JsValue]) -> EvalResult {
    Ok(match name {
        "parseInt" | "Number.parseInt" => {
            let s = args.first().map(to_string).unwrap_or_default();
            let radix_arg = args.get(1).map(to_number).unwrap_or(0.0);
            JsValue::Number(parse_int_js(&s, radix_arg))
        }
        "parseFloat" | "Number.parseFloat" => {
            let s = args.first().map(to_string).unwrap_or_default();
            JsValue::Number(parse_float_js(&s))
        }
        "isNaN" => {
            let n = args.first().map(to_number).unwrap_or(f64::NAN);
            JsValue::Boolean(n.is_nan())
        }
        "Number.isNaN" => {
            JsValue::Boolean(matches!(args.first(), Some(JsValue::Number(n)) if n.is_nan()))
        }
        "isFinite" => {
            let n = args.first().map(to_number).unwrap_or(f64::NAN);
            JsValue::Boolean(n.is_finite())
        }
        "Number.isFinite" => {
            JsValue::Boolean(matches!(args.first(), Some(JsValue::Number(n)) if n.is_finite()))
        }
        "Number.isInteger" => {
            match args.first() {
                Some(JsValue::Number(n)) => JsValue::Boolean(n.is_finite() && n.fract() == 0.0),
                _ => JsValue::Boolean(false),
            }
        }
        "Number.isSafeInteger" => {
            match args.first() {
                Some(JsValue::Number(n)) => JsValue::Boolean(n.is_finite() && n.fract() == 0.0 && n.abs() <= 9007199254740991.0),
                _ => JsValue::Boolean(false),
            }
        }
        "Math.floor" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).floor()),
        "Math.ceil" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).ceil()),
        "Math.round" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).round()),
        "Math.abs" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).abs()),
        "Math.sqrt" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).sqrt()),
        "Math.trunc" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).trunc()),
        "Math.sign" => { let n = args.first().map(to_number).unwrap_or(f64::NAN); JsValue::Number(if n > 0.0 { 1.0 } else if n < 0.0 { -1.0 } else { 0.0 }) }
        "Math.log" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).ln()),
        "Math.pow" => { let b = args.first().map(to_number).unwrap_or(0.0); let e = args.get(1).map(to_number).unwrap_or(0.0); JsValue::Number(b.powf(e)) }
        "Math.max" => JsValue::Number(args.iter().map(to_number).fold(f64::NEG_INFINITY, f64::max)),
        "Math.min" => JsValue::Number(args.iter().map(to_number).fold(f64::INFINITY, f64::min)),
        "Math.random" => JsValue::Number(0.5),
        "Math.sin" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).sin()),
        "Math.cos" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).cos()),
        "Math.tan" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).tan()),
        "Math.asin" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).asin()),
        "Math.acos" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).acos()),
        "Math.atan" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).atan()),
        "Math.atan2" => { let y = args.first().map(to_number).unwrap_or(f64::NAN); let x = args.get(1).map(to_number).unwrap_or(f64::NAN); JsValue::Number(y.atan2(x)) }
        "Math.sinh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).sinh()),
        "Math.cosh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).cosh()),
        "Math.tanh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).tanh()),
        "Math.exp" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).exp()),
        "Math.expm1" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).exp_m1()),
        "Math.log1p" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).ln_1p()),
        "Math.log2" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).log2()),
        "Math.log10" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).log10()),
        "Math.cbrt" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).cbrt()),
        "Math.hypot" => JsValue::Number(args.iter().map(to_number).map(|v| v * v).sum::<f64>().sqrt()),
        "Math.fround" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN) as f32 as f64),
        "Math.clz32" => { let n = args.first().map(to_number).unwrap_or(0.0); let u = if n.is_finite() { n as i64 as u32 } else { 0 }; JsValue::Number(u.leading_zeros() as f64) }
        "Math.asinh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).asinh()),
        "Math.acosh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).acosh()),
        "Math.atanh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).atanh()),
        "Math.imul" => {
            let to_i32 = |v: Option<&JsValue>| -> i32 {
                let n = v.map(to_number).unwrap_or(0.0);
                if n.is_finite() { n.trunc() as i64 as i32 } else { 0 }
            };
            JsValue::Number(to_i32(args.first()).wrapping_mul(to_i32(args.get(1))) as f64)
        }
        "JSON.parse" => {
            let s = args.first().map(to_string).unwrap_or_default();
            json_parse(&s)
        }
        "JSON.stringify" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            let replacer: Option<Vec<String>> = match args.get(1) {
                Some(JsValue::Array(arr)) => Some(arr.iter().map(to_string).collect()),
                _ => None,
            };
            let indent = match args.get(2) {
                Some(JsValue::Number(n)) if *n >= 1.0 => " ".repeat((*n as usize).min(10)),
                Some(JsValue::String(s)) if !s.is_empty() => s.chars().take(10).collect(),
                _ => String::new(),
            };
            if indent.is_empty() {
                JsValue::String(json_stringify(&val, replacer.as_deref()))
            } else {
                JsValue::String(json_stringify_pretty(&val, &indent, 0, replacer.as_deref()))
            }
        }
        "Object.keys" => {
            match args.first() {
                Some(obj) => JsValue::Array(own_keys_of(obj).into_iter().map(JsValue::String).collect()),
                None => JsValue::Array(Vec::new()),
            }
        }
        "Object.values" => {
            match args.first() {
                Some(obj) => JsValue::Array(own_keys_of(obj).into_iter().map(|k| get_property(obj, &k)).collect()),
                None => JsValue::Array(Vec::new()),
            }
        }
        "Object.entries" => {
            match args.first() {
                Some(obj) => JsValue::Array(own_keys_of(obj).into_iter().map(|k| JsValue::Array(vec![JsValue::String(k.clone()), get_property(obj, &k)])).collect()),
                None => JsValue::Array(Vec::new()),
            }
        }
        "Object.fromEntries" => {
            let mut map = HashMap::new();
            let entries = match args.first() {
                Some(JsValue::Array(items)) => items.clone(),
                Some(JsValue::Object(m)) => {
                    if let Some(JsValue::Array(items)) = m.get("__entries__") { items.clone() } else { Vec::new() }
                }
                _ => Vec::new(),
            };
            for entry in entries {
                if let JsValue::Array(pair) = entry {
                    let key = pair.first().map(to_string).unwrap_or_default();
                    let value = pair.get(1).cloned().unwrap_or(JsValue::Undefined);
                    map.insert(key, value);
                }
            }
            JsValue::Object(map)
        }
        "Object.assign" => {
            let mut target = if let Some(JsValue::Object(m)) = args.first() { m.clone() } else { HashMap::new() };
            for src in args.iter().skip(1) { if let JsValue::Object(m) = src { target.extend(m.iter().map(|(k, v)| (k.clone(), v.clone()))); } }
            JsValue::Object(target)
        }
        "Object.freeze" => args.first().cloned().unwrap_or(JsValue::Undefined),
        "Object.hasOwn" => {
            let key = args.get(1).map(to_string).unwrap_or_default();
            let has = match args.first() {
                Some(JsValue::Object(map)) => map.contains_key(&key),
                Some(JsValue::Array(arr)) => key == "length" || key.parse::<usize>().map(|i| i < arr.len()).unwrap_or(false),
                _ => false,
            };
            JsValue::Boolean(has)
        }
        "Object.is" => {
            let a = args.first().cloned().unwrap_or(JsValue::Undefined);
            let b = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            let same = match (&a, &b) {
                (JsValue::Number(x), JsValue::Number(y)) => {
                    if x.is_nan() && y.is_nan() { true }
                    else if *x == 0.0 && *y == 0.0 { x.is_sign_negative() == y.is_sign_negative() }
                    else { x == y }
                }
                _ => strict_eq(&a, &b),
            };
            JsValue::Boolean(same)
        }
        "Object.setPrototypeOf" => {
            let mut target = if let Some(JsValue::Object(m)) = args.first() { m.clone() } else { return Ok(args.first().cloned().unwrap_or(JsValue::Undefined)); };
            match args.get(1) {
                Some(JsValue::Null) | None => { target.remove("__proto__"); }
                Some(proto) => { target.insert("__proto__".to_string(), proto.clone()); }
            }
            JsValue::Object(target)
        }
        "Object.create" => {
            let proto = args.first().cloned().unwrap_or(JsValue::Null);
            let mut obj = HashMap::new();
            if !matches!(proto, JsValue::Null) {
                obj.insert("__proto__".to_string(), proto);
            }
            JsValue::Object(obj)
        }
        "Object.getPrototypeOf" => {
            if let Some(JsValue::Object(map)) = args.first() {
                map.get("__proto__").cloned().unwrap_or(JsValue::Null)
            } else { JsValue::Null }
        }
        "Object.defineProperty" => {
            let mut target = match args.first() {
                Some(JsValue::Object(m)) => m.clone(),
                _ => HashMap::new(),
            };
            let prop = args.get(1).map(to_string).unwrap_or_default();
            if let Some(JsValue::Object(desc)) = args.get(2) {
                apply_descriptor(&mut target, &prop, desc);
            }
            JsValue::Object(target)
        }
        "Object.defineProperties" => {
            let mut target = match args.first() {
                Some(JsValue::Object(m)) => m.clone(),
                _ => HashMap::new(),
            };
            if let Some(JsValue::Object(props)) = args.get(1) {
                for (prop, desc_val) in props {
                    if let JsValue::Object(desc) = desc_val {
                        apply_descriptor(&mut target, prop, desc);
                    }
                }
            }
            JsValue::Object(target)
        }
        "Object.getOwnPropertyDescriptor" => {
            let prop = args.get(1).map(to_string).unwrap_or_default();
            match args.first() {
                Some(JsValue::Object(map)) => match map.get(&prop) {
                    Some(JsValue::Object(desc)) if desc.get("__accessor__") == Some(&JsValue::Boolean(true)) => {
                        let mut out = HashMap::new();
                        out.insert("enumerable".to_string(), desc.get("enumerable").cloned().unwrap_or(JsValue::Boolean(false)));
                        out.insert("configurable".to_string(), desc.get("configurable").cloned().unwrap_or(JsValue::Boolean(false)));
                        if let Some(g) = desc.get("get") { out.insert("get".to_string(), g.clone()); }
                        if let Some(s) = desc.get("set") { out.insert("set".to_string(), s.clone()); }
                        JsValue::Object(out)
                    }
                    Some(val) => {
                        let mut out = HashMap::new();
                        out.insert("value".to_string(), val.clone());
                        out.insert("writable".to_string(), JsValue::Boolean(true));
                        out.insert("enumerable".to_string(), JsValue::Boolean(true));
                        out.insert("configurable".to_string(), JsValue::Boolean(true));
                        JsValue::Object(out)
                    }
                    None => JsValue::Undefined,
                },
                _ => JsValue::Undefined,
            }
        }
        "Object.getOwnPropertyDescriptors" => {
            match args.first() {
                Some(obj @ JsValue::Object(map)) => {
                    let mut out = HashMap::new();
                    for key in own_keys_of(obj) {
                        let desc = match map.get(&key) {
                            Some(JsValue::Object(d)) if d.get("__accessor__") == Some(&JsValue::Boolean(true)) => {
                                let mut acc = HashMap::new();
                                acc.insert("enumerable".to_string(), d.get("enumerable").cloned().unwrap_or(JsValue::Boolean(false)));
                                acc.insert("configurable".to_string(), d.get("configurable").cloned().unwrap_or(JsValue::Boolean(false)));
                                if let Some(g) = d.get("get") { acc.insert("get".to_string(), g.clone()); }
                                if let Some(s) = d.get("set") { acc.insert("set".to_string(), s.clone()); }
                                JsValue::Object(acc)
                            }
                            Some(val) => {
                                let mut data = HashMap::new();
                                data.insert("value".to_string(), val.clone());
                                data.insert("writable".to_string(), JsValue::Boolean(true));
                                data.insert("enumerable".to_string(), JsValue::Boolean(true));
                                data.insert("configurable".to_string(), JsValue::Boolean(true));
                                JsValue::Object(data)
                            }
                            None => continue,
                        };
                        out.insert(key, desc);
                    }
                    JsValue::Object(out)
                }
                _ => JsValue::Object(HashMap::new()),
            }
        }
        "Array.isArray" => JsValue::Boolean(matches!(args.first(), Some(JsValue::Array(_)))),
        "Array.from" => {
            match args.first() {
                Some(JsValue::Array(a)) => JsValue::Array(a.clone()),
                Some(JsValue::String(s)) => JsValue::Array(s.chars().map(|c| JsValue::String(c.to_string())).collect()),
                Some(JsValue::Object(m)) => {
                    match m.get("__type__").map(to_string).as_deref() {
                        Some("Set") => m.get("__items__").cloned().unwrap_or_else(|| JsValue::Array(Vec::new())),
                        Some("Map") => m.get("__entries__").cloned().unwrap_or_else(|| JsValue::Array(Vec::new())),
                        _ => match m.get("length") {
                            Some(len_val) => {
                                let len = to_number(len_val) as usize;
                                JsValue::Array((0..len).map(|i| m.get(&i.to_string()).cloned().unwrap_or(JsValue::Undefined)).collect())
                            }
                            None => JsValue::Array(Vec::new()),
                        },
                    }
                }
                _ => JsValue::Array(Vec::new()),
            }
        }
        "Array.of" => JsValue::Array(args.to_vec()),
        "String.fromCharCode" => {
            let s: String = args.iter().filter_map(|a| { let n = to_number(a) as u32; char::from_u32(n) }).collect();
            JsValue::String(s)
        }
        "String.fromCodePoint" => {
            let s: String = args.iter().filter_map(|a| { let n = to_number(a) as u32; char::from_u32(n) }).collect();
            JsValue::String(s)
        }
        "Date.now" => JsValue::Number(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)),
        "performance.now" => JsValue::Number(perf_now()),
        "performance.mark" => {
            let name = args.first().map(to_string).unwrap_or_default();
            perf_mark(&name);
            JsValue::Undefined
        }
        "performance.measure" => {
            let name = args.first().map(to_string).unwrap_or_default();
            let duration = perf_measure(&name);
            JsValue::Number(duration)
        }
        "console.log" => { push_console("log", args.to_vec()); return Ok(JsValue::Undefined); }
        "console.warn" => { push_console("warn", args.to_vec()); return Ok(JsValue::Undefined); }
        "console.error" => { push_console("error", args.to_vec()); return Ok(JsValue::Undefined); }
        "console.info" => { push_console("info", args.to_vec()); return Ok(JsValue::Undefined); }
        "console.debug" => { push_console("debug", args.to_vec()); return Ok(JsValue::Undefined); }
        "console.assert" => {
            let cond = args.first().map(to_boolean).unwrap_or(false);
            if !cond { push_console("assert", args.iter().skip(1).cloned().collect()); }
            return Ok(JsValue::Undefined);
        }
        "console.count" => {
            let label = args.first().map(to_string).unwrap_or_else(|| "default".into());
            let c = console_count(&label);
            push_console("count", vec![JsValue::String(format!("{}: {}", label, c))]);
            return Ok(JsValue::Undefined);
        }
        "console.countReset" => {
            let label = args.first().map(to_string).unwrap_or_else(|| "default".into());
            console_count_reset(&label);
            return Ok(JsValue::Undefined);
        }
        "console.time" => {
            let label = args.first().map(to_string).unwrap_or_else(|| "default".into());
            console_time(&label);
            return Ok(JsValue::Undefined);
        }
        "console.timeEnd" => {
            let label = args.first().map(to_string).unwrap_or_else(|| "default".into());
            let elapsed = console_time_end(&label).unwrap_or(0.0);
            push_console("timeEnd", vec![JsValue::String(format!("{}: {:.3}ms", label, elapsed))]);
            return Ok(JsValue::Undefined);
        }
        "console.table" => {
            // Render the value as an aligned Markdown table so agents (and the
            // trace collector) get readable structure, not an opaque dump.
            let text = console_table_text(args.first().unwrap_or(&JsValue::Undefined));
            push_console("table", vec![JsValue::String(text)]);
            return Ok(JsValue::Undefined);
        }
        "console.trace" => { push_console("trace", args.to_vec()); return Ok(JsValue::Undefined); }
        "console.group" => { push_console("group", args.to_vec()); return Ok(JsValue::Undefined); }
        "console.groupEnd" => { push_console("groupEnd", vec![]); return Ok(JsValue::Undefined); }
        "console.clear" => { clear_console_output(); return Ok(JsValue::Undefined); }
        "Symbol" => {
            let desc = args.first().map(to_string).unwrap_or_else(|| "symbol".into());
            let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            JsValue::String(format!("__symbol_{}_{}__", desc, id))
        }
        "Symbol.for" => {
            let key = args.first().map(to_string).unwrap_or_default();
            JsValue::String(format!("__symbol_{}__", key))
        }
        "Number" => JsValue::Number(args.first().map(to_number).unwrap_or(0.0)),
        "String" => JsValue::String(args.first().map(to_string).unwrap_or_default()),
        "Boolean" => JsValue::Boolean(args.first().map(to_boolean).unwrap_or(false)),
        "structuredClone" => {
            args.first().cloned().unwrap_or(JsValue::Undefined)
        }
        "queueMicrotask" | "requestAnimationFrame" | "requestIdleCallback" => {
            JsValue::Number(0.0)
        }
        "__noop__" => JsValue::Undefined,
        "__promise_resolve__" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            PROMISE_CAPTURE.with(|cap| { *cap.borrow_mut() = Some((false, val)); });
            JsValue::Undefined
        }
        "__promise_reject__" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            PROMISE_CAPTURE.with(|cap| { *cap.borrow_mut() = Some((true, val)); });
            JsValue::Undefined
        }
        "eval" => {
            let code = args.first().map(to_string).unwrap_or_default();
            if code.is_empty() { return Ok(JsValue::Undefined); }
            match eval_script_standalone(&code) {
                Ok(v) => v,
                Err(_) => JsValue::Undefined,
            }
        }
        "encodeURIComponent" => {
            let s = args.first().map(to_string).unwrap_or_default();
            JsValue::String(encode_uri_component(&s))
        }
        "decodeURIComponent" => {
            let s = args.first().map(to_string).unwrap_or_default();
            JsValue::String(decode_uri_component(&s))
        }
        "Promise.resolve" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            map.insert("__resolved__".to_string(), val);
            JsValue::Object(map)
        }
        "Promise.reject" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            map.insert("__rejected__".to_string(), val);
            JsValue::Object(map)
        }
        "Promise.all" => {
            let mut results = Vec::new();
            if let Some(JsValue::Array(promises)) = args.first() {
                for p in promises {
                    if let JsValue::Object(m) = p {
                        if let Some(rejected) = m.get("__rejected__") {
                            let mut map = HashMap::new();
                            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            map.insert("__rejected__".to_string(), rejected.clone());
                            return Ok(JsValue::Object(map));
                        }
                        results.push(m.get("__resolved__").cloned().unwrap_or(p.clone()));
                    } else {
                        results.push(p.clone());
                    }
                }
            }
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            map.insert("__resolved__".to_string(), JsValue::Array(results));
            JsValue::Object(map)
        }
        "Promise.race" => {
            if let Some(JsValue::Array(promises)) = args.first() {
                for p in promises {
                    if let JsValue::Object(m) = p {
                        if m.get("__resolved__").is_some() || m.get("__rejected__").is_some() {
                            return Ok(JsValue::Object(m.clone()));
                        }
                    } else {
                        let mut map = HashMap::new();
                        map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                        map.insert("__resolved__".to_string(), p.clone());
                        return Ok(JsValue::Object(map));
                    }
                }
            }
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            JsValue::Object(map)
        }
        "Promise.allSettled" => {
            let mut results = Vec::new();
            if let Some(JsValue::Array(promises)) = args.first() {
                for p in promises {
                    let mut entry = HashMap::new();
                    if let JsValue::Object(m) = p {
                        if let Some(rejected) = m.get("__rejected__") {
                            entry.insert("status".to_string(), JsValue::String("rejected".to_string()));
                            entry.insert("reason".to_string(), rejected.clone());
                        } else {
                            entry.insert("status".to_string(), JsValue::String("fulfilled".to_string()));
                            entry.insert("value".to_string(), m.get("__resolved__").cloned().unwrap_or(JsValue::Undefined));
                        }
                    } else {
                        entry.insert("status".to_string(), JsValue::String("fulfilled".to_string()));
                        entry.insert("value".to_string(), p.clone());
                    }
                    results.push(JsValue::Object(entry));
                }
            }
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            map.insert("__resolved__".to_string(), JsValue::Array(results));
            JsValue::Object(map)
        }
        "Object.getOwnPropertyNames" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::Array(own_property_names(&target).into_iter().map(JsValue::String).collect())
        }
        "Reflect.get" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            get_property(&target, &prop)
        }
        "Reflect.set" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            let value = args.get(2).cloned().unwrap_or(JsValue::Undefined);
            let mut t = target;
            JsValue::Boolean(set_property(&mut t, &prop, value))
        }
        "Reflect.has" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            JsValue::Boolean(has_property(&target, &prop))
        }
        "Reflect.deleteProperty" => {
            let mut target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            JsValue::Boolean(delete_property(&mut target, &prop))
        }
        "Reflect.ownKeys" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::Array(own_property_names(&target).into_iter().map(JsValue::String).collect())
        }
        "Reflect.getOwnPropertyDescriptor" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            match &target {
                JsValue::Object(map) => {
                    if let Some(val) = map.get(&prop) {
                        let mut desc = HashMap::new();
                        desc.insert("value".to_string(), val.clone());
                        desc.insert("writable".to_string(), JsValue::Boolean(true));
                        desc.insert("enumerable".to_string(), JsValue::Boolean(true));
                        desc.insert("configurable".to_string(), JsValue::Boolean(true));
                        JsValue::Object(desc)
                    } else {
                        JsValue::Undefined
                    }
                }
                _ => JsValue::Undefined,
            }
        }
        "Reflect.apply" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let this_arg = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            let call_args = match args.get(2) {
                Some(JsValue::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            call_function_with_this(&target, &call_args, &Scope::new_global(), Some(this_arg)).unwrap_or(JsValue::Undefined)
        }
        "Reflect.construct" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let call_args = match args.get(1) {
                Some(JsValue::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            match &target {
                JsValue::Object(class_map) if class_map.get("__type__").map(to_string).as_deref() == Some("class") => {
                    call_class_constructor(class_map, &call_args, &Scope::new_global()).unwrap_or(JsValue::Undefined)
                }
                JsValue::Function { params, body, closure, .. } => {
                    let call_scope = Scope::new_child(closure);
                    let this_obj = JsValue::Object(HashMap::new());
                    Scope::declare(&call_scope, "this", this_obj.clone());
                    for (i, p) in params.iter().enumerate() {
                        Scope::declare(&call_scope, p, call_args.get(i).cloned().unwrap_or(JsValue::Undefined));
                    }
                    Scope::declare(&call_scope, "arguments", JsValue::Array(call_args));
                    match eval_stmt(body, &call_scope) {
                        Err(Signal::Return(v)) if matches!(v, JsValue::Object(_)) => v,
                        _ => Scope::resolve(&call_scope, "this").unwrap_or(this_obj),
                    }
                }
                _ => call_function(&target, &call_args, &Scope::new_global()).unwrap_or(JsValue::Undefined),
            }
        }
        // Timer APIs
        "setTimeout" => super::browser_env::set_timeout(args),
        "setInterval" => super::browser_env::set_interval(args),
        "clearTimeout" | "clearInterval" => super::browser_env::clear_timer(args),
        "flushTimers" => JsValue::Number(super::browser_env::flush_timers() as f64),
        // Fetch API (simulated)
        "fetch" => super::browser_env::call_fetch(args),
        // Web platform globals.
        "getComputedStyle" => super::web_platform::get_computed_style(args.first().unwrap_or(&JsValue::Undefined), args.get(1)),
        "matchMedia" => super::web_platform::match_media(&args.first().map(to_string).unwrap_or_default()),
        "createImageBitmap" => {
            let mut bitmap = HashMap::new();
            bitmap.insert("__type__".to_string(), JsValue::String("ImageBitmap".to_string()));
            bitmap.insert("width".to_string(), JsValue::Number(1.0));
            bitmap.insert("height".to_string(), JsValue::Number(1.0));
            let mut p = HashMap::new();
            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            p.insert("__resolved__".to_string(), JsValue::Object(bitmap));
            JsValue::Object(p)
        }
        // Overflow dispatch: newer globals/statics live in web_apis2.rs.
        _ => return super::web_apis2::call_native_extended(name, args),
    })
}

pub fn json_parse(s: &str) -> JsValue {
    let s = s.trim();
    if s == "null" { return JsValue::Null; }
    if s == "true" { return JsValue::Boolean(true); }
    if s == "false" { return JsValue::Boolean(false); }
    if let Ok(n) = s.parse::<f64>() { return JsValue::Number(n); }
    if s.starts_with('"') && s.ends_with('"') {
        if let Ok(serde_json::Value::String(decoded)) = serde_json::from_str::<serde_json::Value>(s) {
            return JsValue::String(decoded);
        }
        return JsValue::String(s[1..s.len()-1].to_string());
    }
    if s.starts_with('[') {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
            return serde_to_js(&val);
        }
    }
    if s.starts_with('{') {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
            return serde_to_js(&val);
        }
    }
    JsValue::Undefined
}

pub(super) fn serde_to_js(val: &serde_json::Value) -> JsValue {
    match val {
        serde_json::Value::Null => JsValue::Null,
        serde_json::Value::Bool(b) => JsValue::Boolean(*b),
        serde_json::Value::Number(n) => JsValue::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => JsValue::String(s.clone()),
        serde_json::Value::Array(arr) => JsValue::Array(arr.iter().map(serde_to_js).collect()),
        serde_json::Value::Object(map) => JsValue::Object(map.iter().map(|(k, v)| (k.clone(), serde_to_js(v))).collect()),
    }
}

pub fn json_stringify(val: &JsValue, replacer: Option<&[String]>) -> String {
    match val {
        JsValue::Undefined => "undefined".to_string(),
        JsValue::Null => "null".to_string(),
        JsValue::Boolean(b) => b.to_string(),
        JsValue::Number(n) => if n.is_finite() { format_number(*n) } else { "null".to_string() },
        JsValue::String(s) => json_escape_string(s),
        JsValue::Array(arr) => {
            let items: Vec<String> = arr.iter().map(|v| match v {
                JsValue::Undefined | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => "null".to_string(),
                other => json_stringify(other, replacer),
            }).collect();
            format!("[{}]", items.join(","))
        }
        JsValue::Object(map) => {
            let entries: Vec<String> = if let Some(whitelist) = replacer {
                // Per spec: iterate over the replacer array order, not the object's keys
                whitelist.iter().filter_map(|key| {
                    map.get(key).and_then(|v| {
                        match v {
                            JsValue::Undefined | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => None,
                            other => Some(format!("{}:{}", json_escape_string(key), json_stringify(other, replacer))),
                        }
                    })
                }).collect()
            } else {
                map.iter().filter_map(|(k, v)| {
                    match v {
                        JsValue::Undefined | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => None,
                        other => Some(format!("{}:{}", json_escape_string(k), json_stringify(other, replacer))),
                    }
                }).collect()
            };
            format!("{{{}}}", entries.join(","))
        }
        JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => "null".to_string(),
    }
}

fn json_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_stringify_pretty(val: &JsValue, indent: &str, depth: usize, replacer: Option<&[String]>) -> String {
    match val {
        JsValue::Array(arr) if !arr.is_empty() => {
            let pad = indent.repeat(depth + 1);
            let close = indent.repeat(depth);
            let items: Vec<String> = arr.iter().map(|v| {
                let rendered = match v {
                    JsValue::Undefined | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => "null".to_string(),
                    other => json_stringify_pretty(other, indent, depth + 1, replacer),
                };
                format!("{}{}", pad, rendered)
            }).collect();
            format!("[\n{}\n{}]", items.join(",\n"), close)
        }
        JsValue::Object(map) if !map.is_empty() => {
            let pad = indent.repeat(depth + 1);
            let close = indent.repeat(depth);
            let entries: Vec<String> = if let Some(whitelist) = replacer {
                whitelist.iter().filter_map(|key| {
                    map.get(key).and_then(|v| {
                        match v {
                            JsValue::Undefined | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => None,
                            other => Some(format!("{}{}: {}", pad, json_escape_string(key), json_stringify_pretty(other, indent, depth + 1, replacer))),
                        }
                    })
                }).collect()
            } else {
                map.iter().filter_map(|(k, v)| {
                    match v {
                        JsValue::Undefined | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => None,
                        other => Some(format!("{}{}: {}", pad, json_escape_string(k), json_stringify_pretty(other, indent, depth + 1, replacer))),
                    }
                }).collect()
            };
            if entries.is_empty() { return "{}".to_string(); }
            format!("{{\n{}\n{}}}", entries.join(",\n"), close)
        }
        _ => json_stringify(val, replacer),
    }
}

pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'!' | b'*' | b'(' | b')' | b'\'' => out.push(b as char),
            _ => { out.push('%'); out.push_str(&format!("{:02X}", b)); }
        }
    }
    out
}

pub fn decode_uri_component(s: &str) -> String {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i+1..i+3], 16) { out.push(b); i += 3; }
            else { out.push(bytes[i]); i += 1; }
        } else if bytes[i] == b'+' { out.push(b' '); i += 1; }
        else { out.push(bytes[i]); i += 1; }
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── json_parse ─────────────────────────────────────────────────────

    #[test]
    fn json_parse_null() {
        assert_eq!(json_parse("null"), JsValue::Null);
    }

    #[test]
    fn json_parse_booleans() {
        assert_eq!(json_parse("true"), JsValue::Boolean(true));
        assert_eq!(json_parse("false"), JsValue::Boolean(false));
    }

    #[test]
    fn json_parse_number() {
        assert_eq!(json_parse("42"), JsValue::Number(42.0));
        assert_eq!(json_parse("2.72"), JsValue::Number(2.72));
    }

    #[test]
    fn json_parse_string() {
        assert_eq!(json_parse(r#""hello""#), JsValue::String("hello".into()));
    }

    #[test]
    fn json_parse_array() {
        let result = json_parse("[1,2,3]");
        if let JsValue::Array(arr) = result {
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0], JsValue::Number(1.0));
        } else {
            panic!("expected array");
        }
    }

    #[test]
    fn json_parse_object() {
        let result = json_parse(r#"{"a":1}"#);
        if let JsValue::Object(map) = result {
            assert_eq!(map.get("a"), Some(&JsValue::Number(1.0)));
        } else {
            panic!("expected object");
        }
    }

    #[test]
    fn json_parse_invalid_returns_undefined() {
        assert!(matches!(json_parse("not valid json"), JsValue::Undefined));
    }

    // ── json_stringify ─────────────────────────────────────────────────

    #[test]
    fn json_stringify_primitives() {
        assert_eq!(json_stringify(&JsValue::Null, None), "null");
        assert_eq!(json_stringify(&JsValue::Boolean(true), None), "true");
        assert_eq!(json_stringify(&JsValue::Number(42.0), None), "42");
        assert_eq!(json_stringify(&JsValue::String("hi".into()), None), r#""hi""#);
    }

    #[test]
    fn json_stringify_undefined() {
        assert_eq!(json_stringify(&JsValue::Undefined, None), "undefined");
    }

    #[test]
    fn json_stringify_non_finite_number_is_null() {
        assert_eq!(json_stringify(&JsValue::Number(f64::NAN), None), "null");
        assert_eq!(json_stringify(&JsValue::Number(f64::INFINITY), None), "null");
    }

    #[test]
    fn json_stringify_array() {
        let arr = JsValue::Array(vec![JsValue::Number(1.0), JsValue::Null]);
        assert_eq!(json_stringify(&arr, None), "[1,null]");
    }

    #[test]
    fn json_stringify_array_skips_undefined_and_functions() {
        let arr = JsValue::Array(vec![
            JsValue::Number(1.0),
            JsValue::Undefined,
            JsValue::Number(3.0),
        ]);
        assert_eq!(json_stringify(&arr, None), "[1,null,3]");
    }

    #[test]
    fn json_stringify_object_skips_undefined_and_functions() {
        let mut map = HashMap::new();
        map.insert("a".to_string(), JsValue::Number(1.0));
        map.insert("b".to_string(), JsValue::Undefined);
        let s = json_stringify(&JsValue::Object(map), None);
        assert!(s.contains("\"a\":1"));
        assert!(!s.contains("\"b\""));
    }

    // ── encode_uri_component / decode_uri_component ────────────────────

    #[test]
    fn encode_uri_component_alphanumeric_passthrough() {
        assert_eq!(encode_uri_component("hello"), "hello");
        assert_eq!(encode_uri_component("abc123"), "abc123");
    }

    #[test]
    fn encode_uri_component_special_chars() {
        assert_eq!(encode_uri_component("a b"), "a%20b");
        assert_eq!(encode_uri_component("a&b"), "a%26b");
        assert_eq!(encode_uri_component("a=b"), "a%3Db");
    }

    #[test]
    fn decode_uri_component_percent_encoding() {
        assert_eq!(decode_uri_component("a%20b"), "a b");
        assert_eq!(decode_uri_component("a%26b"), "a&b");
    }

    #[test]
    fn decode_uri_component_plus_as_space() {
        assert_eq!(decode_uri_component("a+b"), "a b");
    }

    #[test]
    fn encode_decode_roundtrip() {
        let original = "hello world & foo=bar";
        let encoded = encode_uri_component(original);
        let decoded = decode_uri_component(&encoded);
        assert_eq!(decoded, original);
    }
}
