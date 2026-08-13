//! Tests for the Streams API: ReadableStream, WritableStream, TransformStream,
//! queuing strategies.

use super::*;

// ── ReadableStream ───────────────────────────────────────────────────────────

#[test]
fn readable_stream_construct() {
    let result = eval_full(
        r#"
        let rs = new ReadableStream();
        rs.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("ReadableStream".to_string()));
}

#[test]
fn readable_stream_get_reader() {
    let result = eval_full(
        r#"
        let rs = new ReadableStream();
        let reader = rs.getReader();
        reader.__type__
    "#,
    );
    assert_eq!(
        result,
        JsValue::String("ReadableStreamDefaultReader".to_string())
    );
}

#[test]
fn readable_stream_read_empty() {
    let result = eval_full(
        r#"
        let rs = new ReadableStream();
        let reader = rs.getReader();
        let p = reader.read();
        p.__resolved__.done
    "#,
    );
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn readable_stream_tee() {
    let result = eval_full(
        r#"
        let rs = new ReadableStream();
        let branches = rs.tee();
        branches.length
    "#,
    );
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn readable_stream_cancel() {
    let result = eval_full(
        r#"
        let rs = new ReadableStream();
        let p = rs.cancel();
        p.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("Promise".to_string()));
}

// ── WritableStream ───────────────────────────────────────────────────────────

#[test]
fn writable_stream_construct() {
    let result = eval_full(
        r#"
        let ws = new WritableStream();
        ws.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("WritableStream".to_string()));
}

#[test]
fn writable_stream_get_writer() {
    let result = eval_full(
        r#"
        let ws = new WritableStream();
        let writer = ws.getWriter();
        writer.__type__
    "#,
    );
    assert_eq!(
        result,
        JsValue::String("WritableStreamDefaultWriter".to_string())
    );
}

#[test]
fn writable_stream_writer_write() {
    let result = eval_full(
        r#"
        let ws = new WritableStream();
        let writer = ws.getWriter();
        let p = writer.write('hello');
        p.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("Promise".to_string()));
}

#[test]
fn writable_stream_abort() {
    let result = eval_full(
        r#"
        let ws = new WritableStream();
        let p = ws.abort();
        p.__resolved__
    "#,
    );
    assert_eq!(result, JsValue::Undefined);
}

// ── TransformStream ──────────────────────────────────────────────────────────

#[test]
fn transform_stream_construct() {
    let result = eval_full(
        r#"
        let ts = new TransformStream();
        ts.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("TransformStream".to_string()));
}

#[test]
fn transform_stream_readable_writable() {
    let result = eval_full(
        r#"
        let ts = new TransformStream();
        ts.readable.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("ReadableStream".to_string()));
}

#[test]
fn transform_stream_writable_side() {
    let result = eval_full(
        r#"
        let ts = new TransformStream();
        ts.writable.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("WritableStream".to_string()));
}

// ── Queuing Strategies ───────────────────────────────────────────────────────

#[test]
fn count_queuing_strategy() {
    let result = eval_full(
        r#"
        let cqs = new CountQueuingStrategy({highWaterMark: 5});
        cqs.highWaterMark
    "#,
    );
    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn count_queuing_strategy_size() {
    let result = eval_full(
        r#"
        let cqs = new CountQueuingStrategy({highWaterMark: 1});
        cqs.size('anything')
    "#,
    );
    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn byte_length_queuing_strategy() {
    let result = eval_full(
        r#"
        let blqs = new ByteLengthQueuingStrategy({highWaterMark: 1024});
        blqs.highWaterMark
    "#,
    );
    assert_eq!(result, JsValue::Number(1024.0));
}

#[test]
fn byte_length_queuing_strategy_size_string() {
    let result = eval_full(
        r#"
        let blqs = new ByteLengthQueuingStrategy({highWaterMark: 100});
        blqs.size('hello')
    "#,
    );
    assert_eq!(result, JsValue::Number(5.0));
}

// ── Pipe chain (pragmatic) ───────────────────────────────────────────────────

#[test]
fn readable_stream_pipe_through() {
    let result = eval_full(
        r#"
        let rs = new ReadableStream();
        let ts = new TransformStream();
        let piped = rs.pipeThrough(ts);
        piped.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("ReadableStream".to_string()));
}

#[test]
fn readable_stream_pipe_to() {
    let result = eval_full(
        r#"
        let rs = new ReadableStream();
        let ws = new WritableStream();
        let p = rs.pipeTo(ws);
        p.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("Promise".to_string()));
}
