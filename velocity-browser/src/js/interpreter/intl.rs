use super::coercion::*;
use super::core_methods::is_leap_year;
use super::signal::*;
use crate::js::vm::JsValue;
use std::collections::HashMap;

// ── Intl.* support ───────────────────────────────────────────────────────────

pub(super) fn call_segmenter_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> EvalResult {
    Ok(match method {
        "segment" => {
            let s = args.first().map(to_string).unwrap_or_default();
            let _locale = map
                .get("locale")
                .map(to_string)
                .unwrap_or_else(|| "en".into());
            let granularity = map
                .get("granularity")
                .map(to_string)
                .unwrap_or_else(|| "grapheme".into());
            let segments: Vec<JsValue> = if granularity == "word" {
                s.split_whitespace()
                    .map(|w| {
                        let mut seg = HashMap::new();
                        seg.insert("segment".to_string(), JsValue::String(w.to_string()));
                        seg.insert("isWordLike".to_string(), JsValue::Boolean(true));
                        JsValue::Object(seg)
                    })
                    .collect()
            } else if granularity == "sentence" {
                s.split(['.', '!', '?'])
                    .filter(|s| !s.trim().is_empty())
                    .map(|sent| {
                        let mut seg = HashMap::new();
                        seg.insert(
                            "segment".to_string(),
                            JsValue::String(sent.trim().to_string()),
                        );
                        seg.insert("isWordLike".to_string(), JsValue::Boolean(false));
                        JsValue::Object(seg)
                    })
                    .collect()
            } else {
                s.chars()
                    .map(|c| {
                        let mut seg = HashMap::new();
                        seg.insert("segment".to_string(), JsValue::String(c.to_string()));
                        seg.insert(
                            "isWordLike".to_string(),
                            JsValue::Boolean(c.is_alphanumeric()),
                        );
                        JsValue::Object(seg)
                    })
                    .collect()
            };
            let mut result = HashMap::new();
            result.insert(
                "__type__".to_string(),
                JsValue::String("Segments".to_string()),
            );
            result.insert("__segments__".to_string(), JsValue::Array(segments));
            result.insert("__index__".to_string(), JsValue::Number(0.0));
            JsValue::Object(result)
        }
        "resolvedOptions" => {
            let mut opts = HashMap::new();
            opts.insert(
                "locale".to_string(),
                map.get("locale")
                    .cloned()
                    .unwrap_or(JsValue::String("en".into())),
            );
            opts.insert(
                "granularity".to_string(),
                map.get("granularity")
                    .cloned()
                    .unwrap_or(JsValue::String("grapheme".into())),
            );
            JsValue::Object(opts)
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_collator_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> EvalResult {
    Ok(match method {
        "compare" => {
            let a = args.first().map(to_string).unwrap_or_default();
            let b = args.get(1).map(to_string).unwrap_or_default();
            let sensitivity = map
                .get("sensitivity")
                .map(to_string)
                .unwrap_or_else(|| "variant".into());
            let cmp = if sensitivity == "base" {
                a.to_lowercase().cmp(&b.to_lowercase())
            } else {
                a.cmp(&b)
            };
            JsValue::Number(match cmp {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.0,
            })
        }
        "resolvedOptions" => {
            let mut opts = HashMap::new();
            opts.insert(
                "locale".to_string(),
                map.get("locale")
                    .cloned()
                    .unwrap_or(JsValue::String("en".into())),
            );
            opts.insert(
                "sensitivity".to_string(),
                map.get("sensitivity")
                    .cloned()
                    .unwrap_or(JsValue::String("variant".into())),
            );
            JsValue::Object(opts)
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_number_format_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> EvalResult {
    Ok(match method {
        "format" => {
            let n = args.first().map(to_number).unwrap_or(0.0);
            let _locale = map
                .get("locale")
                .map(to_string)
                .unwrap_or_else(|| "en-US".into());
            let style = map
                .get("style")
                .map(to_string)
                .unwrap_or_else(|| "decimal".into());
            let currency = map
                .get("currency")
                .map(to_string)
                .unwrap_or_else(|| "USD".into());
            let _minimum_fraction_digits = map
                .get("minimumFractionDigits")
                .map(|v| to_number(v) as usize)
                .unwrap_or(0);
            let maximum_fraction_digits = map
                .get("maximumFractionDigits")
                .map(|v| to_number(v) as usize)
                .unwrap_or(3);

            let formatted = if style == "currency" {
                let symbol = if currency == "USD" {
                    "$"
                } else if currency == "EUR" {
                    "€"
                } else if currency == "GBP" {
                    "£"
                } else {
                    &currency
                };
                format!("{}{:.*}", symbol, maximum_fraction_digits, n)
            } else if style == "percent" {
                format!("{:.*}%", maximum_fraction_digits, n * 100.0)
            } else {
                format!("{:.*}", maximum_fraction_digits, n)
            };
            JsValue::String(formatted)
        }
        "formatToParts" => {
            let n = args.first().map(to_number).unwrap_or(0.0);
            let mut parts = Vec::new();
            let mut part = HashMap::new();
            part.insert("type".to_string(), JsValue::String("integer".to_string()));
            part.insert(
                "value".to_string(),
                JsValue::String(format!("{}", n as i64)),
            );
            parts.push(JsValue::Object(part));
            JsValue::Array(parts)
        }
        "resolvedOptions" => {
            let mut opts = HashMap::new();
            opts.insert(
                "locale".to_string(),
                map.get("locale")
                    .cloned()
                    .unwrap_or(JsValue::String("en-US".into())),
            );
            opts.insert(
                "style".to_string(),
                map.get("style")
                    .cloned()
                    .unwrap_or(JsValue::String("decimal".into())),
            );
            JsValue::Object(opts)
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_datetime_format_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> EvalResult {
    Ok(match method {
        "format" => {
            let ts = args.first().map(to_number).unwrap_or(0.0);
            let secs = (ts / 1000.0).floor() as i64;
            let days = secs / 86400;
            let day_secs = secs % 86400;
            let hours = day_secs / 3600;
            let minutes = (day_secs % 3600) / 60;
            let seconds = day_secs % 60;

            let mut y = 1970;
            let mut remaining_days = days;
            loop {
                let days_in_year = if is_leap_year(y) { 366 } else { 365 };
                if remaining_days < days_in_year {
                    break;
                }
                remaining_days -= days_in_year;
                y += 1;
            }
            let month_days = if is_leap_year(y) {
                [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
            } else {
                [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
            };
            let mut m = 0;
            for (i, &md) in month_days.iter().enumerate() {
                if remaining_days < md {
                    m = i;
                    break;
                }
                remaining_days -= md;
            }
            let d = remaining_days + 1;

            let _date_style = map
                .get("dateStyle")
                .map(to_string)
                .unwrap_or_else(|| "full".into());
            let _time_style = map
                .get("timeStyle")
                .map(to_string)
                .unwrap_or_else(|| "full".into());

            let month_names = [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ];
            let day_names = [
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
            ];
            let dow = ((days % 7) + 4) % 7;

            let date_part = format!(
                "{}, {} {}, {}",
                day_names[dow as usize], month_names[m], d, y
            );
            let time_part = format!("{:02}:{:02}:{:02}", hours, minutes, seconds);
            JsValue::String(format!("{} at {}", date_part, time_part))
        }
        "formatToParts" => {
            let mut parts = Vec::new();
            let mut part = HashMap::new();
            part.insert("type".to_string(), JsValue::String("literal".to_string()));
            part.insert("value".to_string(), JsValue::String("".to_string()));
            parts.push(JsValue::Object(part));
            JsValue::Array(parts)
        }
        "resolvedOptions" => {
            let mut opts = HashMap::new();
            opts.insert(
                "locale".to_string(),
                map.get("locale")
                    .cloned()
                    .unwrap_or(JsValue::String("en-US".into())),
            );
            opts.insert(
                "dateStyle".to_string(),
                map.get("dateStyle")
                    .cloned()
                    .unwrap_or(JsValue::String("full".into())),
            );
            opts.insert(
                "timeStyle".to_string(),
                map.get("timeStyle")
                    .cloned()
                    .unwrap_or(JsValue::String("full".into())),
            );
            JsValue::Object(opts)
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_plural_rules_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> EvalResult {
    Ok(match method {
        "select" => {
            let n = args.first().map(to_number).unwrap_or(0.0).abs();
            let _locale = map
                .get("locale")
                .map(to_string)
                .unwrap_or_else(|| "en".into());
            let category = if n == 1.0 { "one" } else { "other" };
            JsValue::String(category.to_string())
        }
        "selectRange" => {
            let _start = args.first().map(to_number).unwrap_or(0.0);
            let _end = args.get(1).map(to_number).unwrap_or(0.0);
            let mut result = Vec::new();
            result.push(JsValue::String("other".to_string()));
            JsValue::Array(result)
        }
        "resolvedOptions" => {
            let mut opts = HashMap::new();
            opts.insert(
                "locale".to_string(),
                map.get("locale")
                    .cloned()
                    .unwrap_or(JsValue::String("en".into())),
            );
            opts.insert(
                "type".to_string(),
                map.get("type")
                    .cloned()
                    .unwrap_or(JsValue::String("cardinal".into())),
            );
            JsValue::Object(opts)
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_relative_time_format_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> EvalResult {
    Ok(match method {
        "format" => {
            let value = args.first().map(to_number).unwrap_or(0.0);
            let unit = args
                .get(1)
                .map(to_string)
                .unwrap_or_else(|| "second".into());
            let _locale = map
                .get("locale")
                .map(to_string)
                .unwrap_or_else(|| "en".into());
            let numeric = map
                .get("numeric")
                .map(to_string)
                .unwrap_or_else(|| "always".into());

            let abs_val = value.abs();
            let unit_str = if abs_val == 1.0 {
                &unit[..unit.len() - 1]
            } else {
                &unit
            };
            let direction = if value < 0.0 { "ago" } else { "from now" };

            if numeric == "auto" && abs_val <= 1.0 && unit == "day" {
                if value == -1.0 {
                    return Ok(JsValue::String("yesterday".to_string()));
                }
                if value == 0.0 {
                    return Ok(JsValue::String("today".to_string()));
                }
                if value == 1.0 {
                    return Ok(JsValue::String("tomorrow".to_string()));
                }
            }

            JsValue::String(format!("in {:.0} {} {}", abs_val, unit_str, direction))
        }
        "formatToParts" => {
            let mut parts = Vec::new();
            let mut part = HashMap::new();
            part.insert("type".to_string(), JsValue::String("literal".to_string()));
            part.insert("value".to_string(), JsValue::String("".to_string()));
            parts.push(JsValue::Object(part));
            JsValue::Array(parts)
        }
        "resolvedOptions" => {
            let mut opts = HashMap::new();
            opts.insert(
                "locale".to_string(),
                map.get("locale")
                    .cloned()
                    .unwrap_or(JsValue::String("en".into())),
            );
            opts.insert(
                "numeric".to_string(),
                map.get("numeric")
                    .cloned()
                    .unwrap_or(JsValue::String("always".into())),
            );
            JsValue::Object(opts)
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_duration_format_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> EvalResult {
    Ok(match method {
        "format" => {
            let duration = args.first().cloned().unwrap_or(JsValue::Undefined);
            if let JsValue::Object(dur_map) = duration {
                let years = dur_map.get("years").map(to_number).unwrap_or(0.0) as i64;
                let months = dur_map.get("months").map(to_number).unwrap_or(0.0) as i64;
                let days = dur_map.get("days").map(to_number).unwrap_or(0.0) as i64;
                let hours = dur_map.get("hours").map(to_number).unwrap_or(0.0) as i64;
                let minutes = dur_map.get("minutes").map(to_number).unwrap_or(0.0) as i64;
                let seconds = dur_map.get("seconds").map(to_number).unwrap_or(0.0) as i64;

                let mut parts = Vec::new();
                if years > 0 {
                    parts.push(format!("{}y", years));
                }
                if months > 0 {
                    parts.push(format!("{}m", months));
                }
                if days > 0 {
                    parts.push(format!("{}d", days));
                }
                if hours > 0 {
                    parts.push(format!("{}h", hours));
                }
                if minutes > 0 {
                    parts.push(format!("{}min", minutes));
                }
                if seconds > 0 {
                    parts.push(format!("{}s", seconds));
                }

                JsValue::String(parts.join(" "))
            } else {
                JsValue::String(String::new())
            }
        }
        "resolvedOptions" => {
            let mut opts = HashMap::new();
            opts.insert(
                "locale".to_string(),
                map.get("locale")
                    .cloned()
                    .unwrap_or(JsValue::String("en".into())),
            );
            JsValue::Object(opts)
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_list_format_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> EvalResult {
    Ok(match method {
        "format" => {
            let list = match args.first() {
                Some(JsValue::Array(arr)) => arr.iter().map(to_string).collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            let list_type = map
                .get("type")
                .map(to_string)
                .unwrap_or_else(|| "conjunction".into());

            if list.is_empty() {
                JsValue::String(String::new())
            } else if list.len() == 1 {
                JsValue::String(list[0].clone())
            } else {
                let conjunction = if list_type == "disjunction" {
                    "or"
                } else {
                    "and"
                };
                let mut result = list[..list.len() - 1].join(", ");
                if let Some(last) = list.last() {
                    result.push_str(&format!(" {} {}", conjunction, last));
                }
                JsValue::String(result)
            }
        }
        "formatToParts" => {
            let mut parts = Vec::new();
            let mut part = HashMap::new();
            part.insert("type".to_string(), JsValue::String("literal".to_string()));
            part.insert("value".to_string(), JsValue::String("".to_string()));
            parts.push(JsValue::Object(part));
            JsValue::Array(parts)
        }
        "resolvedOptions" => {
            let mut opts = HashMap::new();
            opts.insert(
                "locale".to_string(),
                map.get("locale")
                    .cloned()
                    .unwrap_or(JsValue::String("en".into())),
            );
            opts.insert(
                "type".to_string(),
                map.get("type")
                    .cloned()
                    .unwrap_or(JsValue::String("conjunction".into())),
            );
            JsValue::Object(opts)
        }
        _ => JsValue::Undefined,
    })
}

pub(super) fn call_display_names_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> EvalResult {
    Ok(match method {
        "of" => {
            let code = args.first().map(to_string).unwrap_or_default();
            let display_type = map
                .get("type")
                .map(to_string)
                .unwrap_or_else(|| "language".into());
            let _locale = map
                .get("locale")
                .map(to_string)
                .unwrap_or_else(|| "en".into());

            // Simple mapping for common codes
            let display = if display_type == "language" {
                match code.as_str() {
                    "en" => "English",
                    "es" => "Spanish",
                    "fr" => "French",
                    "de" => "German",
                    "zh" => "Chinese",
                    "ja" => "Japanese",
                    "ko" => "Korean",
                    "pt" => "Portuguese",
                    "ru" => "Russian",
                    "ar" => "Arabic",
                    _ => &code,
                }
            } else if display_type == "region" {
                match code.as_str() {
                    "US" => "United States",
                    "GB" => "United Kingdom",
                    "FR" => "France",
                    "DE" => "Germany",
                    "ES" => "Spain",
                    "IT" => "Italy",
                    "JP" => "Japan",
                    "CN" => "China",
                    "KR" => "South Korea",
                    "BR" => "Brazil",
                    _ => &code,
                }
            } else if display_type == "currency" {
                match code.as_str() {
                    "USD" => "US Dollar",
                    "EUR" => "Euro",
                    "GBP" => "British Pound",
                    "JPY" => "Japanese Yen",
                    "CNY" => "Chinese Yuan",
                    _ => &code,
                }
            } else {
                &code
            };

            JsValue::String(display.to_string())
        }
        "resolvedOptions" => {
            let mut opts = HashMap::new();
            opts.insert(
                "locale".to_string(),
                map.get("locale")
                    .cloned()
                    .unwrap_or(JsValue::String("en".into())),
            );
            opts.insert(
                "type".to_string(),
                map.get("type")
                    .cloned()
                    .unwrap_or(JsValue::String("language".into())),
            );
            JsValue::Object(opts)
        }
        _ => JsValue::Undefined,
    })
}

// ── Intl.Locale / Intl.getCanonicalLocales ───────────────────────────────

/// Canonicalize one BCP-47 tag: language lowercase, script Titlecase,
/// region uppercase (pragmatic — no extension/variant handling).
fn canonicalize_tag(tag: &str) -> String {
    tag.split('-')
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part.to_lowercase()
            } else if part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
                let mut c = part.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                    None => String::new(),
                }
            } else if part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()) {
                part.to_uppercase()
            } else {
                part.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// Intl.getCanonicalLocales(tags) → array of canonicalized tags.
pub(super) fn get_canonical_locales(args: &[JsValue]) -> EvalResult {
    let tags: Vec<String> = match args.first() {
        Some(JsValue::Array(arr)) => arr.iter().map(to_string).collect(),
        Some(JsValue::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    };
    let mut out: Vec<JsValue> = Vec::new();
    for tag in tags {
        let canon = canonicalize_tag(&tag);
        if !out
            .iter()
            .any(|v| matches!(v, JsValue::String(s) if *s == canon))
        {
            out.push(JsValue::String(canon));
        }
    }
    Ok(JsValue::Array(out))
}

/// new Intl.Locale(tag[, options]) — subtag fields are stored directly on the
/// object so plain property access (locale.language etc.) works.
pub(super) fn make_intl_locale(args: &[JsValue]) -> JsValue {
    let tag = canonicalize_tag(&args.first().map(to_string).unwrap_or_default());
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("Intl.Locale".to_string()),
    );
    let mut language = String::new();
    let mut script = JsValue::Undefined;
    let mut region = JsValue::Undefined;
    for (i, part) in tag.split('-').enumerate() {
        if i == 0 {
            language = part.to_string();
        } else if part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
            script = JsValue::String(part.to_string());
        } else if part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()) {
            region = JsValue::String(part.to_string());
        }
    }
    // Options override subtags parsed from the tag.
    if let Some(JsValue::Object(opts)) = args.get(1) {
        if let Some(l) = opts.get("language") {
            language = to_string(l).to_lowercase();
        }
        if let Some(s) = opts.get("script") {
            script = JsValue::String(to_string(s));
        }
        if let Some(r) = opts.get("region") {
            region = JsValue::String(to_string(r).to_uppercase());
        }
    }
    let mut base_name = language.clone();
    if let JsValue::String(s) = &script {
        base_name.push('-');
        base_name.push_str(s);
    }
    if let JsValue::String(r) = &region {
        base_name.push('-');
        base_name.push_str(r);
    }
    map.insert("language".to_string(), JsValue::String(language));
    map.insert("script".to_string(), script);
    map.insert("region".to_string(), region);
    map.insert("baseName".to_string(), JsValue::String(base_name));
    JsValue::Object(map)
}

pub(super) fn call_locale_method(map: &HashMap<String, JsValue>, method: &str) -> EvalResult {
    Ok(match method {
        "toString" => map
            .get("baseName")
            .cloned()
            .unwrap_or(JsValue::String(String::new())),
        // maximize(): fill in likely script/region for a few common languages.
        "maximize" => {
            let language = map.get("language").map(to_string).unwrap_or_default();
            let (likely_script, likely_region) = match language.as_str() {
                "en" => ("Latn", "US"),
                "es" => ("Latn", "ES"),
                "fr" => ("Latn", "FR"),
                "de" => ("Latn", "DE"),
                "pt" => ("Latn", "BR"),
                "it" => ("Latn", "IT"),
                "zh" => ("Hans", "CN"),
                "ja" => ("Jpan", "JP"),
                "ko" => ("Kore", "KR"),
                "ru" => ("Cyrl", "RU"),
                "ar" => ("Arab", "EG"),
                _ => ("Latn", "US"),
            };
            let script = match map.get("script") {
                Some(JsValue::String(s)) => s.clone(),
                _ => likely_script.to_string(),
            };
            let region = match map.get("region") {
                Some(JsValue::String(r)) => r.clone(),
                _ => likely_region.to_string(),
            };
            let mut out = map.clone();
            out.insert("script".to_string(), JsValue::String(script.clone()));
            out.insert("region".to_string(), JsValue::String(region.clone()));
            out.insert(
                "baseName".to_string(),
                JsValue::String(format!(
                    "{}-{}-{}",
                    map.get("language").map(to_string).unwrap_or_default(),
                    script,
                    region
                )),
            );
            JsValue::Object(out)
        }
        "minimize" => {
            let mut out = map.clone();
            out.insert("script".to_string(), JsValue::Undefined);
            out.insert("region".to_string(), JsValue::Undefined);
            out.insert(
                "baseName".to_string(),
                map.get("language")
                    .cloned()
                    .unwrap_or(JsValue::String(String::new())),
            );
            JsValue::Object(out)
        }
        _ => JsValue::Undefined,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── canonicalize_tag ───────────────────────────────────────────────

    #[test]
    fn canonicalize_language_only() {
        assert_eq!(canonicalize_tag("EN"), "en");
        assert_eq!(canonicalize_tag("fr"), "fr");
    }

    #[test]
    fn canonicalize_language_region() {
        assert_eq!(canonicalize_tag("en-us"), "en-US");
        assert_eq!(canonicalize_tag("FR-fr"), "fr-FR");
    }

    #[test]
    fn canonicalize_language_script_region() {
        assert_eq!(canonicalize_tag("zh-hant-tw"), "zh-Hant-TW");
    }

    // ── get_canonical_locales ──────────────────────────────────────────

    #[test]
    fn canonical_locales_array() {
        let r = get_canonical_locales(&[JsValue::Array(vec![
            JsValue::String("en-us".into()),
            JsValue::String("FR".into()),
        ])])
        .unwrap();
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 2);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn canonical_locales_string() {
        let r = get_canonical_locales(&[JsValue::String("en-US".into())]).unwrap();
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 1);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn canonical_locales_dedup() {
        let r = get_canonical_locales(&[JsValue::Array(vec![
            JsValue::String("en-us".into()),
            JsValue::String("en-US".into()),
        ])])
        .unwrap();
        if let JsValue::Array(arr) = r {
            assert_eq!(arr.len(), 1); // deduplicated after canonicalization
        } else {
            panic!("expected Array");
        }
    }

    // ── make_intl_locale ───────────────────────────────────────────────

    #[test]
    fn locale_language_only() {
        let r = make_intl_locale(&[JsValue::String("en".into())]);
        if let JsValue::Object(m) = r {
            assert_eq!(m.get("language").unwrap(), &JsValue::String("en".into()));
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn locale_language_region() {
        let r = make_intl_locale(&[JsValue::String("en-US".into())]);
        if let JsValue::Object(m) = r {
            assert_eq!(m.get("language").unwrap(), &JsValue::String("en".into()));
            assert_eq!(m.get("region").unwrap(), &JsValue::String("US".into()));
        } else {
            panic!("expected Object");
        }
    }

    // ── call_segmenter_method ──────────────────────────────────────────

    #[test]
    fn segmenter_grapheme() {
        let mut m = HashMap::new();
        m.insert(
            "granularity".to_string(),
            JsValue::String("grapheme".into()),
        );
        let r = call_segmenter_method(&m, "segment", &[JsValue::String("abc".into())]).unwrap();
        if let JsValue::Object(result) = r {
            if let Some(JsValue::Array(segs)) = result.get("__segments__") {
                assert_eq!(segs.len(), 3); // one per char
            } else {
                panic!("expected segments array");
            }
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn segmenter_word() {
        let mut m = HashMap::new();
        m.insert("granularity".to_string(), JsValue::String("word".into()));
        let r =
            call_segmenter_method(&m, "segment", &[JsValue::String("hello world".into())]).unwrap();
        if let JsValue::Object(result) = r {
            if let Some(JsValue::Array(segs)) = result.get("__segments__") {
                assert_eq!(segs.len(), 2); // two words
            } else {
                panic!("expected segments array");
            }
        } else {
            panic!("expected Object");
        }
    }

    // ── call_collator_method ───────────────────────────────────────────

    #[test]
    fn collator_compare_equal() {
        let m = HashMap::new();
        let r = call_collator_method(
            &m,
            "compare",
            &[JsValue::String("abc".into()), JsValue::String("abc".into())],
        )
        .unwrap();
        assert_eq!(r, JsValue::Number(0.0));
    }

    #[test]
    fn collator_compare_less() {
        let m = HashMap::new();
        let r = call_collator_method(
            &m,
            "compare",
            &[JsValue::String("a".into()), JsValue::String("b".into())],
        )
        .unwrap();
        assert_eq!(r, JsValue::Number(-1.0));
    }

    // ── call_number_format_method ──────────────────────────────────────

    #[test]
    fn number_format_decimal() {
        let m = HashMap::new();
        let r = call_number_format_method(&m, "format", &[JsValue::Number(1234.5)]).unwrap();
        if let JsValue::String(s) = r {
            assert!(s.contains("1234"), "got: {}", s);
        } else {
            panic!("expected String");
        }
    }

    #[test]
    fn number_format_currency() {
        let mut m = HashMap::new();
        m.insert("style".to_string(), JsValue::String("currency".into()));
        m.insert("currency".to_string(), JsValue::String("USD".into()));
        let r = call_number_format_method(&m, "format", &[JsValue::Number(42.0)]).unwrap();
        if let JsValue::String(s) = r {
            assert!(s.contains('$'), "got: {}", s);
        } else {
            panic!("expected String");
        }
    }

    #[test]
    fn number_format_percent() {
        let mut m = HashMap::new();
        m.insert("style".to_string(), JsValue::String("percent".into()));
        let r = call_number_format_method(&m, "format", &[JsValue::Number(0.5)]).unwrap();
        if let JsValue::String(s) = r {
            assert!(s.contains('%'), "got: {}", s);
        } else {
            panic!("expected String");
        }
    }

    // ── call_plural_rules_method ───────────────────────────────────────

    #[test]
    fn plural_select_one() {
        let m = HashMap::new();
        let r = call_plural_rules_method(&m, "select", &[JsValue::Number(1.0)]).unwrap();
        assert_eq!(r, JsValue::String("one".into()));
    }

    #[test]
    fn plural_select_other() {
        let m = HashMap::new();
        let r = call_plural_rules_method(&m, "select", &[JsValue::Number(5.0)]).unwrap();
        assert_eq!(r, JsValue::String("other".into()));
    }

    // ── call_list_format_method ────────────────────────────────────────

    #[test]
    fn list_format_conjunction() {
        let m = HashMap::new();
        let list = JsValue::Array(vec![
            JsValue::String("a".into()),
            JsValue::String("b".into()),
            JsValue::String("c".into()),
        ]);
        let r = call_list_format_method(&m, "format", &[list]).unwrap();
        if let JsValue::String(s) = r {
            assert!(s.contains("and"), "got: {}", s);
        } else {
            panic!("expected String");
        }
    }

    #[test]
    fn list_format_disjunction() {
        let mut m = HashMap::new();
        m.insert("type".to_string(), JsValue::String("disjunction".into()));
        let list = JsValue::Array(vec![
            JsValue::String("x".into()),
            JsValue::String("y".into()),
        ]);
        let r = call_list_format_method(&m, "format", &[list]).unwrap();
        if let JsValue::String(s) = r {
            assert!(s.contains("or"), "got: {}", s);
        } else {
            panic!("expected String");
        }
    }

    #[test]
    fn list_format_empty() {
        let m = HashMap::new();
        let list = JsValue::Array(vec![]);
        let r = call_list_format_method(&m, "format", &[list]).unwrap();
        assert_eq!(r, JsValue::String(String::new()));
    }

    #[test]
    fn list_format_single() {
        let m = HashMap::new();
        let list = JsValue::Array(vec![JsValue::String("only".into())]);
        let r = call_list_format_method(&m, "format", &[list]).unwrap();
        assert_eq!(r, JsValue::String("only".into()));
    }

    // ── call_display_names_method ──────────────────────────────────────

    #[test]
    fn display_names_language() {
        let mut m = HashMap::new();
        m.insert("type".to_string(), JsValue::String("language".into()));
        let r = call_display_names_method(&m, "of", &[JsValue::String("en".into())]).unwrap();
        assert_eq!(r, JsValue::String("English".into()));
    }

    #[test]
    fn display_names_region() {
        let mut m = HashMap::new();
        m.insert("type".to_string(), JsValue::String("region".into()));
        let r = call_display_names_method(&m, "of", &[JsValue::String("US".into())]).unwrap();
        assert_eq!(r, JsValue::String("United States".into()));
    }

    #[test]
    fn display_names_currency() {
        let mut m = HashMap::new();
        m.insert("type".to_string(), JsValue::String("currency".into()));
        let r = call_display_names_method(&m, "of", &[JsValue::String("EUR".into())]).unwrap();
        assert_eq!(r, JsValue::String("Euro".into()));
    }

    #[test]
    fn display_names_unknown_code() {
        let mut m = HashMap::new();
        m.insert("type".to_string(), JsValue::String("language".into()));
        let r = call_display_names_method(&m, "of", &[JsValue::String("xx".into())]).unwrap();
        assert_eq!(r, JsValue::String("xx".into()));
    }

    // ── call_relative_time_format_method ───────────────────────────────

    #[test]
    fn relative_time_future() {
        let m = HashMap::new();
        let r = call_relative_time_format_method(
            &m,
            "format",
            &[JsValue::Number(3.0), JsValue::String("day".into())],
        )
        .unwrap();
        if let JsValue::String(s) = r {
            assert!(s.contains("from now"), "got: {}", s);
        } else {
            panic!("expected String");
        }
    }

    #[test]
    fn relative_time_past() {
        let m = HashMap::new();
        let r = call_relative_time_format_method(
            &m,
            "format",
            &[JsValue::Number(-2.0), JsValue::String("hour".into())],
        )
        .unwrap();
        if let JsValue::String(s) = r {
            assert!(s.contains("ago"), "got: {}", s);
        } else {
            panic!("expected String");
        }
    }

    #[test]
    fn relative_time_auto_yesterday() {
        let mut m = HashMap::new();
        m.insert("numeric".to_string(), JsValue::String("auto".into()));
        let r = call_relative_time_format_method(
            &m,
            "format",
            &[JsValue::Number(-1.0), JsValue::String("day".into())],
        )
        .unwrap();
        assert_eq!(r, JsValue::String("yesterday".into()));
    }

    #[test]
    fn relative_time_auto_tomorrow() {
        let mut m = HashMap::new();
        m.insert("numeric".to_string(), JsValue::String("auto".into()));
        let r = call_relative_time_format_method(
            &m,
            "format",
            &[JsValue::Number(1.0), JsValue::String("day".into())],
        )
        .unwrap();
        assert_eq!(r, JsValue::String("tomorrow".into()));
    }

    // ── call_duration_format_method ────────────────────────────────────

    #[test]
    fn duration_format_basic() {
        let m = HashMap::new();
        let mut dur = HashMap::new();
        dur.insert("hours".to_string(), JsValue::Number(2.0));
        dur.insert("minutes".to_string(), JsValue::Number(30.0));
        let r = call_duration_format_method(&m, "format", &[JsValue::Object(dur)]).unwrap();
        if let JsValue::String(s) = r {
            assert!(s.contains("2h"), "got: {}", s);
            assert!(s.contains("30min"), "got: {}", s);
        } else {
            panic!("expected String");
        }
    }

    #[test]
    fn duration_format_empty() {
        let m = HashMap::new();
        let dur = HashMap::new();
        let r = call_duration_format_method(&m, "format", &[JsValue::Object(dur)]).unwrap();
        assert_eq!(r, JsValue::String(String::new()));
    }
}
