//! `new X(...)` constructor dispatch, extracted from eval.rs to keep it under
//! the file-size budget. Behavior is identical; new constructors (ES2021+ and
//! Web APIs) are appended at the end of the builtin match.

use super::ast::*;
use super::signal::*;
use super::lexer::lex;
use super::parser::Parser;
use super::coercion::*;
use super::function::*;
use super::eval::{eval_expr_node, eval_stmt, call_class_constructor, PROMISE_CAPTURE};
use super::web_apis2::{make_message_channel, make_event_target};
use super::intl::make_intl_locale;
use crate::js::scope::{Scope, ScopeRef};
use crate::js::vm::JsValue;
use std::collections::HashMap;

pub(super) fn eval_new(callee: &Expr, args: &[Expr], scope: &ScopeRef) -> EvalResult {
    let mut evaluated_args = Vec::new();
    for a in args { evaluated_args.push(eval_expr_node(a, scope)?); }
    let name = match callee {
        Expr::Ident(n) => Some(n.as_str()),
        _ => None,
    };
    // Handle Intl.* member-expression constructors (e.g. new Intl.Segmenter(...))
    let intl_name: Option<String> = match callee {
        Expr::Member(obj, prop) => {
            if let Expr::Ident(ns) = obj.as_ref() {
                if ns.as_str() == "Intl" {
                    Some(format!("Intl.{}", prop))
                } else { None }
            } else { None }
        }
        _ => None,
    };
    match name.or(intl_name.as_deref()) {
        Some("Map") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Map".to_string())); map.insert("__entries__".to_string(), JsValue::Array(Vec::new())); if let Some(JsValue::Array(entries)) = evaluated_args.first() { let mut kvs = Vec::new(); for entry in entries { if let JsValue::Array(kv) = entry { if kv.len() >= 2 { kvs.push(JsValue::Array(vec![kv[0].clone(), kv[1].clone()])); } } } map.insert("__entries__".to_string(), JsValue::Array(kvs)); } Ok(JsValue::Object(map)) }
        Some("Set") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Set".to_string())); let mut items = Vec::new(); if let Some(JsValue::Array(init)) = evaluated_args.first() { for v in init { if !items.iter().any(|x| strict_eq(x, v)) { items.push(v.clone()); } } } map.insert("__items__".to_string(), JsValue::Array(items)); Ok(JsValue::Object(map)) }
        Some("WeakMap") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("WeakMap".to_string())); let mut kvs = Vec::new(); if let Some(JsValue::Array(entries)) = evaluated_args.first() { for entry in entries { if let JsValue::Array(kv) = entry { if kv.len() >= 2 { kvs.push(JsValue::Array(vec![kv[0].clone(), kv[1].clone()])); } } } } map.insert("__entries__".to_string(), JsValue::Array(kvs)); Ok(JsValue::Object(map)) }
        Some("WeakSet") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("WeakSet".to_string())); let mut items = Vec::new(); if let Some(JsValue::Array(init)) = evaluated_args.first() { for v in init { if !items.iter().any(|x| strict_eq(x, v)) { items.push(v.clone()); } } } map.insert("__items__".to_string(), JsValue::Array(items)); Ok(JsValue::Object(map)) }
        Some("Date") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Date".to_string())); let ts = if let Some(arg) = evaluated_args.first() { match arg { JsValue::Number(n) => *n, JsValue::String(s) => s.parse::<f64>().unwrap_or(0.0), _ => std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0), } } else { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0) }; map.insert("__value__".to_string(), JsValue::Number(ts)); Ok(JsValue::Object(map)) }
        Some("Error") | Some("TypeError") | Some("RangeError") | Some("ReferenceError") => { let mut map = HashMap::new(); let msg = evaluated_args.first().map(to_string).unwrap_or_default(); map.insert("message".to_string(), JsValue::String(msg)); map.insert("name".to_string(), JsValue::String(name.unwrap().to_string())); Ok(JsValue::Object(map)) }
        Some("Promise") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Promise".to_string())); if let Some(executor) = evaluated_args.first() { PROMISE_CAPTURE.with(|cap| { *cap.borrow_mut() = None; }); let resolve_fn = JsValue::NativeFunction("__promise_resolve__".to_string()); let reject_fn = JsValue::NativeFunction("__promise_reject__".to_string()); let exec_scope = Scope::new_child(scope); let _ = call_function(executor, &[resolve_fn, reject_fn], &exec_scope); let captured = PROMISE_CAPTURE.with(|cap| cap.borrow().clone()); match captured { Some((false, val)) => { map.insert("__resolved__".to_string(), val); } Some((true, reason)) => { map.insert("__rejected__".to_string(), reason); } None => { map.insert("__resolved__".to_string(), JsValue::Undefined); } } } else { map.insert("__resolved__".to_string(), JsValue::Undefined); } Ok(JsValue::Object(map)) }
        Some("RegExp") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("RegExp".to_string())); let source = evaluated_args.first().map(to_string).unwrap_or_default(); map.insert("source".to_string(), JsValue::String(source)); let flags = evaluated_args.get(1).map(to_string).unwrap_or_default(); let has_g = flags.contains('g'); let has_i = flags.contains('i'); let has_m = flags.contains('m'); map.insert("flags".to_string(), JsValue::String(flags)); map.insert("global".to_string(), JsValue::Boolean(has_g)); map.insert("ignoreCase".to_string(), JsValue::Boolean(has_i)); map.insert("multiline".to_string(), JsValue::Boolean(has_m)); Ok(JsValue::Object(map)) }
        Some("Function") => { let body_str = evaluated_args.last().map(to_string).unwrap_or_default(); let params: Vec<String> = if evaluated_args.len() > 1 { evaluated_args[..evaluated_args.len()-1].iter().map(to_string).collect() } else { Vec::new() }; let body_code = format!("{{ {} }}", body_str); match lex(&body_code).and_then(|tokens| { let mut parser = Parser::new(tokens); parser.parse_block().map_err(|e| e.to_string()) }) { Ok(body) => Ok(JsValue::Function { name: Some("anonymous".to_string()), params, body, closure: scope.clone() }), Err(_) => Ok(JsValue::Undefined), } }
        Some("Proxy") => { let target = evaluated_args.first().cloned().unwrap_or(JsValue::Object(HashMap::new())); let handler = evaluated_args.get(1).cloned().unwrap_or(JsValue::Object(HashMap::new())); Ok(JsValue::Proxy { target: Box::new(target), handler: Box::new(handler) }) }
        Some("URL") => {
            let url_str = evaluated_args.first().map(to_string).unwrap_or_default();
            let base = evaluated_args.get(1).and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None });
            match super::browser_env::make_url(&url_str, base) {
                Ok(url) => Ok(url),
                Err(msg) => Err(Signal::Throw(JsValue::String(format!("TypeError: {}", msg)))),
            }
        }
        Some("URLSearchParams") => {
            let init = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_url_search_params(&init))
        }
        Some("AbortController") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("AbortController".to_string())); let mut signal = HashMap::new(); signal.insert("__type__".to_string(), JsValue::String("AbortSignal".to_string())); signal.insert("aborted".to_string(), JsValue::Boolean(false)); map.insert("signal".to_string(), JsValue::Object(signal)); Ok(JsValue::Object(map)) }
        Some("TextEncoder") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("TextEncoder".to_string())); map.insert("encoding".to_string(), JsValue::String("utf-8".to_string())); Ok(JsValue::Object(map)) }
        Some("TextDecoder") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("TextDecoder".to_string())); map.insert("encoding".to_string(), JsValue::String("utf-8".to_string())); Ok(JsValue::Object(map)) }
        Some("Response") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Response".to_string())); let body = evaluated_args.first().map(to_string).unwrap_or_default(); map.insert("__body__".to_string(), JsValue::String(body)); map.insert("status".to_string(), JsValue::Number(200.0)); map.insert("statusText".to_string(), JsValue::String("OK".to_string())); map.insert("headers".to_string(), JsValue::Object(HashMap::new())); Ok(JsValue::Object(map)) }
        Some("Blob") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Blob".to_string())); let parts = evaluated_args.first().cloned().unwrap_or(JsValue::Undefined); let data = match &parts { JsValue::Array(arr) => { let mut bytes: Vec<JsValue> = Vec::new(); for p in arr { match p { JsValue::String(s) => bytes.extend(s.as_bytes().iter().map(|b| JsValue::Number(*b as f64))), JsValue::Array(b) => bytes.extend(b.iter().filter_map(|v| if let JsValue::Number(n) = v { Some(*n) } else { None }).map(JsValue::Number)), _ => {} } } JsValue::Array(bytes) }, JsValue::String(s) => JsValue::String(s.clone()), _ => JsValue::Array(Vec::new()), }; map.insert("__data__".to_string(), data); let options = evaluated_args.get(1); let mime = if let Some(JsValue::Object(opts)) = options { opts.get("type").map(to_string).unwrap_or_default() } else { String::new() }; map.insert("__mime__".to_string(), JsValue::String(mime)); Ok(JsValue::Object(map)) }
        Some("Uint8Array") | Some("Int8Array") | Some("Uint16Array") | Some("Int16Array") | Some("Uint32Array") | Some("Int32Array") | Some("Float32Array") | Some("Float64Array") | Some("Uint8ClampedArray") => { let mut map = HashMap::new(); let type_name = name.unwrap(); map.insert("__type__".to_string(), JsValue::String(type_name.to_string())); let size = evaluated_args.first().map(to_number).unwrap_or(0.0) as usize; let data: Vec<JsValue> = (0..size).map(|_| JsValue::Number(0.0)).collect(); map.insert("__data__".to_string(), JsValue::Array(data)); Ok(JsValue::Object(map)) }
        Some("DataView") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("DataView".to_string())); let buffer = evaluated_args.first().cloned().unwrap_or(JsValue::Undefined); map.insert("__buffer__".to_string(), buffer); let byte_offset = evaluated_args.get(1).map(to_number).unwrap_or(0.0); map.insert("__byteOffset__".to_string(), JsValue::Number(byte_offset)); let byte_length = evaluated_args.get(2).map(to_number).unwrap_or(0.0); map.insert("__byteLength__".to_string(), JsValue::Number(byte_length)); Ok(JsValue::Object(map)) }
        Some("ArrayBuffer") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("ArrayBuffer".to_string())); let size = evaluated_args.first().map(to_number).unwrap_or(0.0) as usize; let data: Vec<JsValue> = (0..size).map(|_| JsValue::Number(0.0)).collect(); map.insert("__data__".to_string(), JsValue::Array(data)); Ok(JsValue::Object(map)) }
        Some("Intl.Segmenter") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Intl.Segmenter".to_string())); let options = evaluated_args.first().cloned(); if let Some(JsValue::Object(opts)) = options { for (k, v) in opts { if k == "locale" || k == "granularity" { map.insert(k, v); } } } Ok(JsValue::Object(map)) }
        Some("Intl.Collator") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Intl.Collator".to_string())); let options = evaluated_args.first().cloned(); if let Some(JsValue::Object(opts)) = options { for (k, v) in opts { if k == "locale" || k == "sensitivity" { map.insert(k, v); } } } Ok(JsValue::Object(map)) }
        Some("Intl.NumberFormat") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Intl.NumberFormat".to_string())); let options = evaluated_args.first().cloned(); if let Some(JsValue::Object(opts)) = options { for (k, v) in opts { map.insert(k, v); } } Ok(JsValue::Object(map)) }
        Some("Intl.DateTimeFormat") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Intl.DateTimeFormat".to_string())); let options = evaluated_args.first().cloned(); if let Some(JsValue::Object(opts)) = options { for (k, v) in opts { map.insert(k, v); } } Ok(JsValue::Object(map)) }
        Some("Intl.PluralRules") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Intl.PluralRules".to_string())); let options = evaluated_args.first().cloned(); if let Some(JsValue::Object(opts)) = options { for (k, v) in opts { map.insert(k, v); } } Ok(JsValue::Object(map)) }
        Some("Intl.RelativeTimeFormat") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Intl.RelativeTimeFormat".to_string())); let options = evaluated_args.first().cloned(); if let Some(JsValue::Object(opts)) = options { for (k, v) in opts { map.insert(k, v); } } Ok(JsValue::Object(map)) }
        Some("Intl.DurationFormat") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Intl.DurationFormat".to_string())); let options = evaluated_args.first().cloned(); if let Some(JsValue::Object(opts)) = options { for (k, v) in opts { map.insert(k, v); } } Ok(JsValue::Object(map)) }
        Some("Intl.ListFormat") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Intl.ListFormat".to_string())); let options = evaluated_args.first().cloned(); if let Some(JsValue::Object(opts)) = options { for (k, v) in opts { map.insert(k, v); } } Ok(JsValue::Object(map)) }
        Some("Intl.DisplayNames") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("Intl.DisplayNames".to_string())); let options = evaluated_args.first().cloned(); if let Some(JsValue::Object(opts)) = options { for (k, v) in opts { map.insert(k, v); } } Ok(JsValue::Object(map)) }
        Some("Intl.Locale") => Ok(make_intl_locale(&evaluated_args)),
        Some("MessageChannel") => Ok(make_message_channel()),
        Some("EventTarget") => Ok(make_event_target()),
        // Pragmatic WeakRef: no GC in this engine, so it holds a strong clone.
        Some("WeakRef") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("WeakRef".to_string())); map.insert("__target__".to_string(), evaluated_args.first().cloned().unwrap_or(JsValue::Undefined)); Ok(JsValue::Object(map)) }
        // Pragmatic FinalizationRegistry: cleanup callbacks never fire.
        Some("FinalizationRegistry") => { let mut map = HashMap::new(); map.insert("__type__".to_string(), JsValue::String("FinalizationRegistry".to_string())); map.insert("__callback__".to_string(), evaluated_args.first().cloned().unwrap_or(JsValue::Undefined)); Ok(JsValue::Object(map)) }
        // Browser environment constructors.
        Some("Headers") => Ok(super::browser_env::make_headers(evaluated_args.first())),
        Some("FormData") => Ok(super::browser_env::make_form_data()),
        Some("Event") => {
            let event_type = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_event(&event_type, evaluated_args.get(1)))
        }
        Some("CustomEvent") => {
            let event_type = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_custom_event(&event_type, evaluated_args.get(1)))
        }
        Some("DOMParser") => Ok(super::browser_env::make_dom_parser()),
        Some("XMLHttpRequest") => Ok(super::browser_env::make_xhr()),
        Some("MutationObserver") => {
            let callback = evaluated_args.first().cloned().unwrap_or(JsValue::Undefined);
            Ok(super::browser_env::make_mutation_observer(callback))
        }
        Some("BroadcastChannel") => {
            let name = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_broadcast_channel(&name))
        }
        Some("IntersectionObserver") => {
            let callback = evaluated_args.first().cloned().unwrap_or(JsValue::Undefined);
            Ok(super::web_platform::make_intersection_observer(callback, evaluated_args.get(1)))
        }
        Some("ResizeObserver") => {
            let callback = evaluated_args.first().cloned().unwrap_or(JsValue::Undefined);
            Ok(super::web_platform::make_resize_observer(callback))
        }
        Some("WebSocket") => {
            let url = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::web_platform::make_web_socket(&url, evaluated_args.get(1)))
        }
        Some("FileReader") => Ok(super::web_platform::make_file_reader()),
        Some("CSSStyleSheet") => Ok(super::web_platform::make_css_style_sheet()),
        Some("DOMRect") => {
            let x = evaluated_args.first().map(to_number).unwrap_or(0.0);
            let y = evaluated_args.get(1).map(to_number).unwrap_or(0.0);
            let w = evaluated_args.get(2).map(to_number).unwrap_or(0.0);
            let h = evaluated_args.get(3).map(to_number).unwrap_or(0.0);
            Ok(super::web_platform::make_dom_rect(x, y, w, h))
        }
        Some("ReadableStream") => Ok(super::streams::make_readable_stream(evaluated_args.first())),
        Some("WritableStream") => Ok(super::streams::make_writable_stream(evaluated_args.first())),
        Some("TransformStream") => Ok(super::streams::make_transform_stream(evaluated_args.first())),
        Some("CountQueuingStrategy") => {
            let hwm = evaluated_args.first().and_then(|v| if let JsValue::Object(m) = v { m.get("highWaterMark").map(to_number) } else { None }).unwrap_or(1.0);
            Ok(super::streams::make_count_queuing_strategy(hwm))
        }
        Some("ByteLengthQueuingStrategy") => {
            let hwm = evaluated_args.first().and_then(|v| if let JsValue::Object(m) = v { m.get("highWaterMark").map(to_number) } else { None }).unwrap_or(1.0);
            Ok(super::streams::make_byte_length_queuing_strategy(hwm))
        }
        Some("Worker") => {
            let url = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::web_platform::make_worker(&url))
        }
        Some("SharedWorker") => {
            let url = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::web_platform::make_shared_worker(&url))
        }
        Some("Path2D") => Ok(super::canvas::make_path_2d()),
        Some("OffscreenCanvas") => {
            let w = evaluated_args.first().map(to_number).unwrap_or(300.0) as u32;
            let h = evaluated_args.get(1).map(to_number).unwrap_or(150.0) as u32;
            Ok(super::canvas::make_offscreen_canvas(w, h))
        }
        Some("DOMException") => {
            let message = evaluated_args.first().map(to_string).unwrap_or_default();
            let name = evaluated_args.get(1).map(to_string).unwrap_or_else(|| "Error".into());
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("DOMException".to_string()));
            map.insert("message".to_string(), JsValue::String(message));
            map.insert("name".to_string(), JsValue::String(name));
            map.insert("code".to_string(), JsValue::Number(0.0));
            Ok(JsValue::Object(map))
        }
        Some("Text") => {
            let data = evaluated_args.first().map(to_string).unwrap_or_default();
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Text".to_string()));
            map.insert("data".to_string(), JsValue::String(data.clone()));
            map.insert("textContent".to_string(), JsValue::String(data.clone()));
            map.insert("wholeText".to_string(), JsValue::String(data));
            map.insert("nodeType".to_string(), JsValue::Number(3.0));
            map.insert("length".to_string(), JsValue::Number(0.0));
            Ok(JsValue::Object(map))
        }
        Some("Comment") => {
            let data = evaluated_args.first().map(to_string).unwrap_or_default();
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Comment".to_string()));
            map.insert("data".to_string(), JsValue::String(data.clone()));
            map.insert("textContent".to_string(), JsValue::String(data));
            map.insert("nodeType".to_string(), JsValue::Number(8.0));
            Ok(JsValue::Object(map))
        }
        Some("DocumentFragment") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("DocumentFragment".to_string()));
            map.insert("childElementCount".to_string(), JsValue::Number(0.0));
            Ok(JsValue::Object(map))
        }
        Some("DOMMatrix") => {
            let a = evaluated_args.first().map(to_number).unwrap_or(1.0);
            let b = evaluated_args.get(1).map(to_number).unwrap_or(0.0);
            let c = evaluated_args.get(2).map(to_number).unwrap_or(0.0);
            let d = evaluated_args.get(3).map(to_number).unwrap_or(1.0);
            let e = evaluated_args.get(4).map(to_number).unwrap_or(0.0);
            let f = evaluated_args.get(5).map(to_number).unwrap_or(0.0);
            Ok(super::web_platform::make_dom_matrix(a, b, c, d, e, f))
        }
        Some("PerformanceObserver") => {
            let callback = evaluated_args.first().cloned().unwrap_or(JsValue::Undefined);
            Ok(super::web_platform::make_performance_observer(callback))
        }
        Some("AudioContext") | Some("webkitAudioContext") => {
            let mut ctx = HashMap::new();
            ctx.insert("__type__".to_string(), JsValue::String("AudioContext".to_string()));
            ctx.insert("state".to_string(), JsValue::String("running".to_string()));
            ctx.insert("currentTime".to_string(), JsValue::Number(0.0));
            ctx.insert("sampleRate".to_string(), JsValue::Number(44100.0));
            ctx.insert("baseLatency".to_string(), JsValue::Number(0.01));
            Ok(JsValue::Object(ctx))
        }
        Some("SpeechSynthesisUtterance") => {
            let text = evaluated_args.first().map(to_string).unwrap_or_default();
            let mut utt = HashMap::new();
            utt.insert("__type__".to_string(), JsValue::String("SpeechSynthesisUtterance".to_string()));
            utt.insert("text".to_string(), JsValue::String(text));
            utt.insert("lang".to_string(), JsValue::String(String::new()));
            utt.insert("pitch".to_string(), JsValue::Number(1.0));
            utt.insert("rate".to_string(), JsValue::Number(1.0));
            utt.insert("volume".to_string(), JsValue::Number(1.0));
            Ok(JsValue::Object(utt))
        }
        Some("Notification") => {
            let title = evaluated_args.first().map(to_string).unwrap_or_default();
            let mut notif = HashMap::new();
            notif.insert("__type__".to_string(), JsValue::String("Notification".to_string()));
            notif.insert("title".to_string(), JsValue::String(title));
            notif.insert("body".to_string(), evaluated_args.get(1).and_then(|v| if let JsValue::Object(m) = v { m.get("body").cloned() } else { None }).unwrap_or(JsValue::String(String::new())));
            notif.insert("tag".to_string(), JsValue::String(String::new()));
            notif.insert("icon".to_string(), JsValue::String(String::new()));
            Ok(JsValue::Object(notif))
        }
        // ── Specialized Event constructors ─────────────────────────────────────
        Some("UIEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("UIEvent", &et, evaluated_args.get(1), &[
                ("detail", JsValue::Number(0.0)),
                ("view", JsValue::Null),
            ]))
        }
        Some("FocusEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("FocusEvent", &et, evaluated_args.get(1), &[
                ("relatedTarget", JsValue::Null),
            ]))
        }
        Some("MouseEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("MouseEvent", &et, evaluated_args.get(1), &[
                ("clientX", JsValue::Number(0.0)),
                ("clientY", JsValue::Number(0.0)),
                ("screenX", JsValue::Number(0.0)),
                ("screenY", JsValue::Number(0.0)),
                ("button", JsValue::Number(0.0)),
                ("buttons", JsValue::Number(0.0)),
                ("altKey", JsValue::Boolean(false)),
                ("ctrlKey", JsValue::Boolean(false)),
                ("shiftKey", JsValue::Boolean(false)),
                ("metaKey", JsValue::Boolean(false)),
                ("relatedTarget", JsValue::Null),
            ]))
        }
        Some("PointerEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("PointerEvent", &et, evaluated_args.get(1), &[
                ("clientX", JsValue::Number(0.0)),
                ("clientY", JsValue::Number(0.0)),
                ("screenX", JsValue::Number(0.0)),
                ("screenY", JsValue::Number(0.0)),
                ("button", JsValue::Number(0.0)),
                ("buttons", JsValue::Number(0.0)),
                ("pointerId", JsValue::Number(0.0)),
                ("width", JsValue::Number(1.0)),
                ("height", JsValue::Number(1.0)),
                ("pressure", JsValue::Number(0.0)),
                ("pointerType", JsValue::String("mouse".to_string())),
                ("isPrimary", JsValue::Boolean(true)),
                ("altKey", JsValue::Boolean(false)),
                ("ctrlKey", JsValue::Boolean(false)),
                ("shiftKey", JsValue::Boolean(false)),
                ("metaKey", JsValue::Boolean(false)),
                ("relatedTarget", JsValue::Null),
            ]))
        }
        Some("KeyboardEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("KeyboardEvent", &et, evaluated_args.get(1), &[
                ("key", JsValue::String(String::new())),
                ("code", JsValue::String(String::new())),
                ("location", JsValue::Number(0.0)),
                ("repeat", JsValue::Boolean(false)),
                ("isComposing", JsValue::Boolean(false)),
                ("altKey", JsValue::Boolean(false)),
                ("ctrlKey", JsValue::Boolean(false)),
                ("shiftKey", JsValue::Boolean(false)),
                ("metaKey", JsValue::Boolean(false)),
            ]))
        }
        Some("InputEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("InputEvent", &et, evaluated_args.get(1), &[
                ("data", JsValue::Null),
                ("inputType", JsValue::String("insertText".to_string())),
                ("isComposing", JsValue::Boolean(false)),
                ("targetRanges", JsValue::Array(Vec::new())),
            ]))
        }
        Some("CompositionEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("CompositionEvent", &et, evaluated_args.get(1), &[
                ("data", JsValue::String(String::new())),
            ]))
        }
        Some("WheelEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("WheelEvent", &et, evaluated_args.get(1), &[
                ("deltaX", JsValue::Number(0.0)),
                ("deltaY", JsValue::Number(0.0)),
                ("deltaZ", JsValue::Number(0.0)),
                ("deltaMode", JsValue::Number(0.0)),
            ]))
        }
        Some("DragEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            let mut dt = HashMap::new();
            dt.insert("__type__".to_string(), JsValue::String("DataTransfer".to_string()));
            dt.insert("dropEffect".to_string(), JsValue::String("none".to_string()));
            dt.insert("effectAllowed".to_string(), JsValue::String("uninitialized".to_string()));
            dt.insert("types".to_string(), JsValue::Array(Vec::new()));
            Ok(super::browser_env::make_typed_event("DragEvent", &et, evaluated_args.get(1), &[
                ("dataTransfer", JsValue::Object(dt)),
            ]))
        }
        Some("ClipboardEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            let mut dt = HashMap::new();
            dt.insert("__type__".to_string(), JsValue::String("DataTransfer".to_string()));
            dt.insert("dropEffect".to_string(), JsValue::String("none".to_string()));
            dt.insert("effectAllowed".to_string(), JsValue::String("uninitialized".to_string()));
            dt.insert("types".to_string(), JsValue::Array(Vec::new()));
            Ok(super::browser_env::make_typed_event("ClipboardEvent", &et, evaluated_args.get(1), &[
                ("clipboardData", JsValue::Object(dt)),
            ]))
        }
        Some("ErrorEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("ErrorEvent", &et, evaluated_args.get(1), &[
                ("message", JsValue::String(String::new())),
                ("filename", JsValue::String(String::new())),
                ("lineno", JsValue::Number(0.0)),
                ("colno", JsValue::Number(0.0)),
                ("error", JsValue::Null),
            ]))
        }
        Some("MessageEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("MessageEvent", &et, evaluated_args.get(1), &[
                ("data", JsValue::Null),
                ("origin", JsValue::String(String::new())),
                ("lastEventId", JsValue::String(String::new())),
                ("source", JsValue::Null),
                ("ports", JsValue::Array(Vec::new())),
            ]))
        }
        Some("StorageEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("StorageEvent", &et, evaluated_args.get(1), &[
                ("key", JsValue::Null),
                ("oldValue", JsValue::Null),
                ("newValue", JsValue::Null),
                ("url", JsValue::String(String::new())),
                ("storageArea", JsValue::Null),
            ]))
        }
        Some("HashChangeEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("HashChangeEvent", &et, evaluated_args.get(1), &[
                ("oldURL", JsValue::String(String::new())),
                ("newURL", JsValue::String(String::new())),
            ]))
        }
        Some("PopStateEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("PopStateEvent", &et, evaluated_args.get(1), &[
                ("state", JsValue::Null),
            ]))
        }
        Some("PageTransitionEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("PageTransitionEvent", &et, evaluated_args.get(1), &[
                ("persisted", JsValue::Boolean(false)),
            ]))
        }
        Some("ProgressEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("ProgressEvent", &et, evaluated_args.get(1), &[
                ("lengthComputable", JsValue::Boolean(false)),
                ("loaded", JsValue::Number(0.0)),
                ("total", JsValue::Number(0.0)),
            ]))
        }
        Some("SubmitEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("SubmitEvent", &et, evaluated_args.get(1), &[
                ("submitter", JsValue::Null),
            ]))
        }
        Some("FormDataEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("FormDataEvent", &et, evaluated_args.get(1), &[
                ("formData", super::browser_env::make_form_data()),
            ]))
        }
        Some("AnimationEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("AnimationEvent", &et, evaluated_args.get(1), &[
                ("animationName", JsValue::String(String::new())),
                ("elapsedTime", JsValue::Number(0.0)),
                ("pseudoElement", JsValue::String(String::new())),
            ]))
        }
        Some("TransitionEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("TransitionEvent", &et, evaluated_args.get(1), &[
                ("propertyName", JsValue::String(String::new())),
                ("elapsedTime", JsValue::Number(0.0)),
                ("pseudoElement", JsValue::String(String::new())),
            ]))
        }
        Some("SecurityPolicyViolationEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("SecurityPolicyViolationEvent", &et, evaluated_args.get(1), &[
                ("documentURI", JsValue::String(String::new())),
                ("violatedDirective", JsValue::String(String::new())),
                ("blockedURI", JsValue::String(String::new())),
                ("disposition", JsValue::String("enforce".to_string())),
            ]))
        }
        Some("TouchEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("TouchEvent", &et, evaluated_args.get(1), &[
                ("touches", JsValue::Array(Vec::new())),
                ("targetTouches", JsValue::Array(Vec::new())),
                ("changedTouches", JsValue::Array(Vec::new())),
                ("altKey", JsValue::Boolean(false)),
                ("ctrlKey", JsValue::Boolean(false)),
                ("shiftKey", JsValue::Boolean(false)),
                ("metaKey", JsValue::Boolean(false)),
            ]))
        }
        Some("BeforeUnloadEvent") => {
            let et = evaluated_args.first().map(to_string).unwrap_or_default();
            Ok(super::browser_env::make_typed_event("BeforeUnloadEvent", &et, evaluated_args.get(1), &[
                ("returnValue", JsValue::String(String::new())),
            ]))
        }
        Some("DataTransfer") => {
            let mut dt = HashMap::new();
            dt.insert("__type__".to_string(), JsValue::String("DataTransfer".to_string()));
            dt.insert("dropEffect".to_string(), JsValue::String("none".to_string()));
            dt.insert("effectAllowed".to_string(), JsValue::String("uninitialized".to_string()));
            dt.insert("types".to_string(), JsValue::Array(Vec::new()));
            dt.insert("files".to_string(), JsValue::Array(Vec::new()));
            Ok(JsValue::Object(dt))
        }
        Some("DataTransferItem") => {
            let mut item = HashMap::new();
            item.insert("__type__".to_string(), JsValue::String("DataTransferItem".to_string()));
            item.insert("kind".to_string(), JsValue::String("string".to_string()));
            item.insert("type".to_string(), JsValue::String(String::new()));
            Ok(JsValue::Object(item))
        }
        _ => {
            let callee_val = eval_expr_node(callee, scope)?;
            if let JsValue::Object(class_map) = &callee_val {
                let is_class = class_map.get("__type__").map(|v| matches!(v, JsValue::String(s) if s == "class")).unwrap_or(false);
                if is_class { return call_class_constructor(class_map, &evaluated_args, scope); }
            }
            if let JsValue::Function { params, body, closure, .. } = &callee_val {
                let call_scope = Scope::new_child(closure);
                let this_obj = JsValue::Object(HashMap::new());
                Scope::declare(&call_scope, "this", this_obj.clone());
                for (i, p) in params.iter().enumerate() { let val = evaluated_args.get(i).cloned().unwrap_or(JsValue::Undefined); Scope::declare(&call_scope, p, val); }
                Scope::declare(&call_scope, "arguments", JsValue::Array(evaluated_args));
                match eval_stmt(body, &call_scope) {
                    Ok(_) | Err(Signal::Return(JsValue::Undefined)) => Ok(Scope::resolve(&call_scope, "this").unwrap_or(this_obj)),
                    Err(Signal::Return(v)) => { if matches!(v, JsValue::Object(_)) { Ok(v) } else { Ok(Scope::resolve(&call_scope, "this").unwrap_or(JsValue::Object(HashMap::new()))) } }
                    Err(e) => Err(e),
                }
            } else { Ok(JsValue::Object(HashMap::new())) }
        }
    }
}
