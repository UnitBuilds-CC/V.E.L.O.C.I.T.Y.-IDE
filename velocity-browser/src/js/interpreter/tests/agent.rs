use super::super::console::{clear_console_output, get_console_output};
use super::*;

// ── Console API ──────────────────────────────────────────────────────────

#[test]
fn console_api() {
    clear_console_output();
    assert_eq!(eval_full("console.log('hello')"), JsValue::Undefined);
    let output = get_console_output();
    assert!(!output.is_empty());
    assert_eq!(output.last().unwrap().level, "log");
    clear_console_output();
    assert_eq!(eval_full("console.warn('warning')"), JsValue::Undefined);
    let output2 = get_console_output();
    assert_eq!(output2.last().unwrap().level, "warn");
    clear_console_output();
}

#[test]
fn console_table_renders_markdown() {
    eval_full("console.table([{fruit: 'apple', qty: 3}])");
    let output = get_console_output();
    let rec = output
        .iter()
        .rev()
        .find(|r| r.level == "table")
        .expect("table record");
    match &rec.args[0] {
        JsValue::String(s) => {
            assert!(s.contains("| (index) | fruit | qty |"), "got: {}", s);
            assert!(s.contains("| 0 | apple | 3 |"), "got: {}", s);
        }
        other => panic!("Expected rendered string, got {:?}", other),
    }
}

#[test]
fn console_table_renders_plain_object() {
    eval_full("console.table({alpha: 'one', beta: 'two'})");
    let output = get_console_output();
    let rec = output
        .iter()
        .rev()
        .find(|r| {
            r.level == "table" && matches!(&r.args[0], JsValue::String(s) if s.contains("alpha"))
        })
        .expect("table record");
    match &rec.args[0] {
        JsValue::String(s) => {
            assert!(s.contains("| alpha | one |"), "got: {}", s);
            assert!(s.contains("| beta | two |"), "got: {}", s);
        }
        other => panic!("Expected rendered string, got {:?}", other),
    }
}

#[test]
fn get_console_text_exposes_logs_to_agents() {
    let result = eval_full("console.log('settle-marker-42'); document.getConsoleText()");
    match result {
        JsValue::String(s) => assert!(s.contains("log: settle-marker-42"), "got: {}", s),
        other => panic!("Expected string, got {:?}", other),
    }
}

// ── AbortController ──────────────────────────────────────────────────────

#[test]
fn abort_controller() {
    assert_eq!(
        eval_full("var ac = new AbortController(); ac.signal.aborted"),
        JsValue::Boolean(false)
    );
    assert_eq!(
        eval_full("var ac = new AbortController(); ac.abort(); ac.signal.aborted"),
        JsValue::Boolean(true)
    );
}

// ── TextEncoder/TextDecoder ──────────────────────────────────────────────

#[test]
fn text_encoder() {
    let result = eval_full("var enc = new TextEncoder(); enc.encode('A')");
    match result {
        JsValue::Array(arr) => {
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0], JsValue::Number(65.0));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn text_decoder() {
    assert_eq!(
        eval_full("var dec = new TextDecoder(); dec.decode([65, 66, 67])"),
        JsValue::String("ABC".to_string())
    );
}

// ── RegExp test/exec ─────────────────────────────────────────────────────

#[test]
fn regexp_test_exec() {
    assert_eq!(
        eval_full("var re = new RegExp('abc'); re.test('xabcx')"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("var re = new RegExp('xyz'); re.test('xabcx')"),
        JsValue::Boolean(false)
    );
    // Case insensitive
    assert_eq!(
        eval_full("var re = new RegExp('abc', 'i'); re.test('XABCX')"),
        JsValue::Boolean(true)
    );
    // exec
    match eval_full("var re = new RegExp('abc'); re.exec('xabcx')") {
        JsValue::Array(arr) => {
            assert_eq!(arr[0], JsValue::String("abc".to_string()));
        }
        _ => panic!("Expected array from exec"),
    }
}

// ── Enhanced Date methods ────────────────────────────────────────────────

#[test]
fn date_methods() {
    // Date(0) = 1970-01-01T00:00:00.000Z
    assert_eq!(
        eval_full("var d = new Date(0); d.getTime()"),
        JsValue::Number(0.0)
    );
    assert_eq!(
        eval_full("var d = new Date(0); d.getFullYear()"),
        JsValue::Number(1970.0)
    );
    assert_eq!(
        eval_full("var d = new Date(0); d.getMonth()"),
        JsValue::Number(0.0)
    );
    assert_eq!(
        eval_full("var d = new Date(0); d.getDate()"),
        JsValue::Number(1.0)
    );
    assert_eq!(
        eval_full("var d = new Date(0); d.getDay()"),
        JsValue::Number(4.0)
    ); // Thursday
    assert_eq!(
        eval_full("var d = new Date(0); d.getHours()"),
        JsValue::Number(0.0)
    );
    assert_eq!(
        eval_full("var d = new Date(0); d.getMinutes()"),
        JsValue::Number(0.0)
    );
    assert_eq!(
        eval_full("var d = new Date(0); d.getSeconds()"),
        JsValue::Number(0.0)
    );
    // toISOString
    assert_eq!(
        eval_full("var d = new Date(0); d.toISOString()"),
        JsValue::String("1970-01-01T00:00:00.000Z".to_string())
    );
}

// ── Object.prototype methods ─────────────────────────────────────────────

#[test]
fn object_proto_methods() {
    assert_eq!(
        eval_full("var o = {a: 1}; o.hasOwnProperty('a')"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("var o = {a: 1}; o.hasOwnProperty('b')"),
        JsValue::Boolean(false)
    );
    assert_eq!(
        eval_full("var o = {a: 1}; o.propertyIsEnumerable('a')"),
        JsValue::Boolean(true)
    );
}

// ── Boolean methods ──────────────────────────────────────────────────────

#[test]
fn boolean_methods() {
    assert_eq!(
        eval_full("var b = true; b.toString()"),
        JsValue::String("true".to_string())
    );
    assert_eq!(
        eval_full("var b = false; b.valueOf()"),
        JsValue::Boolean(false)
    );
}

// ── NativeFunction methods ───────────────────────────────────────────────

#[test]
fn native_function_methods() {
    let result = eval_full("parseInt.toString()");
    match result {
        JsValue::String(s) => assert!(s.contains("native code")),
        _ => panic!("Expected string"),
    }
}

// ── structuredClone ──────────────────────────────────────────────────────

#[test]
fn structuredclone_works() {
    assert_eq!(eval_full("structuredClone(42)"), JsValue::Number(42.0));
}
