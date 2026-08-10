//! Fuzz target: WebAssembly runner validation.
//!
//! Feeds arbitrary byte strings into the WASM validator and asserts
//! it never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;
use velocity_ide::compiler::wasm_runner::WasmPluginRunner;

fuzz_target!(|data: &[u8]| {
    // validate() must not panic on any input; Err is acceptable.
    let _ = WasmPluginRunner::validate(data);
});
