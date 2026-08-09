use super::*;

// ── Intl.Segmenter ───────────────────────────────────────────────────────

#[test]
fn intl_segmenter() {
    let result = eval_full("var seg = new Intl.Segmenter({locale: 'en', granularity: 'grapheme'}); seg.resolvedOptions().locale");
    assert_eq!(result, JsValue::String("en".to_string()));
}

// ── Intl.Collator ────────────────────────────────────────────────────────

#[test]
fn intl_collator() {
    let result = eval_full("var c = new Intl.Collator({locale: 'en'}); c.compare('a', 'b')");
    assert_eq!(result, JsValue::Number(-1.0));
}

// ── Intl.NumberFormat ────────────────────────────────────────────────────

#[test]
fn intl_number_format() {
    let result = eval_full(
        "var nf = new Intl.NumberFormat({locale: 'en-US', style: 'decimal'}); nf.format(1234.5)",
    );
    match result {
        JsValue::String(s) => assert!(s.contains("1234")),
        _ => panic!("Expected string"),
    }
}

// ── Intl.DateTimeFormat ──────────────────────────────────────────────────

#[test]
fn intl_datetime_format() {
    let result = eval_full("var dtf = new Intl.DateTimeFormat({locale: 'en-US'}); dtf.format(0)");
    match result {
        JsValue::String(s) => assert!(!s.is_empty()),
        _ => panic!("Expected string"),
    }
}

// ── Intl.PluralRules ─────────────────────────────────────────────────────

#[test]
fn intl_plural_rules() {
    assert_eq!(
        eval_full("var pr = new Intl.PluralRules({locale: 'en'}); pr.select(1)"),
        JsValue::String("one".to_string())
    );
    assert_eq!(
        eval_full("var pr = new Intl.PluralRules({locale: 'en'}); pr.select(2)"),
        JsValue::String("other".to_string())
    );
}

// ── Intl.RelativeTimeFormat ──────────────────────────────────────────────

#[test]
fn intl_relative_time_format() {
    let result =
        eval_full("var rtf = new Intl.RelativeTimeFormat({locale: 'en'}); rtf.format(-1, 'day')");
    match result {
        JsValue::String(s) => assert!(s.contains("yesterday") || s.contains("ago")),
        _ => panic!("Expected string"),
    }
}

// ── Intl.DurationFormat ──────────────────────────────────────────────────

#[test]
fn intl_duration_format() {
    let result = eval_full(
        "var df = new Intl.DurationFormat({locale: 'en'}); df.format({hours: 1, minutes: 30})",
    );
    match result {
        JsValue::String(s) => assert!(s.contains("1h") || s.contains("30min")),
        _ => panic!("Expected string"),
    }
}

// ── Intl.ListFormat ──────────────────────────────────────────────────────

#[test]
fn intl_list_format() {
    let result = eval_full("var lf = new Intl.ListFormat({locale: 'en', type: 'conjunction'}); lf.format(['a', 'b', 'c'])");
    match result {
        JsValue::String(s) => assert!(s.contains("and")),
        _ => panic!("Expected string"),
    }
}

// ── Intl.DisplayNames ────────────────────────────────────────────────────

#[test]
fn intl_display_names() {
    assert_eq!(
        eval_full("var dn = new Intl.DisplayNames({locale: 'en', type: 'language'}); dn.of('en')"),
        JsValue::String("English".to_string())
    );
    assert_eq!(
        eval_full("var dn = new Intl.DisplayNames({locale: 'en', type: 'region'}); dn.of('US')"),
        JsValue::String("United States".to_string())
    );
}
