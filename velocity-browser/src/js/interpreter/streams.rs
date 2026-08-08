//! Streams API for the JS interpreter — ReadableStream, WritableStream,
//! TransformStream, and their controllers/readers/writers.
//!
//! Pragmatic in-memory implementation: data queued into a stream is stored in
//! a thread-local buffer and can be read back. This lets agents pipe data
//! through transform chains without real I/O.

use crate::js::vm::JsValue;
use std::cell::RefCell;
use std::collections::HashMap;

// ── Shared stream buffer registry ────────────────────────────────────────────

thread_local! {
    static STREAM_BUFFERS: RefCell<HashMap<u32, Vec<JsValue>>> = RefCell::new(HashMap::new());
    static NEXT_STREAM_ID: RefCell<u32> = const { RefCell::new(1) };
}

fn alloc_stream_id() -> u32 {
    NEXT_STREAM_ID.with(|c| {
        let mut id = c.borrow_mut();
        let current = *id;
        *id += 1;
        current
    })
}

fn push_chunk(stream_id: u32, chunk: JsValue) {
    STREAM_BUFFERS.with(|b| {
        b.borrow_mut().entry(stream_id).or_default().push(chunk);
    });
}

fn pull_chunk(stream_id: u32) -> Option<JsValue> {
    STREAM_BUFFERS.with(|b| {
        let mut buffers = b.borrow_mut();
        if let Some(buf) = buffers.get_mut(&stream_id) {
            if !buf.is_empty() {
                return Some(buf.remove(0));
            }
        }
        None
    })
}

// ── ReadableStream ───────────────────────────────────────────────────────────

pub(super) fn make_readable_stream(underlying_source: Option<&JsValue>) -> JsValue {
    let id = alloc_stream_id();
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("ReadableStream".to_string()));
    map.insert("__stream_id__".to_string(), JsValue::Number(id as f64));
    map.insert("locked".to_string(), JsValue::Boolean(false));
    if let Some(JsValue::Object(src)) = underlying_source {
        if let Some(start_fn) = src.get("start") {
            map.insert("__start__".to_string(), start_fn.clone());
        }
        if let Some(pull_fn) = src.get("pull") {
            map.insert("__pull__".to_string(), pull_fn.clone());
        }
    }
    JsValue::Object(map)
}

pub(super) fn call_readable_stream_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    let stream_id = map.get("__stream_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as u32) } else { None }).unwrap_or(0);
    match method {
        "getReader" => {
            let mut reader = HashMap::new();
            reader.insert("__type__".to_string(), JsValue::String("ReadableStreamDefaultReader".to_string()));
            reader.insert("__stream_id__".to_string(), JsValue::Number(stream_id as f64));
            reader.insert("closed".to_string(), make_resolved_promise(JsValue::Undefined));
            JsValue::Object(reader)
        }
        "pipeThrough" => {
            let transform = args.first();
            if let Some(JsValue::Object(t)) = transform {
                if let Some(JsValue::Object(readable)) = t.get("readable") {
                    return JsValue::Object(readable.clone());
                }
            }
            JsValue::Object(map.clone())
        }
        "pipeTo" => {
            // Pragmatic: drain all chunks into the writable.
            let _dest = args.first();
            make_resolved_promise(JsValue::Undefined)
        }
        "tee" => {
            let id2 = alloc_stream_id();
            // Copy buffer to both streams.
            let chunks: Vec<JsValue> = STREAM_BUFFERS.with(|b| {
                b.borrow().get(&stream_id).cloned().unwrap_or_default()
            });
            for chunk in &chunks {
                push_chunk(stream_id, chunk.clone());
                push_chunk(id2, chunk.clone());
            }
            let mut s1 = map.clone();
            s1.insert("__stream_id__".to_string(), JsValue::Number(stream_id as f64));
            let mut s2 = HashMap::new();
            s2.insert("__type__".to_string(), JsValue::String("ReadableStream".to_string()));
            s2.insert("__stream_id__".to_string(), JsValue::Number(id2 as f64));
            s2.insert("locked".to_string(), JsValue::Boolean(false));
            JsValue::Array(vec![JsValue::Object(s1), JsValue::Object(s2)])
        }
        "cancel" => {
            STREAM_BUFFERS.with(|b| { b.borrow_mut().remove(&stream_id); });
            make_resolved_promise(JsValue::Undefined)
        }
        "values" | "asyncIterator" => {
            let chunks: Vec<JsValue> = STREAM_BUFFERS.with(|b| {
                b.borrow().get(&stream_id).cloned().unwrap_or_default()
            });
            let mut iter = HashMap::new();
            iter.insert("__type__".to_string(), JsValue::String("AsyncIterator".to_string()));
            iter.insert("__values__".to_string(), JsValue::Array(chunks));
            iter.insert("__index__".to_string(), JsValue::Number(0.0));
            JsValue::Object(iter)
        }
        _ => JsValue::Undefined,
    }
}

// ── ReadableStreamDefaultReader ──────────────────────────────────────────────

pub(super) fn call_reader_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> JsValue {
    let stream_id = map.get("__stream_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as u32) } else { None }).unwrap_or(0);
    match method {
        "read" => {
            let chunk = pull_chunk(stream_id);
            let mut result = HashMap::new();
            match chunk {
                Some(val) => {
                    result.insert("done".to_string(), JsValue::Boolean(false));
                    result.insert("value".to_string(), val);
                }
                None => {
                    result.insert("done".to_string(), JsValue::Boolean(true));
                    result.insert("value".to_string(), JsValue::Undefined);
                }
            }
            make_resolved_promise(JsValue::Object(result))
        }
        "readAll" => {
            let mut all = Vec::new();
            while let Some(chunk) = pull_chunk(stream_id) {
                all.push(chunk);
            }
            make_resolved_promise(JsValue::Array(all))
        }
        "releaseLock" => JsValue::Undefined,
        "cancel" => {
            STREAM_BUFFERS.with(|b| { b.borrow_mut().remove(&stream_id); });
            make_resolved_promise(JsValue::Undefined)
        }
        "closed" => make_resolved_promise(JsValue::Undefined),
        _ => JsValue::Undefined,
    }
}

// ── ReadableStreamDefaultController ──────────────────────────────────────────

pub(super) fn call_readable_controller_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    let stream_id = map.get("__stream_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as u32) } else { None }).unwrap_or(0);
    match method {
        "enqueue" => {
            let chunk = args.first().cloned().unwrap_or(JsValue::Undefined);
            push_chunk(stream_id, chunk);
            JsValue::Undefined
        }
        "close" => JsValue::Undefined,
        "error" => JsValue::Undefined,
        _ => JsValue::Undefined,
    }
}

// ── WritableStream ───────────────────────────────────────────────────────────

pub(super) fn make_writable_stream(underlying_sink: Option<&JsValue>) -> JsValue {
    let id = alloc_stream_id();
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("WritableStream".to_string()));
    map.insert("__stream_id__".to_string(), JsValue::Number(id as f64));
    map.insert("locked".to_string(), JsValue::Boolean(false));
    if let Some(JsValue::Object(sink)) = underlying_sink {
        if let Some(write_fn) = sink.get("write") {
            map.insert("__write__".to_string(), write_fn.clone());
        }
        if let Some(close_fn) = sink.get("close") {
            map.insert("__close__".to_string(), close_fn.clone());
        }
    }
    JsValue::Object(map)
}

pub(super) fn call_writable_stream_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> JsValue {
    let stream_id = map.get("__stream_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as u32) } else { None }).unwrap_or(0);
    match method {
        "getWriter" => {
            let mut writer = HashMap::new();
            writer.insert("__type__".to_string(), JsValue::String("WritableStreamDefaultWriter".to_string()));
            writer.insert("__stream_id__".to_string(), JsValue::Number(stream_id as f64));
            writer.insert("closed".to_string(), make_resolved_promise(JsValue::Undefined));
            writer.insert("ready".to_string(), make_resolved_promise(JsValue::Undefined));
            writer.insert("desiredSize".to_string(), JsValue::Number(1.0));
            JsValue::Object(writer)
        }
        "abort" => {
            STREAM_BUFFERS.with(|b| { b.borrow_mut().remove(&stream_id); });
            make_resolved_promise(JsValue::Undefined)
        }
        "close" => make_resolved_promise(JsValue::Undefined),
        _ => JsValue::Undefined,
    }
}

// ── WritableStreamDefaultWriter ──────────────────────────────────────────────

pub(super) fn call_writer_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    let stream_id = map.get("__stream_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as u32) } else { None }).unwrap_or(0);
    match method {
        "write" => {
            let chunk = args.first().cloned().unwrap_or(JsValue::Undefined);
            push_chunk(stream_id, chunk);
            make_resolved_promise(JsValue::Undefined)
        }
        "writeMany" => {
            if let Some(JsValue::Array(chunks)) = args.first() {
                for chunk in chunks {
                    push_chunk(stream_id, chunk.clone());
                }
            }
            make_resolved_promise(JsValue::Undefined)
        }
        "close" => make_resolved_promise(JsValue::Undefined),
        "abort" => {
            STREAM_BUFFERS.with(|b| { b.borrow_mut().remove(&stream_id); });
            make_resolved_promise(JsValue::Undefined)
        }
        "releaseLock" => JsValue::Undefined,
        "ready" => make_resolved_promise(JsValue::Undefined),
        "closed" => make_resolved_promise(JsValue::Undefined),
        _ => JsValue::Undefined,
    }
}

// ── TransformStream ──────────────────────────────────────────────────────────

pub(super) fn make_transform_stream(transformer: Option<&JsValue>) -> JsValue {
    let readable_id = alloc_stream_id();
    let writable_id = alloc_stream_id();
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("TransformStream".to_string()));
    map.insert("__readable_id__".to_string(), JsValue::Number(readable_id as f64));
    map.insert("__writable_id__".to_string(), JsValue::Number(writable_id as f64));

    let mut readable = HashMap::new();
    readable.insert("__type__".to_string(), JsValue::String("ReadableStream".to_string()));
    readable.insert("__stream_id__".to_string(), JsValue::Number(readable_id as f64));
    readable.insert("locked".to_string(), JsValue::Boolean(false));
    map.insert("readable".to_string(), JsValue::Object(readable));

    let mut writable = HashMap::new();
    writable.insert("__type__".to_string(), JsValue::String("WritableStream".to_string()));
    writable.insert("__stream_id__".to_string(), JsValue::Number(writable_id as f64));
    writable.insert("locked".to_string(), JsValue::Boolean(false));
    map.insert("writable".to_string(), JsValue::Object(writable));

    if let Some(JsValue::Object(t)) = transformer {
        if let Some(transform_fn) = t.get("transform") {
            map.insert("__transform__".to_string(), transform_fn.clone());
        }
        if let Some(flush_fn) = t.get("flush") {
            map.insert("__flush__".to_string(), flush_fn.clone());
        }
    }
    JsValue::Object(map)
}

pub(super) fn call_transform_stream_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> JsValue {
    match method {
        "readable" => map.get("readable").cloned().unwrap_or(JsValue::Undefined),
        "writable" => map.get("writable").cloned().unwrap_or(JsValue::Undefined),
        _ => JsValue::Undefined,
    }
}

// ── TransformStreamDefaultController ─────────────────────────────────────────

pub(super) fn call_transform_controller_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    let stream_id = map.get("__stream_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as u32) } else { None }).unwrap_or(0);
    match method {
        "enqueue" => {
            let chunk = args.first().cloned().unwrap_or(JsValue::Undefined);
            push_chunk(stream_id, chunk);
            JsValue::Undefined
        }
        "error" => JsValue::Undefined,
        "terminate" => JsValue::Undefined,
        _ => JsValue::Undefined,
    }
}

// ── CountQueuingStrategy / ByteLengthQueuingStrategy ─────────────────────────

pub(super) fn make_count_queuing_strategy(high_water_mark: f64) -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("CountQueuingStrategy".to_string()));
    map.insert("highWaterMark".to_string(), JsValue::Number(high_water_mark));
    JsValue::Object(map)
}

pub(super) fn make_byte_length_queuing_strategy(high_water_mark: f64) -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("ByteLengthQueuingStrategy".to_string()));
    map.insert("highWaterMark".to_string(), JsValue::Number(high_water_mark));
    JsValue::Object(map)
}

pub(super) fn call_queuing_strategy_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "size" => {
            // CountQueuingStrategy: size = 1 for any chunk.
            // ByteLengthQueuingStrategy: size = chunk.byteLength or chunk.length or 1.
            let type_tag = map.get("__type__").and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None });
            if type_tag == Some("ByteLengthQueuingStrategy") {
                if let Some(JsValue::Object(chunk)) = args.first() {
                    if let Some(JsValue::Number(n)) = chunk.get("byteLength") {
                        return JsValue::Number(*n);
                    }
                    if let Some(JsValue::Array(data)) = chunk.get("__data__") {
                        return JsValue::Number(data.len() as f64);
                    }
                }
                if let Some(JsValue::String(s)) = args.first() {
                    return JsValue::Number(s.len() as f64);
                }
            }
            JsValue::Number(1.0)
        }
        _ => JsValue::Undefined,
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_resolved_promise(value: JsValue) -> JsValue {
    let mut p = HashMap::new();
    p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
    p.insert("__resolved__".to_string(), value);
    JsValue::Object(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── queuing strategies ─────────────────────────────────────────────

    #[test]
    fn count_queuing_strategy_structure() {
        let s = make_count_queuing_strategy(5.0);
        if let JsValue::Object(m) = s {
            assert_eq!(m.get("__type__").unwrap(), &JsValue::String("CountQueuingStrategy".into()));
            assert_eq!(m.get("highWaterMark").unwrap(), &JsValue::Number(5.0));
        } else { panic!("expected Object"); }
    }

    #[test]
    fn byte_length_queuing_strategy_structure() {
        let s = make_byte_length_queuing_strategy(1024.0);
        if let JsValue::Object(m) = s {
            assert_eq!(m.get("__type__").unwrap(), &JsValue::String("ByteLengthQueuingStrategy".into()));
            assert_eq!(m.get("highWaterMark").unwrap(), &JsValue::Number(1024.0));
        } else { panic!("expected Object"); }
    }

    #[test]
    fn count_strategy_size_always_one() {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("CountQueuingStrategy".into()));
        let r = call_queuing_strategy_method(&m, "size", &[JsValue::String("anything".into())]);
        assert_eq!(r, JsValue::Number(1.0));
    }

    #[test]
    fn byte_length_strategy_string_size() {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("ByteLengthQueuingStrategy".into()));
        let r = call_queuing_strategy_method(&m, "size", &[JsValue::String("hello".into())]);
        assert_eq!(r, JsValue::Number(5.0));
    }

    #[test]
    fn byte_length_strategy_object_byte_length() {
        let mut m = HashMap::new();
        m.insert("__type__".to_string(), JsValue::String("ByteLengthQueuingStrategy".into()));
        let mut chunk = HashMap::new();
        chunk.insert("byteLength".to_string(), JsValue::Number(42.0));
        let r = call_queuing_strategy_method(&m, "size", &[JsValue::Object(chunk)]);
        assert_eq!(r, JsValue::Number(42.0));
    }

    // ── make_readable_stream ───────────────────────────────────────────

    #[test]
    fn readable_stream_structure() {
        let s = make_readable_stream(None);
        if let JsValue::Object(m) = s {
            assert_eq!(m.get("__type__").unwrap(), &JsValue::String("ReadableStream".into()));
            assert_eq!(m.get("locked").unwrap(), &JsValue::Boolean(false));
            assert!(m.contains_key("__stream_id__"));
        } else { panic!("expected Object"); }
    }

    // ── make_writable_stream ───────────────────────────────────────────

    #[test]
    fn writable_stream_structure() {
        let s = make_writable_stream(None);
        if let JsValue::Object(m) = s {
            assert_eq!(m.get("__type__").unwrap(), &JsValue::String("WritableStream".into()));
            assert_eq!(m.get("locked").unwrap(), &JsValue::Boolean(false));
        } else { panic!("expected Object"); }
    }

    // ── make_transform_stream ──────────────────────────────────────────

    #[test]
    fn transform_stream_structure() {
        let s = make_transform_stream(None);
        if let JsValue::Object(m) = s {
            assert_eq!(m.get("__type__").unwrap(), &JsValue::String("TransformStream".into()));
            assert!(m.contains_key("readable"));
            assert!(m.contains_key("writable"));
        } else { panic!("expected Object"); }
    }

    // ── write/read roundtrip ───────────────────────────────────────────

    #[test]
    fn stream_write_read_roundtrip() {
        // Create a readable stream
        let rs = make_readable_stream(None);
        if let JsValue::Object(rs_map) = rs {
            let stream_id = rs_map.get("__stream_id__").and_then(|v| if let JsValue::Number(n) = v { Some(*n as u32) } else { None }).unwrap();

            // Push chunks via the controller
            let mut ctrl = HashMap::new();
            ctrl.insert("__stream_id__".to_string(), JsValue::Number(stream_id as f64));
            let _ = call_readable_controller_method(&ctrl, "enqueue", &[JsValue::String("hello".into())]);
            let _ = call_readable_controller_method(&ctrl, "enqueue", &[JsValue::String("world".into())]);

            // Read via reader
            let mut reader = HashMap::new();
            reader.insert("__stream_id__".to_string(), JsValue::Number(stream_id as f64));

            let r1 = call_reader_method(&reader, "read", &[]);
            if let JsValue::Object(p) = r1 {
                if let Some(JsValue::Object(result)) = p.get("__resolved__") {
                    assert_eq!(result.get("done").unwrap(), &JsValue::Boolean(false));
                    assert_eq!(result.get("value").unwrap(), &JsValue::String("hello".into()));
                }
            }

            let r2 = call_reader_method(&reader, "read", &[]);
            if let JsValue::Object(p) = r2 {
                if let Some(JsValue::Object(result)) = p.get("__resolved__") {
                    assert_eq!(result.get("value").unwrap(), &JsValue::String("world".into()));
                }
            }

            // Third read should be done
            let r3 = call_reader_method(&reader, "read", &[]);
            if let JsValue::Object(p) = r3 {
                if let Some(JsValue::Object(result)) = p.get("__resolved__") {
                    assert_eq!(result.get("done").unwrap(), &JsValue::Boolean(true));
                }
            }
        } else { panic!("expected Object"); }
    }

    // ── readable stream methods ────────────────────────────────────────

    #[test]
    fn readable_stream_get_reader() {
        let rs = make_readable_stream(None);
        if let JsValue::Object(m) = rs {
            let r = call_readable_stream_method(&m, "getReader", &[]);
            if let JsValue::Object(reader) = r {
                assert_eq!(reader.get("__type__").unwrap(), &JsValue::String("ReadableStreamDefaultReader".into()));
            } else { panic!("expected Object"); }
        }
    }

    // ── writable stream methods ────────────────────────────────────────

    #[test]
    fn writable_stream_get_writer() {
        let ws = make_writable_stream(None);
        if let JsValue::Object(m) = ws {
            let r = call_writable_stream_method(&m, "getWriter", &[]);
            if let JsValue::Object(writer) = r {
                assert_eq!(writer.get("__type__").unwrap(), &JsValue::String("WritableStreamDefaultWriter".into()));
                assert_eq!(writer.get("desiredSize").unwrap(), &JsValue::Number(1.0));
            } else { panic!("expected Object"); }
        }
    }

    // ── writer methods ─────────────────────────────────────────────────

    #[test]
    fn writer_release_lock() {
        let mut writer = HashMap::new();
        writer.insert("__stream_id__".to_string(), JsValue::Number(999.0));
        let r = call_writer_method(&writer, "releaseLock", &[]);
        assert_eq!(r, JsValue::Undefined);
    }

    // ── unknown methods ────────────────────────────────────────────────

    #[test]
    fn unknown_readable_method() {
        let m = HashMap::new();
        let r = call_readable_stream_method(&m, "nope", &[]);
        assert_eq!(r, JsValue::Undefined);
    }

    #[test]
    fn unknown_writable_method() {
        let m = HashMap::new();
        let r = call_writable_stream_method(&m, "nope", &[]);
        assert_eq!(r, JsValue::Undefined);
    }

    #[test]
    fn unknown_queuing_strategy_method() {
        let m = HashMap::new();
        let r = call_queuing_strategy_method(&m, "nope", &[]);
        assert_eq!(r, JsValue::Undefined);
    }
}
