//! Fuzz target: NDA lexer.
//!
//! Feeds arbitrary byte strings into the NDA tokenizer and asserts it
//! never panics — it must always return `Ok(tokens)` or `Err(msg)`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use velocity_ide::compiler::nda_lexer::NdaLexer;

fuzz_target!(|data: &[u8]| {
    let source = String::from_utf8_lossy(data);
    let mut lexer = NdaLexer::new(&source);
    // The lexer must not panic on any input; errors are acceptable.
    let _ = lexer.tokenize();
});
