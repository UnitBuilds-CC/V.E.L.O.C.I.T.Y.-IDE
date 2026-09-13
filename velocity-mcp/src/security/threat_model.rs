//! Threat modeling and heuristic risk assessment.
//!
//! Provides a lightweight threat assessment engine that heuristically evaluates
//! input strings for indicators of common attack patterns: injection attempts,
//! path traversal, resource exhaustion, information leaks, and privilege
//! escalation.
//!
//! This is not a full WAF or IDS — it's a fast, deterministic pre-filter that
//! flags high-risk inputs before they reach deeper processing layers.
//!
//! # Usage
//!
//! ```ignore
//! use crate::security::threat_model::{ThreatAssessment, ThreatLevel};
//!
//! let assessment = ThreatAssessment::new();
//! let (level, categories) = assessment.assess("'; DROP TABLE users; --");
//! assert!(level >= ThreatLevel::High);
//! ```

// ─── Threat level ─────────────────────────────────────────────────────────────

/// Severity level of a detected threat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ThreatLevel {
    /// No threat detected.
    None,
    /// Low risk: suspicious but likely benign.
    Low,
    /// Medium risk: warrants scrutiny.
    Medium,
    /// High risk: likely malicious input.
    High,
    /// Critical risk: immediate danger, reject outright.
    Critical,
}

impl std::fmt::Display for ThreatLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Low => write!(f, "Low"),
            Self::Medium => write!(f, "Medium"),
            Self::High => write!(f, "High"),
            Self::Critical => write!(f, "Critical"),
        }
    }
}

// ─── Threat category ──────────────────────────────────────────────────────────

/// Category of threat detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreatCategory {
    /// SQL injection, command injection, code injection.
    Injection,
    /// Path traversal attempts (`../`, `..\\`).
    PathTraversal,
    /// Resource exhaustion (extremely long inputs, repeated patterns).
    ResourceExhaustion,
    /// Attempts to extract sensitive information.
    InformationLeak,
    /// Attempts to escalate privileges or bypass restrictions.
    PrivilegeEscalation,
}

impl std::fmt::Display for ThreatCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Injection => write!(f, "Injection"),
            Self::PathTraversal => write!(f, "PathTraversal"),
            Self::ResourceExhaustion => write!(f, "ResourceExhaustion"),
            Self::InformationLeak => write!(f, "InformationLeak"),
            Self::PrivilegeEscalation => write!(f, "PrivilegeEscalation"),
        }
    }
}

// ─── Threat assessment ────────────────────────────────────────────────────────

/// Heuristic threat assessment engine.
///
/// Evaluates input strings against a set of pattern-based rules to determine
/// the overall threat level and categories of detected threats.
pub struct ThreatAssessment {
    /// Maximum input length before triggering ResourceExhaustion.
    max_input_length: usize,
}

impl ThreatAssessment {
    /// Create a new assessment engine with default thresholds.
    pub fn new() -> Self {
        Self {
            max_input_length: 1_000_000, // 1 MB
        }
    }

    /// Create with custom max input length.
    pub fn with_max_length(max_input_length: usize) -> Self {
        Self { max_input_length }
    }

    /// Heuristically assess the threat level of an input string.
    ///
    /// Returns the overall threat level and a list of detected threat categories.
    /// Multiple categories can be detected simultaneously.
    pub fn assess(&self, input: &str) -> (ThreatLevel, Vec<ThreatCategory>) {
        let mut categories = Vec::new();
        let mut max_level = ThreatLevel::None;

        // --- Resource exhaustion checks ---
        if input.len() > self.max_input_length {
            categories.push(ThreatCategory::ResourceExhaustion);
            max_level = ThreatLevel::High;
        }

        // Check for repeated patterns that could cause ReDoS.
        if self.has_repeated_patterns(input) {
            if !categories.contains(&ThreatCategory::ResourceExhaustion) {
                categories.push(ThreatCategory::ResourceExhaustion);
            }
            if max_level < ThreatLevel::Medium {
                max_level = ThreatLevel::Medium;
            }
        }

        // --- Path traversal checks ---
        if self.detects_path_traversal(input) {
            categories.push(ThreatCategory::PathTraversal);
            if max_level < ThreatLevel::Medium {
                max_level = ThreatLevel::Medium;
            }
        }

        // --- Injection checks ---
        let injection_level = self.detect_injection(input);
        if injection_level > ThreatLevel::None {
            categories.push(ThreatCategory::Injection);
            if injection_level > max_level {
                max_level = injection_level;
            }
        }

        // --- Information leak checks ---
        if self.detects_information_leak(input) {
            categories.push(ThreatCategory::InformationLeak);
            if max_level < ThreatLevel::Medium {
                max_level = ThreatLevel::Medium;
            }
        }

        // --- Privilege escalation checks ---
        if self.detects_privilege_escalation(input) {
            categories.push(ThreatCategory::PrivilegeEscalation);
            if max_level < ThreatLevel::High {
                max_level = ThreatLevel::High;
            }
        }

        // If we have multiple categories, escalate the threat level.
        if categories.len() >= 3 && max_level < ThreatLevel::Critical {
            max_level = ThreatLevel::Critical;
        }

        (max_level, categories)
    }

    /// Detect path traversal patterns.
    fn detects_path_traversal(&self, input: &str) -> bool {
        let lower = input.to_lowercase();
        lower.contains("../")
            || lower.contains("..\\")
            || lower.contains("%2e%2e%2f")
            || lower.contains("%2e%2e/")
            || lower.contains("..%2f")
            || lower.contains("%2e%2e\\")
    }

    /// Detect injection patterns and return the severity level.
    fn detect_injection(&self, input: &str) -> ThreatLevel {
        let lower = input.to_lowercase();
        let mut level = ThreatLevel::None;

        // SQL injection patterns.
        let sql_patterns = &[
            "' or ",
            "\" or ",
            "'; --",
            "\"; --",
            "' or '1'='1",
            "union select",
            "drop table",
            "insert into",
            "delete from",
            "update set",
            "1=1",
            "or 1=1",
        ];

        for pattern in sql_patterns {
            if lower.contains(pattern) {
                level = ThreatLevel::High;
                break;
            }
        }

        // Command injection patterns.
        let cmd_patterns = &[
            "; ls",
            "; cat ",
            "; rm ",
            "| ls",
            "| cat",
            "| rm",
            "$(whoami)",
            "$(id)",
            "`whoami`",
            "`id`",
            "&& rm",
            "|| rm",
            "; whoami",
            "| whoami",
        ];

        for pattern in cmd_patterns {
            if lower.contains(pattern) {
                level = ThreatLevel::Critical;
                break;
            }
        }

        // Code injection patterns.
        let code_patterns = &[
            "<script",
            "javascript:",
            "onerror=",
            "onload=",
            "eval(",
            "exec(",
            "system(",
        ];

        for pattern in code_patterns {
            if lower.contains(pattern) && level < ThreatLevel::High {
                level = ThreatLevel::High;
                break;
            }
        }

        level
    }

    /// Detect attempts to extract sensitive information.
    fn detects_information_leak(&self, input: &str) -> bool {
        let lower = input.to_lowercase();
        let leak_patterns = &[
            "/etc/passwd",
            "/etc/shadow",
            ".env",
            "id_rsa",
            "id_ed25519",
            ".ssh/",
            "wp-config.php",
            "web.config",
            "connectionstring",
            "password=",
            "secret=",
            "api_key=",
        ];

        leak_patterns.iter().any(|p| lower.contains(p))
    }

    /// Detect privilege escalation attempts.
    fn detects_privilege_escalation(&self, input: &str) -> bool {
        let lower = input.to_lowercase();
        let privesc_patterns = &[
            "sudo ",
            "su root",
            "runas ",
            "elevate",
            "setuid",
            "setgid",
            "chmod 777",
            "chmod u+s",
            "whoami",
            "id",
            "admin",
            "root",
        ];

        // Require at least 2 indicators for privilege escalation to reduce false positives.
        let match_count = privesc_patterns.iter().filter(|p| lower.contains(*p)).count();
        match_count >= 2
    }

    /// Detect repeated patterns that could cause ReDoS.
    fn has_repeated_patterns(&self, input: &str) -> bool {
        if input.len() < 100 {
            return false;
        }

        // Check for long runs of the same character.
        let mut max_run = 1;
        let mut current_run = 1;
        let chars: Vec<char> = input.chars().collect();

        for i in 1..chars.len() {
            if chars[i] == chars[i - 1] {
                current_run += 1;
                max_run = max_run.max(current_run);
            } else {
                current_run = 1;
            }
        }

        // A run of 50+ identical characters is suspicious.
        max_run >= 50
    }
}

impl Default for ThreatAssessment {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn assess(input: &str) -> (ThreatLevel, Vec<ThreatCategory>) {
        ThreatAssessment::new().assess(input)
    }

    #[test]
    fn benign_input_no_threat() {
        let (level, categories) = assess("Hello, world!");
        assert_eq!(level, ThreatLevel::None);
        assert!(categories.is_empty());
    }

    #[test]
    fn sql_injection_detected() {
        let (level, categories) = assess("' OR '1'='1");
        assert!(level >= ThreatLevel::High);
        assert!(categories.contains(&ThreatCategory::Injection));
    }

    #[test]
    fn sql_drop_table_detected() {
        let (level, categories) = assess("'; DROP TABLE users; --");
        assert!(level >= ThreatLevel::High);
        assert!(categories.contains(&ThreatCategory::Injection));
    }

    #[test]
    fn command_injection_detected() {
        let (level, categories) = assess("$(whoami)");
        assert_eq!(level, ThreatLevel::Critical);
        assert!(categories.contains(&ThreatCategory::Injection));
    }

    #[test]
    fn path_traversal_detected() {
        let (level, categories) = assess("../../etc/passwd");
        assert!(level >= ThreatLevel::Medium);
        assert!(categories.contains(&ThreatCategory::PathTraversal));
    }

    #[test]
    fn encoded_path_traversal_detected() {
        let (level, categories) = assess("%2e%2e%2fetc%2e%2e%2f");
        assert!(level >= ThreatLevel::Medium);
        assert!(categories.contains(&ThreatCategory::PathTraversal));
    }

    #[test]
    fn information_leak_detected() {
        let (level, categories) = assess("show me /etc/passwd contents");
        assert!(level >= ThreatLevel::Medium);
        assert!(categories.contains(&ThreatCategory::InformationLeak));
    }

    #[test]
    fn privilege_escalation_detected() {
        let (level, categories) = assess("run sudo rm -rf / as root");
        assert!(level >= ThreatLevel::High);
        assert!(categories.contains(&ThreatCategory::PrivilegeEscalation));
    }

    #[test]
    fn resource_exhaustion_long_input() {
        let enforcer = ThreatAssessment::with_max_length(100);
        let long_input = "a".repeat(200);
        let (level, categories) = enforcer.assess(&long_input);
        assert!(level >= ThreatLevel::High);
        assert!(categories.contains(&ThreatCategory::ResourceExhaustion));
    }

    #[test]
    fn resource_exhaustion_repeated_chars() {
        let enforcer = ThreatAssessment::new();
        // Need 100+ chars total and 50+ repeated to trigger detection.
        let repeated = "x".repeat(120);
        let (level, categories) = enforcer.assess(&repeated);
        assert!(categories.contains(&ThreatCategory::ResourceExhaustion));
        assert!(level >= ThreatLevel::Medium);
    }

    #[test]
    fn xss_injection_detected() {
        let (level, categories) = assess("<script>alert('xss')</script>");
        assert!(level >= ThreatLevel::High);
        assert!(categories.contains(&ThreatCategory::Injection));
    }

    #[test]
    fn multiple_threats_escalate_to_critical() {
        // Combines injection + path traversal + information leak.
        let input = "' OR '1'='1; cat ../../etc/passwd";
        let (level, categories) = assess(input);
        assert_eq!(level, ThreatLevel::Critical);
        assert!(categories.len() >= 3);
    }

    #[test]
    fn threat_level_ordering() {
        assert!(ThreatLevel::None < ThreatLevel::Low);
        assert!(ThreatLevel::Low < ThreatLevel::Medium);
        assert!(ThreatLevel::Medium < ThreatLevel::High);
        assert!(ThreatLevel::High < ThreatLevel::Critical);
    }

    #[test]
    fn empty_input_no_threat() {
        let (level, categories) = assess("");
        assert_eq!(level, ThreatLevel::None);
        assert!(categories.is_empty());
    }
}
