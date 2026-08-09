use super::coercion::*;
use super::eval::{eval_stmt, MAX_PROXY_TRAP_DEPTH, PROXY_TRAP_DEPTH};
use super::native::call_native;
use super::signal::*;
use crate::js::scope::{Scope, ScopeRef};
use crate::js::vm::JsValue;
use std::collections::HashMap;

pub fn call_function(func: &JsValue, args: &[JsValue], _caller_scope: &ScopeRef) -> EvalResult {
    call_function_with_this(func, args, _caller_scope, None)
}

/// Call a function with an explicit `this` binding.
/// Returns (result, updated_this) so the caller can write-back mutations.
pub fn call_function_with_this(
    func: &JsValue,
    args: &[JsValue],
    _caller_scope: &ScopeRef,
    this_val: Option<JsValue>,
) -> EvalResult {
    match func {
        JsValue::Function {
            name,
            params,
            body,
            closure,
            ..
        } => {
            let is_generator = name
                .as_ref()
                .map(|n| n.starts_with("__generator__"))
                .unwrap_or(false);
            let call_scope = Scope::new_child(closure);
            if let Some(this) = this_val {
                Scope::declare(&call_scope, "this", this);
            }
            for (i, p) in params.iter().enumerate() {
                let val = args.get(i).cloned().unwrap_or(JsValue::Undefined);
                Scope::declare(&call_scope, p, val);
            }
            Scope::declare(&call_scope, "arguments", JsValue::Array(args.to_vec()));
            if is_generator {
                Scope::declare(&call_scope, "__yield_values__", JsValue::Array(Vec::new()));
                let _ = eval_stmt(body, &call_scope);
                let values = Scope::resolve(&call_scope, "__yield_values__")
                    .unwrap_or(JsValue::Array(Vec::new()));
                let mut iter = HashMap::new();
                iter.insert(
                    "__type__".to_string(),
                    JsValue::String("Generator".to_string()),
                );
                iter.insert("__values__".to_string(), values);
                iter.insert("__index__".to_string(), JsValue::Number(0.0));
                Ok(JsValue::Object(iter))
            } else {
                match eval_stmt(body, &call_scope) {
                    Ok(v) => Ok(v),
                    Err(Signal::Return(v)) => Ok(v),
                    Err(Signal::Throw(v)) => Err(Signal::Throw(v)),
                    Err(Signal::Break | Signal::Continue) => Ok(JsValue::Undefined),
                }
            }
        }
        JsValue::NativeFunction(name) => call_native(name, args),
        JsValue::Object(map)
            if map.get("__type__").map(to_string).as_deref() == Some("AsyncFunction") =>
        {
            if let Some(inner) = map.get("__inner__") {
                match call_function_with_this(inner, args, _caller_scope, this_val) {
                    Ok(val) => {
                        let mut promise = HashMap::new();
                        promise.insert(
                            "__type__".to_string(),
                            JsValue::String("Promise".to_string()),
                        );
                        promise.insert("__resolved__".to_string(), val);
                        Ok(JsValue::Object(promise))
                    }
                    Err(Signal::Throw(reason)) => {
                        let mut promise = HashMap::new();
                        promise.insert(
                            "__type__".to_string(),
                            JsValue::String("Promise".to_string()),
                        );
                        promise.insert("__rejected__".to_string(), reason);
                        Ok(JsValue::Object(promise))
                    }
                    Err(other) => Err(other),
                }
            } else {
                Ok(JsValue::Undefined)
            }
        }
        JsValue::Proxy { target, handler } => {
            if let JsValue::Object(h_map) = handler.as_ref() {
                if let Some(trap) = h_map.get("apply") {
                    if !matches!(trap, JsValue::NativeFunction(_)) {
                        let depth = PROXY_TRAP_DEPTH.with(|d| {
                            let cur = d.get();
                            if cur >= MAX_PROXY_TRAP_DEPTH {
                                return cur;
                            }
                            d.set(cur + 1);
                            cur
                        });
                        if depth < MAX_PROXY_TRAP_DEPTH {
                            let this_arg = this_val.clone().unwrap_or(JsValue::Undefined);
                            let args_array = JsValue::Array(args.to_vec());
                            let result = call_function(
                                trap,
                                &[(**target).clone(), this_arg, args_array],
                                &Scope::new_global(),
                            );
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            return result;
                        }
                    }
                }
            }
            call_function_with_this(target, args, _caller_scope, this_val)
        }
        JsValue::Object(map)
            if map.get("__type__").map(to_string).as_deref() == Some("BoundFunction") =>
        {
            let target = map.get("__target__").cloned().unwrap_or(JsValue::Undefined);
            let bound_this = map.get("__this__").cloned().unwrap_or(JsValue::Undefined);
            let mut full_args = match map.get("__args__") {
                Some(JsValue::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            full_args.extend_from_slice(args);
            call_function_with_this(&target, &full_args, _caller_scope, Some(bound_this))
        }
        _ => Ok(JsValue::Undefined),
    }
}

/// Call a function with `this` bound, and return the mutated `this` value after execution.
pub(super) fn call_method_with_this_writeback(
    func: &JsValue,
    args: &[JsValue],
    _scope: &ScopeRef,
    this_val: JsValue,
) -> (EvalResult, JsValue) {
    match func {
        JsValue::Function {
            params,
            body,
            closure,
            ..
        } => {
            let call_scope = Scope::new_child(closure);
            Scope::declare(&call_scope, "this", this_val.clone());
            if let JsValue::Object(this_map) = &this_val {
                if let Some(JsValue::String(class_name)) = this_map.get("__class_name__") {
                    if let Some(JsValue::Object(class_obj)) = Scope::resolve(closure, class_name) {
                        if let Some(parent) = class_obj.get("__parent__") {
                            Scope::declare(&call_scope, "__super__", parent.clone());
                        }
                    }
                }
            }
            for (i, p) in params.iter().enumerate() {
                let val = args.get(i).cloned().unwrap_or(JsValue::Undefined);
                Scope::declare(&call_scope, p, val);
            }
            Scope::declare(&call_scope, "arguments", JsValue::Array(args.to_vec()));
            let result = match eval_stmt(body, &call_scope) {
                Ok(v) => Ok(v),
                Err(Signal::Return(v)) => Ok(v),
                Err(Signal::Throw(v)) => Err(Signal::Throw(v)),
                Err(Signal::Break | Signal::Continue) => Ok(JsValue::Undefined),
            };
            let updated_this = Scope::resolve(&call_scope, "this").unwrap_or(this_val);
            (result, updated_this)
        }
        JsValue::NativeFunction(name) => (call_native(name, args), this_val),
        _ => (Ok(JsValue::Undefined), this_val),
    }
}

/// Spec-style parseInt: skips leading whitespace, honours an optional sign,
/// auto-detects a 0x/0X hex prefix when radix is unspecified (0) or 16, and
/// consumes the longest valid digit run for the radix. Returns NaN when no
/// digits are present or the radix is out of the 2..=36 range.
pub fn parse_int_js(input: &str, radix_arg: f64) -> f64 {
    let chars: Vec<char> = input.trim_start().chars().collect();
    let mut i = 0;
    let mut sign = 1.0;
    match chars.first() {
        Some('+') => i += 1,
        Some('-') => {
            sign = -1.0;
            i += 1;
        }
        _ => {}
    }
    let mut radix = if radix_arg.is_finite() {
        radix_arg as i64
    } else {
        0
    };
    if (radix == 0 || radix == 16)
        && chars.get(i) == Some(&'0')
        && matches!(chars.get(i + 1), Some('x') | Some('X'))
    {
        i += 2;
        radix = 16;
    }
    if radix == 0 {
        radix = 10;
    }
    if !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    let mut value = 0.0;
    let mut any = false;
    for &c in &chars[i..] {
        match c.to_digit(radix as u32) {
            Some(d) => {
                value = value * radix as f64 + d as f64;
                any = true;
            }
            None => break,
        }
    }
    if any {
        sign * value
    } else {
        f64::NAN
    }
}

/// Spec-style parseFloat: skips leading whitespace and parses the longest
/// leading substring that forms a valid decimal (with optional sign, fraction,
/// and exponent) or Infinity. Returns NaN when no numeric prefix is present.
pub fn parse_float_js(input: &str) -> f64 {
    let s = input.trim_start();
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    if unsigned.starts_with("Infinity") {
        return if s.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    if matches!(chars.first(), Some('+') | Some('-')) {
        i += 1;
    }
    let mut seen_digit = false;
    let mut seen_dot = false;
    while i < chars.len() {
        match chars[i] {
            c if c.is_ascii_digit() => {
                seen_digit = true;
                i += 1;
            }
            '.' if !seen_dot => {
                seen_dot = true;
                i += 1;
            }
            _ => break,
        }
    }
    if seen_digit && matches!(chars.get(i), Some('e') | Some('E')) {
        let mut j = i + 1;
        if matches!(chars.get(j), Some('+') | Some('-')) {
            j += 1;
        }
        let mut exp_digit = false;
        while matches!(chars.get(j), Some(c) if c.is_ascii_digit()) {
            exp_digit = true;
            j += 1;
        }
        if exp_digit {
            i = j;
        }
    }
    if !seen_digit {
        return f64::NAN;
    }
    chars[..i]
        .iter()
        .collect::<String>()
        .parse::<f64>()
        .unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_int_js ───────────────────────────────────────────────────

    #[test]
    fn parse_int_decimal() {
        assert_eq!(parse_int_js("42", 10.0), 42.0);
        assert_eq!(parse_int_js("  123  ", 10.0), 123.0);
    }

    #[test]
    fn parse_int_negative() {
        assert_eq!(parse_int_js("-7", 10.0), -7.0);
        assert_eq!(parse_int_js("+5", 10.0), 5.0);
    }

    #[test]
    fn parse_int_hex_auto_detect() {
        assert_eq!(parse_int_js("0xff", 0.0), 255.0);
        assert_eq!(parse_int_js("0X10", 0.0), 16.0);
    }

    #[test]
    fn parse_int_hex_explicit_radix() {
        assert_eq!(parse_int_js("ff", 16.0), 255.0);
        assert_eq!(parse_int_js("10", 16.0), 16.0);
    }

    #[test]
    fn parse_int_binary_radix() {
        assert_eq!(parse_int_js("1010", 2.0), 10.0);
    }

    #[test]
    fn parse_int_octal_radix() {
        assert_eq!(parse_int_js("17", 8.0), 15.0);
    }

    #[test]
    fn parse_int_invalid_radix_returns_nan() {
        assert!(parse_int_js("42", 1.0).is_nan());
        assert!(parse_int_js("42", 37.0).is_nan());
    }

    #[test]
    fn parse_int_no_digits_returns_nan() {
        assert!(parse_int_js("abc", 10.0).is_nan());
        assert!(parse_int_js("", 10.0).is_nan());
    }

    #[test]
    fn parse_int_stops_at_first_invalid() {
        assert_eq!(parse_int_js("123abc", 10.0), 123.0);
    }

    // ── parse_float_js ─────────────────────────────────────────────────

    #[test]
    fn parse_float_decimal() {
        assert_eq!(parse_float_js("2.72"), 2.72);
        assert_eq!(parse_float_js("  1.5  "), 1.5);
    }

    #[test]
    fn parse_float_integer() {
        assert_eq!(parse_float_js("42"), 42.0);
    }

    #[test]
    fn parse_float_infinity() {
        assert_eq!(parse_float_js("Infinity"), f64::INFINITY);
        assert_eq!(parse_float_js("-Infinity"), f64::NEG_INFINITY);
    }

    #[test]
    fn parse_float_nan_for_non_numeric() {
        assert!(parse_float_js("abc").is_nan());
        assert!(parse_float_js("").is_nan());
    }

    #[test]
    fn parse_float_exponent() {
        assert_eq!(parse_float_js("1e3"), 1000.0);
        assert_eq!(parse_float_js("2.5e-2"), 0.025);
    }

    #[test]
    fn parse_float_signed() {
        assert_eq!(parse_float_js("-5.5"), -5.5);
        assert_eq!(parse_float_js("+3.0"), 3.0);
    }
}
