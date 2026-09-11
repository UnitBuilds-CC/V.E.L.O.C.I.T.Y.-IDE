use super::engine::reports::summarize_network_activity;
use super::engine::snapshot_diff::{role_actionability, url_encode};

#[test]
fn url_encode_handles_special_characters() {
    // Spaces use '+' (form encoding).
    assert_eq!(url_encode("hello world"), "hello+world");
    // Special chars use percent-encoding.
    assert_eq!(url_encode("a&b=c"), "a%26b%3Dc");
    assert_eq!(url_encode(""), "");
}

#[test]
fn role_actionability_returns_expected_levels() {
    // Interactive roles should have high actionability.
    assert!(role_actionability("button") > 0);
    assert!(role_actionability("textbox") > 0);
    // Unknown roles get a low default but non-zero score.
    assert!(role_actionability("presentation") < role_actionability("button"));
}

#[test]
fn summarize_network_activity_empty_events() {
    let summary = summarize_network_activity(&[]);
    assert_eq!(summary.redirect_count, 0);
    assert_eq!(summary.download_count, 0);
    assert_eq!(summary.event_count, 0);
}
