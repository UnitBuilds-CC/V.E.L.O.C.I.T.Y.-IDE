//! Fuzz target: HTML5 parser.
//!
//! Feeds arbitrary byte strings into the HTML5 parser and asserts
//! it never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;
use velocity_browser::parser::HtmlParser;

fuzz_target!(|data: &[u8]| {
    let html = String::from_utf8_lossy(data);
    // parse_html5 must not panic on any input.
    let _nodes = HtmlParser::parse_html5(&html);
});
