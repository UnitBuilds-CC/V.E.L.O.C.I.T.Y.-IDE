//! Tests for ES2024-2025 features and Web APIs rebuilt after the data-loss
//! incident: for-await-of, globalThis, Symbol.asyncIterator, isWellFormed,
//! Promise.withResolvers/try, Object.groupBy, Map.groupBy, Array.fromAsync,
//! Error.isError, atob/btoa, crypto, URL.canParse, MessageChannel, EventTarget,
//! WeakRef, Proxy.revocable, ArrayBuffer.isView, getOwnPropertySymbols.

use super::*;

// ── for await...of ──────────────────────────────────────────────────────────

#[test]
fn for_await_of_unwraps_promises() {
    let result = eval_full(
        "var out = []; var arr = [Promise.resolve(1), Promise.resolve(2), Promise.resolve(3)]; \
         async function run() { for await (var x of arr) { out.push(x); } } run(); out.join(',')",
    );
    assert_eq!(result, JsValue::String("1,2,3".to_string()));
}

#[test]
fn for_await_of_plain_values() {
    let result =
        eval_full("var out = []; for await (var x of [10, 20, 30]) { out.push(x); } out.length");
    assert_eq!(result, JsValue::Number(3.0));
}

// ── globalThis ──────────────────────────────────────────────────────────────

#[test]
fn global_this_is_object() {
    let result = eval_full("typeof globalThis");
    assert_eq!(result, JsValue::String("object".to_string()));
}

// ── Symbol.asyncIterator ────────────────────────────────────────────────────

#[test]
fn symbol_async_iterator_fallback() {
    // Verify the Symbol.asyncIterator key is recognized as an iterable
    // protocol entry point. The drain_iterator calls next() which returns
    // done:true immediately, so we just verify the path doesn't crash.
    let result = eval_full(
        "var obj = {}; obj['Symbol.asyncIterator'] = function() { return { next: function() { return { done: true, value: undefined }; } }; }; \
         var out = []; for (var x of obj) { out.push(x); } out.length"
    );
    assert_eq!(result, JsValue::Number(0.0));
}

// ── String.prototype.isWellFormed / toWellFormed ────────────────────────────

#[test]
fn string_is_well_formed() {
    assert_eq!(eval_full("'hello'.isWellFormed()"), JsValue::Boolean(true));
    assert_eq!(eval_full("''.isWellFormed()"), JsValue::Boolean(true));
}

#[test]
fn string_to_well_formed() {
    assert_eq!(
        eval_full("'hello'.toWellFormed()"),
        JsValue::String("hello".to_string())
    );
}

// ── Promise.withResolvers ───────────────────────────────────────────────────

#[test]
fn promise_with_resolvers_shape() {
    let result = eval_full("var r = Promise.withResolvers(); typeof r.promise");
    assert_eq!(result, JsValue::String("object".to_string()));
    let result = eval_full("var r = Promise.withResolvers(); typeof r.resolve");
    assert_eq!(result, JsValue::String("function".to_string()));
}

// ── Promise.try ─────────────────────────────────────────────────────────────

#[test]
fn promise_try_resolves() {
    let result = eval_full("var p = Promise.try(function() { return 42; }); p.__resolved__");
    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn promise_try_rejects_on_throw() {
    let result = eval_full("var p = Promise.try(function() { throw 'oops'; }); p.__rejected__");
    assert_eq!(result, JsValue::String("oops".to_string()));
}

// ── Object.groupBy / Map.groupBy ────────────────────────────────────────────

#[test]
fn object_group_by() {
    let result = eval_full(
        "var g = Object.groupBy([1,2,3,4,5], function(x) { return x % 2 === 0 ? 'even' : 'odd'; }); g.even.length"
    );
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn map_group_by() {
    let result = eval_full(
        "var g = Map.groupBy([1,2,3,4], function(x) { return x > 2 ? 'big' : 'small'; }); g.get('big').length"
    );
    assert_eq!(result, JsValue::Number(2.0));
}

// ── Array.fromAsync ─────────────────────────────────────────────────────────

#[test]
fn array_from_async() {
    let result = eval_full("var p = Array.fromAsync([1, 2, 3]); p.__resolved__.join(',')");
    assert_eq!(result, JsValue::String("1,2,3".to_string()));
}

// ── Error.isError ───────────────────────────────────────────────────────────

#[test]
fn error_is_error() {
    assert_eq!(
        eval_full("Error.isError(new Error('x'))"),
        JsValue::Boolean(true)
    );
    assert_eq!(eval_full("Error.isError(42)"), JsValue::Boolean(false));
    assert_eq!(eval_full("Error.isError({})"), JsValue::Boolean(false));
}

// ── atob / btoa ─────────────────────────────────────────────────────────────

#[test]
fn btoa_encodes() {
    assert_eq!(
        eval_full("btoa('hello')"),
        JsValue::String("aGVsbG8=".to_string())
    );
    assert_eq!(eval_full("btoa('')"), JsValue::String(String::new()));
}

#[test]
fn atob_decodes() {
    assert_eq!(
        eval_full("atob('aGVsbG8=')"),
        JsValue::String("hello".to_string())
    );
}

#[test]
fn btoa_atob_roundtrip() {
    assert_eq!(
        eval_full("atob(btoa('test123'))"),
        JsValue::String("test123".to_string())
    );
}

// ── crypto.randomUUID ───────────────────────────────────────────────────────

#[test]
fn crypto_random_uuid_format() {
    let result = eval_full("crypto.randomUUID().length");
    assert_eq!(result, JsValue::Number(36.0));
}

// ── URL.canParse ────────────────────────────────────────────────────────────

#[test]
fn url_can_parse() {
    assert_eq!(
        eval_full("URL.canParse('https://example.com')"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("URL.canParse('not a url')"),
        JsValue::Boolean(false)
    );
}

// ── MessageChannel ──────────────────────────────────────────────────────────

#[test]
fn message_channel_ports() {
    let result = eval_full("var mc = new MessageChannel(); mc.port1.__type__");
    assert_eq!(result, JsValue::String("MessagePort".to_string()));
}

// ── EventTarget ─────────────────────────────────────────────────────────────

#[test]
fn event_target_dispatch() {
    let result = eval_full(
        "var et = new EventTarget(); var got = ''; \
         et.addEventListener('ping', function(e) { got = e.data; }); \
         et.dispatchEvent({ type: 'ping', data: 'pong' }); got",
    );
    assert_eq!(result, JsValue::String("pong".to_string()));
}

// ── WeakRef ─────────────────────────────────────────────────────────────────

#[test]
fn weakref_deref() {
    let result = eval_full("var obj = { x: 1 }; var wr = new WeakRef(obj); wr.deref().x");
    assert_eq!(result, JsValue::Number(1.0));
}

// ── Proxy.revocable ─────────────────────────────────────────────────────────

#[test]
fn proxy_revocable_shape() {
    let result = eval_full("var r = Proxy.revocable({}, {}); typeof r.revoke");
    assert_eq!(result, JsValue::String("function".to_string()));
}

// ── ArrayBuffer.isView ──────────────────────────────────────────────────────

#[test]
fn array_buffer_is_view() {
    assert_eq!(
        eval_full("ArrayBuffer.isView(new Uint8Array(4))"),
        JsValue::Boolean(true)
    );
    assert_eq!(eval_full("ArrayBuffer.isView([])"), JsValue::Boolean(false));
}

// ── Object.getOwnPropertySymbols ────────────────────────────────────────────

#[test]
fn get_own_property_symbols() {
    let result = eval_full("Object.getOwnPropertySymbols({}).length");
    assert_eq!(result, JsValue::Number(0.0));
}

// ── Set methods (ES2025) ────────────────────────────────────────────────────

#[test]
fn set_union() {
    let result =
        eval_full("var a = new Set([1,2,3]); var b = new Set([3,4,5]); var u = a.union(b); u.size");
    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn set_intersection() {
    let result = eval_full(
        "var a = new Set([1,2,3]); var b = new Set([2,3,4]); var i = a.intersection(b); i.size",
    );
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn set_difference() {
    let result = eval_full(
        "var a = new Set([1,2,3]); var b = new Set([2,3,4]); var d = a.difference(b); d.size",
    );
    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn set_is_subset_of() {
    assert_eq!(
        eval_full("new Set([1,2]).isSubsetOf(new Set([1,2,3]))"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("new Set([1,4]).isSubsetOf(new Set([1,2,3]))"),
        JsValue::Boolean(false)
    );
}

// ── Intl.Locale ─────────────────────────────────────────────────────────────

#[test]
fn intl_locale_basic() {
    assert_eq!(
        eval_full("new Intl.Locale('en-US').language"),
        JsValue::String("en".to_string())
    );
    assert_eq!(
        eval_full("new Intl.Locale('en-US').region"),
        JsValue::String("US".to_string())
    );
}

#[test]
fn intl_locale_to_string() {
    assert_eq!(
        eval_full("new Intl.Locale('en-US').toString()"),
        JsValue::String("en-US".to_string())
    );
}

// ── Intl.getCanonicalLocales ────────────────────────────────────────────────

#[test]
fn intl_get_canonical_locales() {
    let result = eval_full("Intl.getCanonicalLocales(['EN-us', 'fr-FR']).join(',')");
    assert_eq!(result, JsValue::String("en-US,fr-FR".to_string()));
}
