//! Central registry of NDA predicate ids.
//!
//! Every fact emitted for agent consumption uses a predicate id from this
//! registry instead of a magic number sprinkled through the codebase. Ids are
//! grouped into stable per-subsystem ranges so new predicates can be added
//! without colliding, and so a decoder can tell at a glance which subsystem a
//! fact came from.
//!
//! Ranges (inclusive):
//! - 10..=39    Agentic Object Model (AOM)
//! - 40..=69    Canvas / drawing
//! - 70..=99    Layout / geometry
//! - 100..=129  Session / document
//! - 200..=249  Network
//! - 250..=279  OCR / opaque visual regions
//!
//! Numeric values of already-persisted predicates are preserved to stay
//! compatible with existing NDA streams and tests.

// ---------------------------------------------------------------------------
// Agentic Object Model (AOM): 10..=39
// ---------------------------------------------------------------------------
/// ARIA/computed role of a node ("button", "link", "textbox", ...).
pub const AOM_ROLE: u16 = 10;
/// Accessible name (aria-label / placeholder / name / id / title).
pub const AOM_NAME: u16 = 11;
/// Current value of an input-like node.
pub const AOM_VALUE: u16 = 12;
/// Actionability score 0..=100 (how likely the agent wants to act on it).
pub const AOM_ACTIONABILITY: u16 = 13;
/// Node currently has focus.
pub const AOM_FOCUSED: u16 = 14;
/// Node is expanded (aria-expanded=true).
pub const AOM_EXPANDED: u16 = 15;

// ---------------------------------------------------------------------------
// Canvas / drawing: 40..=69
// ---------------------------------------------------------------------------
/// Canvas rendering context type ("2d", "webgl", ...).
pub const CANVAS_CONTEXT: u16 = 40;
/// Canvas dimensions, formatted "WxH".
pub const CANVAS_SIZE: u16 = 41;
/// Number of recorded draw calls.
pub const CANVAS_DRAW_CALLS: u16 = 42;
/// A text string drawn to the canvas (fillText/strokeText) - readable content.
pub const CANVAS_TEXT: u16 = 43;
/// A drawn shape command summary (rect/path/arc), formatted per extractor.
pub const CANVAS_SHAPE: u16 = 44;
/// An image drawn to the canvas (source reference + destination rect).
pub const CANVAS_IMAGE: u16 = 45;
/// A human-readable summary line describing the canvas contents.
pub const CANVAS_SUMMARY: u16 = 46;

// ---------------------------------------------------------------------------
// Layout / geometry: 70..=99
// ---------------------------------------------------------------------------
/// Bounding box of a node, formatted "x,y,w,h".
pub const LAYOUT_BOUNDS: u16 = 70;
/// Visibility ("visible" | "hidden").
pub const LAYOUT_VISIBILITY: u16 = 71;
/// Display mode ("block" | "inline" | "flex" | ...).
pub const LAYOUT_DISPLAY: u16 = 72;

// ---------------------------------------------------------------------------
// Session / document: 100..=129
// ---------------------------------------------------------------------------
/// Current URL of the session.
pub const SESSION_URL: u16 = 100;
/// Page title of the session.
pub const SESSION_TITLE: u16 = 101;
/// A cookie name -> value pair.
pub const SESSION_COOKIE: u16 = 102;
/// A local/session storage key -> value pair.
pub const SESSION_STORAGE: u16 = 103;

// ---------------------------------------------------------------------------
// Network: 200..=249
// ---------------------------------------------------------------------------
/// Request method for a URL.
pub const NET_METHOD: u16 = 200;
/// Response status for a URL.
pub const NET_STATUS: u16 = 201;

// ---------------------------------------------------------------------------
// OCR / opaque visual regions: 250..=279
// ---------------------------------------------------------------------------
/// A recognized-text region from real OCR (only emitted when genuinely read).
pub const OCR_TEXT: u16 = 252;
/// An opaque region the browser cannot interpret structurally.
/// Formatted "x,y,w,h". Confidence is intentionally 0 - never fabricated.
pub const OCR_OPAQUE_REGION: u16 = 253;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicate_ids_are_stable() {
        // These values are persisted in NDA streams; changing them breaks
        // decoding of previously captured state. Pin them explicitly.
        assert_eq!(AOM_ROLE, 10);
        assert_eq!(AOM_NAME, 11);
        assert_eq!(CANVAS_CONTEXT, 40);
        assert_eq!(LAYOUT_BOUNDS, 70);
        assert_eq!(SESSION_URL, 100);
        assert_eq!(NET_METHOD, 200);
        assert_eq!(OCR_TEXT, 252);
    }

    #[test]
    fn predicate_ids_are_unique() {
        let ids = [
            AOM_ROLE, AOM_NAME, AOM_VALUE, AOM_ACTIONABILITY, AOM_FOCUSED, AOM_EXPANDED,
            CANVAS_CONTEXT, CANVAS_SIZE, CANVAS_DRAW_CALLS, CANVAS_TEXT, CANVAS_SHAPE,
            CANVAS_IMAGE, CANVAS_SUMMARY, LAYOUT_BOUNDS, LAYOUT_VISIBILITY, LAYOUT_DISPLAY,
            SESSION_URL, SESSION_TITLE, SESSION_COOKIE, SESSION_STORAGE,
            NET_METHOD, NET_STATUS, OCR_TEXT, OCR_OPAQUE_REGION,
        ];
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            assert!(seen.insert(id), "duplicate predicate id {id}");
        }
    }
}
