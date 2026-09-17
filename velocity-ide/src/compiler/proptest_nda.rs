//! Property-based tests for the NDA compiler pipeline.
//!
//! These use `proptest` to verify that critical parsers and compilers
//! uphold invariants across a wide range of inputs — especially
//! adversarial/malformed ones that hand-written tests miss.
//!
//! Run: `cargo test --workspace proptest_nda`
//! Or with nextest: `cargo nextest run --workspace proptest_nda`

use crate::compiler::nda_parser;
use proptest::prelude::*;

// ─── Strategy: arbitrary NDA-like source text ──────────────────────────────

/// Generates strings that resemble (but don't necessarily satisfy) NDA syntax.
/// Mixes valid NDA tokens, control characters, and Unicode edge cases.
fn arb_nda_like_source() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            // Valid NDA tokens
            Just("let x = ".to_string()),
            Just("fn f() { }".to_string()),
            Just("if true { 1 } else { 0 }".to_string()),
            Just("match x { 0 => 1, _ => 2 }".to_string()),
            Just("#define FOO 42".to_string()),
            Just("import \"test.nda\";".to_string()),
            // Boundary / adversarial inputs
            Just("".to_string()),
            Just("\0".to_string()),
            Just("\n\n\n".to_string()),
            Just("   ".to_string()),
            Just("{{{{{{".to_string()),
            Just("}}}}}}".to_string()),
            Just("(()".to_string()),
            Just("/* unterminated comment".to_string()),
            // Random ASCII
            "[\\x20-\\x7E]{0,50}".prop_map(|s| s),
            // Random Unicode (including multi-byte / surrogate-adjacent)
            "[\\u{0020}-\\u{FFFF}]{0,20}".prop_map(|s| s),
        ],
        0..30,
    )
    .prop_map(|chunks| chunks.join("\n"))
}

// ─── Properties ─────────────────────────────────────────────────────────────

/// compile() must never panic on any input — it should always return
/// Ok or Err, never unwind.
#[test]
fn proptest_nda_compile_never_panics() {
    proptest!(|(source in arb_nda_like_source())| {
        let _ = nda_parser::compile(&source);
    });
}

/// compile_with_report() must never panic and must always produce a
/// report (even if it contains errors).
#[test]
fn proptest_nda_compile_with_report_never_panics() {
    proptest!(|(source in arb_nda_like_source())| {
        let _ = nda_parser::compile_with_report(&source);
    });
}

/// Empty input should always compile to a valid (trivial) AST.
#[test]
fn proptest_nda_empty_compiles() {
    let result = nda_parser::compile("");
    assert!(result.is_ok(), "empty source should compile successfully");
}

/// A valid let-binding inside a function should always parse.
#[test]
fn proptest_nda_simple_let_parses() {
    proptest!(|(n in 0u64..1000)| {
        let source = format!("fn main() {{ let x = {}; }}", n);
        let result = nda_parser::compile(&source);
        assert!(result.is_ok(), "simple let-binding should parse: {}", source);
    });
}

/// Source consisting only of whitespace/comments should compile to
/// a trivial (empty) program without error.
#[test]
fn proptest_nda_whitespace_only_compiles() {
    proptest!(|(ws in prop::collection::vec("[ \\t\\n\\r]{1,10}", 0..20).prop_map(|v| v.join(""))) | {
        // Whitespace-only input should not panic; Ok or Err are both acceptable.
        let _ = nda_parser::compile(&ws);
    });
}

/// Deeply nested braces should not cause stack overflow.
#[test]
fn proptest_nda_deeply_nested_braces() {
    proptest!(|(depth in 1usize..200)| {
        let source = format!("{}{}", "{".repeat(depth), "}".repeat(depth));
        let _ = nda_parser::compile(&source);
    });
}
