// Generated submodules for shaders

pub mod act_bitnet;
pub mod act_nda;
pub mod act_qwen;
pub mod attn_contig;
pub mod attn_ndakv;
pub mod attn_softmax;
pub mod bias_add;
pub mod fp2;
pub mod fp4;
pub mod int4;
pub mod kv_write;
pub mod nda;
pub mod residual_add;
pub mod rms_norm;
pub mod rope;
pub mod swiglu;
pub mod ternary;

pub use act_bitnet::ACT_BITNET_SPV;
pub use act_nda::ACT_NDA_SPV;
pub use act_qwen::ACT_QWEN_SPV;
pub use attn_contig::ATTN_CONTIG_SPV;
pub use attn_ndakv::ATTN_NDAKV_SPV;
pub use attn_softmax::ATTN_SOFTMAX_SPV;
pub use bias_add::BIAS_ADD_SPV;
pub use fp2::FP2_SPV;
pub use fp4::FP4_SPV;
pub use int4::INT4_SPV;
pub use kv_write::KV_WRITE_SPV;
pub use nda::NDA_SPV;
pub use residual_add::RESIDUAL_ADD_SPV;
pub use rms_norm::RMS_NORM_SPV;
pub use rope::ROPE_SPV;
pub use swiglu::SWIGLU_SPV;
pub use ternary::TERNARY_SPV;

use serde::Serialize;

/// SPIR-V magic number (first 4 bytes of every valid SPIR-V module).
const SPIRV_MAGIC: u32 = 0x07230203;

/// Describes a single compiled shader.
#[derive(Debug, Clone, Serialize)]
pub struct ShaderEntry {
    pub name: &'static str,
    pub spv_words: usize,
    pub spv_bytes: usize,
    pub valid_header: bool,
}

/// Registry of all compiled shaders with diagnostic info.
#[derive(Debug, Clone, Serialize)]
pub struct ShaderRegistry {
    pub shader_count: usize,
    pub total_spv_bytes: usize,
    pub shaders: Vec<ShaderEntry>,
    pub validation_issues: Vec<String>,
}

/// Validate a SPIR-V bytecode slice.
fn validate_spirv(spv: &[u32], name: &str) -> Vec<String> {
    let mut issues = Vec::new();
    if spv.is_empty() {
        issues.push(format!("{name}: SPIR-V bytecode is empty"));
    } else if spv[0] != SPIRV_MAGIC {
        issues.push(format!(
            "{name}: invalid SPIR-V magic number (got 0x{:08X}, expected 0x{:08X})",
            spv[0], SPIRV_MAGIC
        ));
    }
    if spv.len() < 5 {
        issues.push(format!("{name}: SPIR-V too short ({} words)", spv.len()));
    }
    issues
}

/// Build a single shader entry.
fn shader_entry(name: &'static str, spv: &[u32]) -> ShaderEntry {
    ShaderEntry {
        name,
        spv_words: spv.len(),
        spv_bytes: spv.len() * 4,
        valid_header: !spv.is_empty() && spv[0] == SPIRV_MAGIC,
    }
}

/// Build the full shader registry with validation.
pub fn shader_registry() -> ShaderRegistry {
    let shaders = vec![
        shader_entry("act_bitnet", ACT_BITNET_SPV),
        shader_entry("act_nda", ACT_NDA_SPV),
        shader_entry("act_qwen", ACT_QWEN_SPV),
        shader_entry("attn_contig", ATTN_CONTIG_SPV),
        shader_entry("attn_ndakv", ATTN_NDAKV_SPV),
        shader_entry("attn_softmax", ATTN_SOFTMAX_SPV),
        shader_entry("bias_add", BIAS_ADD_SPV),
        shader_entry("fp2", FP2_SPV),
        shader_entry("fp4", FP4_SPV),
        shader_entry("int4", INT4_SPV),
        shader_entry("kv_write", KV_WRITE_SPV),
        shader_entry("nda", NDA_SPV),
        shader_entry("residual_add", RESIDUAL_ADD_SPV),
        shader_entry("rms_norm", RMS_NORM_SPV),
        shader_entry("rope", ROPE_SPV),
        shader_entry("swiglu", SWIGLU_SPV),
        shader_entry("ternary", TERNARY_SPV),
    ];
    let total_bytes: usize = shaders.iter().map(|s| s.spv_bytes).sum();
    let mut issues = Vec::new();
    for s in &shaders {
        issues.extend(validate_spirv(
            match s.name {
                "act_bitnet" => ACT_BITNET_SPV,
                "act_nda" => ACT_NDA_SPV,
                "act_qwen" => ACT_QWEN_SPV,
                "attn_contig" => ATTN_CONTIG_SPV,
                "attn_ndakv" => ATTN_NDAKV_SPV,
                "attn_softmax" => ATTN_SOFTMAX_SPV,
                "bias_add" => BIAS_ADD_SPV,
                "fp2" => FP2_SPV,
                "fp4" => FP4_SPV,
                "int4" => INT4_SPV,
                "kv_write" => KV_WRITE_SPV,
                "nda" => NDA_SPV,
                "residual_add" => RESIDUAL_ADD_SPV,
                "rms_norm" => RMS_NORM_SPV,
                "rope" => ROPE_SPV,
                "swiglu" => SWIGLU_SPV,
                "ternary" => TERNARY_SPV,
                _ => &[],
            },
            s.name,
        ));
    }
    ShaderRegistry {
        shader_count: shaders.len(),
        total_spv_bytes: total_bytes,
        shaders,
        validation_issues: issues,
    }
}

// ─── Additional diagnostics ───────────────────────────────────────────────────

/// Categorize a shader by its name.
pub fn shader_category(name: &str) -> &'static str {
    match name {
        "act_bitnet" | "act_nda" | "act_qwen" | "swiglu" => "activation",
        "attn_contig" | "attn_ndakv" | "attn_softmax" => "attention",
        "bias_add" | "residual_add" => "arithmetic",
        "fp2" | "fp4" | "int4" | "ternary" => "quantization",
        "kv_write" => "kv_cache",
        "nda" => "core",
        "rms_norm" | "rope" => "normalization",
        _ => "other",
    }
}

/// Return the SPIR-V bytecode for a named shader, or None if not found.
pub fn shader_bytecode(name: &str) -> Option<&'static [u32]> {
    match name {
        "act_bitnet" => Some(ACT_BITNET_SPV),
        "act_nda" => Some(ACT_NDA_SPV),
        "act_qwen" => Some(ACT_QWEN_SPV),
        "attn_contig" => Some(ATTN_CONTIG_SPV),
        "attn_ndakv" => Some(ATTN_NDAKV_SPV),
        "attn_softmax" => Some(ATTN_SOFTMAX_SPV),
        "bias_add" => Some(BIAS_ADD_SPV),
        "fp2" => Some(FP2_SPV),
        "fp4" => Some(FP4_SPV),
        "int4" => Some(INT4_SPV),
        "kv_write" => Some(KV_WRITE_SPV),
        "nda" => Some(NDA_SPV),
        "residual_add" => Some(RESIDUAL_ADD_SPV),
        "rms_norm" => Some(RMS_NORM_SPV),
        "rope" => Some(ROPE_SPV),
        "swiglu" => Some(SWIGLU_SPV),
        "ternary" => Some(TERNARY_SPV),
        _ => None,
    }
}

/// Category distribution across the shader registry.
#[derive(Debug, Clone, Serialize)]
pub struct ShaderCategoryDistribution {
    pub activation_count: usize,
    pub attention_count: usize,
    pub arithmetic_count: usize,
    pub quantization_count: usize,
    pub kv_cache_count: usize,
    pub core_count: usize,
    pub normalization_count: usize,
    pub other_count: usize,
    pub categories: Vec<(String, usize)>,
}

/// Compute the category distribution of shaders in the registry.
pub fn shader_category_distribution(reg: &ShaderRegistry) -> ShaderCategoryDistribution {
    let mut activation = 0usize;
    let mut attention = 0usize;
    let mut arithmetic = 0usize;
    let mut quantization = 0usize;
    let mut kv_cache = 0usize;
    let mut core = 0usize;
    let mut normalization = 0usize;
    let mut other = 0usize;

    for s in &reg.shaders {
        match shader_category(s.name) {
            "activation" => activation += 1,
            "attention" => attention += 1,
            "arithmetic" => arithmetic += 1,
            "quantization" => quantization += 1,
            "kv_cache" => kv_cache += 1,
            "core" => core += 1,
            "normalization" => normalization += 1,
            _ => other += 1,
        }
    }

    let mut categories = vec![
        ("activation".to_string(), activation),
        ("attention".to_string(), attention),
        ("arithmetic".to_string(), arithmetic),
        ("quantization".to_string(), quantization),
        ("kv_cache".to_string(), kv_cache),
        ("core".to_string(), core),
        ("normalization".to_string(), normalization),
    ];
    if other > 0 {
        categories.push(("other".to_string(), other));
    }
    categories.sort_by(|a, b| b.1.cmp(&a.1));

    ShaderCategoryDistribution {
        activation_count: activation,
        attention_count: attention,
        arithmetic_count: arithmetic,
        quantization_count: quantization,
        kv_cache_count: kv_cache,
        core_count: core,
        normalization_count: normalization,
        other_count: other,
        categories,
    }
}

/// Size statistics for shaders in the registry.
#[derive(Debug, Clone, Serialize)]
pub struct ShaderSizeStats {
    pub min_bytes: usize,
    pub max_bytes: usize,
    pub avg_bytes: f64,
    pub total_bytes: usize,
    pub min_shader: String,
    pub max_shader: String,
}

/// Compute size statistics across all shaders in the registry.
pub fn shader_size_stats(reg: &ShaderRegistry) -> Option<ShaderSizeStats> {
    if reg.shaders.is_empty() {
        return None;
    }
    let mut min_bytes = usize::MAX;
    let mut max_bytes = 0usize;
    let mut min_shader = String::new();
    let mut max_shader = String::new();

    for s in &reg.shaders {
        if s.spv_bytes < min_bytes {
            min_bytes = s.spv_bytes;
            min_shader = s.name.to_string();
        }
        if s.spv_bytes > max_bytes {
            max_bytes = s.spv_bytes;
            max_shader = s.name.to_string();
        }
    }

    let avg = reg.total_spv_bytes as f64 / reg.shaders.len() as f64;

    Some(ShaderSizeStats {
        min_bytes,
        max_bytes,
        avg_bytes: avg,
        total_bytes: reg.total_spv_bytes,
        min_shader,
        max_shader,
    })
}

impl ShaderRegistry {
    /// Look up a shader by name.
    pub fn find_shader(&self, name: &str) -> Option<&ShaderEntry> {
        self.shaders.iter().find(|s| s.name == name)
    }

    /// Return the largest shader by SPIR-V bytecode size.
    pub fn largest_shader(&self) -> Option<&ShaderEntry> {
        self.shaders.iter().max_by_key(|s| s.spv_bytes)
    }

    /// Return the smallest shader by SPIR-V bytecode size.
    pub fn smallest_shader(&self) -> Option<&ShaderEntry> {
        self.shaders.iter().min_by_key(|s| s.spv_bytes)
    }

    /// Validate the registry for consistency.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = self.validation_issues.clone();
        if self.shader_count != self.shaders.len() {
            issues.push(format!(
                "shader_count ({}) != shaders vec len ({})",
                self.shader_count,
                self.shaders.len()
            ));
        }
        let computed_total: usize = self.shaders.iter().map(|s| s.spv_bytes).sum();
        if computed_total != self.total_spv_bytes {
            issues.push(format!(
                "total_spv_bytes mismatch: header={} computed={}",
                self.total_spv_bytes, computed_total
            ));
        }
        // Check for duplicate shader names.
        let mut names: Vec<&str> = self.shaders.iter().map(|s| s.name).collect();
        let original_len = names.len();
        names.sort();
        names.dedup();
        if names.len() != original_len {
            issues.push("duplicate shader names detected".to_string());
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_all_shaders() {
        let reg = shader_registry();
        assert_eq!(reg.shader_count, 17);
    }

    #[test]
    fn registry_all_valid() {
        let reg = shader_registry();
        assert!(reg.validation_issues.is_empty(), "issues: {:?}", reg.validation_issues);
    }

    #[test]
    fn registry_all_have_valid_headers() {
        let reg = shader_registry();
        for s in &reg.shaders {
            assert!(s.valid_header, "shader {} has invalid header", s.name);
        }
    }

    #[test]
    fn registry_total_bytes_positive() {
        let reg = shader_registry();
        assert!(reg.total_spv_bytes > 0);
    }

    #[test]
    fn validate_spirv_empty() {
        let issues = validate_spirv(&[], "test");
        assert!(!issues.is_empty());
        assert!(issues[0].contains("empty"));
    }

    #[test]
    fn validate_spirv_bad_magic() {
        let bad = vec![0xDEADBEEF, 0, 0, 0, 0];
        let issues = validate_spirv(&bad, "test");
        assert!(issues.iter().any(|i| i.contains("magic")));
    }

    #[test]
    fn validate_spirv_too_short() {
        let short = vec![SPIRV_MAGIC, 0, 0, 0];
        let issues = validate_spirv(&short, "test");
        assert!(issues.iter().any(|i| i.contains("too short")));
    }

    #[test]
    fn shader_entry_sizes_correct() {
        let reg = shader_registry();
        for s in &reg.shaders {
            assert_eq!(s.spv_bytes, s.spv_words * 4);
        }
    }

    #[test]
    fn registry_serializes() {
        let reg = shader_registry();
        let json = serde_json::to_string(&reg).unwrap();
        assert!(json.contains("shader_count"));
        assert!(json.contains("total_spv_bytes"));
        assert!(json.contains("rms_norm"));
    }

    #[test]
    fn shader_entry_serializes() {
        let entry = shader_entry("test", &[SPIRV_MAGIC, 0, 0, 0, 0]);
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"name\":\"test\""));
        assert!(json.contains("valid_header"));
    }

    // ─── New diagnostic tests ──────────────────────────────────────────────────

    #[test]
    fn shader_category_nonempty() {
        let reg = shader_registry();
        for s in &reg.shaders {
            let cat = shader_category(s.name);
            assert!(!cat.is_empty(), "shader {} has empty category", s.name);
        }
    }

    #[test]
    fn shader_category_known_values() {
        assert_eq!(shader_category("act_bitnet"), "activation");
        assert_eq!(shader_category("attn_contig"), "attention");
        assert_eq!(shader_category("bias_add"), "arithmetic");
        assert_eq!(shader_category("fp2"), "quantization");
        assert_eq!(shader_category("kv_write"), "kv_cache");
        assert_eq!(shader_category("nda"), "core");
        assert_eq!(shader_category("rms_norm"), "normalization");
        assert_eq!(shader_category("unknown_shader"), "other");
    }

    #[test]
    fn shader_bytecode_found() {
        assert!(shader_bytecode("nda").is_some());
        assert!(shader_bytecode("rms_norm").is_some());
        assert!(shader_bytecode("nonexistent").is_none());
    }

    #[test]
    fn shader_bytecode_has_valid_magic() {
        let reg = shader_registry();
        for s in &reg.shaders {
            if let Some(spv) = shader_bytecode(s.name) {
                assert!(!spv.is_empty());
                assert_eq!(spv[0], SPIRV_MAGIC);
            }
        }
    }

    #[test]
    fn category_distribution_sums_to_total() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        let sum = dist.activation_count
            + dist.attention_count
            + dist.arithmetic_count
            + dist.quantization_count
            + dist.kv_cache_count
            + dist.core_count
            + dist.normalization_count
            + dist.other_count;
        assert_eq!(sum, reg.shader_count);
    }

    #[test]
    fn category_distribution_serializes() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        let json = serde_json::to_string(&dist).unwrap();
        assert!(json.contains("activation_count"));
    }

    #[test]
    fn shader_size_stats_valid() {
        let reg = shader_registry();
        let stats = shader_size_stats(&reg).unwrap();
        assert!(stats.min_bytes > 0);
        assert!(stats.max_bytes >= stats.min_bytes);
        assert!(stats.avg_bytes > 0.0);
        assert_eq!(stats.total_bytes, reg.total_spv_bytes);
        assert!(!stats.min_shader.is_empty());
        assert!(!stats.max_shader.is_empty());
    }

    #[test]
    fn shader_size_stats_empty_registry() {
        let empty = ShaderRegistry {
            shader_count: 0,
            total_spv_bytes: 0,
            shaders: vec![],
            validation_issues: vec![],
        };
        assert!(shader_size_stats(&empty).is_none());
    }

    #[test]
    fn shader_size_stats_serializes() {
        let reg = shader_registry();
        let stats = shader_size_stats(&reg).unwrap();
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("min_bytes"));
        assert!(json.contains("max_bytes"));
    }

    #[test]
    fn registry_find_shader() {
        let reg = shader_registry();
        assert!(reg.find_shader("nda").is_some());
        assert!(reg.find_shader("nonexistent").is_none());
        let entry = reg.find_shader("rms_norm").unwrap();
        assert!(entry.valid_header);
    }

    #[test]
    fn registry_largest_and_smallest() {
        let reg = shader_registry();
        let largest = reg.largest_shader().unwrap();
        let smallest = reg.smallest_shader().unwrap();
        assert!(largest.spv_bytes >= smallest.spv_bytes);
        assert!(!largest.name.is_empty());
        assert!(!smallest.name.is_empty());
    }

    #[test]
    fn registry_validate_clean() {
        let reg = shader_registry();
        let issues = reg.validate();
        assert!(issues.is_empty(), "issues: {:?}", issues);
    }

    #[test]
    fn registry_validate_detects_count_mismatch() {
        let reg = ShaderRegistry {
            shader_count: 999, // wrong
            total_spv_bytes: 0,
            shaders: vec![shader_entry("test", &[SPIRV_MAGIC, 0, 0, 0, 0])],
            validation_issues: vec![],
        };
        let issues = reg.validate();
        assert!(issues.iter().any(|i| i.contains("shader_count")));
    }

    #[test]
    fn registry_validate_detects_total_mismatch() {
        let reg = ShaderRegistry {
            shader_count: 1,
            total_spv_bytes: 9999, // wrong
            shaders: vec![shader_entry("test", &[SPIRV_MAGIC, 0, 0, 0, 0])],
            validation_issues: vec![],
        };
        let issues = reg.validate();
        assert!(issues.iter().any(|i| i.contains("total_spv_bytes")));
    }
}
