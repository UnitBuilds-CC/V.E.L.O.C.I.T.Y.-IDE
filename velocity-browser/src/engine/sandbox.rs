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
            || self
                .capabilities
                .allow_network_hosts
                .iter()
                .any(|allowed| host.contains(allowed))
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
        self.typed_violations
            .iter()
            .filter(|v| &v.category == category)
            .collect()
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

/// Parsed Content-Security-Policy directives.
#[derive(Debug, Clone, Default)]
pub struct ContentSecurityPolicy {
    pub script_src: Vec<String>,
    pub style_src: Vec<String>,
    pub img_src: Vec<String>,
    pub connect_src: Vec<String>,
    pub font_src: Vec<String>,
    pub media_src: Vec<String>,
    pub object_src: Vec<String>,
    pub default_src: Vec<String>,
    pub report_uri: Option<String>,
    pub report_only: bool,
}

impl ContentSecurityPolicy {
    /// Parse a CSP header string into directives.
    pub fn parse(header: &str, report_only: bool) -> Self {
        let mut csp = ContentSecurityPolicy {
            report_only,
            ..Default::default()
        };
        for directive in header.split(';') {
            let parts: Vec<&str> = directive.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            let (name, values) = (parts[0], &parts[1..]);
            let src_list: Vec<String> = values.iter().map(|s| s.to_string()).collect();
            match name {
                "script-src" => csp.script_src = src_list,
                "style-src" => csp.style_src = src_list,
                "img-src" => csp.img_src = src_list,
                "connect-src" => csp.connect_src = src_list,
                "font-src" => csp.font_src = src_list,
                "media-src" => csp.media_src = src_list,
                "object-src" => csp.object_src = src_list,
                "default-src" => csp.default_src = src_list,
                "report-uri" => csp.report_uri = values.first().map(|s| s.to_string()),
                _ => {}
            }
        }
        csp
    }

    /// Check if a URL is allowed by a given source list.
    fn is_allowed(source_list: &[String], url: &str) -> bool {
        if source_list.is_empty() {
            return true;
        } // no restriction
        for src in source_list {
            if src == "'self'" {
                // Allow same-origin (simplified: same scheme+host)
                if !url.contains("://") || url.starts_with("same-origin") {
                    return true;
                }
            } else if src == "'unsafe-inline'" || src == "'unsafe-eval'" || src == "'none'" {
                continue;
            } else if src == "*" || url.contains(src) {
                return true;
            }
        }
        false
    }

    /// Check if a script load is allowed.
    pub fn allows_script(&self, url: &str) -> bool {
        let list = if !self.script_src.is_empty() {
            &self.script_src
        } else {
            &self.default_src
        };
        Self::is_allowed(list, url)
    }

    /// Check if a style load is allowed.
    pub fn allows_style(&self, url: &str) -> bool {
        let list = if !self.style_src.is_empty() {
            &self.style_src
        } else {
            &self.default_src
        };
        Self::is_allowed(list, url)
    }

    /// Check if an image load is allowed.
    pub fn allows_image(&self, url: &str) -> bool {
        let list = if !self.img_src.is_empty() {
            &self.img_src
        } else {
            &self.default_src
        };
        Self::is_allowed(list, url)
    }

    /// Check if a fetch/XHR connection is allowed.
    pub fn allows_connect(&self, url: &str) -> bool {
        let list = if !self.connect_src.is_empty() {
            &self.connect_src
        } else {
            &self.default_src
        };
        Self::is_allowed(list, url)
    }

    /// Serialize back to a CSP header string.
    pub fn to_header_string(&self) -> String {
        let mut parts = Vec::new();
        if !self.default_src.is_empty() {
            parts.push(format!("default-src {}", self.default_src.join(" ")));
        }
        if !self.script_src.is_empty() {
            parts.push(format!("script-src {}", self.script_src.join(" ")));
        }
        if !self.style_src.is_empty() {
            parts.push(format!("style-src {}", self.style_src.join(" ")));
        }
        if !self.img_src.is_empty() {
            parts.push(format!("img-src {}", self.img_src.join(" ")));
        }
        if !self.connect_src.is_empty() {
            parts.push(format!("connect-src {}", self.connect_src.join(" ")));
        }
        if let Some(uri) = &self.report_uri {
            parts.push(format!("report-uri {}", uri));
        }
        parts.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strict_sandbox_violations() {
        let caps = SandboxCapabilities::strict_isolation()
            .with_network_allowlist(vec!["api.example.com".into()]);
        let mut sb = TabSandbox::new("tab1", caps);
        assert!(sb.check_network_access("api.example.com").is_ok());
        assert!(sb.check_network_access("evil.com").is_err());
        assert!(sb.check_file_access("/etc/passwd").is_err());
        assert_eq!(sb.violation_count(), 2);
    }

    #[test]
    fn test_permissive_sandbox() {
        let caps = SandboxCapabilities::permissive();
        let mut sb = TabSandbox::new("tab2", caps);
        assert!(sb.check_network_access("anything.com").is_ok());
        assert!(sb.check_file_access("/any/path").is_ok());
        assert!(sb.is_clean());
    }

    #[test]
    fn test_violations_by_category() {
        let caps = SandboxCapabilities::strict_isolation()
            .with_network_allowlist(vec!["allowed.com".into()]);
        let mut sb = TabSandbox::new("tab3", caps);
        let _ = sb.check_network_access("blocked1.com");
        let _ = sb.check_network_access("blocked2.com");
        let _ = sb.check_file_access("/secret");
        assert_eq!(
            sb.violations_by_category(&ViolationCategory::Network).len(),
            2
        );
        assert_eq!(
            sb.violations_by_category(&ViolationCategory::FileSystem)
                .len(),
            1
        );
    }

    #[test]
    fn test_csp_parse() {
        let csp = ContentSecurityPolicy::parse(
            "default-src 'self'; script-src https://cdn.example.com; img-src *",
            false,
        );
        assert_eq!(csp.default_src, vec!["'self'"]);
        assert_eq!(csp.script_src, vec!["https://cdn.example.com"]);
        assert_eq!(csp.img_src, vec!["*"]);
        assert!(!csp.report_only);
    }

    #[test]
    fn test_csp_allows_script() {
        let csp = ContentSecurityPolicy::parse("script-src https://cdn.example.com 'self'", false);
        assert!(csp.allows_script("https://cdn.example.com/app.js"));
        assert!(!csp.allows_script("https://evil.com/malware.js"));
    }

    #[test]
    fn test_csp_wildcard() {
        let csp = ContentSecurityPolicy::parse("img-src *", false);
        assert!(csp.allows_image("https://any-image-host.com/photo.png"));
    }

    #[test]
    fn test_csp_fallback_to_default() {
        let csp = ContentSecurityPolicy::parse("default-src https://trusted.com", false);
        // No script-src set, falls back to default-src
        assert!(csp.allows_script("https://trusted.com/app.js"));
        assert!(!csp.allows_script("https://untrusted.com/app.js"));
    }

    #[test]
    fn test_csp_to_header() {
        let csp =
            ContentSecurityPolicy::parse("default-src 'self'; script-src https://cdn.com", false);
        let header = csp.to_header_string();
        assert!(header.contains("default-src"));
        assert!(header.contains("script-src"));
    }

    #[test]
    fn test_check_wasm_allowed() {
        let caps = SandboxCapabilities::strict_isolation(); // allow_wasm_execution = true
        let mut sb = TabSandbox::new("w1", caps);
        assert!(sb.check_wasm_execution("module.wasm").is_ok());
        assert!(sb.is_clean());
    }

    #[test]
    fn test_check_wasm_blocked() {
        let caps = SandboxCapabilities::permissive().without_scripts(); // disables wasm too
        let mut sb = TabSandbox::new("w2", caps);
        assert!(sb.check_wasm_execution("evil.wasm").is_err());
        assert_eq!(sb.violation_count(), 1);
        let wasm_viols = sb.violations_by_category(&ViolationCategory::Wasm);
        assert_eq!(wasm_viols.len(), 1);
        assert!(wasm_viols[0].detail.contains("evil.wasm"));
    }

    #[test]
    fn test_check_storage_allowed() {
        let caps = SandboxCapabilities::permissive();
        let mut sb = TabSandbox::new("s1", caps);
        assert!(sb.check_storage_access("localStorage").is_ok());
    }

    #[test]
    fn test_check_storage_blocked() {
        let mut caps = SandboxCapabilities::permissive();
        caps.allow_storage_access = false;
        let mut sb = TabSandbox::new("s2", caps);
        assert!(sb.check_storage_access("cookie").is_err());
        assert_eq!(
            sb.violations_by_category(&ViolationCategory::Storage).len(),
            1
        );
    }

    #[test]
    fn test_check_popup_blocked() {
        let mut caps = SandboxCapabilities::strict_isolation();
        caps.allow_popups = false;
        let mut sb = TabSandbox::new("p1", caps);
        let err = sb.check_popup("https://ads.example.com").unwrap_err();
        assert!(err.contains("Popup"));
        assert_eq!(
            sb.violations_by_category(&ViolationCategory::Popup).len(),
            1
        );
    }

    #[test]
    fn test_check_top_navigation_blocked() {
        let caps = SandboxCapabilities::strict_isolation();
        let mut sb = TabSandbox::new("nav1", caps);
        assert!(sb.check_top_navigation("https://phishing.com").is_err());
        let nav_viols = sb.violations_by_category(&ViolationCategory::Navigation);
        assert_eq!(nav_viols.len(), 1);
        assert!(nav_viols[0].detail.contains("phishing.com"));
    }

    #[test]
    fn test_reset_violations() {
        let caps = SandboxCapabilities::strict_isolation();
        let mut sb = TabSandbox::new("r1", caps);
        let _ = sb.check_file_access("/a");
        let _ = sb.check_file_access("/b");
        let _ = sb.check_popup("https://x.com");
        assert_eq!(sb.violation_count(), 3);
        assert!(!sb.is_clean());
        sb.reset_violations();
        assert_eq!(sb.violation_count(), 0);
        assert!(sb.is_clean());
        assert!(sb.violations.is_empty());
    }

    #[test]
    fn test_update_capabilities() {
        let caps = SandboxCapabilities::strict_isolation();
        let mut sb = TabSandbox::new("u1", caps);
        assert!(sb.check_file_access("/x").is_err());
        // Now upgrade to permissive
        sb.update_capabilities(SandboxCapabilities::permissive());
        assert!(sb.check_file_access("/x").is_ok());
    }

    #[test]
    fn test_export_sandbox_nda() {
        let caps = SandboxCapabilities::strict_isolation();
        let mut sb = TabSandbox::new("tab_nda", caps);
        let _ = sb.check_file_access("/secret");
        let _ = sb.check_popup("https://popup.com");
        let triples = sb.export_sandbox_nda();
        assert_eq!(triples.len(), 2);
        // All triples use predicate 220
        for t in &triples {
            assert_eq!(t.predicate_id, 220);
        }
        // subject_hash is hash of "tab_nda", not zero
        assert_eq!(triples[0].subject_hash, crate::nda::hash_str("tab_nda"));
    }

    #[test]
    fn test_csp_parse_report_uri() {
        let csp = ContentSecurityPolicy::parse(
            "default-src 'self'; report-uri https://report.example.com/csp",
            false,
        );
        assert_eq!(
            csp.report_uri,
            Some("https://report.example.com/csp".to_string())
        );
    }

    #[test]
    fn test_csp_report_only_flag() {
        let csp = ContentSecurityPolicy::parse("default-src 'self'", true);
        assert!(csp.report_only);
    }

    #[test]
    fn test_csp_allows_style() {
        let csp = ContentSecurityPolicy::parse("style-src https://cdn.styles.com", false);
        assert!(csp.allows_style("https://cdn.styles.com/main.css"));
        assert!(!csp.allows_style("https://evil.com/inject.css"));
    }

    #[test]
    fn test_csp_allows_connect_fallback() {
        let csp = ContentSecurityPolicy::parse("default-src https://api.trusted.com", false);
        // No connect-src set, falls back to default-src
        assert!(csp.allows_connect("https://api.trusted.com/data"));
        assert!(!csp.allows_connect("https://evil.com/steal"));
    }

    #[test]
    fn test_csp_to_header_includes_report_uri() {
        let csp = ContentSecurityPolicy::parse(
            "default-src 'self'; script-src https://cdn.com; report-uri https://r.com/csp",
            false,
        );
        let header = csp.to_header_string();
        assert!(header.contains("report-uri https://r.com/csp"));
        assert!(header.contains("default-src 'self'"));
        assert!(header.contains("script-src https://cdn.com"));
    }

    #[test]
    fn test_with_network_allowlist_builder() {
        let caps = SandboxCapabilities::strict_isolation()
            .with_network_allowlist(vec!["a.com".into(), "b.com".into()]);
        assert_eq!(caps.allow_network_hosts.len(), 2);
        let mut sb = TabSandbox::new("net1", caps);
        assert!(sb.check_network_access("a.com").is_ok());
        assert!(sb.check_network_access("b.com").is_ok());
        assert!(sb.check_network_access("c.com").is_err());
    }

    #[test]
    fn test_without_scripts_disables_wasm() {
        let caps = SandboxCapabilities::permissive().without_scripts();
        assert!(!caps.allow_scripts);
        assert!(!caps.allow_wasm_execution);
    }
}
