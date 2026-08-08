//! Additional Web APIs: crypto (randomUUID/getRandomValues), base64 (atob/btoa),
//! URL.canParse, MessageChannel/MessagePort, EventTarget, WeakRef,
//! FinalizationRegistry and Proxy.revocable — plus `call_native_extended`, the
//! overflow dispatch table for newer global statics (delegated from `call_native`).

use super::signal::*;
use super::coercion::*;
use super::function::call_function;
use super::core_methods::*;
use super::intl::get_canonical_locales;
use crate::js::scope::ScopeRef;
use crate::js::vm::JsValue;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

thread_local! {
    /// Monotonic id source for message channels and revocable proxies.
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
    /// PRNG state for crypto.* (xorshift64, lazily seeded from SystemTime).
    static RNG_STATE: Cell<u64> = const { Cell::new(0) };
    /// onmessage handlers keyed by (channel id, port number). Ports are value
    /// objects (cloned on assignment), so cross-port delivery needs shared state.
    static PORT_HANDLERS: RefCell<HashMap<(u64, u8), JsValue>> = RefCell::new(HashMap::new());
    /// Ids of proxies revoked via Proxy.revocable(...).revoke().
    static REVOKED_PROXIES: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
}

fn next_id() -> u64 {
    NEXT_ID.with(|c| { let v = c.get(); c.set(v + 1); v })
}

/// Build a plain error object matching the shape produced by `new Error(...)`.
pub(super) fn make_error(name: &str, message: &str) -> JsValue {
    let mut m = HashMap::new();
    m.insert("name".to_string(), JsValue::String(name.to_string()));
    m.insert("message".to_string(), JsValue::String(message.to_string()));
    JsValue::Object(m)
}

fn throw(name: &str, message: &str) -> Signal {
    Signal::Throw(make_error(name, message))
}

/// Overflow dispatch for global statics not handled in `call_native`.
pub(super) fn call_native_extended(name: &str, args: &[JsValue]) -> EvalResult {
    // Revoke callback minted by Proxy.revocable — the proxy id is encoded in
    // the native-function name because NativeFunction carries only a string.
    if let Some(id_str) = name.strip_prefix("__proxy_revoke__:") {
        if let Ok(id) = id_str.parse::<u64>() {
            REVOKED_PROXIES.with(|r| { r.borrow_mut().insert(id); });
        }
        return Ok(JsValue::Undefined);
    }
    Ok(match name {
        "Promise.withResolvers" => promise_with_resolvers(),
        "Promise.try" => return promise_try(args),
        "Object.groupBy" => return object_group_by(args),
        "Map.groupBy" => return map_group_by(args),
        "Array.fromAsync" => return array_from_async(args),
        "Error.isError" => error_is_error(args.first()),
        "Object.getOwnPropertySymbols" => get_own_property_symbols(args.first()),
        "ArrayBuffer.isView" => JsValue::Boolean(is_array_buffer_view(args.first())),
        "Proxy.revocable" => proxy_revocable(args),
        "URL.canParse" => JsValue::Boolean(url_can_parse(&args.first().map(to_string).unwrap_or_default())),
        "crypto.randomUUID" => JsValue::String(random_uuid()),
        "crypto.getRandomValues" => crypto_get_random_values(args),
        "atob" => return atob_impl(&args.first().map(to_string).unwrap_or_default()),
        "btoa" => return btoa_impl(&args.first().map(to_string).unwrap_or_default()),
        "Intl.getCanonicalLocales" => return get_canonical_locales(args),
        _ => JsValue::Undefined,
    })
}

// ── crypto ───────────────────────────────────────────────────────────────────

/// xorshift64 step; no external crates, seeded once from the system clock.
fn next_rand() -> u64 {
    RNG_STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15) | 1;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}

fn rand_byte() -> u8 {
    (next_rand() >> 24) as u8
}

/// RFC4122 version-4 UUID string.
fn random_uuid() -> String {
    let mut b = [0u8; 16];
    for byte in b.iter_mut() { *byte = rand_byte(); }
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC4122 variant
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Fill a typed array (or plain array) with random bytes (0-255) and return it.
/// Values are cloned in this engine, so the *returned* array carries the fill.
fn crypto_get_random_values(args: &[JsValue]) -> JsValue {
    match args.first() {
        Some(JsValue::Object(m)) => {
            let mut out = m.clone();
            if let Some(JsValue::Array(data)) = m.get("__data__") {
                let filled: Vec<JsValue> = data.iter().map(|_| JsValue::Number(rand_byte() as f64)).collect();
                out.insert("__data__".to_string(), JsValue::Array(filled));
            }
            JsValue::Object(out)
        }
        Some(JsValue::Array(a)) => JsValue::Array(a.iter().map(|_| JsValue::Number(rand_byte() as f64)).collect()),
        _ => JsValue::Undefined,
    }
}

// ── base64 (atob / btoa) ─────────────────────────────────────────────────────

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// btoa: encode a latin1 string as base64. Characters above U+00FF throw
/// InvalidCharacterError, matching the Web spec.
pub(super) fn btoa_impl(s: &str) -> EvalResult {
    let mut bytes = Vec::with_capacity(s.len());
    for c in s.chars() {
        let v = c as u32;
        if v > 0xFF {
            return Err(throw("InvalidCharacterError", "btoa: character outside of the Latin1 range"));
        }
        bytes.push(v as u8);
    }
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(B64_ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64_ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64_ALPHABET[n as usize & 63] as char } else { '=' });
    }
    Ok(JsValue::String(out))
}

/// atob: decode base64 to a latin1 string. Throws InvalidCharacterError on any
/// invalid character, excessive padding, or an impossible length.
pub(super) fn atob_impl(s: &str) -> EvalResult {
    let cleaned: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let trimmed = cleaned.trim_end_matches('=');
    if cleaned.len() - trimmed.len() > 2 || trimmed.len() % 4 == 1 {
        return Err(throw("InvalidCharacterError", "atob: invalid base64 input length"));
    }
    let mut acc: u32 = 0;
    let mut nbits = 0;
    let mut out_bytes: Vec<u8> = Vec::new();
    for c in trimmed.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => return Err(throw("InvalidCharacterError", "atob: invalid base64 character")),
        };
        acc = (acc << 6) | v;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out_bytes.push((acc >> nbits) as u8);
        }
    }
    Ok(JsValue::String(out_bytes.iter().map(|b| *b as char).collect()))
}

// ── URL.canParse ─────────────────────────────────────────────────────────────

/// Pragmatic scheme://host validity check: a non-empty ASCII-alphabetic
/// scheme followed by "://" and a non-empty host (or a `data:`/`about:` form).
fn url_can_parse(url: &str) -> bool {
    let url = url.trim();
    if let Some(rest) = url.split_once("://").map(|(scheme, rest)| {
        (!scheme.is_empty()
            && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.'))
            .then_some(rest)
    }) {
        return rest.is_some_and(|r| !r.is_empty() && !r.starts_with('/'));
    }
    // Scheme-only URLs like data:, about:, mailto: are parseable too.
    url.split_once(':').is_some_and(|(scheme, rest)| {
        !scheme.is_empty()
            && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
            && !rest.is_empty()
    })
}

// ── Proxy.revocable ──────────────────────────────────────────────────────────

/// Proxy.revocable(target, handler) → { proxy, revoke }. The proxy's id is
/// stashed inside the handler object (values are cloned, so the id travels
/// with every copy); revoke() records the id in REVOKED_PROXIES and
/// `check_revoked` makes subsequent member access throw a TypeError.
fn proxy_revocable(args: &[JsValue]) -> JsValue {
    let target = args.first().cloned().unwrap_or(JsValue::Undefined);
    let handler = args.get(1).cloned().unwrap_or_else(|| JsValue::Object(HashMap::new()));
    let id = next_id();
    let mut handler_map = match handler {
        JsValue::Object(m) => m,
        _ => HashMap::new(),
    };
    handler_map.insert("__revoke_id__".to_string(), JsValue::Number(id as f64));
    let proxy = JsValue::Proxy {
        target: Box::new(target),
        handler: Box::new(JsValue::Object(handler_map)),
    };
    let mut out = HashMap::new();
    out.insert("proxy".to_string(), proxy);
    out.insert("revoke".to_string(), JsValue::NativeFunction(format!("__proxy_revoke__:{}", id)));
    JsValue::Object(out)
}

/// Throw TypeError if `v` is a proxy whose revoke() has been called.
pub(super) fn check_revoked(v: &JsValue) -> Result<(), Signal> {
    if let JsValue::Proxy { handler, .. } = v {
        if let JsValue::Object(h) = handler.as_ref() {
            if let Some(JsValue::Number(id)) = h.get("__revoke_id__") {
                let revoked = REVOKED_PROXIES.with(|r| r.borrow().contains(&(*id as u64)));
                if revoked {
                    return Err(throw("TypeError", "Cannot perform operation on a proxy that has been revoked"));
                }
            }
        }
    }
    Ok(())
}

// ── MessageChannel / MessagePort ─────────────────────────────────────────────

/// new MessageChannel() → { port1, port2 }. Ports are value objects carrying a
/// shared channel id; onmessage handlers live in the PORT_HANDLERS registry so
/// postMessage on one (cloned) port can still reach the other's handler.
pub(super) fn make_message_channel() -> JsValue {
    let id = next_id();
    let make_port = |port_no: u8| {
        let mut p = HashMap::new();
        p.insert("__type__".to_string(), JsValue::String("MessagePort".to_string()));
        p.insert("__channel__".to_string(), JsValue::Number(id as f64));
        p.insert("__port__".to_string(), JsValue::Number(port_no as f64));
        JsValue::Object(p)
    };
    let mut out = HashMap::new();
    out.insert("__type__".to_string(), JsValue::String("MessageChannel".to_string()));
    out.insert("port1".to_string(), make_port(1));
    out.insert("port2".to_string(), make_port(2));
    JsValue::Object(out)
}

/// Called from set_property when assigning `port.onmessage = fn` so the
/// handler is visible to the sibling port despite value semantics.
pub(super) fn register_port_handler(map: &HashMap<String, JsValue>, handler: &JsValue) {
    let channel = match map.get("__channel__") { Some(JsValue::Number(n)) => *n as u64, _ => return };
    let port = match map.get("__port__") { Some(JsValue::Number(n)) => *n as u8, _ => return };
    PORT_HANDLERS.with(|h| { h.borrow_mut().insert((channel, port), handler.clone()); });
}

pub(super) fn call_message_port_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    match method {
        // Delivery is synchronous (pragmatic model — no task queue).
        "postMessage" => {
            let channel = match map.get("__channel__") { Some(JsValue::Number(n)) => *n as u64, _ => return Ok(JsValue::Undefined) };
            let port = match map.get("__port__") { Some(JsValue::Number(n)) => *n as u8, _ => return Ok(JsValue::Undefined) };
            let other = if port == 1 { 2 } else { 1 };
            // Clone the handler out so the borrow is released before the call
            // (the handler itself may post messages back).
            let handler = PORT_HANDLERS.with(|h| h.borrow().get(&(channel, other)).cloned());
            if let Some(handler) = handler {
                let mut event = HashMap::new();
                event.insert("data".to_string(), args.first().cloned().unwrap_or(JsValue::Undefined));
                call_function(&handler, &[JsValue::Object(event)], scope)?;
            }
            Ok(JsValue::Undefined)
        }
        _ => Ok(JsValue::Undefined), // start/close and unknown methods: no-ops
    }
}

// ── EventTarget ──────────────────────────────────────────────────────────────

pub(super) fn make_event_target() -> JsValue {
    let mut m = HashMap::new();
    m.insert("__type__".to_string(), JsValue::String("EventTarget".to_string()));
    m.insert("__listeners__".to_string(), JsValue::Object(HashMap::new()));
    JsValue::Object(m)
}

/// Pragmatic listener identity key: functions are not reference-comparable in
/// this value-semantics engine, so we key on name + parameter list.
fn listener_key(v: &JsValue) -> String {
    match v {
        JsValue::Function { name, params, .. } => format!("fn:{}({})", name.as_deref().unwrap_or(""), params.join(",")),
        JsValue::NativeFunction(n) => format!("native:{}", n),
        other => to_string(other),
    }
}

/// addEventListener / removeEventListener / dispatchEvent — synchronous
/// dispatch; mutations to __listeners__ persist via the caller's writeback.
pub(super) fn call_event_target_method(map: &mut HashMap<String, JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    let mut listeners = match map.get("__listeners__") {
        Some(JsValue::Object(l)) => l.clone(),
        _ => HashMap::new(),
    };
    match method {
        "addEventListener" => {
            let ev_type = args.first().map(to_string).unwrap_or_default();
            let handler = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            if matches!(handler, JsValue::Function { .. } | JsValue::NativeFunction(_)) {
                let entry = listeners.entry(ev_type).or_insert_with(|| JsValue::Array(Vec::new()));
                if let JsValue::Array(list) = entry {
                    // Per spec, adding the same listener twice is a no-op.
                    let key = listener_key(&handler);
                    if !list.iter().any(|l| listener_key(l) == key) {
                        list.push(handler);
                    }
                }
            }
            map.insert("__listeners__".to_string(), JsValue::Object(listeners));
            Ok(JsValue::Undefined)
        }
        "removeEventListener" => {
            let ev_type = args.first().map(to_string).unwrap_or_default();
            let key = args.get(1).map(listener_key).unwrap_or_default();
            if let Some(JsValue::Array(list)) = listeners.get_mut(&ev_type) {
                list.retain(|l| listener_key(l) != key);
            }
            map.insert("__listeners__".to_string(), JsValue::Object(listeners));
            Ok(JsValue::Undefined)
        }
        "dispatchEvent" => {
            let event = args.first().cloned().unwrap_or(JsValue::Undefined);
            let ev_type = match &event {
                JsValue::Object(e) => e.get("type").map(to_string).unwrap_or_default(),
                other => to_string(other),
            };
            if let Some(JsValue::Array(list)) = listeners.get(&ev_type) {
                for handler in list {
                    call_function(handler, &[event.clone()], scope)?;
                }
            }
            Ok(JsValue::Boolean(true))
        }
        _ => Ok(JsValue::Undefined),
    }
}

// ── WeakRef / FinalizationRegistry ───────────────────────────────────────────

/// Pragmatic WeakRef: this engine has no GC, so the "weak" ref is simply a
/// strong clone of the target; deref() always returns it.
pub(super) fn call_weakref_method(map: &HashMap<String, JsValue>, method: &str) -> EvalResult {
    match method {
        "deref" => Ok(map.get("__target__").cloned().unwrap_or(JsValue::Undefined)),
        _ => Ok(JsValue::Undefined),
    }
}

/// Pragmatic FinalizationRegistry: objects are never collected, so callbacks
/// never fire. register/unregister/cleanupSome are accepted no-ops.
pub(super) fn call_finalization_registry_method(method: &str) -> EvalResult {
    match method {
        "register" | "unregister" | "cleanupSome" => Ok(JsValue::Undefined),
        _ => Ok(JsValue::Undefined),
    }
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

    fn get_bool(v: &JsValue) -> bool {
        match v {
            JsValue::Boolean(b) => *b,
            _ => panic!("expected Boolean"),
        }
    }

    fn unwrap(v: EvalResult) -> JsValue {
        v.expect("expected Ok")
    }

    // ── make_error ───────────────────────────────────────────────────────

    #[test]
    fn make_error_structure() {
        let err = make_error("TypeError", "something went wrong");
        let m = get_obj(&err);
        assert_eq!(get_str(m.get("name").unwrap()), "TypeError");
        assert_eq!(get_str(m.get("message").unwrap()), "something went wrong");
    }

    #[test]
    fn make_error_empty_message() {
        let err = make_error("Error", "");
        let m = get_obj(&err);
        assert_eq!(get_str(m.get("message").unwrap()), "");
    }

    // ── random_uuid ──────────────────────────────────────────────────────

    #[test]
    fn random_uuid_format() {
        let uuid = random_uuid();
        // UUID v4 format: 8-4-4-4-12 hex chars
        assert_eq!(uuid.len(), 36);
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
        // Version 4: parts[2] starts with '4'
        assert!(parts[2].starts_with('4'));
    }

    #[test]
    fn random_uuid_unique() {
        let uuid1 = random_uuid();
        let uuid2 = random_uuid();
        assert_ne!(uuid1, uuid2);
    }

    // ── btoa_impl / atob_impl ────────────────────────────────────────────

    #[test]
    fn btoa_simple() {
        let result = unwrap(btoa_impl("hello"));
        assert_eq!(get_str(&result), "aGVsbG8=");
    }

    #[test]
    fn btoa_empty() {
        let result = unwrap(btoa_impl(""));
        assert_eq!(get_str(&result), "");
    }

    #[test]
    fn btoa_padding_one() {
        let result = unwrap(btoa_impl("a"));
        assert_eq!(get_str(&result), "YQ==");
    }

    #[test]
    fn btoa_padding_two() {
        let result = unwrap(btoa_impl("ab"));
        assert_eq!(get_str(&result), "YWI=");
    }

    #[test]
    fn btoa_no_padding() {
        let result = unwrap(btoa_impl("abc"));
        assert_eq!(get_str(&result), "YWJj");
    }

    #[test]
    fn btoa_invalid_character() {
        let result = btoa_impl("hello\u{1F600}");
        match result {
            Err(Signal::Throw(err)) => {
                let m = get_obj(&err);
                assert_eq!(get_str(m.get("name").unwrap()), "InvalidCharacterError");
            }
            _ => panic!("expected Throw signal"),
        }
    }

    #[test]
    fn atob_simple() {
        let result = unwrap(atob_impl("aGVsbG8="));
        assert_eq!(get_str(&result), "hello");
    }

    #[test]
    fn atob_empty() {
        let result = unwrap(atob_impl(""));
        assert_eq!(get_str(&result), "");
    }

    #[test]
    fn atob_no_padding() {
        let result = unwrap(atob_impl("YWJj"));
        assert_eq!(get_str(&result), "abc");
    }

    #[test]
    fn atob_with_whitespace() {
        let result = unwrap(atob_impl("aGVs bG8="));
        assert_eq!(get_str(&result), "hello");
    }

    #[test]
    fn atob_invalid_character() {
        let result = atob_impl("invalid!base64");
        match result {
            Err(Signal::Throw(err)) => {
                let m = get_obj(&err);
                assert_eq!(get_str(m.get("name").unwrap()), "InvalidCharacterError");
            }
            _ => panic!("expected Throw signal"),
        }
    }

    #[test]
    fn btoa_atob_roundtrip() {
        let original = "hello world";
        let encoded = unwrap(btoa_impl(original));
        let decoded = unwrap(atob_impl(get_str(&encoded)));
        assert_eq!(get_str(&decoded), original);
    }

    // ── url_can_parse ────────────────────────────────────────────────────

    #[test]
    fn url_can_parse_http() {
        assert!(url_can_parse("http://example.com"));
    }

    #[test]
    fn url_can_parse_https() {
        assert!(url_can_parse("https://example.com"));
    }

    #[test]
    fn url_can_parse_ftp() {
        assert!(url_can_parse("ftp://files.example.com"));
    }

    #[test]
    fn url_can_parse_data() {
        assert!(url_can_parse("data:text/html,<h1>test</h1>"));
    }

    #[test]
    fn url_can_parse_invalid_no_scheme() {
        assert!(!url_can_parse("://example.com"));
    }

    #[test]
    fn url_can_parse_invalid_no_host() {
        assert!(!url_can_parse("http://"));
    }

    #[test]
    fn url_can_parse_invalid_no_separator() {
        assert!(!url_can_parse("httpexample.com"));
    }

    #[test]
    fn url_can_parse_with_path() {
        assert!(url_can_parse("https://example.com/path/to/page"));
    }

    #[test]
    fn url_can_parse_with_port() {
        assert!(url_can_parse("http://localhost:8080"));
    }

    // ── crypto_get_random_values ─────────────────────────────────────────

    #[test]
    fn crypto_get_random_values_array() {
        let arr = JsValue::Array(vec![JsValue::Number(0.0); 10]);
        let result = crypto_get_random_values(&[arr]);
        if let JsValue::Array(filled) = result {
            assert_eq!(filled.len(), 10);
            // Check that at least some values are non-zero (probabilistic)
            let non_zero = filled.iter().filter(|v| {
                if let JsValue::Number(n) = v { *n != 0.0 } else { false }
            }).count();
            assert!(non_zero > 0);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn crypto_get_random_values_object() {
        let mut obj = HashMap::new();
        obj.insert("__data__".to_string(), JsValue::Array(vec![JsValue::Number(0.0); 5]));
        let result = crypto_get_random_values(&[JsValue::Object(obj)]);
        if let JsValue::Object(m) = result {
            if let JsValue::Array(data) = m.get("__data__").unwrap() {
                assert_eq!(data.len(), 5);
            } else {
                panic!("expected Array");
            }
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn crypto_get_random_values_none() {
        let result = crypto_get_random_values(&[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_native_extended ─────────────────────────────────────────────

    #[test]
    fn call_native_extended_crypto_uuid() {
        let result = unwrap(call_native_extended("crypto.randomUUID", &[]));
        let uuid = get_str(&result);
        assert_eq!(uuid.len(), 36);
        assert!(uuid.contains('-'));
    }

    #[test]
    fn call_native_extended_atob() {
        let result = unwrap(call_native_extended("atob", &[JsValue::String("aGVsbG8=".into())]));
        assert_eq!(get_str(&result), "hello");
    }

    #[test]
    fn call_native_extended_btoa() {
        let result = unwrap(call_native_extended("btoa", &[JsValue::String("hello".into())]));
        assert_eq!(get_str(&result), "aGVsbG8=");
    }

    #[test]
    fn call_native_extended_url_can_parse() {
        let result = unwrap(call_native_extended("URL.canParse", &[JsValue::String("https://example.com".into())]));
        assert!(get_bool(&result));
    }

    #[test]
    fn call_native_extended_unknown() {
        let result = unwrap(call_native_extended("unknownFunction", &[]));
        assert!(matches!(result, JsValue::Undefined));
    }
}