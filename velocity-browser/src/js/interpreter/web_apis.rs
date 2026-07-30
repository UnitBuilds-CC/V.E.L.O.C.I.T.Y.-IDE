use super::signal::*;
use super::coercion::*;
use crate::js::vm::JsValue;
use std::collections::HashMap;

// ── AbortController / AbortSignal ────────────────────────────────────────────

pub(super) fn call_abort_controller_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "abort" => {
            if let Some(JsValue::Object(signal_map)) = map.get("signal") {
                let mut updated_signal = signal_map.clone();
                updated_signal.insert("aborted".to_string(), JsValue::Boolean(true));
                let mut updated = map.clone();
                updated.insert("signal".to_string(), JsValue::Object(updated_signal));
                return Ok(JsValue::Object(updated));
            }
            JsValue::Undefined
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_abort_signal_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "aborted" => map.get("aborted").cloned().unwrap_or(JsValue::Boolean(false)),
        "throwIfAborted" => {
            if map.get("aborted").map(to_boolean).unwrap_or(false) {
                return Err(Signal::Throw(JsValue::String("AbortError".to_string())));
            }
            JsValue::Undefined
        }
        _ => JsValue::Undefined,
    })
}

// ── TextEncoder / TextDecoder ────────────────────────────────────────────────

pub(super) fn call_text_encoder_method(_map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "encode" => {
            let s = args.first().map(to_string).unwrap_or_default();
            let bytes: Vec<JsValue> = s.as_bytes().iter().map(|b| JsValue::Number(*b as f64)).collect();
            JsValue::Array(bytes)
        }
        "encoding" => JsValue::String("utf-8".to_string()),
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_text_decoder_method(_map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "decode" => {
            let bytes: Vec<u8> = match args.first() {
                Some(JsValue::Array(arr)) => arr.iter().filter_map(|v| if let JsValue::Number(n) = v { Some(*n as u8) } else { None }).collect(),
                _ => Vec::new(),
            };
            JsValue::String(String::from_utf8_lossy(&bytes).to_string())
        }
        "encoding" => JsValue::String("utf-8".to_string()),
        _ => JsValue::Undefined,
    })
}

// ── Response / Blob / TypedArray / DataView ──────────────────────────────────

pub(super) fn call_response_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "ok" => {
            let status = map.get("status").map(to_number).unwrap_or(200.0);
            JsValue::Boolean((200.0..300.0).contains(&status))
        }
        "status" => map.get("status").cloned().unwrap_or(JsValue::Number(200.0)),
        "statusText" => map.get("statusText").cloned().unwrap_or(JsValue::String("OK".to_string())),
        "headers" => map.get("headers").cloned().unwrap_or(JsValue::Object(HashMap::new())),
        "url" => map.get("url").cloned().unwrap_or(JsValue::String(String::new())),
        "text" => map.get("__body__").cloned().unwrap_or(JsValue::String(String::new())),
        "json" => {
            let body = map.get("__body__").map(to_string).unwrap_or_default();
            super::native::json_parse(&body)
        }
        "clone" => JsValue::Object(map.clone()),
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_blob_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "size" => {
            let data = match map.get("__data__") {
                Some(JsValue::Array(arr)) => arr.len(),
                Some(JsValue::String(s)) => s.len(),
                _ => 0,
            };
            JsValue::Number(data as f64)
        }
        "type" => map.get("__mime__").cloned().unwrap_or(JsValue::String(String::new())),
        "text" => {
            match map.get("__data__") {
                Some(JsValue::String(s)) => JsValue::String(s.clone()),
                Some(JsValue::Array(arr)) => {
                    let bytes: Vec<u8> = arr.iter().filter_map(|v| if let JsValue::Number(n) = v { Some(*n as u8) } else { None }).collect();
                    JsValue::String(String::from_utf8_lossy(&bytes).to_string())
                }
                _ => JsValue::String(String::new()),
            }
        }
        "arrayBuffer" => {
            match map.get("__data__") {
                Some(JsValue::String(s)) => JsValue::Array(s.as_bytes().iter().map(|b| JsValue::Number(*b as f64)).collect()),
                Some(JsValue::Array(arr)) => JsValue::Array(arr.clone()),
                _ => JsValue::Array(Vec::new()),
            }
        }
        "slice" => {
            let start = _args.first().map(to_number).unwrap_or(0.0) as usize;
            let end = _args.get(1).map(to_number).unwrap_or(0.0) as usize;
            match map.get("__data__") {
                Some(JsValue::String(s)) => {
                    let sliced = s.get(start..end).unwrap_or("");
                    let mut new_map = HashMap::new();
                    new_map.insert("__type__".to_string(), JsValue::String("Blob".to_string()));
                    new_map.insert("__data__".to_string(), JsValue::String(sliced.to_string()));
                    JsValue::Object(new_map)
                }
                _ => JsValue::Object(map.clone()),
            }
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_typed_array_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "length" => {
            let data = match map.get("__data__") {
                Some(JsValue::Array(arr)) => arr.len(),
                _ => 0,
            };
            JsValue::Number(data as f64)
        }
        "byteLength" => {
            let data = match map.get("__data__") {
                Some(JsValue::Array(arr)) => arr.len(),
                _ => 0,
            };
            JsValue::Number(data as f64)
        }
        "byteOffset" => map.get("__offset__").cloned().unwrap_or(JsValue::Number(0.0)),
        "buffer" => map.get("__buffer__").cloned().unwrap_or(JsValue::Undefined),
        "set" => {
            let mut updated = map.clone();
            if let (Some(JsValue::Array(src)), Some(JsValue::Array(data))) = (_args.first(), map.get("__data__")) {
                let offset = _args.get(1).map(to_number).unwrap_or(0.0) as usize;
                let mut new_data = data.clone();
                for (i, v) in src.iter().enumerate() {
                    let idx = offset + i;
                    if idx < new_data.len() { new_data[idx] = v.clone(); }
                    else { new_data.push(v.clone()); }
                }
                updated.insert("__data__".to_string(), JsValue::Array(new_data));
            }
            JsValue::Object(updated)
        }
        "slice" => {
            let start = _args.first().map(to_number).unwrap_or(0.0) as usize;
            let end = _args.get(1).map(to_number).unwrap_or(0.0) as usize;
            let data = match map.get("__data__") {
                Some(JsValue::Array(arr)) => arr.get(start..end).unwrap_or(&[]).to_vec(),
                _ => Vec::new(),
            };
            let mut new_map = HashMap::new();
            new_map.insert("__type__".to_string(), map.get("__type__").cloned().unwrap_or(JsValue::String("TypedArray".to_string())));
            new_map.insert("__data__".to_string(), JsValue::Array(data));
            JsValue::Object(new_map)
        }
        "fill" => {
            let val = _args.first().cloned().unwrap_or(JsValue::Number(0.0));
            let start = _args.get(1).map(to_number).unwrap_or(0.0) as usize;
            let end = _args.get(2).map(to_number).unwrap_or(0.0) as usize;
            let mut updated = map.clone();
            if let Some(JsValue::Array(data)) = map.get("__data__") {
                let mut new_data = data.clone();
                let end_idx = if end == 0 { new_data.len() } else { end.min(new_data.len()) };
                for i in start..end_idx { new_data[i] = val.clone(); }
                updated.insert("__data__".to_string(), JsValue::Array(new_data));
            }
            JsValue::Object(updated)
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_dataview_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "buffer" => map.get("__buffer__").cloned().unwrap_or(JsValue::Undefined),
        "byteLength" => map.get("__byteLength__").cloned().unwrap_or(JsValue::Number(0.0)),
        "byteOffset" => map.get("__byteOffset__").cloned().unwrap_or(JsValue::Number(0.0)),
        "getInt8" | "getUint8" => {
            let offset = _args.first().map(to_number).unwrap_or(0.0) as usize;
            match map.get("__buffer__") {
                Some(JsValue::Object(buf)) => match buf.get("__data__") {
                    Some(JsValue::Array(data)) => data.get(offset).map(to_number).map(JsValue::Number).unwrap_or(JsValue::Number(0.0)),
                    _ => JsValue::Number(0.0),
                },
                _ => JsValue::Number(0.0),
            }
        }
        "setInt8" | "setUint8" => {
            let offset = _args.first().map(to_number).unwrap_or(0.0) as usize;
            let val = _args.get(1).cloned().unwrap_or(JsValue::Number(0.0));
            let mut updated = map.clone();
            if let Some(JsValue::Object(buf)) = map.get("__buffer__") {
                let mut new_buf = buf.clone();
                if let Some(JsValue::Array(data)) = buf.get("__data__") {
                    let mut new_data = data.clone();
                    if offset < new_data.len() { new_data[offset] = val; }
                    new_buf.insert("__data__".to_string(), JsValue::Array(new_data));
                }
                updated.insert("__buffer__".to_string(), JsValue::Object(new_buf));
            }
            JsValue::Object(updated)
        }
        _ => JsValue::Undefined,
    })
}

// ── RegExp methods ───────────────────────────────────────────────────────────

pub(super) fn call_regexp_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> EvalResult {
    let source = map.get("source").map(to_string).unwrap_or_default();
    let flags = map.get("flags").map(to_string).unwrap_or_default();
    let global = flags.contains('g');
    let ignore_case = flags.contains('i');
    
    let pattern = if ignore_case {
        format!("(?i){}", source)
    } else {
        source.clone()
    };
    
    let re = regex::Regex::new(&pattern).ok();
    
    Ok(match method {
        "test" => {
            let s = args.first().map(to_string).unwrap_or_default();
            JsValue::Boolean(re.map(|r| r.is_match(&s)).unwrap_or(false))
        }
        "exec" => {
            let s = args.first().map(to_string).unwrap_or_default();
            if let Some(r) = re {
                if let Some(m) = r.find(&s) {
                    let mut result = vec![JsValue::String(m.as_str().to_string())];
                    result.push(JsValue::Number(m.start() as f64));
                    result.push(JsValue::String(s.to_string()));
                    return Ok(JsValue::Array(result));
                }
            }
            JsValue::Null
        }
        "toString" => JsValue::String(format!("/{}/{}", source, flags)),
        "source" => JsValue::String(source),
        "flags" => JsValue::String(flags),
        "global" => JsValue::Boolean(global),
        "ignoreCase" => JsValue::Boolean(ignore_case),
        "multiline" => JsValue::Boolean(flags.contains('m')),
        _ => JsValue::Undefined,
    })
}
