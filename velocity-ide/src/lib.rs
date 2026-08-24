// V.E.L.O.C.I.T.Y.-IDE — library facade
// Re-exports modules so that binaries in src/bin/ can use `velocity_ide::*`.

pub mod compiler;
pub mod errors;
pub mod model;
pub mod nda;
pub mod nda_int;
pub mod pipeline_bridge;
pub mod pipeline_nda;
pub mod safety;
pub mod sandbox;
pub mod site_map;
pub mod tokenizer;
pub mod velocity_client;
pub mod provider_usage;
pub mod credential_guard;
pub mod wiki;

use serde::Serialize;

// ─── Library Metadata ─────────────────────────────────────────────────────────

/// Library version from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Library name.
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Build target architecture.
pub const TARGET_ARCH: &str = std::env::consts::ARCH;

/// Build target OS.
pub const TARGET_OS: &str = std::env::consts::OS;

/// Return the library version string.
pub fn version() -> &'static str {
    VERSION
}

/// Return a human-readable build banner.
pub fn banner() -> String {
    format!(
        "V.E.L.O.C.I.T.Y.-IDE v{} ({}-{})",
        VERSION, TARGET_OS, TARGET_ARCH
    )
}

// ─── Module Inventory ─────────────────────────────────────────────────────────

/// Description of a library module.
#[derive(Debug, Clone, Serialize)]
pub struct ModuleInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub is_public: bool,
}

/// Return the full inventory of library modules.
pub fn module_inventory() -> Vec<ModuleInfo> {
    vec![
        ModuleInfo { name: "compiler", description: "Rust-to-NDA compiler, JIT, shaders, Vulkan driver", is_public: true },
        ModuleInfo { name: "errors", description: "Unified error types for all modules", is_public: true },
        ModuleInfo { name: "model", description: "Transformer model configs, weights, FP32/Zero inference", is_public: true },
        ModuleInfo { name: "nda", description: "NDA-GEMV benchmark and core NDA operations", is_public: true },
        ModuleInfo { name: "nda_int", description: "Integer NDA arithmetic, quantized GEMV", is_public: true },
        ModuleInfo { name: "pipeline_bridge", description: "Dual-path NDA pipeline bridge", is_public: true },
        ModuleInfo { name: "pipeline_nda", description: "Pure NDA-native pipeline execution", is_public: true },
        ModuleInfo { name: "safety", description: "Deadlock detection, poisoning recovery, scope validation", is_public: true },
        ModuleInfo { name: "sandbox", description: "Sandboxed NDA tree execution engine", is_public: true },
        ModuleInfo { name: "site_map", description: "Triple store, Merkle verifier, serialization", is_public: true },
        ModuleInfo { name: "tokenizer", description: "BPE tokenizer with batch encoding", is_public: true },
        ModuleInfo { name: "velocity_client", description: "Velocity Router HTTP client and diagnostics", is_public: true },
        ModuleInfo { name: "provider_usage", description: "Multi-provider API key management and usage queries", is_public: true },
        ModuleInfo { name: "credential_guard", description: "Credential boundary: env var scrubbing, audit logging", is_public: true },
        ModuleInfo { name: "wiki", description: "Wiki generation, search, markdown rendering", is_public: true },
    ]
}

// ─── Library Diagnostics ──────────────────────────────────────────────────────

/// Diagnostic snapshot of the library state.
#[derive(Debug, Clone, Serialize)]
pub struct LibraryInfo {
    pub name: String,
    pub version: String,
    pub target_os: String,
    pub target_arch: String,
    pub module_count: usize,
    pub modules: Vec<ModuleInfo>,
    pub features: Vec<String>,
}

/// Return a diagnostic snapshot of the library.
pub fn library_info() -> LibraryInfo {
    let modules = module_inventory();
    let mut features = Vec::new();
    features.push("serde".into());
    features.push("clap".into());
    features.push("ureq".into());
    features.push("clap_complete".into());
    features.push("env_logger".into());
    features.push("dotenvy".into());
    features.push("credential_boundary".into());
    features.push("json_output".into());
    features.push("shell_completions".into());
    features.push("dual_path_pipeline".into());
    features.push("sandbox_execution".into());
    features.push("merkle_verification".into());
    features.push("jit_compilation".into());
    features.push("vulkan_compute".into());
    LibraryInfo {
        name: NAME.into(),
        version: VERSION.into(),
        target_os: TARGET_OS.into(),
        target_arch: TARGET_ARCH.into(),
        module_count: modules.len(),
        modules,
        features,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_nonempty() {
        assert!(!VERSION.is_empty());
        assert!(!version().is_empty());
    }

    #[test]
    fn banner_contains_version() {
        let b = banner();
        assert!(b.contains(VERSION));
        assert!(b.contains("V.E.L.O.C.I.T.Y.-IDE"));
    }

    #[test]
    fn module_inventory_has_all_modules() {
        let inv = module_inventory();
        assert!(inv.len() >= 15);
        let names: Vec<&str> = inv.iter().map(|m| m.name).collect();
        assert!(names.contains(&"compiler"));
        assert!(names.contains(&"sandbox"));
        assert!(names.contains(&"velocity_client"));
        assert!(names.contains(&"credential_guard"));
        assert!(names.contains(&"wiki"));
    }

    #[test]
    fn all_modules_are_public() {
        for m in module_inventory() {
            assert!(m.is_public, "module {} should be public", m.name);
        }
    }

    #[test]
    fn library_info_serializes() {
        let info = library_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("velocity-ide") || json.contains("velocity_ide"));
        assert!(json.contains("modules"));
        assert!(json.contains("features"));
        assert!(info.module_count >= 15);
        assert!(!info.features.is_empty());
    }

    #[test]
    fn target_os_and_arch_are_set() {
        assert!(!TARGET_OS.is_empty());
        assert!(!TARGET_ARCH.is_empty());
        let info = library_info();
        assert!(!info.target_os.is_empty());
        assert!(!info.target_arch.is_empty());
    }

    #[test]
    fn features_include_credential_boundary() {
        let info = library_info();
        assert!(info.features.contains(&"credential_boundary".to_string()));
    }

    // ── ModuleInfo tests ─────────────────────────────────────────────────────

    #[test]
    fn module_info_json_key_count() {
        let m = ModuleInfo { name: "test", description: "desc", is_public: true };
        let json = serde_json::to_string(&m).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 3);
    }

    #[test]
    fn module_info_clone_independence() {
        let m = ModuleInfo { name: "original", description: "desc", is_public: true };
        let cloned = m.clone();
        assert_eq!(cloned.name, "original");
        assert_eq!(cloned.description, "desc");
        assert!(cloned.is_public);
    }

    #[test]
    fn module_info_debug_format() {
        let m = ModuleInfo { name: "test_mod", description: "A test module", is_public: true };
        let dbg = format!("{:?}", m);
        assert!(dbg.contains("ModuleInfo"));
        assert!(dbg.contains("test_mod"));
    }

    #[test]
    fn module_info_serialization_roundtrip() {
        let m = ModuleInfo { name: "compiler", description: "JIT compiler", is_public: true };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"name\":\"compiler\""));
        assert!(json.contains("\"is_public\":true"));
    }

    // ── LibraryInfo tests ────────────────────────────────────────────────────

    #[test]
    fn library_info_json_key_count() {
        let info = library_info();
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        // 7 fields: name, version, target_os, target_arch, module_count, modules, features
        assert_eq!(v.as_object().unwrap().len(), 7);
    }

    #[test]
    fn library_info_clone_independence() {
        let info = library_info();
        let mut cloned = info.clone();
        cloned.module_count = 9999;
        cloned.features.push("injected".into());
        assert_eq!(info.module_count, 15);
        assert!(!info.features.contains(&"injected".to_string()));
    }

    #[test]
    fn library_info_debug_format() {
        let info = library_info();
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("LibraryInfo"));
        assert!(dbg.contains("module_count"));
    }

    #[test]
    fn library_info_pretty_json() {
        let info = library_info();
        let pretty = serde_json::to_string_pretty(&info).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));
        let v: serde_json::Value = serde_json::from_str(&pretty).unwrap();
        assert_eq!(v["module_count"], 15);
    }

    #[test]
    fn library_info_name_matches_constant() {
        let info = library_info();
        assert_eq!(info.name, NAME);
        assert_eq!(info.version, VERSION);
    }

    #[test]
    fn library_info_module_count_matches_inventory() {
        let info = library_info();
        let inv = module_inventory();
        assert_eq!(info.module_count, inv.len());
        assert_eq!(info.modules.len(), inv.len());
    }

    #[test]
    fn library_info_features_exact_count() {
        let info = library_info();
        // 14 features defined in library_info()
        assert_eq!(info.features.len(), 14);
    }

    #[test]
    fn library_info_features_no_duplicates() {
        let info = library_info();
        let mut unique = info.features.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), info.features.len(), "features should have no duplicates");
    }

    #[test]
    fn library_info_features_all_nonempty() {
        let info = library_info();
        for f in &info.features {
            assert!(!f.is_empty(), "feature should not be empty");
        }
    }

    // ── Module inventory detailed tests ──────────────────────────────────────

    #[test]
    fn module_inventory_all_descriptions_nonempty() {
        for m in module_inventory() {
            assert!(!m.description.is_empty(), "module {} has empty description", m.name);
        }
    }

    #[test]
    fn module_inventory_all_names_nonempty() {
        for m in module_inventory() {
            assert!(!m.name.is_empty(), "module has empty name");
        }
    }

    #[test]
    fn module_inventory_contains_expected_modules() {
        let inv = module_inventory();
        let names: Vec<&str> = inv.iter().map(|m| m.name).collect();
        let expected = [
            "compiler", "errors", "model", "nda", "nda_int",
            "pipeline_bridge", "pipeline_nda", "safety", "sandbox",
            "site_map", "tokenizer", "velocity_client", "provider_usage",
            "credential_guard", "wiki",
        ];
        for e in &expected {
            assert!(names.contains(e), "missing module: {}", e);
        }
    }

    #[test]
    fn module_inventory_no_duplicate_names() {
        let inv = module_inventory();
        let mut names: Vec<&str> = inv.iter().map(|m| m.name).collect();
        let original_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), original_len, "duplicate module names found");
    }

    #[test]
    fn module_inventory_serializes() {
        let inv = module_inventory();
        let json = serde_json::to_string(&inv).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 15);
    }

    // ── Banner tests ─────────────────────────────────────────────────────────

    #[test]
    fn banner_contains_os_and_arch() {
        let b = banner();
        assert!(b.contains(TARGET_OS));
        assert!(b.contains(TARGET_ARCH));
    }

    #[test]
    fn banner_format() {
        let b = banner();
        assert!(b.starts_with("V.E.L.O.C.I.T.Y.-IDE v"));
        assert!(b.contains(&format!("({}-)", TARGET_OS)) || b.contains(&format!("{}-{}", TARGET_OS, TARGET_ARCH)));
    }

    #[test]
    fn version_matches_cargo_pkg() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn name_matches_cargo_pkg() {
        assert_eq!(NAME, env!("CARGO_PKG_NAME"));
    }

    #[test]
    fn library_info_json_all_field_values() {
        let info = library_info();
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["name"].as_str().unwrap().contains("velocity"));
        assert!(!v["version"].as_str().unwrap().is_empty());
        assert!(!v["target_os"].as_str().unwrap().is_empty());
        assert!(!v["target_arch"].as_str().unwrap().is_empty());
        assert!(v["module_count"].as_u64().unwrap() >= 15);
        assert!(v["modules"].is_array());
        assert!(v["features"].is_array());
    }
}
