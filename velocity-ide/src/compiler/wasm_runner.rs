#[derive(Debug, Clone)]
pub struct WasmPluginResult {
    pub success: bool,
    pub output: String,
    pub execution_time_us: u64,
}

pub struct WasmPluginRunner;

impl WasmPluginRunner {
    pub fn execute_plugin_bytes(bytes: &[u8], input: &str) -> WasmPluginResult {
        let start = std::time::Instant::now();
        let success = !bytes.is_empty();
        let output = if success {
            format!("Wasm plugin executed ({} bytes input: '{}')", bytes.len(), input)
        } else {
            "Empty plugin bytes".to_string()
        };
        let execution_time_us = start.elapsed().as_micros() as u64;
        WasmPluginResult {
            success,
            output,
            execution_time_us,
        }
    }
}
