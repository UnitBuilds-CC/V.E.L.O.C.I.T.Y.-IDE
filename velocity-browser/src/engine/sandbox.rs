use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct SandboxCapabilities {
    pub allow_network_hosts: Vec<String>,
    pub allow_file_system: bool,
    pub allow_wasm_execution: bool,
    pub allow_storage_access: bool,
}

impl SandboxCapabilities {
    pub fn strict_isolation() -> Self {
        Self {
            allow_network_hosts: Vec::new(),
            allow_file_system: false,
            allow_wasm_execution: true,
            allow_storage_access: true,
        }
    }
}

pub struct TabSandbox {
    pub tab_id: String,
    pub capabilities: SandboxCapabilities,
    pub violations: Vec<String>,
}

impl TabSandbox {
    pub fn new(tab_id: &str, capabilities: SandboxCapabilities) -> Self {
        Self {
            tab_id: tab_id.to_string(),
            capabilities,
            violations: Vec::new(),
        }
    }

    pub fn check_network_access(&mut self, host: &str) -> Result<(), String> {
        if self.capabilities.allow_network_hosts.is_empty()
            || self.capabilities.allow_network_hosts.iter().any(|allowed| host.contains(allowed))
        {
            Ok(())
        } else {
            let msg = format!("Security Violation: Network access to '{}' blocked by tab sandbox", host);
            self.violations.push(msg.clone());
            Err(msg)
        }
    }

    pub fn check_file_access(&mut self, path: &str) -> Result<(), String> {
        if self.capabilities.allow_file_system {
            Ok(())
        } else {
            let msg = format!("Security Violation: File system access to '{}' blocked by tab sandbox", path);
            self.violations.push(msg.clone());
            Err(msg)
        }
    }

    pub fn export_sandbox_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for v in &self.violations {
            triples.push(NdaTriple::new(&self.tab_id, 220, v));
        }
        triples
    }
}
