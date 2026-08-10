//! Fuzz target: NDA parser.
//!
//! Feeds arbitrary strings into the NDA compiler pipeline (lexer → parser)
//! and asserts it never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;
use velocity_ide::compiler::nda_parser;

fuzz_target!(|data: &[u8]| {
    let source = String::from_utf8_lossy(data);
    // compile() must not panic; Err is fine.
    let _ = nda_parser::compile(&source);
});
