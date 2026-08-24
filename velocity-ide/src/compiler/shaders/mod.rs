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

    // ── Block 101: Shader registry extended tests ────────────────────────────

    #[test]
    fn shader_entry_clone() {
        let entry = shader_entry("test", &[SPIRV_MAGIC, 0, 0, 0, 0]);
        let cloned = entry.clone();
        assert_eq!(cloned.name, entry.name);
        assert_eq!(cloned.spv_words, entry.spv_words);
        assert_eq!(cloned.spv_bytes, entry.spv_bytes);
        assert_eq!(cloned.valid_header, entry.valid_header);
    }

    #[test]
    fn registry_validate_detects_duplicate_names() {
        let e = shader_entry("dup", &[SPIRV_MAGIC, 0, 0, 0, 0]);
        let reg = ShaderRegistry {
            shader_count: 2,
            total_spv_bytes: e.spv_bytes * 2,
            shaders: vec![e.clone(), e],
            validation_issues: vec![],
        };
        let issues = reg.validate();
        assert!(issues.iter().any(|i| i.contains("duplicate")));
    }

    #[test]
    fn shader_category_all_known_shaders() {
        let known = vec![
            ("act_bitnet", "activation"),
            ("act_nda", "activation"),
            ("act_qwen", "activation"),
            ("swiglu", "activation"),
            ("attn_contig", "attention"),
            ("attn_ndakv", "attention"),
            ("attn_softmax", "attention"),
            ("bias_add", "arithmetic"),
            ("residual_add", "arithmetic"),
            ("fp2", "quantization"),
            ("fp4", "quantization"),
            ("int4", "quantization"),
            ("ternary", "quantization"),
            ("kv_write", "kv_cache"),
            ("nda", "core"),
            ("rms_norm", "normalization"),
            ("rope", "normalization"),
        ];
        for (name, expected_cat) in &known {
            assert_eq!(
                shader_category(name),
                *expected_cat,
                "shader {} expected category {}, got {}",
                name,
                expected_cat,
                shader_category(name)
            );
        }
    }

    #[test]
    fn shader_bytecode_all_known_shaders() {
        let reg = shader_registry();
        for s in &reg.shaders {
            let bc = shader_bytecode(s.name);
            assert!(bc.is_some(), "shader_bytecode returned None for {}", s.name);
            let spv = bc.unwrap();
            assert!(!spv.is_empty(), "shader {} has empty bytecode", s.name);
            assert_eq!(spv[0], SPIRV_MAGIC, "shader {} has bad magic", s.name);
        }
    }

    #[test]
    fn shader_bytecode_unknown_returns_none() {
        assert!(shader_bytecode("nonexistent_shader").is_none());
    }

    #[test]
    fn category_distribution_sorted_descending() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        // Categories should be sorted by count descending
        for window in dist.categories.windows(2) {
            assert!(
                window[0].1 >= window[1].1,
                "categories not sorted: {:?} before {:?}",
                window[0],
                window[1]
            );
        }
    }

    #[test]
    fn category_distribution_known_counts() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        assert_eq!(dist.activation_count, 4); // act_bitnet, act_nda, act_qwen, swiglu
        assert_eq!(dist.attention_count, 3); // attn_contig, attn_ndakv, attn_softmax
        assert_eq!(dist.arithmetic_count, 2); // bias_add, residual_add
        assert_eq!(dist.quantization_count, 4); // fp2, fp4, int4, ternary
        assert_eq!(dist.kv_cache_count, 1); // kv_write
        assert_eq!(dist.core_count, 1); // nda
        assert_eq!(dist.normalization_count, 2); // rms_norm, rope
    }

    #[test]
    fn shader_size_stats_single_shader() {
        let reg = ShaderRegistry {
            shader_count: 1,
            total_spv_bytes: 20,
            shaders: vec![shader_entry("only", &[SPIRV_MAGIC, 0, 0, 0, 0])],
            validation_issues: vec![],
        };
        let stats = shader_size_stats(&reg).unwrap();
        assert_eq!(stats.min_bytes, stats.max_bytes);
        assert_eq!(stats.min_shader, "only");
        assert_eq!(stats.max_shader, "only");
        assert!((stats.avg_bytes - 20.0).abs() < 0.01);
    }

    #[test]
    fn shader_entry_invalid_header() {
        let entry = shader_entry("bad", &[]);
        assert!(!entry.valid_header);
        assert_eq!(entry.spv_words, 0);
        assert_eq!(entry.spv_bytes, 0);
    }

    #[test]
    fn shader_entry_bad_magic() {
        let entry = shader_entry("bad_magic", &[0xDEADBEEF, 0, 0, 0, 0]);
        assert!(!entry.valid_header);
    }

    #[test]
    fn registry_find_shader_returns_correct_entry() {
        let reg = shader_registry();
        let entry = reg.find_shader("fp4").unwrap();
        assert_eq!(entry.name, "fp4");
        assert!(entry.valid_header);
        assert!(entry.spv_bytes > 0);
    }

    #[test]
    fn registry_shader_count_matches_shaders_len() {
        let reg = shader_registry();
        assert_eq!(reg.shader_count, reg.shaders.len());
    }

    #[test]
    fn registry_total_bytes_matches_sum() {
        let reg = shader_registry();
        let computed: usize = reg.shaders.iter().map(|s| s.spv_bytes).sum();
        assert_eq!(computed, reg.total_spv_bytes);
    }

    // ── Block 135: Shader registry comprehensive tests ───────────────────────

    #[test]
    fn validate_spirv_valid_bytecode() {
        let valid = vec![SPIRV_MAGIC, 0x00010300, 0, 0, 0]; // magic + version + 3 more
        let issues = validate_spirv(&valid, "good_shader");
        assert!(issues.is_empty(), "expected no issues, got: {:?}", issues);
    }

    #[test]
    fn validate_spirv_valid_large_bytecode() {
        let mut spv = vec![SPIRV_MAGIC, 0x00010300, 0, 0, 0];
        spv.extend_from_slice(&[0; 100]); // pad to 105 words
        let issues = validate_spirv(&spv, "large_shader");
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_spirv_empty_includes_name() {
        let issues = validate_spirv(&[], "my_shader");
        // Empty triggers both "empty" and "too short" (len < 5)
        assert!(issues.len() >= 2);
        assert!(issues.iter().any(|i| i.contains("my_shader") && i.contains("empty")));
        assert!(issues.iter().any(|i| i.contains("too short")));
    }

    #[test]
    fn validate_spirv_bad_magic_reports_expected_and_got() {
        let bad = vec![0xBADBAD00, 0, 0, 0, 0];
        let issues = validate_spirv(&bad, "broken");
        assert!(issues.iter().any(|i| i.contains("0xBADBAD00")));
        assert!(issues.iter().any(|i| i.contains("0x07230203")));
    }

    #[test]
    fn validate_spirv_bad_magic_and_too_short() {
        // Only 2 words AND bad magic → should produce 2 issues
        let bad = vec![0xDEADBEEF, 0];
        let issues = validate_spirv(&bad, "dual_problem");
        assert!(issues.len() >= 2, "expected >=2 issues, got: {:?}", issues);
        assert!(issues.iter().any(|i| i.contains("magic")));
        assert!(issues.iter().any(|i| i.contains("too short")));
    }

    #[test]
    fn validate_spirv_exactly_five_words_valid() {
        let spv = vec![SPIRV_MAGIC, 1, 2, 3, 4];
        let issues = validate_spirv(&spv, "minimal");
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_spirv_four_words_valid_magic() {
        // 4 words with correct magic but still too short
        let spv = vec![SPIRV_MAGIC, 1, 2, 3];
        let issues = validate_spirv(&spv, "short");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("too short"));
        assert!(issues[0].contains("4 words"));
    }

    #[test]
    fn shader_entry_one_word() {
        let entry = shader_entry("tiny", &[SPIRV_MAGIC]);
        assert_eq!(entry.spv_words, 1);
        assert_eq!(entry.spv_bytes, 4);
        assert!(entry.valid_header);
    }

    #[test]
    fn shader_entry_six_words() {
        let spv = vec![SPIRV_MAGIC, 0, 0, 0, 0, 0];
        let entry = shader_entry("medium", &spv);
        assert_eq!(entry.spv_words, 6);
        assert_eq!(entry.spv_bytes, 24);
        assert!(entry.valid_header);
    }

    #[test]
    fn shader_entry_empty_bytecode() {
        let entry = shader_entry("empty", &[]);
        assert_eq!(entry.spv_words, 0);
        assert_eq!(entry.spv_bytes, 0);
        assert!(!entry.valid_header);
    }

    #[test]
    fn shader_category_distribution_debug_clone() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        // Debug
        let debug_str = format!("{:?}", dist);
        assert!(debug_str.contains("ShaderCategoryDistribution"));
        // Clone
        let cloned = dist.clone();
        assert_eq!(cloned.activation_count, dist.activation_count);
        assert_eq!(cloned.attention_count, dist.attention_count);
        assert_eq!(cloned.arithmetic_count, dist.arithmetic_count);
        assert_eq!(cloned.quantization_count, dist.quantization_count);
        assert_eq!(cloned.kv_cache_count, dist.kv_cache_count);
        assert_eq!(cloned.core_count, dist.core_count);
        assert_eq!(cloned.normalization_count, dist.normalization_count);
        assert_eq!(cloned.other_count, dist.other_count);
        assert_eq!(cloned.categories.len(), dist.categories.len());
    }

    #[test]
    fn shader_category_distribution_json_all_fields() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        let json = serde_json::to_string(&dist).unwrap();
        assert!(json.contains("\"activation_count\""));
        assert!(json.contains("\"attention_count\""));
        assert!(json.contains("\"arithmetic_count\""));
        assert!(json.contains("\"quantization_count\""));
        assert!(json.contains("\"kv_cache_count\""));
        assert!(json.contains("\"core_count\""));
        assert!(json.contains("\"normalization_count\""));
        assert!(json.contains("\"other_count\""));
        assert!(json.contains("\"categories\""));
    }

    #[test]
    fn shader_category_distribution_empty_registry() {
        let empty = ShaderRegistry {
            shader_count: 0,
            total_spv_bytes: 0,
            shaders: vec![],
            validation_issues: vec![],
        };
        let dist = shader_category_distribution(&empty);
        assert_eq!(dist.activation_count, 0);
        assert_eq!(dist.attention_count, 0);
        assert_eq!(dist.arithmetic_count, 0);
        assert_eq!(dist.quantization_count, 0);
        assert_eq!(dist.kv_cache_count, 0);
        assert_eq!(dist.core_count, 0);
        assert_eq!(dist.normalization_count, 0);
        assert_eq!(dist.other_count, 0);
        // The 7 standard categories are always present (even with 0 counts);
        // only "other" is conditionally added.
        assert_eq!(dist.categories.len(), 7);
        for (_, count) in &dist.categories {
            assert_eq!(*count, 0, "empty registry should have all counts = 0");
        }
    }

    #[test]
    fn shader_category_distribution_other_count_with_unknown() {
        let reg = ShaderRegistry {
            shader_count: 1,
            total_spv_bytes: 20,
            shaders: vec![shader_entry("mystery", &[SPIRV_MAGIC, 0, 0, 0, 0])],
            validation_issues: vec![],
        };
        let dist = shader_category_distribution(&reg);
        assert_eq!(dist.other_count, 1);
        // "other" should appear in categories
        assert!(dist.categories.iter().any(|(name, count)| name == "other" && *count == 1));
    }

    #[test]
    fn shader_category_distribution_categories_have_correct_names() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        let cat_names: Vec<&str> = dist.categories.iter().map(|(n, _)| n.as_str()).collect();
        // Must include the 7 standard categories (other omitted since other_count == 0)
        assert!(cat_names.contains(&"activation"));
        assert!(cat_names.contains(&"attention"));
        assert!(cat_names.contains(&"arithmetic"));
        assert!(cat_names.contains(&"quantization"));
        assert!(cat_names.contains(&"kv_cache"));
        assert!(cat_names.contains(&"core"));
        assert!(cat_names.contains(&"normalization"));
    }

    #[test]
    fn shader_size_stats_debug_clone() {
        let reg = shader_registry();
        let stats = shader_size_stats(&reg).unwrap();
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("ShaderSizeStats"));
        let cloned = stats.clone();
        assert_eq!(cloned.min_bytes, stats.min_bytes);
        assert_eq!(cloned.max_bytes, stats.max_bytes);
        assert!((cloned.avg_bytes - stats.avg_bytes).abs() < f64::EPSILON);
        assert_eq!(cloned.total_bytes, stats.total_bytes);
        assert_eq!(cloned.min_shader, stats.min_shader);
        assert_eq!(cloned.max_shader, stats.max_shader);
    }

    #[test]
    fn shader_size_stats_json_all_fields() {
        let reg = shader_registry();
        let stats = shader_size_stats(&reg).unwrap();
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"min_bytes\""));
        assert!(json.contains("\"max_bytes\""));
        assert!(json.contains("\"avg_bytes\""));
        assert!(json.contains("\"total_bytes\""));
        assert!(json.contains("\"min_shader\""));
        assert!(json.contains("\"max_shader\""));
    }

    #[test]
    fn shader_size_stats_avg_formula() {
        let reg = ShaderRegistry {
            shader_count: 3,
            total_spv_bytes: 60,
            shaders: vec![
                shader_entry("a", &[SPIRV_MAGIC, 0, 0, 0, 0]), // 20 bytes
                shader_entry("b", &[SPIRV_MAGIC, 0, 0, 0, 0]), // 20 bytes
                shader_entry("c", &[SPIRV_MAGIC, 0, 0, 0, 0]), // 20 bytes
            ],
            validation_issues: vec![],
        };
        let stats = shader_size_stats(&reg).unwrap();
        assert!((stats.avg_bytes - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn shader_size_stats_avg_unequal_sizes() {
        // 2 shaders: 8 bytes and 24 bytes → total=32, avg=16
        let reg = ShaderRegistry {
            shader_count: 2,
            total_spv_bytes: 32,
            shaders: vec![
                shader_entry("small", &[SPIRV_MAGIC, 0]),           // 2 words = 8 bytes
                shader_entry("large", &[SPIRV_MAGIC, 0, 0, 0, 0, 0]), // 6 words = 24 bytes
            ],
            validation_issues: vec![],
        };
        let stats = shader_size_stats(&reg).unwrap();
        assert_eq!(stats.min_bytes, 8);
        assert_eq!(stats.max_bytes, 24);
        assert_eq!(stats.min_shader, "small");
        assert_eq!(stats.max_shader, "large");
        assert!((stats.avg_bytes - 16.0).abs() < f64::EPSILON);
        assert_eq!(stats.total_bytes, 32);
    }

    #[test]
    fn registry_validate_with_preexisting_issues() {
        let reg = ShaderRegistry {
            shader_count: 1,
            total_spv_bytes: 20,
            shaders: vec![shader_entry("test", &[SPIRV_MAGIC, 0, 0, 0, 0])],
            validation_issues: vec!["pre-existing issue".to_string()],
        };
        let issues = reg.validate();
        assert!(issues.iter().any(|i| i == "pre-existing issue"));
    }

    #[test]
    fn registry_validate_all_issues_combined() {
        // count mismatch + total mismatch + duplicate names + pre-existing
        let e = shader_entry("dup", &[SPIRV_MAGIC, 0, 0, 0, 0]);
        let reg = ShaderRegistry {
            shader_count: 99,
            total_spv_bytes: 9999,
            shaders: vec![e.clone(), e],
            validation_issues: vec!["existing".to_string()],
        };
        let issues = reg.validate();
        assert!(issues.len() >= 4, "expected >=4 issues, got: {:?}", issues);
        assert!(issues.iter().any(|i| i == "existing"));
        assert!(issues.iter().any(|i| i.contains("shader_count")));
        assert!(issues.iter().any(|i| i.contains("total_spv_bytes")));
        assert!(issues.iter().any(|i| i.contains("duplicate")));
    }

    #[test]
    fn all_shader_names_unique() {
        let reg = shader_registry();
        let mut names: Vec<&str> = reg.shaders.iter().map(|s| s.name).collect();
        let original_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), original_len, "duplicate shader names found");
    }

    #[test]
    fn all_shaders_have_nonzero_words() {
        let reg = shader_registry();
        for s in &reg.shaders {
            assert!(s.spv_words > 0, "shader {} has zero words", s.name);
            assert!(s.spv_bytes > 0, "shader {} has zero bytes", s.name);
        }
    }

    #[test]
    fn all_shaders_bytecode_length_matches_entry() {
        let reg = shader_registry();
        for s in &reg.shaders {
            if let Some(spv) = shader_bytecode(s.name) {
                assert_eq!(
                    spv.len(),
                    s.spv_words,
                    "shader {} bytecode len {} != entry spv_words {}",
                    s.name,
                    spv.len(),
                    s.spv_words
                );
            }
        }
    }

    #[test]
    fn registry_find_shader_empty_registry() {
        let empty = ShaderRegistry {
            shader_count: 0,
            total_spv_bytes: 0,
            shaders: vec![],
            validation_issues: vec![],
        };
        assert!(empty.find_shader("nda").is_none());
    }

    #[test]
    fn registry_largest_shader_empty_registry() {
        let empty = ShaderRegistry {
            shader_count: 0,
            total_spv_bytes: 0,
            shaders: vec![],
            validation_issues: vec![],
        };
        assert!(empty.largest_shader().is_none());
    }

    #[test]
    fn registry_smallest_shader_empty_registry() {
        let empty = ShaderRegistry {
            shader_count: 0,
            total_spv_bytes: 0,
            shaders: vec![],
            validation_issues: vec![],
        };
        assert!(empty.smallest_shader().is_none());
    }

    #[test]
    fn registry_debug_clone() {
        let reg = shader_registry();
        let debug_str = format!("{:?}", reg);
        assert!(debug_str.contains("ShaderRegistry"));
        assert!(debug_str.contains("shader_count"));

        let cloned = reg.clone();
        assert_eq!(cloned.shader_count, reg.shader_count);
        assert_eq!(cloned.total_spv_bytes, reg.total_spv_bytes);
        assert_eq!(cloned.shaders.len(), reg.shaders.len());
        assert_eq!(cloned.validation_issues, reg.validation_issues);
    }

    #[test]
    fn shader_entry_debug() {
        let entry = shader_entry("debug_test", &[SPIRV_MAGIC, 0, 0, 0, 0]);
        let debug_str = format!("{:?}", entry);
        assert!(debug_str.contains("ShaderEntry"));
        assert!(debug_str.contains("debug_test"));
    }

    #[test]
    fn registry_validate_empty_clean() {
        let empty = ShaderRegistry {
            shader_count: 0,
            total_spv_bytes: 0,
            shaders: vec![],
            validation_issues: vec![],
        };
        let issues = empty.validate();
        assert!(issues.is_empty(), "empty registry should have no issues: {:?}", issues);
    }

    #[test]
    fn shader_size_stats_min_max_correct_against_manual() {
        let reg = shader_registry();
        let stats = shader_size_stats(&reg).unwrap();
        // Manually compute min/max
        let manual_min = reg.shaders.iter().map(|s| s.spv_bytes).min().unwrap();
        let manual_max = reg.shaders.iter().map(|s| s.spv_bytes).max().unwrap();
        assert_eq!(stats.min_bytes, manual_min);
        assert_eq!(stats.max_bytes, manual_max);
    }

    #[test]
    fn shader_size_stats_min_shader_has_min_bytes() {
        let reg = shader_registry();
        let stats = shader_size_stats(&reg).unwrap();
        let entry = reg.find_shader(&stats.min_shader).unwrap();
        assert_eq!(entry.spv_bytes, stats.min_bytes);
    }

    #[test]
    fn shader_size_stats_max_shader_has_max_bytes() {
        let reg = shader_registry();
        let stats = shader_size_stats(&reg).unwrap();
        let entry = reg.find_shader(&stats.max_shader).unwrap();
        assert_eq!(entry.spv_bytes, stats.max_bytes);
    }

    #[test]
    fn spirv_magic_constant_correct() {
        // Verify the magic constant is the standard SPIR-V magic number
        assert_eq!(SPIRV_MAGIC, 0x07230203);
    }

    #[test]
    fn category_distribution_categories_vec_nonempty_for_real_registry() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        assert!(!dist.categories.is_empty());
        // All entries should have non-zero counts
        for (_, count) in &dist.categories {
            assert!(*count > 0, "category with zero count should not appear in vec");
        }
    }

    #[test]
    fn shader_bytecode_returns_same_as_spv_constant() {
        // Verify that shader_bytecode returns the same slice as the exported constant
        let bc = shader_bytecode("nda").unwrap();
        assert_eq!(bc.len(), NDA_SPV.len());
        assert_eq!(bc[0], NDA_SPV[0]);
    }

    #[test]
    fn registry_shaders_order_preserved() {
        // The registry should preserve insertion order
        let reg = shader_registry();
        assert_eq!(reg.shaders[0].name, "act_bitnet");
        assert_eq!(reg.shaders[1].name, "act_nda");
        assert_eq!(reg.shaders[16].name, "ternary");
    }

    // ── Block 163: Shader registry JSON structure, cross-validation, edge cases ──

    #[test]
    fn shader_entry_json_key_count() {
        let entry = shader_entry("test", &[SPIRV_MAGIC, 0, 0, 0, 0]);
        let v: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&entry).unwrap(),
        )
        .unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 4, "ShaderEntry should have exactly 4 JSON keys");
        assert!(obj.contains_key("name"));
        assert!(obj.contains_key("spv_words"));
        assert!(obj.contains_key("spv_bytes"));
        assert!(obj.contains_key("valid_header"));
    }

    #[test]
    fn shader_entry_json_values() {
        let entry = shader_entry("my_shader", &[SPIRV_MAGIC, 0, 0, 0, 0]);
        let v: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&entry).unwrap(),
        )
        .unwrap();
        assert_eq!(v["name"], "my_shader");
        assert_eq!(v["spv_words"], 5);
        assert_eq!(v["spv_bytes"], 20);
        assert_eq!(v["valid_header"], true);
    }

    #[test]
    fn shader_entry_json_empty_bytecode() {
        let entry = shader_entry("empty", &[]);
        let v: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&entry).unwrap(),
        )
        .unwrap();
        assert_eq!(v["spv_words"], 0);
        assert_eq!(v["spv_bytes"], 0);
        assert_eq!(v["valid_header"], false);
    }

    #[test]
    fn shader_registry_json_key_count() {
        let reg = shader_registry();
        let v: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&reg).unwrap(),
        )
        .unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 4, "ShaderRegistry should have exactly 4 JSON keys");
        assert!(obj.contains_key("shader_count"));
        assert!(obj.contains_key("total_spv_bytes"));
        assert!(obj.contains_key("shaders"));
        assert!(obj.contains_key("validation_issues"));
    }

    #[test]
    fn shader_registry_json_values() {
        let reg = shader_registry();
        let v: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&reg).unwrap(),
        )
        .unwrap();
        assert_eq!(v["shader_count"], 17);
        assert_eq!(v["shaders"].as_array().unwrap().len(), 17);
        assert_eq!(v["validation_issues"].as_array().unwrap().len(), 0);
        let total: usize = v["total_spv_bytes"].as_u64().unwrap() as usize;
        assert!(total > 0);
    }

    #[test]
    fn shader_category_distribution_json_key_count() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        let v: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&dist).unwrap(),
        )
        .unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 9, "ShaderCategoryDistribution should have exactly 9 JSON keys");
        for key in &[
            "activation_count", "attention_count", "arithmetic_count",
            "quantization_count", "kv_cache_count", "core_count",
            "normalization_count", "other_count", "categories",
        ] {
            assert!(obj.contains_key(*key), "missing key: {}", key);
        }
    }

    #[test]
    fn shader_size_stats_json_key_count() {
        let reg = shader_registry();
        let stats = shader_size_stats(&reg).unwrap();
        let v: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&stats).unwrap(),
        )
        .unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 6, "ShaderSizeStats should have exactly 6 JSON keys");
        for key in &["min_bytes", "max_bytes", "avg_bytes", "total_bytes", "min_shader", "max_shader"] {
            assert!(obj.contains_key(*key), "missing key: {}", key);
        }
    }

    #[test]
    fn shader_entry_pretty_json() {
        let entry = shader_entry("pretty", &[SPIRV_MAGIC, 0, 0, 0, 0]);
        let pretty = serde_json::to_string_pretty(&entry).unwrap();
        assert!(pretty.contains("\"name\": \"pretty\""));
        assert!(pretty.contains("\"spv_words\": 5"));
        assert!(pretty.contains("\"valid_header\": true"));
    }

    #[test]
    fn registry_pretty_json() {
        let reg = shader_registry();
        let pretty = serde_json::to_string_pretty(&reg).unwrap();
        assert!(pretty.contains("\"shader_count\": 17"));
        assert!(pretty.contains("\"shaders\""));
        assert!(pretty.contains("\"validation_issues\""));
    }

    #[test]
    fn cross_validate_bytecode_lens_match_entries() {
        let reg = shader_registry();
        for s in &reg.shaders {
            let bc = shader_bytecode(s.name).unwrap();
            assert_eq!(
                bc.len(), s.spv_words,
                "shader {}: bytecode len {} != entry spv_words {}",
                s.name, bc.len(), s.spv_words
            );
            assert_eq!(
                bc.len() * 4, s.spv_bytes,
                "shader {}: bytecode bytes {} != entry spv_bytes {}",
                s.name, bc.len() * 4, s.spv_bytes
            );
        }
    }

    #[test]
    fn shader_entry_bytes_formula_various_sizes() {
        for word_count in [1, 2, 5, 10, 50, 100, 1000] {
            let spv: Vec<u32> = vec![SPIRV_MAGIC; word_count];
            let entry = shader_entry("sized", &spv);
            assert_eq!(entry.spv_words, word_count);
            assert_eq!(entry.spv_bytes, word_count * 4);
            assert!(entry.valid_header);
        }
    }

    #[test]
    fn find_shader_all_17_shaders() {
        let reg = shader_registry();
        let names = vec![
            "act_bitnet", "act_nda", "act_qwen", "attn_contig", "attn_ndakv",
            "attn_softmax", "bias_add", "fp2", "fp4", "int4", "kv_write",
            "nda", "residual_add", "rms_norm", "rope", "swiglu", "ternary",
        ];
        for name in &names {
            let entry = reg.find_shader(name);
            assert!(entry.is_some(), "find_shader returned None for {}", name);
            let e = entry.unwrap();
            assert_eq!(e.name, *name);
            assert!(e.valid_header);
            assert!(e.spv_words > 0);
        }
    }

    #[test]
    fn shader_category_multiple_unknown() {
        assert_eq!(shader_category(""), "other");
        assert_eq!(shader_category("foo"), "other");
        assert_eq!(shader_category("NDA"), "other"); // case-sensitive
        assert_eq!(shader_category("act_bitnet "), "other"); // trailing space
    }

    #[test]
    fn registry_clone_independence() {
        let mut reg = shader_registry();
        let original_count = reg.shader_count;
        let cloned = reg.clone();
        reg.shader_count = 0;
        reg.shaders.clear();
        assert_eq!(cloned.shader_count, original_count);
        assert_eq!(cloned.shaders.len(), original_count);
    }

    #[test]
    fn shader_entry_clone_independence() {
        let mut entry = shader_entry("original", &[SPIRV_MAGIC, 0, 0, 0, 0]);
        let cloned = entry.clone();
        entry.name = "modified";
        assert_eq!(cloned.name, "original");
    }

    #[test]
    fn category_distribution_with_mixed_shaders() {
        let reg = ShaderRegistry {
            shader_count: 3,
            total_spv_bytes: 60,
            shaders: vec![
                shader_entry("act_bitnet", &[SPIRV_MAGIC, 0, 0, 0, 0]), // activation
                shader_entry("nda", &[SPIRV_MAGIC, 0, 0, 0, 0]),       // core
                shader_entry("unknown", &[SPIRV_MAGIC, 0, 0, 0, 0]),   // other
            ],
            validation_issues: vec![],
        };
        let dist = shader_category_distribution(&reg);
        assert_eq!(dist.activation_count, 1);
        assert_eq!(dist.core_count, 1);
        assert_eq!(dist.other_count, 1);
        assert_eq!(dist.attention_count, 0);
        assert_eq!(dist.arithmetic_count, 0);
        assert_eq!(dist.quantization_count, 0);
        assert_eq!(dist.kv_cache_count, 0);
        assert_eq!(dist.normalization_count, 0);
        // 7 standard + "other" = 8
        assert_eq!(dist.categories.len(), 8);
    }

    #[test]
    fn category_distribution_only_other_shaders() {
        let reg = ShaderRegistry {
            shader_count: 2,
            total_spv_bytes: 40,
            shaders: vec![
                shader_entry("mystery_a", &[SPIRV_MAGIC, 0, 0, 0, 0]),
                shader_entry("mystery_b", &[SPIRV_MAGIC, 0, 0, 0, 0]),
            ],
            validation_issues: vec![],
        };
        let dist = shader_category_distribution(&reg);
        assert_eq!(dist.other_count, 2);
        assert_eq!(dist.activation_count, 0);
        // 7 standard categories always present + "other" = 8
        assert_eq!(dist.categories.len(), 8);
        // "other" should be first (highest count after sort)
        assert_eq!(dist.categories[0].0, "other");
        assert_eq!(dist.categories[0].1, 2);
    }

    #[test]
    fn shader_size_stats_two_shaders_different_sizes() {
        let reg = ShaderRegistry {
            shader_count: 2,
            total_spv_bytes: 32, // 8 + 24 = 32
            shaders: vec![
                shader_entry("tiny", &[SPIRV_MAGIC, 0]),               // 2 words = 8 bytes
                shader_entry("big", &[SPIRV_MAGIC, 0, 0, 0, 0, 0]),   // 6 words = 24 bytes
            ],
            validation_issues: vec![],
        };
        let stats = shader_size_stats(&reg).unwrap();
        assert_eq!(stats.min_bytes, 8);
        assert_eq!(stats.max_bytes, 24);
        assert_eq!(stats.min_shader, "tiny");
        assert_eq!(stats.max_shader, "big");
        assert!((stats.avg_bytes - 16.0).abs() < f64::EPSILON);
        assert_eq!(stats.total_bytes, 32);
    }

    #[test]
    fn shader_size_stats_json_numeric_values() {
        let reg = ShaderRegistry {
            shader_count: 1,
            total_spv_bytes: 20,
            shaders: vec![shader_entry("only", &[SPIRV_MAGIC, 0, 0, 0, 0])],
            validation_issues: vec![],
        };
        let stats = shader_size_stats(&reg).unwrap();
        let v: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&stats).unwrap(),
        )
        .unwrap();
        assert_eq!(v["min_bytes"], 20);
        assert_eq!(v["max_bytes"], 20);
        assert_eq!(v["total_bytes"], 20);
        assert_eq!(v["min_shader"], "only");
        assert_eq!(v["max_shader"], "only");
        assert!((v["avg_bytes"].as_f64().unwrap() - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn validate_spirv_one_word_only_magic() {
        let spv = vec![SPIRV_MAGIC];
        let issues = validate_spirv(&spv, "single");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("too short"));
        assert!(issues[0].contains("1 words"));
    }

    #[test]
    fn validate_spirv_zero_words_reports_word_count() {
        // Empty triggers both "empty" and "too short"
        let issues = validate_spirv(&[], "zero");
        assert!(issues.iter().any(|i| i.contains("0 words")));
    }

    #[test]
    fn registry_validate_total_mismatch_only() {
        // shader_count correct but total_spv_bytes wrong
        let e = shader_entry("a", &[SPIRV_MAGIC, 0, 0, 0, 0]); // 20 bytes
        let reg = ShaderRegistry {
            shader_count: 1,
            total_spv_bytes: 999,
            shaders: vec![e],
            validation_issues: vec![],
        };
        let issues = reg.validate();
        assert!(issues.iter().any(|i| i.contains("total_spv_bytes")));
        assert!(!issues.iter().any(|i| i.contains("shader_count")));
    }

    #[test]
    fn registry_validate_count_mismatch_only() {
        // total_spv_bytes correct but shader_count wrong
        let e = shader_entry("a", &[SPIRV_MAGIC, 0, 0, 0, 0]); // 20 bytes
        let reg = ShaderRegistry {
            shader_count: 50,
            total_spv_bytes: e.spv_bytes,
            shaders: vec![e],
            validation_issues: vec![],
        };
        let issues = reg.validate();
        assert!(issues.iter().any(|i| i.contains("shader_count")));
        assert!(!issues.iter().any(|i| i.contains("total_spv_bytes")));
    }

    #[test]
    fn shader_bytecode_all_return_some_for_exact_names() {
        let exact_names = vec![
            "act_bitnet", "act_nda", "act_qwen", "attn_contig", "attn_ndakv",
            "attn_softmax", "bias_add", "fp2", "fp4", "int4", "kv_write",
            "nda", "residual_add", "rms_norm", "rope", "swiglu", "ternary",
        ];
        for name in &exact_names {
            assert!(
                shader_bytecode(name).is_some(),
                "shader_bytecode({}) should return Some",
                name
            );
        }
    }

    #[test]
    fn shader_bytecode_none_for_various_unknowns() {
        let unknowns = vec!["", "NDA", "nda ", "act_BITNET", "nonexistent", "fp8", "int8"];
        for name in &unknowns {
            assert!(
                shader_bytecode(name).is_none(),
                "shader_bytecode({}) should return None",
                name
            );
        }
    }

    #[test]
    fn distribution_categories_vec_length_for_real_registry() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        // Real registry has 0 "other" shaders, so categories vec has 7 entries
        assert_eq!(dist.categories.len(), 7);
    }

    #[test]
    fn shader_size_stats_pretty_json() {
        let reg = shader_registry();
        let stats = shader_size_stats(&reg).unwrap();
        let pretty = serde_json::to_string_pretty(&stats).unwrap();
        assert!(pretty.contains("\"min_bytes\""));
        assert!(pretty.contains("\"avg_bytes\""));
        assert!(pretty.contains("\"max_shader\""));
    }

    #[test]
    fn category_distribution_pretty_json() {
        let reg = shader_registry();
        let dist = shader_category_distribution(&reg);
        let pretty = serde_json::to_string_pretty(&dist).unwrap();
        assert!(pretty.contains("\"activation_count\""));
        assert!(pretty.contains("\"categories\""));
    }
}
