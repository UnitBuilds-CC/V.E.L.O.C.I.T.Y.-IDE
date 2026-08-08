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

#[cfg(test)]
mod tests {
    use super::*;

    fn get_obj(v: &JsValue) -> &HashMap<String, JsValue> {
        match v {
            JsValue::Object(m) => m,
            _ => panic!("expected Object"),
        }
    }

    fn get_str(v: &JsValue) -> &str {
        match v {
            JsValue::String(s) => s.as_str(),
            _ => panic!("expected String"),
        }
    }

    fn get_num(v: &JsValue) -> f64 {
        match v {
            JsValue::Number(n) => *n,
            _ => panic!("expected Number"),
        }
    }

    fn get_bool(v: &JsValue) -> bool {
        match v {
            JsValue::Boolean(b) => *b,
            _ => panic!("expected Boolean"),
        }
    }

    fn unwrap(v: EvalResult) -> JsValue {
        v.expect("expected Ok")
    }

    // ── call_abort_controller_method ─────────────────────────────────────

    #[test]
    fn abort_controller_abort() {
        let mut controller = HashMap::new();
        let mut signal = HashMap::new();
        signal.insert("aborted".to_string(), JsValue::Boolean(false));
        controller.insert("signal".to_string(), JsValue::Object(signal));
        
        let result = unwrap(call_abort_controller_method(&controller, "abort", &[]));
        let m = get_obj(&result);
        if let JsValue::Object(sig) = m.get("signal").unwrap() {
            assert!(get_bool(sig.get("aborted").unwrap()));
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn abort_controller_unknown_method() {
        let mut controller = HashMap::new();
        let signal = HashMap::new();
        controller.insert("signal".to_string(), JsValue::Object(signal));
        
        let result = unwrap(call_abort_controller_method(&controller, "unknownMethod", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_abort_signal_method ─────────────────────────────────────────

    #[test]
    fn abort_signal_not_aborted() {
        let mut signal = HashMap::new();
        signal.insert("aborted".to_string(), JsValue::Boolean(false));
        
        let result = unwrap(call_abort_signal_method(&signal, "aborted", &[]));
        assert!(!get_bool(&result));
    }

    #[test]
    fn abort_signal_is_aborted() {
        let mut signal = HashMap::new();
        signal.insert("aborted".to_string(), JsValue::Boolean(true));
        
        let result = unwrap(call_abort_signal_method(&signal, "aborted", &[]));
        assert!(get_bool(&result));
    }

    #[test]
    fn abort_signal_throw_if_aborted_false() {
        let mut signal = HashMap::new();
        signal.insert("aborted".to_string(), JsValue::Boolean(false));
        
        let result = unwrap(call_abort_signal_method(&signal, "throwIfAborted", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }

    #[test]
    fn abort_signal_throw_if_aborted_true() {
        let mut signal = HashMap::new();
        signal.insert("aborted".to_string(), JsValue::Boolean(true));
        
        let result = call_abort_signal_method(&signal, "throwIfAborted", &[]);
        match result {
            Err(Signal::Throw(JsValue::String(s))) => assert_eq!(s, "AbortError"),
            _ => panic!("expected Throw signal"),
        }
    }

    #[test]
    fn abort_signal_unknown_method() {
        let signal = HashMap::new();
        let result = unwrap(call_abort_signal_method(&signal, "unknownMethod", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_text_encoder_method ─────────────────────────────────────────

    #[test]
    fn text_encoder_encode() {
        let encoder = HashMap::new();
        let result = unwrap(call_text_encoder_method(&encoder, "encode", &[JsValue::String("hello".into())]));
        if let JsValue::Array(bytes) = result {
            assert_eq!(bytes.len(), 5);
            assert_eq!(get_num(&bytes[0]), 104.0); // 'h'
            assert_eq!(get_num(&bytes[1]), 101.0); // 'e'
            assert_eq!(get_num(&bytes[2]), 108.0); // 'l'
            assert_eq!(get_num(&bytes[3]), 108.0); // 'l'
            assert_eq!(get_num(&bytes[4]), 111.0); // 'o'
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn text_encoder_encode_empty() {
        let encoder = HashMap::new();
        let result = unwrap(call_text_encoder_method(&encoder, "encode", &[JsValue::String("".into())]));
        if let JsValue::Array(bytes) = result {
            assert_eq!(bytes.len(), 0);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn text_encoder_encoding() {
        let encoder = HashMap::new();
        let result = unwrap(call_text_encoder_method(&encoder, "encoding", &[]));
        assert_eq!(get_str(&result), "utf-8");
    }

    #[test]
    fn text_encoder_unknown_method() {
        let encoder = HashMap::new();
        let result = unwrap(call_text_encoder_method(&encoder, "unknownMethod", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_text_decoder_method ─────────────────────────────────────────

    #[test]
    fn text_decoder_decode() {
        let decoder = HashMap::new();
        let bytes = JsValue::Array(vec![
            JsValue::Number(104.0),
            JsValue::Number(101.0),
            JsValue::Number(108.0),
            JsValue::Number(108.0),
            JsValue::Number(111.0),
        ]);
        let result = unwrap(call_text_decoder_method(&decoder, "decode", &[bytes]));
        assert_eq!(get_str(&result), "hello");
    }

    #[test]
    fn text_decoder_decode_empty() {
        let decoder = HashMap::new();
        let bytes = JsValue::Array(vec![]);
        let result = unwrap(call_text_decoder_method(&decoder, "decode", &[bytes]));
        assert_eq!(get_str(&result), "");
    }

    #[test]
    fn text_decoder_encoding() {
        let decoder = HashMap::new();
        let result = unwrap(call_text_decoder_method(&decoder, "encoding", &[]));
        assert_eq!(get_str(&result), "utf-8");
    }

    #[test]
    fn text_decoder_unknown_method() {
        let decoder = HashMap::new();
        let result = unwrap(call_text_decoder_method(&decoder, "unknownMethod", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_response_method ─────────────────────────────────────────────

    #[test]
    fn response_ok_true() {
        let mut response = HashMap::new();
        response.insert("status".to_string(), JsValue::Number(200.0));
        
        let result = unwrap(call_response_method(&response, "ok", &[]));
        assert!(get_bool(&result));
    }

    #[test]
    fn response_ok_false_404() {
        let mut response = HashMap::new();
        response.insert("status".to_string(), JsValue::Number(404.0));
        
        let result = unwrap(call_response_method(&response, "ok", &[]));
        assert!(!get_bool(&result));
    }

    #[test]
    fn response_status() {
        let mut response = HashMap::new();
        response.insert("status".to_string(), JsValue::Number(201.0));
        
        let result = unwrap(call_response_method(&response, "status", &[]));
        assert_eq!(get_num(&result), 201.0);
    }

    #[test]
    fn response_status_default() {
        let response = HashMap::new();
        let result = unwrap(call_response_method(&response, "status", &[]));
        assert_eq!(get_num(&result), 200.0);
    }

    #[test]
    fn response_status_text() {
        let mut response = HashMap::new();
        response.insert("statusText".to_string(), JsValue::String("Created".into()));
        
        let result = unwrap(call_response_method(&response, "statusText", &[]));
        assert_eq!(get_str(&result), "Created");
    }

    #[test]
    fn response_text() {
        let mut response = HashMap::new();
        response.insert("__body__".to_string(), JsValue::String("hello world".into()));
        
        let result = unwrap(call_response_method(&response, "text", &[]));
        assert_eq!(get_str(&result), "hello world");
    }

    #[test]
    fn response_clone() {
        let mut response = HashMap::new();
        response.insert("status".to_string(), JsValue::Number(200.0));
        response.insert("__body__".to_string(), JsValue::String("test".into()));
        
        let result = unwrap(call_response_method(&response, "clone", &[]));
        let cloned = get_obj(&result);
        assert_eq!(get_num(cloned.get("status").unwrap()), 200.0);
    }

    #[test]
    fn response_unknown_method() {
        let response = HashMap::new();
        let result = unwrap(call_response_method(&response, "unknownMethod", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_blob_method ─────────────────────────────────────────────────

    #[test]
    fn blob_size_string() {
        let mut blob = HashMap::new();
        blob.insert("__data__".to_string(), JsValue::String("hello".into()));
        
        let result = unwrap(call_blob_method(&blob, "size", &[]));
        assert_eq!(get_num(&result), 5.0);
    }

    #[test]
    fn blob_size_array() {
        let mut blob = HashMap::new();
        blob.insert("__data__".to_string(), JsValue::Array(vec![
            JsValue::Number(1.0),
            JsValue::Number(2.0),
            JsValue::Number(3.0),
        ]));
        
        let result = unwrap(call_blob_method(&blob, "size", &[]));
        assert_eq!(get_num(&result), 3.0);
    }

    #[test]
    fn blob_type() {
        let mut blob = HashMap::new();
        blob.insert("__mime__".to_string(), JsValue::String("text/plain".into()));
        
        let result = unwrap(call_blob_method(&blob, "type", &[]));
        assert_eq!(get_str(&result), "text/plain");
    }

    #[test]
    fn blob_text_string() {
        let mut blob = HashMap::new();
        blob.insert("__data__".to_string(), JsValue::String("hello".into()));
        
        let result = unwrap(call_blob_method(&blob, "text", &[]));
        assert_eq!(get_str(&result), "hello");
    }

    #[test]
    fn blob_text_array() {
        let mut blob = HashMap::new();
        blob.insert("__data__".to_string(), JsValue::Array(vec![
            JsValue::Number(104.0),
            JsValue::Number(101.0),
            JsValue::Number(108.0),
            JsValue::Number(108.0),
            JsValue::Number(111.0),
        ]));
        
        let result = unwrap(call_blob_method(&blob, "text", &[]));
        assert_eq!(get_str(&result), "hello");
    }

    #[test]
    fn blob_unknown_method() {
        let blob = HashMap::new();
        let result = unwrap(call_blob_method(&blob, "unknownMethod", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_typed_array_method ──────────────────────────────────────────

    #[test]
    fn typed_array_length() {
        let mut arr = HashMap::new();
        arr.insert("__data__".to_string(), JsValue::Array(vec![
            JsValue::Number(1.0),
            JsValue::Number(2.0),
            JsValue::Number(3.0),
        ]));
        
        let result = unwrap(call_typed_array_method(&arr, "length", &[]));
        assert_eq!(get_num(&result), 3.0);
    }

    #[test]
    fn typed_array_byte_length() {
        let mut arr = HashMap::new();
        arr.insert("__data__".to_string(), JsValue::Array(vec![
            JsValue::Number(1.0),
            JsValue::Number(2.0),
        ]));
        
        let result = unwrap(call_typed_array_method(&arr, "byteLength", &[]));
        assert_eq!(get_num(&result), 2.0);
    }

    #[test]
    fn typed_array_byte_offset() {
        let mut arr = HashMap::new();
        arr.insert("__offset__".to_string(), JsValue::Number(8.0));
        
        let result = unwrap(call_typed_array_method(&arr, "byteOffset", &[]));
        assert_eq!(get_num(&result), 8.0);
    }

    #[test]
    fn typed_array_unknown_method() {
        let arr = HashMap::new();
        let result = unwrap(call_typed_array_method(&arr, "unknownMethod", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_dataview_method ─────────────────────────────────────────────

    #[test]
    fn dataview_byte_length() {
        let mut view = HashMap::new();
        view.insert("__byteLength__".to_string(), JsValue::Number(16.0));
        
        let result = unwrap(call_dataview_method(&view, "byteLength", &[]));
        assert_eq!(get_num(&result), 16.0);
    }

    #[test]
    fn dataview_byte_offset() {
        let mut view = HashMap::new();
        view.insert("__byteOffset__".to_string(), JsValue::Number(4.0));
        
        let result = unwrap(call_dataview_method(&view, "byteOffset", &[]));
        assert_eq!(get_num(&result), 4.0);
    }

    #[test]
    fn dataview_unknown_method() {
        let view = HashMap::new();
        let result = unwrap(call_dataview_method(&view, "unknownMethod", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_regexp_method ───────────────────────────────────────────────

    #[test]
    fn regexp_test_match() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("hello".into()));
        regexp.insert("flags".to_string(), JsValue::String("".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "test", &[JsValue::String("hello world".into())]));
        assert!(get_bool(&result));
    }

    #[test]
    fn regexp_test_no_match() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("xyz".into()));
        regexp.insert("flags".to_string(), JsValue::String("".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "test", &[JsValue::String("hello world".into())]));
        assert!(!get_bool(&result));
    }

    #[test]
    fn regexp_test_case_insensitive() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("hello".into()));
        regexp.insert("flags".to_string(), JsValue::String("i".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "test", &[JsValue::String("HELLO world".into())]));
        assert!(get_bool(&result));
    }

    #[test]
    fn regexp_exec_match() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("hello".into()));
        regexp.insert("flags".to_string(), JsValue::String("".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "exec", &[JsValue::String("hello world".into())]));
        if let JsValue::Array(arr) = result {
            assert_eq!(get_str(&arr[0]), "hello");
            assert_eq!(get_num(&arr[1]), 0.0); // start index
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn regexp_exec_no_match() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("xyz".into()));
        regexp.insert("flags".to_string(), JsValue::String("".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "exec", &[JsValue::String("hello world".into())]));
        assert!(matches!(result, JsValue::Null));
    }

    #[test]
    fn regexp_to_string() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("hello".into()));
        regexp.insert("flags".to_string(), JsValue::String("gi".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "toString", &[]));
        assert_eq!(get_str(&result), "/hello/gi");
    }

    #[test]
    fn regexp_source() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("test".into()));
        regexp.insert("flags".to_string(), JsValue::String("".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "source", &[]));
        assert_eq!(get_str(&result), "test");
    }

    #[test]
    fn regexp_flags() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("test".into()));
        regexp.insert("flags".to_string(), JsValue::String("gim".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "flags", &[]));
        assert_eq!(get_str(&result), "gim");
    }

    #[test]
    fn regexp_global() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("test".into()));
        regexp.insert("flags".to_string(), JsValue::String("g".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "global", &[]));
        assert!(get_bool(&result));
    }

    #[test]
    fn regexp_ignore_case() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("test".into()));
        regexp.insert("flags".to_string(), JsValue::String("i".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "ignoreCase", &[]));
        assert!(get_bool(&result));
    }

    #[test]
    fn regexp_multiline() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("test".into()));
        regexp.insert("flags".to_string(), JsValue::String("m".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "multiline", &[]));
        assert!(get_bool(&result));
    }

    #[test]
    fn regexp_unknown_method() {
        let mut regexp = HashMap::new();
        regexp.insert("source".to_string(), JsValue::String("test".into()));
        regexp.insert("flags".to_string(), JsValue::String("".into()));
        
        let result = unwrap(call_regexp_method(&regexp, "unknownMethod", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }
}
