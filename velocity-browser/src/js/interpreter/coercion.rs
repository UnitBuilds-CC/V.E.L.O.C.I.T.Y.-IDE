use crate::js::vm::JsValue;

pub(super) fn to_primitive(v: &JsValue) -> JsValue {
    match v {
        JsValue::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(|x| match x {
                JsValue::Null | JsValue::Undefined => String::new(),
                other => to_string(other),
            }).collect();
            JsValue::String(parts.join(","))
        }
        JsValue::Object(_) => JsValue::String("[object Object]".to_string()),
        other => other.clone(),
    }
}

pub fn to_number(v: &JsValue) -> f64 {
    match v {
        JsValue::Number(n) => *n,
        JsValue::Boolean(b) => if *b { 1.0 } else { 0.0 },
        JsValue::String(s) => string_to_number(s),
        JsValue::Null => 0.0,
        JsValue::Undefined => f64::NAN,
        _ => f64::NAN,
    }
}

fn string_to_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() { return 0.0; }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).map(|n| n as f64).unwrap_or(f64::NAN);
    }
    if let Some(bin) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        return i64::from_str_radix(bin, 2).map(|n| n as f64).unwrap_or(f64::NAN);
    }
    if let Some(oct) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
        return i64::from_str_radix(oct, 8).map(|n| n as f64).unwrap_or(f64::NAN);
    }
    match t.chars().next() {
        Some(c) if c.is_ascii_digit() || c == '+' || c == '-' || c == '.' => t.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

pub fn to_boolean(v: &JsValue) -> bool {
    match v {
        JsValue::Boolean(b) => *b,
        JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
        JsValue::String(s) => !s.is_empty(),
        JsValue::Null | JsValue::Undefined => false,
        JsValue::Array(_) | JsValue::Object(_) | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => true,
    }
}

pub fn to_string(v: &JsValue) -> String {
    match v {
        JsValue::String(s) => s.clone(),
        JsValue::Number(n) => format_number(*n),
        JsValue::Boolean(b) => b.to_string(),
        JsValue::Null => "null".to_string(),
        JsValue::Undefined => "undefined".to_string(),
        JsValue::Array(arr) => arr.iter().map(to_string).collect::<Vec<_>>().join(","),
        JsValue::Object(_) => "[object Object]".to_string(),
        JsValue::Function { name, .. } => format!("function {}() {{ [native code] }}", name.as_deref().unwrap_or("anonymous")),
        JsValue::NativeFunction(n) => format!("function {}() {{ [native code] }}", n),
        JsValue::Proxy { .. } => "[object Proxy]".to_string(),
    }
}

pub(super) fn format_number(n: f64) -> String {
    if n.is_nan() { return "NaN".to_string(); }
    if n.is_infinite() { return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string(); }
    if n == 0.0 { return "0".to_string(); }
    let negative = n < 0.0;
    let a = n.abs();
    let raw = format!("{}", a);
    let (mut digits, exp10) = if let Some(e_pos) = raw.find('e') {
        let sig = &raw[..e_pos];
        let exp: i64 = raw[e_pos + 1..].parse().unwrap_or(0);
        let d: String = sig.chars().filter(|c| *c != '.').collect();
        let frac = sig.find('.').map(|p| (sig.len() - p - 1) as i64).unwrap_or(0);
        (d, exp - frac)
    } else if let Some(p) = raw.find('.') {
        let d: String = raw.chars().filter(|c| *c != '.').collect();
        (d, p as i64)
    } else {
        let len = raw.len() as i64;
        (raw, len)
    };
    let leading = digits.len() - digits.trim_start_matches('0').len();
    digits = digits[leading..].to_string();
    let exp10 = exp10 - leading as i64;
    while digits.len() > 1 && digits.ends_with('0') {
        let cand = &digits[..digits.len() - 1];
        let e = exp10 - cand.len() as i64;
        if format!("{}e{}", cand, e).parse::<f64>() == Ok(a) { digits = cand.to_string(); } else { break; }
    }
    let k = digits.len() as i64;
    let body = if k <= exp10 && exp10 <= 21 {
        format!("{}{}", digits, "0".repeat((exp10 - k) as usize))
    } else if exp10 > 0 && exp10 < k {
        format!("{}.{}", &digits[..exp10 as usize], &digits[exp10 as usize..])
    } else if exp10 <= 0 && exp10 > -6 {
        format!("0.{}{}", "0".repeat((-exp10) as usize), digits)
    } else {
        let mantissa = if k == 1 { digits.clone() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
        let e = exp10 - 1;
        format!("{}e{}{}", mantissa, if e >= 0 { "+" } else { "-" }, e.abs())
    };
    if negative { format!("-{}", body) } else { body }
}

pub fn typeof_str(v: &JsValue) -> &'static str {
    match v {
        JsValue::Undefined => "undefined",
        JsValue::Null => "object",
        JsValue::Boolean(_) => "boolean",
        JsValue::Number(_) => "number",
        JsValue::String(_) => "string",
        JsValue::Function { .. } | JsValue::NativeFunction(_) => "function",
        JsValue::Array(_) | JsValue::Object(_) | JsValue::Proxy { .. } => "object",
    }
}

pub(super) fn relational_cmp(l: &JsValue, r: &JsValue) -> Option<std::cmp::Ordering> {
    if let (JsValue::String(a), JsValue::String(b)) = (l, r) {
        return Some(a.cmp(b));
    }
    let ln = to_number(l);
    let rn = to_number(r);
    ln.partial_cmp(&rn)
}

pub(super) fn loose_eq(l: &JsValue, r: &JsValue) -> bool {
    match (l, r) {
        (JsValue::Null | JsValue::Undefined, JsValue::Null | JsValue::Undefined) => true,
        (JsValue::Null | JsValue::Undefined, _) | (_, JsValue::Null | JsValue::Undefined) => false,
        (JsValue::String(a), JsValue::String(b)) => a == b,
        (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
        _ => to_number(l) == to_number(r),
    }
}

pub(super) fn strict_eq(l: &JsValue, r: &JsValue) -> bool {
    match (l, r) {
        (JsValue::Undefined, JsValue::Undefined) => true,
        (JsValue::Null, JsValue::Null) => true,
        (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
        (JsValue::Number(a), JsValue::Number(b)) => a == b,
        (JsValue::String(a), JsValue::String(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── to_number ──────────────────────────────────────────────────────

    #[test]
    fn to_number_passthrough() {
        assert_eq!(to_number(&JsValue::Number(42.0)), 42.0);
        assert!(to_number(&JsValue::Number(f64::NAN)).is_nan());
    }

    #[test]
    fn to_number_boolean() {
        assert_eq!(to_number(&JsValue::Boolean(true)), 1.0);
        assert_eq!(to_number(&JsValue::Boolean(false)), 0.0);
    }

    #[test]
    fn to_number_string_decimal() {
        assert_eq!(to_number(&JsValue::String("42".into())), 42.0);
        assert_eq!(to_number(&JsValue::String("2.72".into())), 2.72);
        assert_eq!(to_number(&JsValue::String("  10  ".into())), 10.0);
    }

    #[test]
    fn to_number_string_hex_bin_oct() {
        assert_eq!(to_number(&JsValue::String("0xff".into())), 255.0);
        assert_eq!(to_number(&JsValue::String("0X10".into())), 16.0);
        assert_eq!(to_number(&JsValue::String("0b1010".into())), 10.0);
        assert_eq!(to_number(&JsValue::String("0o17".into())), 15.0);
    }

    #[test]
    fn to_number_string_special() {
        assert_eq!(to_number(&JsValue::String("".into())), 0.0);
        assert_eq!(to_number(&JsValue::String("Infinity".into())), f64::INFINITY);
        assert_eq!(to_number(&JsValue::String("-Infinity".into())), f64::NEG_INFINITY);
        assert!(to_number(&JsValue::String("abc".into())).is_nan());
    }

    #[test]
    fn to_number_null_undefined() {
        assert_eq!(to_number(&JsValue::Null), 0.0);
        assert!(to_number(&JsValue::Undefined).is_nan());
    }

    // ── to_boolean ─────────────────────────────────────────────────────

    #[test]
    fn to_boolean_falsy_values() {
        assert!(!to_boolean(&JsValue::Boolean(false)));
        assert!(!to_boolean(&JsValue::Number(0.0)));
        assert!(!to_boolean(&JsValue::Number(f64::NAN)));
        assert!(!to_boolean(&JsValue::String(String::new())));
        assert!(!to_boolean(&JsValue::Null));
        assert!(!to_boolean(&JsValue::Undefined));
    }

    #[test]
    fn to_boolean_truthy_values() {
        assert!(to_boolean(&JsValue::Boolean(true)));
        assert!(to_boolean(&JsValue::Number(1.0)));
        assert!(to_boolean(&JsValue::Number(-1.0)));
        assert!(to_boolean(&JsValue::String("hi".into())));
        assert!(to_boolean(&JsValue::Array(vec![])));
        assert!(to_boolean(&JsValue::Object(HashMap::new())));
    }

    // ── to_string ──────────────────────────────────────────────────────

    #[test]
    fn to_string_primitives() {
        assert_eq!(to_string(&JsValue::String("hello".into())), "hello");
        assert_eq!(to_string(&JsValue::Number(42.0)), "42");
        assert_eq!(to_string(&JsValue::Boolean(true)), "true");
        assert_eq!(to_string(&JsValue::Null), "null");
        assert_eq!(to_string(&JsValue::Undefined), "undefined");
    }

    #[test]
    fn to_string_number_edge_cases() {
        assert_eq!(to_string(&JsValue::Number(f64::NAN)), "NaN");
        assert_eq!(to_string(&JsValue::Number(f64::INFINITY)), "Infinity");
        assert_eq!(to_string(&JsValue::Number(f64::NEG_INFINITY)), "-Infinity");
        assert_eq!(to_string(&JsValue::Number(0.0)), "0");
    }

    #[test]
    fn to_string_array_joins_with_comma() {
        let arr = JsValue::Array(vec![JsValue::Number(1.0), JsValue::Number(2.0), JsValue::Number(3.0)]);
        assert_eq!(to_string(&arr), "1,2,3");
    }

    #[test]
    fn to_string_object() {
        assert_eq!(to_string(&JsValue::Object(HashMap::new())), "[object Object]");
    }

    // ── typeof_str ─────────────────────────────────────────────────────

    #[test]
    fn typeof_str_all_types() {
        assert_eq!(typeof_str(&JsValue::Undefined), "undefined");
        assert_eq!(typeof_str(&JsValue::Null), "object"); // JS quirk
        assert_eq!(typeof_str(&JsValue::Boolean(true)), "boolean");
        assert_eq!(typeof_str(&JsValue::Number(1.0)), "number");
        assert_eq!(typeof_str(&JsValue::String("x".into())), "string");
        assert_eq!(typeof_str(&JsValue::Array(vec![])), "object");
        assert_eq!(typeof_str(&JsValue::Object(HashMap::new())), "object");
    }

    #[test]
    fn typeof_str_functions() {
        let func = JsValue::Function {
            name: Some("foo".into()),
            params: vec![],
            body: crate::js::interpreter::Stmt::Block(vec![]),
            closure: crate::js::scope::Scope::new_global(),
        };
        assert_eq!(typeof_str(&func), "function");
        assert_eq!(typeof_str(&JsValue::NativeFunction("parseInt".into())), "function");
    }

    // ── strict_eq ──────────────────────────────────────────────────────

    #[test]
    fn strict_eq_same_types() {
        assert!(strict_eq(&JsValue::Number(1.0), &JsValue::Number(1.0)));
        assert!(!strict_eq(&JsValue::Number(1.0), &JsValue::Number(2.0)));
        assert!(strict_eq(&JsValue::String("a".into()), &JsValue::String("a".into())));
        assert!(strict_eq(&JsValue::Null, &JsValue::Null));
        assert!(strict_eq(&JsValue::Undefined, &JsValue::Undefined));
    }

    #[test]
    fn strict_eq_different_types() {
        assert!(!strict_eq(&JsValue::Number(1.0), &JsValue::String("1".into())));
        assert!(!strict_eq(&JsValue::Null, &JsValue::Undefined));
        assert!(!strict_eq(&JsValue::Boolean(true), &JsValue::Number(1.0)));
    }

    // ── loose_eq ───────────────────────────────────────────────────────

    #[test]
    fn loose_eq_null_undefined() {
        assert!(loose_eq(&JsValue::Null, &JsValue::Undefined));
        assert!(loose_eq(&JsValue::Undefined, &JsValue::Null));
        assert!(loose_eq(&JsValue::Null, &JsValue::Null));
    }

    #[test]
    fn loose_eq_null_not_loose_equal_to_zero() {
        assert!(!loose_eq(&JsValue::Null, &JsValue::Number(0.0)));
        assert!(!loose_eq(&JsValue::Undefined, &JsValue::Number(0.0)));
    }

    #[test]
    fn loose_eq_coerces_number_string() {
        assert!(loose_eq(&JsValue::Number(1.0), &JsValue::String("1".into())));
        assert!(loose_eq(&JsValue::Boolean(true), &JsValue::Number(1.0)));
    }

    // ── format_number ──────────────────────────────────────────────────

    #[test]
    fn format_number_special() {
        assert_eq!(format_number(f64::NAN), "NaN");
        assert_eq!(format_number(f64::INFINITY), "Infinity");
        assert_eq!(format_number(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(format_number(0.0), "0");
    }

    #[test]
    fn format_number_integers_and_decimals() {
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(42.0), "42");
        assert_eq!(format_number(-7.0), "-7");
    }

    // ── to_primitive ───────────────────────────────────────────────────

    #[test]
    fn to_primitive_array_joins() {
        let arr = JsValue::Array(vec![JsValue::Number(1.0), JsValue::Number(2.0)]);
        assert_eq!(to_primitive(&arr), JsValue::String("1,2".into()));
    }

    #[test]
    fn to_primitive_array_null_becomes_empty() {
        let arr = JsValue::Array(vec![JsValue::Null, JsValue::Number(1.0), JsValue::Undefined]);
        assert_eq!(to_primitive(&arr), JsValue::String(",1,".into()));
    }

    #[test]
    fn to_primitive_object() {
        assert_eq!(to_primitive(&JsValue::Object(HashMap::new())), JsValue::String("[object Object]".into()));
    }

    #[test]
    fn to_primitive_passthrough_for_primitives() {
        assert_eq!(to_primitive(&JsValue::Number(5.0)), JsValue::Number(5.0));
        assert_eq!(to_primitive(&JsValue::String("hi".into())), JsValue::String("hi".into()));
    }

    // ── relational_cmp ─────────────────────────────────────────────────

    #[test]
    fn relational_cmp_strings_lexicographic() {
        let a = JsValue::String("abc".into());
        let b = JsValue::String("xyz".into());
        assert_eq!(relational_cmp(&a, &b), Some(std::cmp::Ordering::Less));
        assert_eq!(relational_cmp(&b, &a), Some(std::cmp::Ordering::Greater));
        assert_eq!(relational_cmp(&a, &a), Some(std::cmp::Ordering::Equal));
    }

    #[test]
    fn relational_cmp_numbers() {
        let a = JsValue::Number(10.0);
        let b = JsValue::Number(20.0);
        assert_eq!(relational_cmp(&a, &b), Some(std::cmp::Ordering::Less));
    }

    #[test]
    fn relational_cmp_nan_is_none() {
        let nan = JsValue::Number(f64::NAN);
        let one = JsValue::Number(1.0);
        assert_eq!(relational_cmp(&nan, &one), None);
    }
}
