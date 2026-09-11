//! Fuzz target: NDA compiler validation.
//!
//! Feeds arbitrary strings into the NDA compiler and asserts
//! it never panics. Err results are acceptable.

#![no_main]

use libfuzzer_sys::fuzz_target;
use velocity_ide::compiler::nda_parser;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        // compile() must not panic on any input; Err is acceptable.
        let _ = nda_parser::compile(source);
    }
});
