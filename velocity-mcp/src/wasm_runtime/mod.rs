//! WASM runtime abstraction for cross-language tool execution.
//!
//! Each language interpreter (QuickJS, MicroPython, etc.) is compiled to WASM
//! and runs in-process via Wasmer. This provides sandboxed execution without
//! needing system interpreters installed.

pub mod quickjs;
pub mod micropython;
pub mod wasi;

use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, LazyLock, Mutex};

/// Shared memory layout constants for WASM runtimes.
pub mod memory_layout {
    pub const EXEC_SLOT: u64 = 512 * 1024; // 512KB - for source code / args
    pub const ARGS_SLOT: u64 = 4 * 1024; // 4KB - for tool args JSON
    pub const NAME_SLOT: u64 = 8 * 1024; // 8KB - for tool name
}

/// Global WASM module compilation cache.
/// Maps WASM bytecode hash to cached Module for faster cold starts.
static MODULE_CACHE: LazyLock<Mutex<HashMap<u64, Arc<Vec<u8>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Cache a WASM module by its hash for faster subsequent loads.
pub fn cache_module(hash: u64, bytes: &[u8]) {
    if let Ok(mut cache) = MODULE_CACHE.lock() {
        cache.insert(hash, Arc::new(bytes.to_vec()));
    }
}

/// Retrieve a cached WASM module by hash.
pub fn get_cached_module(hash: u64) -> Option<Arc<Vec<u8>>> {
    MODULE_CACHE.lock().ok()?.get(&hash).cloned()
}

/// Execute a WASM tool call with the given runtime.
/// Returns the output string from the WASM execution.
pub fn execute_wasm_tool(
    runtime: &mut dyn WasmRuntimeTrait,
    tool_name: &str,
    args_json: &str,
) -> Result<String, Box<dyn Error>> {
    runtime.call_tool(tool_name, args_json)
}

/// Trait for WASM runtime implementations.
pub trait WasmRuntimeTrait {
    /// Call a tool by name with JSON arguments.
    fn call_tool(&mut self, name: &str, args: &str) -> Result<String, Box<dyn Error>>;

    /// Get the runtime name (e.g., "quickjs", "micropython").
    fn name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_cache() {
        let hash = 12345;
        let bytes = vec![0, 1, 2, 3];
        cache_module(hash, &bytes);
        let cached = get_cached_module(hash);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().as_ref(), &bytes);
    }

    #[test]
    fn test_module_cache_miss() {
        let cached = get_cached_module(99999);
        assert!(cached.is_none());
    }
}
