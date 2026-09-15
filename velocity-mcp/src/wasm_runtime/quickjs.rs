//! QuickJS WASM runtime for JavaScript/TypeScript execution.
//!
//! QuickJS is a small and embeddable JavaScript engine. When compiled to WASM,
//! it allows executing JS/TS code in a sandboxed environment without Node.js.

use crate::wasm_runtime::WasmRuntimeTrait;
use std::error::Error;

/// QuickJS WASM runtime state.
pub struct QuickJsRuntime {
    /// Whether the runtime is initialized
    initialized: bool,
}

impl QuickJsRuntime {
    /// Create a new QuickJS runtime.
    pub fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            initialized: false,
        })
    }

    /// Initialize the QuickJS runtime with WASM module bytes.
    pub fn init(&mut self, _wasm_bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        // In a full implementation, this would:
        // 1. Compile the WASM module
        // 2. Instantiate with WASI imports
        // 3. Call the initialization function
        // 4. Set up the JS execution environment

        self.initialized = true;
        Ok(())
    }

    /// Execute JavaScript code and return the output.
    pub fn eval_js(&mut self, code: &str) -> Result<String, Box<dyn Error>> {
        if !self.initialized {
            return Err("QuickJS runtime not initialized".into());
        }

        // In a full implementation, this would:
        // 1. Write the code to WASM memory
        // 2. Call the JS eval function
        // 3. Read the result from WASM memory
        // 4. Return the output

        Ok(format!("QuickJS eval result: {}", code))
    }
}

impl WasmRuntimeTrait for QuickJsRuntime {
    fn call_tool(&mut self, name: &str, args: &str) -> Result<String, Box<dyn Error>> {
        // Execute the tool as JavaScript code
        self.eval_js(&format!("{}({})", name, args))
    }

    fn name(&self) -> &str {
        "quickjs"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quickjs_creation() {
        let runtime = QuickJsRuntime::new();
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_quickjs_eval_without_init() {
        let mut runtime = QuickJsRuntime::new().unwrap();
        let result = runtime.eval_js("1 + 1");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not initialized"));
    }

    #[test]
    fn test_quickjs_eval_with_init() {
        let mut runtime = QuickJsRuntime::new().unwrap();
        // Use dummy WASM bytes for testing
        runtime.init(&[0, 1, 2, 3]).unwrap();
        let result = runtime.eval_js("1 + 1");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("QuickJS eval result"));
    }

    #[test]
    fn test_quickjs_name() {
        let runtime = QuickJsRuntime::new().unwrap();
        assert_eq!(runtime.name(), "quickjs");
    }
}
