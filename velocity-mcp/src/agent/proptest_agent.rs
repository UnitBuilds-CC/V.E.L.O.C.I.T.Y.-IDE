//! Property-based tests for the agent NDA text encoding/decoding and
//! chat-history sanitization logic.
//!
//! Run: `cargo test -p velocity_mcp proptest_agent`

use proptest::prelude::*;
use super::nda::{encode_nda_text, decode_nda_text, encode_optional_nda_text, decode_optional_nda_text};
use super::executor::utils::sanitize_chat_token;

// ─── NDA text encode/decode roundtrip ──────────────────────────────────────

proptest! {
    /// encode → decode must be a perfect roundtrip for any input string.
    fn proptest_agent_nda_text_roundtrip(original in ".*") {
        let encoded = encode_nda_text(&original);
        let decoded = decode_nda_text(&encoded);
        prop_assert_eq!(decoded, original, "encode/decode roundtrip failed");
    }

    /// Encoded text must never contain raw tabs, newlines, or carriage returns.
    fn proptest_agent_nda_encoded_has_no_raw_control_chars(input in ".*") {
        let encoded = encode_nda_text(&input);
        prop_assert!(!encoded.contains('\t'), "encoded text contains raw tab");
        prop_assert!(!encoded.contains('\n'), "encoded text contains raw newline");
        prop_assert!(!encoded.contains('\r'), "encoded text contains raw CR");
    }

    /// Optional encode/decode roundtrip: Some(v) ↔ encode ↔ decode ↔ Some(v)
    fn proptest_agent_nda_optional_roundtrip(original in ".*") {
        let encoded = encode_optional_nda_text(Some(&original));
        let decoded = decode_optional_nda_text(&encoded);
        prop_assert_eq!(decoded, Some(original));
    }

    /// sanitize_chat_token must never panic and must produce output no longer
    /// than a reasonable multiple of input length.
    fn proptest_agent_sanitize_never_panics(input in "[\\x20-\\x7E\\n\\t]{0,500}") {
        let result = sanitize_chat_token(&input);
        prop_assert!(result.len() <= input.len() * 4 + 100);
    }

    /// Backslash at end of encoded string must decode without panic.
    fn proptest_agent_trailing_backslash_decodes(prefix in "[^\\\\]{0,100}") {
        let input = format!("{}\\", prefix);
        let _ = decode_nda_text(&input);
    }
}
