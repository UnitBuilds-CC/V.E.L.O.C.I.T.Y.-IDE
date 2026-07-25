use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct SandboxCapabilities {
    pub allow_network_hosts: Vec<String>,
    pub allow_file_system: bool,
    pub allow_wasm_execution: bool,
    pub allow_storage_access: bool,
    pub allow_popups: bool,
    pub allow_forms: bool,
    pub allow_scripts: bool,
    pub allow_same_origin: bool,
    pub allow_top_navigation: bool,
}

impl SandboxCapabilities {
    pub fn strict_isolation() -> Self {
        Self {
            allow_network_hosts: Vec::new(),
            allow_file_system: false,
            allow_wasm_execution: true,
            allow_storage_access: true,
            allow_popups: false,
            allow_forms: false,
            allow_scripts: true,
            allow_same_origin: false,
            allow_top_navigation: false,
        }
    }

    /// Permissive sandbox: everything allowed (for trusted content).
    pub fn permissive() -> Self {
        Self {
            allow_network_hosts: Vec::new(), // empty = allow all
            allow_file_system: true,
            allow_wasm_execution: true,
            allow_storage_access: true,
            allow_popups: true,
            allow_forms: true,
            allow_scripts: true,
            allow_same_origin: true,
            allow_top_navigation: true,
        }
    }

    /// Restrict network to specific hosts only.
    pub fn with_network_allowlist(mut self, hosts: Vec<String>) -> Self {
        self.allow_network_hosts = hosts;
        self
    }

    /// Disable script execution.
    pub fn without_scripts(mut self) -> Self {
        self.allow_scripts = false;
        self.allow_wasm_execution = false;
        self
    }
}

/// Sandbox violation with timestamp and severity.
#[derive(Debug, Clone)]
pub struct SandboxViolation {
    pub category: ViolationCategory,
    pub detail: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViolationCategory {
    Network,
    FileSystem,
    Wasm,
    Storage,
    Popup,
    Navigation,
}

pub struct TabSandbox {
    pub tab_id: String,
    pub capabilities: SandboxCapabilities,
    pub violations: Vec<String>,
    pub typed_violations: Vec<SandboxViolation>,
}

impl TabSandbox {
    pub fn new(tab_id: &str, capabilities: SandboxCapabilities) -> Self {
        Self {
            tab_id: tab_id.to_string(),
            capabilities,
            violations: Vec::new(),
            typed_violations: Vec::new(),
        }
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn record_violation(&mut self, category: ViolationCategory, detail: &str) -> String {
        let msg = format!("Security Violation: {} blocked by tab sandbox", detail);
        self.typed_violations.push(SandboxViolation {
            category,
            detail: detail.to_string(),
            timestamp_ms: Self::now_ms(),
        });
        self.violations.push(msg.clone());
        msg
    }

    pub fn check_network_access(&mut self, host: &str) -> Result<(), String> {
        if self.capabilities.allow_network_hosts.is_empty()
            || self.capabilities.allow_network_hosts.iter().any(|allowed| host.contains(allowed))
        {
            Ok(())
        } else {
            let detail = format!("Network access to '{}'", host);
            Err(self.record_violation(ViolationCategory::Network, &detail))
        }
    }

    pub fn check_file_access(&mut self, path: &str) -> Result<(), String> {
        if self.capabilities.allow_file_system {
            Ok(())
        } else {
            let detail = format!("File system access to '{}'", path);
            Err(self.record_violation(ViolationCategory::FileSystem, &detail))
        }
    }

    /// Check if WASM execution is allowed.
    pub fn check_wasm_execution(&mut self, module_name: &str) -> Result<(), String> {
        if self.capabilities.allow_wasm_execution {
            Ok(())
        } else {
            let detail = format!("WASM module '{}' execution", module_name);
            Err(self.record_violation(ViolationCategory::Wasm, &detail))
        }
    }

    /// Check if storage access is allowed.
    pub fn check_storage_access(&mut self, storage_type: &str) -> Result<(), String> {
        if self.capabilities.allow_storage_access {
            Ok(())
        } else {
            let detail = format!("{} storage access", storage_type);
            Err(self.record_violation(ViolationCategory::Storage, &detail))
        }
    }

    /// Check if popup creation is allowed.
    pub fn check_popup(&mut self, url: &str) -> Result<(), String> {
        if self.capabilities.allow_popups {
            Ok(())
        } else {
            let detail = format!("Popup to '{}'", url);
            Err(self.record_violation(ViolationCategory::Popup, &detail))
        }
    }

    /// Check if top-level navigation is allowed.
    pub fn check_top_navigation(&mut self, url: &str) -> Result<(), String> {
        if self.capabilities.allow_top_navigation {
            Ok(())
        } else {
            let detail = format!("Top-level navigation to '{}'", url);
            Err(self.record_violation(ViolationCategory::Navigation, &detail))
        }
    }

    /// Get violations by category.
    pub fn violations_by_category(&self, category: &ViolationCategory) -> Vec<&SandboxViolation> {
        self.typed_violations.iter().filter(|v| &v.category == category).collect()
    }

    /// Total violation count.
    pub fn violation_count(&self) -> usize {
        self.typed_violations.len()
    }

    /// Whether the sandbox has any violations.
    pub fn is_clean(&self) -> bool {
        self.typed_violations.is_empty()
    }

    /// Reset all violations (e.g., after navigating to a new page).
    pub fn reset_violations(&mut self) {
        self.violations.clear();
        self.typed_violations.clear();
    }

    /// Update capabilities dynamically.
    pub fn update_capabilities(&mut self, new_caps: SandboxCapabilities) {
        self.capabilities = new_caps;
    }

    pub fn export_sandbox_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for v in &self.violations {
            triples.push(NdaTriple::new(&self.tab_id, 220, v));
        }
        triples
    }
}
