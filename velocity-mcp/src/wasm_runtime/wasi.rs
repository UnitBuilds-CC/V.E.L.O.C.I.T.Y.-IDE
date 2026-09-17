//! WASI environment for WASM runtimes.
//!
//! Provides the WebAssembly System Interface (WASI) imports that WASM modules
//! need for file I/O, environment variables, clock access, etc.

/// WASI environment state shared across WASM executions.
pub struct WasiEnv {
    /// Preopened directories (sandboxed filesystem access)
    pub preopens: Vec<(String, String)>,
    /// Environment variables
    pub env_vars: Vec<(String, String)>,
    /// Arguments passed to the WASM module
    pub args: Vec<String>,
    /// Stdout capture buffer
    pub stdout_buffer: Vec<u8>,
    /// Stderr capture buffer
    pub stderr_buffer: Vec<u8>,
    /// Exit code from the WASM module
    pub exit_code: i32,
}

impl Default for WasiEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl WasiEnv {
    /// Create a new WASI environment with default settings.
    pub fn new() -> Self {
        Self {
            preopens: Vec::new(),
            env_vars: Vec::new(),
            args: Vec::new(),
            stdout_buffer: Vec::new(),
            stderr_buffer: Vec::new(),
            exit_code: 0,
        }
    }

    /// Add an environment variable.
    pub fn add_env_var(&mut self, key: &str, value: &str) {
        self.env_vars.push((key.to_string(), value.to_string()));
    }

    /// Add a command-line argument.
    pub fn add_arg(&mut self, arg: &str) {
        self.args.push(arg.to_string());
    }

    /// Add a preopened directory mapping.
    pub fn add_preopen(&mut self, guest_path: &str, host_path: &str) {
        self.preopens
            .push((guest_path.to_string(), host_path.to_string()));
    }

    /// Get stdout contents as string.
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout_buffer).to_string()
    }

    /// Get stderr contents as string.
    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr_buffer).to_string()
    }

    /// Clear all buffers and reset state.
    pub fn reset(&mut self) {
        self.stdout_buffer.clear();
        self.stderr_buffer.clear();
        self.exit_code = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasi_env_creation() {
        let env = WasiEnv::new();
        assert!(env.preopens.is_empty());
        assert!(env.env_vars.is_empty());
        assert!(env.args.is_empty());
        assert!(env.stdout_buffer.is_empty());
        assert!(env.stderr_buffer.is_empty());
        assert_eq!(env.exit_code, 0);
    }

    #[test]
    fn test_wasi_env_add_env_var() {
        let mut env = WasiEnv::new();
        env.add_env_var("TEST_KEY", "test_value");
        assert_eq!(env.env_vars.len(), 1);
        assert_eq!(env.env_vars[0].0, "TEST_KEY");
        assert_eq!(env.env_vars[0].1, "test_value");
    }

    #[test]
    fn test_wasi_env_add_arg() {
        let mut env = WasiEnv::new();
        env.add_arg("arg1");
        env.add_arg("arg2");
        assert_eq!(env.args.len(), 2);
    }

    #[test]
    fn test_wasi_env_reset() {
        let mut env = WasiEnv::new();
        env.stdout_buffer.push(b'h');
        env.stderr_buffer.push(b'e');
        env.exit_code = 1;

        env.reset();

        assert!(env.stdout_buffer.is_empty());
        assert!(env.stderr_buffer.is_empty());
        assert_eq!(env.exit_code, 0);
    }

    #[test]
    fn test_wasi_env_stdout_str() {
        let mut env = WasiEnv::new();
        env.stdout_buffer.extend_from_slice(b"Hello, World!");
        assert_eq!(env.stdout_str(), "Hello, World!");
    }
}
