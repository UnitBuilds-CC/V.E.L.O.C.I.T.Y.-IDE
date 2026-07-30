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
