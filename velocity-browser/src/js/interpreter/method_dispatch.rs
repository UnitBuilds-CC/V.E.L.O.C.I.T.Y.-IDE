use super::coercion::*;
use super::collections::*;
use super::core_methods::*;
use super::function::call_function_with_this;
use super::intl::*;
use super::signal::*;
use super::web_apis::*;
use crate::js::scope::ScopeRef;
use crate::js::vm::JsValue;
use std::collections::HashMap;

pub fn call_method(obj: &JsValue, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    match obj {
        JsValue::Array(arr) => {
            let mut a = arr.clone();
            call_array_method(&mut a, method, args, scope)
        }
        JsValue::String(s) => {
            // replace/replaceAll with a function replacement needs scope for the callback.
            if (method == "replace" || method == "replaceAll")
                && matches!(
                    args.get(1),
                    Some(JsValue::Function { .. } | JsValue::NativeFunction(_))
                )
            {
                return string_replace_with_fn(s, method, args, scope);
            }
            Ok(call_string_method(s, method, args))
        }
        JsValue::Object(map) => {
            // Check for Map/Set/Promise builtins
            let type_tag = map.get("__type__").map(to_string);
            match type_tag.as_deref() {
                Some("Map") | Some("WeakMap") => {
                    let mut m = map.clone();
                    return call_map_method(&mut m, method, args, scope);
                }
                Some("Set") | Some("WeakSet") => {
                    let mut m = map.clone();
                    return call_set_method(&mut m, method, args, scope);
                }
                Some("Promise") => return call_promise_method(map, method, args, scope),
                Some("Date") => return call_date_method_enhanced(map, method, args),
                Some("Generator") => return call_generator_method(map, method),
                Some("RegExp") => return call_regexp_method(map, method, args),
                Some("AbortController") => return call_abort_controller_method(map, method, args),
                Some("AbortSignal") => return call_abort_signal_method(map, method, args),
                Some("TextEncoder") => return call_text_encoder_method(map, method, args),
                Some("TextDecoder") => return call_text_decoder_method(map, method, args),
                Some("Response") => return call_response_method(map, method, args),
                Some("Blob") => return call_blob_method(map, method, args),
                Some("Uint8Array")
                | Some("Int8Array")
                | Some("Uint16Array")
                | Some("Int16Array")
                | Some("Uint32Array")
                | Some("Int32Array")
                | Some("Float32Array")
                | Some("Float64Array")
                | Some("Uint8ClampedArray") => return call_typed_array_method(map, method, args),
                Some("DataView") => return call_dataview_method(map, method, args),
                Some("Intl.Segmenter") => return call_segmenter_method(map, method, args),
                Some("Intl.Collator") => return call_collator_method(map, method, args),
                Some("Intl.NumberFormat") => return call_number_format_method(map, method, args),
                Some("Intl.DateTimeFormat") => {
                    return call_datetime_format_method(map, method, args)
                }
                Some("Intl.PluralRules") => return call_plural_rules_method(map, method, args),
                Some("Intl.RelativeTimeFormat") => {
                    return call_relative_time_format_method(map, method, args)
                }
                Some("Intl.DurationFormat") => {
                    return call_duration_format_method(map, method, args)
                }
                Some("Intl.ListFormat") => return call_list_format_method(map, method, args),
                Some("Intl.DisplayNames") => return call_display_names_method(map, method, args),
                Some("Segments") => {
                    // Iterator for segments
                    if method == "next" {
                        let segments = match map.get("__segments__") {
                            Some(JsValue::Array(arr)) => arr.clone(),
                            _ => Vec::new(),
                        };
                        let index = match map.get("__index__") {
                            Some(JsValue::Number(n)) => *n as usize,
                            _ => 0,
                        };
                        if index < segments.len() {
                            let mut updated = map.clone();
                            updated.insert(
                                "__index__".to_string(),
                                JsValue::Number((index + 1) as f64),
                            );
                            return Ok(JsValue::Object(updated));
                        }
                        let mut done = HashMap::new();
                        done.insert("done".to_string(), JsValue::Boolean(true));
                        done.insert("value".to_string(), JsValue::Undefined);
                        return Ok(JsValue::Object(done));
                    }
                }
                // A bound function answers call/apply/bind by re-targeting its
                // stored target; invoking it is handled in call_function_with_this.
                Some("BoundFunction") => {
                    let target = map.get("__target__").cloned().unwrap_or(JsValue::Undefined);
                    let bound_this = map.get("__this__").cloned().unwrap_or(JsValue::Undefined);
                    let bound_args = match map.get("__args__") {
                        Some(JsValue::Array(a)) => a.clone(),
                        _ => Vec::new(),
                    };
                    return match method {
                        "call" => {
                            let this_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                            call_function_with_this(&target, &args[1..], scope, Some(this_arg))
                        }
                        "apply" => {
                            let this_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                            let call_args = match args.get(1) {
                                Some(JsValue::Array(a)) => a.clone(),
                                _ => Vec::new(),
                            };
                            call_function_with_this(&target, &call_args, scope, Some(this_arg))
                        }
                        "bind" => {
                            let this_arg = args.first().cloned().unwrap_or(bound_this);
                            let mut bound = bound_args;
                            bound.extend(args.iter().skip(1).cloned());
                            let mut m = HashMap::new();
                            m.insert(
                                "__type__".to_string(),
                                JsValue::String("BoundFunction".to_string()),
                            );
                            m.insert("__target__".to_string(), target);
                            m.insert("__this__".to_string(), this_arg);
                            m.insert("__args__".to_string(), JsValue::Array(bound));
                            Ok(JsValue::Object(m))
                        }
                        _ => Ok(JsValue::Undefined),
                    };
                }
                _ => {}
            }
            // Call method with `this` bound to the object
            if let Some(func) = map.get(method) {
                return call_function_with_this(func, args, scope, Some(obj.clone()));
            }
            call_object_method_enhanced(map, method, args)
        }
        JsValue::Number(n) => Ok(call_number_method(*n, method, args)),
        JsValue::Boolean(b) => call_boolean_method(*b, method, args),
        JsValue::NativeFunction(name) => call_native_function_method(name, method, args),
        // Function.prototype.call / apply / bind.
        JsValue::Function { .. } => match method {
            "call" => {
                let this_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                call_function_with_this(obj, &args[1..], scope, Some(this_arg))
            }
            "apply" => {
                let this_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                let call_args = match args.get(1) {
                    Some(JsValue::Array(a)) => a.clone(),
                    _ => Vec::new(),
                };
                call_function_with_this(obj, &call_args, scope, Some(this_arg))
            }
            "bind" => {
                let this_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                let bound_args: Vec<JsValue> = args.iter().skip(1).cloned().collect();
                let mut m = HashMap::new();
                m.insert(
                    "__type__".to_string(),
                    JsValue::String("BoundFunction".to_string()),
                );
                m.insert("__target__".to_string(), obj.clone());
                m.insert("__this__".to_string(), this_arg);
                m.insert("__args__".to_string(), JsValue::Array(bound_args));
                Ok(JsValue::Object(m))
            }
            _ => Ok(JsValue::Undefined),
        },
        _ => Ok(JsValue::Undefined),
    }
}

/// Resolve a read-only constant exposed on a builtin namespace object
/// (e.g. `Math.PI`, `Number.MAX_SAFE_INTEGER`) when no user binding shadows it.
pub(super) fn builtin_namespace_constant(ns: &str, prop: &str) -> Option<JsValue> {
    use std::f64::consts;
    let v = match (ns, prop) {
        ("Math", "PI") => consts::PI,
        ("Math", "E") => consts::E,
        ("Math", "LN2") => consts::LN_2,
        ("Math", "LN10") => consts::LN_10,
        ("Math", "LOG2E") => consts::LOG2_E,
        ("Math", "LOG10E") => consts::LOG10_E,
        ("Math", "SQRT2") => consts::SQRT_2,
        ("Math", "SQRT1_2") => consts::FRAC_1_SQRT_2,
        ("Number", "MAX_SAFE_INTEGER") => 9007199254740991.0,
        ("Number", "MIN_SAFE_INTEGER") => -9007199254740991.0,
        ("Number", "MAX_VALUE") => f64::MAX,
        ("Number", "MIN_VALUE") => f64::MIN_POSITIVE,
        ("Number", "EPSILON") => f64::EPSILON,
        ("Number", "POSITIVE_INFINITY") => f64::INFINITY,
        ("Number", "NEGATIVE_INFINITY") => f64::NEG_INFINITY,
        ("Number", "NaN") => f64::NAN,
        _ => return None,
    };
    Some(JsValue::Number(v))
}

pub(super) fn flatten_array(a: &[JsValue], depth: usize) -> Vec<JsValue> {
    let mut out = Vec::new();
    for item in a {
        match item {
            JsValue::Array(inner) if depth > 0 => out.extend(flatten_array(inner, depth - 1)),
            other => out.push(other.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── flatten_array ──────────────────────────────────────────────────

    #[test]
    fn flatten_flat_passthrough() {
        let arr = vec![JsValue::Number(1.0), JsValue::Number(2.0)];
        let flat = flatten_array(&arr, 1);
        assert_eq!(flat, vec![JsValue::Number(1.0), JsValue::Number(2.0)]);
    }

    #[test]
    fn flatten_one_level() {
        let arr = vec![
            JsValue::Number(1.0),
            JsValue::Array(vec![JsValue::Number(2.0), JsValue::Number(3.0)]),
        ];
        let flat = flatten_array(&arr, 1);
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[0], JsValue::Number(1.0));
        assert_eq!(flat[1], JsValue::Number(2.0));
        assert_eq!(flat[2], JsValue::Number(3.0));
    }

    #[test]
    fn flatten_zero_depth_does_not_recurse() {
        let arr = vec![JsValue::Array(vec![JsValue::Number(1.0)])];
        let flat = flatten_array(&arr, 0);
        assert_eq!(flat.len(), 1);
        assert!(matches!(flat[0], JsValue::Array(_)));
    }

    #[test]
    fn flatten_deep_nested() {
        let arr = vec![JsValue::Array(vec![JsValue::Array(vec![JsValue::Number(
            1.0,
        )])])];
        let flat = flatten_array(&arr, 3);
        assert_eq!(flat, vec![JsValue::Number(1.0)]);
    }

    #[test]
    fn flatten_empty_array() {
        let arr: Vec<JsValue> = vec![];
        let flat = flatten_array(&arr, 5);
        assert!(flat.is_empty());
    }

    // ── builtin_namespace_constant ─────────────────────────────────────

    #[test]
    fn math_constants() {
        let pi = builtin_namespace_constant("Math", "PI");
        assert!(pi.is_some());
        if let Some(JsValue::Number(n)) = pi {
            assert!((n - std::f64::consts::PI).abs() < 1e-10);
        }
        let e = builtin_namespace_constant("Math", "E");
        assert!(e.is_some());
    }

    #[test]
    fn number_constants() {
        let max_safe = builtin_namespace_constant("Number", "MAX_SAFE_INTEGER");
        assert!(max_safe.is_some());
        if let Some(JsValue::Number(n)) = max_safe {
            assert_eq!(n, 9007199254740991.0);
        }
        let epsilon = builtin_namespace_constant("Number", "EPSILON");
        assert!(epsilon.is_some());
    }

    #[test]
    fn unknown_namespace_returns_none() {
        assert!(builtin_namespace_constant("Math", "NONEXISTENT").is_none());
        assert!(builtin_namespace_constant("JSON", "parse").is_none());
        assert!(builtin_namespace_constant("Number", "toString").is_none());
    }
}
