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
}
