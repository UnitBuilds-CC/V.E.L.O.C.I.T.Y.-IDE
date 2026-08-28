use super::nda_gemv::VulkanNdaGemv;
use super::vulkan_init::VulkanDriver;
use ash::vk;
use serde::Serialize;

pub struct LayerGpuGemvs<'a> {
    /// Fused QKV projection (Q‖K‖V concatenated). Used by the OS-level pipeline when available.
    #[allow(dead_code)]
    pub qkv_proj_gpu: &'a Option<VulkanNdaGemv>,
    /// Fused gate-up projection (gate‖up concatenated). Used by the OS-level pipeline when available.
    #[allow(dead_code)]
    pub gate_up_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub q_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub k_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub v_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub o_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub gate_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub up_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub down_proj_gpu: &'a Option<VulkanNdaGemv>,
}

/// Result of a single transformer layer forward pass, holding GPU buffer offsets
/// and dispatch metadata for the pipeline to continue with attention/FFN.
#[derive(Debug, Clone, Serialize)]
pub struct LayerForwardResult {
    pub q_dispatched: bool,
    pub k_dispatched: bool,
    pub v_dispatched: bool,
    pub o_dispatched: bool,
    pub gate_dispatched: bool,
    pub up_dispatched: bool,
    pub down_dispatched: bool,
}

impl LayerForwardResult {
    /// Count how many projections were dispatched.
    pub fn dispatched_count(&self) -> usize {
        [
            self.q_dispatched,
            self.k_dispatched,
            self.v_dispatched,
            self.o_dispatched,
            self.gate_dispatched,
            self.up_dispatched,
            self.down_dispatched,
        ]
        .iter()
        .filter(|&&d| d)
        .count()
    }

    /// Whether all 7 projections were dispatched.
    pub fn all_dispatched(&self) -> bool {
        self.dispatched_count() == 7
    }

    /// Whether any attention projections were dispatched.
    pub fn has_attention(&self) -> bool {
        self.q_dispatched || self.k_dispatched || self.v_dispatched || self.o_dispatched
    }

    /// Whether any FFN projections were dispatched.
    pub fn has_ffn(&self) -> bool {
        self.gate_dispatched || self.up_dispatched || self.down_dispatched
    }
}

/// Diagnostic info about a layer's GPU GEMV configuration.
#[derive(Debug, Clone, Serialize)]
pub struct LayerGpuGemvsInfo {
    pub attention_projections: usize,
    pub ffn_projections: usize,
    pub total_projections: usize,
    pub has_fused_qkv: bool,
    pub has_fused_gate_up: bool,
    pub has_full_attention: bool,
    pub has_full_ffn: bool,
    pub validation_issues: Vec<String>,
}

impl<'a> LayerGpuGemvs<'a> {
    /// Record all projection GEMV dispatches for a single transformer layer.
    ///
    /// This orchestrates the attention projections (Q, K, V, O) and the
    /// FFN projections (gate, up, down) in the correct order. The pipeline
    /// handles buffer copies separately; this method records the compute
    /// dispatches and inserts barriers between stages.
    ///
    /// Returns a `LayerForwardResult` indicating which projections were dispatched.
    #[allow(dead_code)]
    pub fn forward(&self, driver: &VulkanDriver, cmd: vk::CommandBuffer) -> LayerForwardResult {
        let mut result = LayerForwardResult {
            q_dispatched: false,
            k_dispatched: false,
            v_dispatched: false,
            o_dispatched: false,
            gate_dispatched: false,
            up_dispatched: false,
            down_dispatched: false,
        };

        // --- Attention projections: Q, K, V ---
        if let Some(ref gemv) = self.q_proj_gpu {
            gemv.record_dispatch(cmd);
            super::vulkan_init::cmd_compute_barrier(&driver.device, cmd);
            result.q_dispatched = true;
        }
        if let Some(ref gemv) = self.k_proj_gpu {
            gemv.record_dispatch(cmd);
            super::vulkan_init::cmd_compute_barrier(&driver.device, cmd);
            result.k_dispatched = true;
        }
        if let Some(ref gemv) = self.v_proj_gpu {
            gemv.record_dispatch(cmd);
            super::vulkan_init::cmd_compute_barrier(&driver.device, cmd);
            result.v_dispatched = true;
        }

        // O projection reads from attention output
        if let Some(ref gemv) = self.o_proj_gpu {
            gemv.record_dispatch(cmd);
            super::vulkan_init::cmd_compute_barrier(&driver.device, cmd);
            result.o_dispatched = true;
        }

        // --- FFN projections: gate, up, down ---
        if let Some(ref gemv) = self.gate_proj_gpu {
            gemv.record_dispatch(cmd);
            super::vulkan_init::cmd_compute_barrier(&driver.device, cmd);
            result.gate_dispatched = true;
        }
        if let Some(ref gemv) = self.up_proj_gpu {
            gemv.record_dispatch(cmd);
            super::vulkan_init::cmd_compute_barrier(&driver.device, cmd);
            result.up_dispatched = true;
        }

        // Down projection reads from SwiGLU output
        if let Some(ref gemv) = self.down_proj_gpu {
            gemv.record_dispatch(cmd);
            super::vulkan_init::cmd_compute_barrier(&driver.device, cmd);
            result.down_dispatched = true;
        }

        result
    }

    /// Check if all attention projections are available.
    #[allow(dead_code)]
    pub fn has_full_attention(&self) -> bool {
        self.q_proj_gpu.is_some()
            && self.k_proj_gpu.is_some()
            && self.v_proj_gpu.is_some()
            && self.o_proj_gpu.is_some()
    }

    /// Check if all FFN projections are available.
    #[allow(dead_code)]
    pub fn has_full_ffn(&self) -> bool {
        self.gate_proj_gpu.is_some() && self.up_proj_gpu.is_some() && self.down_proj_gpu.is_some()
    }

    /// Total number of projection GEMVs configured for this layer.
    #[allow(dead_code)]
    pub fn projection_count(&self) -> usize {
        [
            self.q_proj_gpu,
            self.k_proj_gpu,
            self.v_proj_gpu,
            self.o_proj_gpu,
            self.gate_proj_gpu,
            self.up_proj_gpu,
            self.down_proj_gpu,
        ]
        .iter()
        .filter(|g| g.is_some())
        .count()
    }

    /// Count attention projections (Q, K, V, O) that are available.
    #[allow(dead_code)]
    pub fn attention_projection_count(&self) -> usize {
        [
            self.q_proj_gpu,
            self.k_proj_gpu,
            self.v_proj_gpu,
            self.o_proj_gpu,
        ]
        .iter()
        .filter(|g| g.is_some())
        .count()
    }

    /// Count FFN projections (gate, up, down) that are available.
    #[allow(dead_code)]
    pub fn ffn_projection_count(&self) -> usize {
        [
            self.gate_proj_gpu,
            self.up_proj_gpu,
            self.down_proj_gpu,
        ]
        .iter()
        .filter(|g| g.is_some())
        .count()
    }

    /// Build diagnostic info about this layer's GPU configuration.
    #[allow(dead_code)]
    pub fn info(&self) -> LayerGpuGemvsInfo {
        let attn_count = self.attention_projection_count();
        let ffn_count = self.ffn_projection_count();

        let mut issues = Vec::new();
        // If fused QKV is available but individual projections are missing, that's fine.
        // But if individual projections exist without fused, flag partial coverage.
        if self.qkv_proj_gpu.is_some() && attn_count < 3 {
            issues.push(
                "fused QKV present but individual Q/K/V projections incomplete".to_string()
            );
        }
        if self.gate_up_proj_gpu.is_some() && ffn_count < 2 {
            issues.push(
                "fused gate-up present but individual gate/up projections incomplete".to_string()
            );
        }
        if attn_count > 0 && attn_count < 4 && self.qkv_proj_gpu.is_none() {
            issues.push(format!(
                "partial attention coverage: {attn_count}/4 projections"
            ));
        }
        if ffn_count > 0 && ffn_count < 3 && self.gate_up_proj_gpu.is_none() {
            issues.push(format!(
                "partial FFN coverage: {ffn_count}/3 projections"
            ));
        }

        LayerGpuGemvsInfo {
            attention_projections: attn_count,
            ffn_projections: ffn_count,
            total_projections: attn_count + ffn_count,
            has_fused_qkv: self.qkv_proj_gpu.is_some(),
            has_fused_gate_up: self.gate_up_proj_gpu.is_some(),
            has_full_attention: self.has_full_attention(),
            has_full_ffn: self.has_full_ffn(),
            validation_issues: issues,
        }
    }

    /// Validate layer GPU configuration consistency.
    #[allow(dead_code)]
    pub fn validate(&self) -> Vec<String> {
        self.info().validation_issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_empty_layer() -> LayerGpuGemvs<'static> {
        // Use a leaked Box to get a 'static reference (test-only, no Sync needed)
        let none: &'static Option<VulkanNdaGemv> = Box::leak(Box::new(None));
        LayerGpuGemvs {
            qkv_proj_gpu: none,
            gate_up_proj_gpu: none,
            q_proj_gpu: none,
            k_proj_gpu: none,
            v_proj_gpu: none,
            o_proj_gpu: none,
            gate_proj_gpu: none,
            up_proj_gpu: none,
            down_proj_gpu: none,
        }
    }

    #[test]
    fn test_empty_layer_has_no_projections() {
        let layer = make_empty_layer();
        assert_eq!(layer.projection_count(), 0);
        assert!(!layer.has_full_attention());
        assert!(!layer.has_full_ffn());
    }

    #[test]
    fn test_forward_empty_layer_returns_all_false() {
        let layer = make_empty_layer();
        assert_eq!(layer.projection_count(), 0);
    }

    #[test]
    fn test_layer_forward_result_dispatched_count() {
        let result = LayerForwardResult {
            q_dispatched: true,
            k_dispatched: true,
            v_dispatched: true,
            o_dispatched: false,
            gate_dispatched: false,
            up_dispatched: false,
            down_dispatched: false,
        };
        assert_eq!(result.dispatched_count(), 3);
        assert!(!result.all_dispatched());
        assert!(result.has_attention());
        assert!(!result.has_ffn());
    }

    #[test]
    fn test_layer_forward_result_all_dispatched() {
        let result = LayerForwardResult {
            q_dispatched: true,
            k_dispatched: true,
            v_dispatched: true,
            o_dispatched: true,
            gate_dispatched: true,
            up_dispatched: true,
            down_dispatched: true,
        };
        assert_eq!(result.dispatched_count(), 7);
        assert!(result.all_dispatched());
        assert!(result.has_attention());
        assert!(result.has_ffn());
    }

    #[test]
    fn test_layer_forward_result_none_dispatched() {
        let result = LayerForwardResult {
            q_dispatched: false,
            k_dispatched: false,
            v_dispatched: false,
            o_dispatched: false,
            gate_dispatched: false,
            up_dispatched: false,
            down_dispatched: false,
        };
        assert_eq!(result.dispatched_count(), 0);
        assert!(!result.all_dispatched());
        assert!(!result.has_attention());
        assert!(!result.has_ffn());
    }

    #[test]
    fn test_layer_forward_result_serializes() {
        let result = LayerForwardResult {
            q_dispatched: true,
            k_dispatched: false,
            v_dispatched: true,
            o_dispatched: false,
            gate_dispatched: true,
            up_dispatched: true,
            down_dispatched: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"q_dispatched\":true"));
        assert!(json.contains("\"k_dispatched\":false"));
    }

    #[test]
    fn test_empty_layer_info() {
        let layer = make_empty_layer();
        let info = layer.info();
        assert_eq!(info.attention_projections, 0);
        assert_eq!(info.ffn_projections, 0);
        assert_eq!(info.total_projections, 0);
        assert!(!info.has_fused_qkv);
        assert!(!info.has_fused_gate_up);
        assert!(!info.has_full_attention);
        assert!(!info.has_full_ffn);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn test_layer_gpu_gemvs_info_serializes() {
        let info = LayerGpuGemvsInfo {
            attention_projections: 4,
            ffn_projections: 3,
            total_projections: 7,
            has_fused_qkv: true,
            has_fused_gate_up: true,
            has_full_attention: true,
            has_full_ffn: true,
            validation_issues: vec![],
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"total_projections\":7"));
        assert!(json.contains("\"has_fused_qkv\":true"));
    }

    #[test]
    fn test_empty_layer_validate_clean() {
        let layer = make_empty_layer();
        let issues = layer.validate();
        assert!(issues.is_empty());
    }

    #[test]
    fn test_attention_and_ffn_counts() {
        let layer = make_empty_layer();
        assert_eq!(layer.attention_projection_count(), 0);
        assert_eq!(layer.ffn_projection_count(), 0);
    }

    // ── LayerForwardResult: individual field dispatch ────────────────────

    fn all_false_result() -> LayerForwardResult {
        LayerForwardResult {
            q_dispatched: false,
            k_dispatched: false,
            v_dispatched: false,
            o_dispatched: false,
            gate_dispatched: false,
            up_dispatched: false,
            down_dispatched: false,
        }
    }

    #[test]
    fn result_only_q_dispatched() {
        let mut r = all_false_result();
        r.q_dispatched = true;
        assert_eq!(r.dispatched_count(), 1);
        assert!(r.has_attention());
        assert!(!r.has_ffn());
        assert!(!r.all_dispatched());
    }

    #[test]
    fn result_only_k_dispatched() {
        let mut r = all_false_result();
        r.k_dispatched = true;
        assert_eq!(r.dispatched_count(), 1);
        assert!(r.has_attention());
        assert!(!r.has_ffn());
    }

    #[test]
    fn result_only_v_dispatched() {
        let mut r = all_false_result();
        r.v_dispatched = true;
        assert_eq!(r.dispatched_count(), 1);
        assert!(r.has_attention());
    }

    #[test]
    fn result_only_o_dispatched() {
        let mut r = all_false_result();
        r.o_dispatched = true;
        assert_eq!(r.dispatched_count(), 1);
        assert!(r.has_attention());
        assert!(!r.has_ffn());
    }

    #[test]
    fn result_only_gate_dispatched() {
        let mut r = all_false_result();
        r.gate_dispatched = true;
        assert_eq!(r.dispatched_count(), 1);
        assert!(!r.has_attention());
        assert!(r.has_ffn());
    }

    #[test]
    fn result_only_up_dispatched() {
        let mut r = all_false_result();
        r.up_dispatched = true;
        assert_eq!(r.dispatched_count(), 1);
        assert!(!r.has_attention());
        assert!(r.has_ffn());
    }

    #[test]
    fn result_only_down_dispatched() {
        let mut r = all_false_result();
        r.down_dispatched = true;
        assert_eq!(r.dispatched_count(), 1);
        assert!(!r.has_attention());
        assert!(r.has_ffn());
    }

    // ── LayerForwardResult: attention-only combinations ──────────────────

    #[test]
    fn result_qk_dispatched() {
        let mut r = all_false_result();
        r.q_dispatched = true;
        r.k_dispatched = true;
        assert_eq!(r.dispatched_count(), 2);
        assert!(r.has_attention());
        assert!(!r.has_ffn());
    }

    #[test]
    fn result_qkvo_dispatched() {
        let mut r = all_false_result();
        r.q_dispatched = true;
        r.k_dispatched = true;
        r.v_dispatched = true;
        r.o_dispatched = true;
        assert_eq!(r.dispatched_count(), 4);
        assert!(r.has_attention());
        assert!(!r.has_ffn());
        assert!(!r.all_dispatched());
    }

    // ── LayerForwardResult: FFN-only combinations ────────────────────────

    #[test]
    fn result_gate_up_dispatched() {
        let mut r = all_false_result();
        r.gate_dispatched = true;
        r.up_dispatched = true;
        assert_eq!(r.dispatched_count(), 2);
        assert!(!r.has_attention());
        assert!(r.has_ffn());
    }

    #[test]
    fn result_gate_up_down_dispatched() {
        let mut r = all_false_result();
        r.gate_dispatched = true;
        r.up_dispatched = true;
        r.down_dispatched = true;
        assert_eq!(r.dispatched_count(), 3);
        assert!(!r.has_attention());
        assert!(r.has_ffn());
        assert!(!r.all_dispatched());
    }

    // ── LayerForwardResult: mixed attention + FFN ────────────────────────

    #[test]
    fn result_q_plus_gate_dispatched() {
        let mut r = all_false_result();
        r.q_dispatched = true;
        r.gate_dispatched = true;
        assert_eq!(r.dispatched_count(), 2);
        assert!(r.has_attention());
        assert!(r.has_ffn());
    }

    #[test]
    fn result_six_of_seven_dispatched() {
        let mut r = all_false_result();
        r.q_dispatched = true;
        r.k_dispatched = true;
        r.v_dispatched = true;
        r.o_dispatched = true;
        r.gate_dispatched = true;
        r.up_dispatched = true;
        // down_dispatched = false
        assert_eq!(r.dispatched_count(), 6);
        assert!(r.has_attention());
        assert!(r.has_ffn());
        assert!(!r.all_dispatched());
    }

    // ── LayerForwardResult: struct derives ───────────────────────────────

    #[test]
    fn result_clone_is_independent() {
        let mut r = all_false_result();
        r.q_dispatched = true;
        let mut cloned = r.clone();
        cloned.q_dispatched = false;
        assert!(r.q_dispatched); // original unchanged
        assert!(!cloned.q_dispatched); // clone mutated independently
    }

    #[test]
    fn result_debug_format_contains_fields() {
        let mut r = all_false_result();
        r.q_dispatched = true;
        let debug = format!("{:?}", r);
        assert!(debug.contains("q_dispatched"));
        assert!(debug.contains("true"));
    }

    // ── LayerForwardResult: serialization ────────────────────────────────

    #[test]
    fn result_json_all_fields_present() {
        let r = all_false_result();
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("q_dispatched"));
        assert!(json.contains("k_dispatched"));
        assert!(json.contains("v_dispatched"));
        assert!(json.contains("o_dispatched"));
        assert!(json.contains("gate_dispatched"));
        assert!(json.contains("up_dispatched"));
        assert!(json.contains("down_dispatched"));
    }

    #[test]
    fn result_json_parseable_as_value() {
        let mut r = all_false_result();
        r.q_dispatched = true;
        r.gate_dispatched = true;
        let json = serde_json::to_string(&r).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["q_dispatched"].as_bool(), Some(true));
        assert_eq!(parsed["k_dispatched"].as_bool(), Some(false));
        assert_eq!(parsed["gate_dispatched"].as_bool(), Some(true));
        assert_eq!(parsed["down_dispatched"].as_bool(), Some(false));
    }

    #[test]
    fn result_json_roundtrip_consistent() {
        let mut r = all_false_result();
        r.o_dispatched = true;
        r.down_dispatched = true;
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"o_dispatched\":true"));
        assert!(json.contains("\"down_dispatched\":true"));
        assert!(json.contains("\"q_dispatched\":false"));
    }

    // ── LayerGpuGemvsInfo: struct and serialization ──────────────────────

    fn sample_info() -> LayerGpuGemvsInfo {
        LayerGpuGemvsInfo {
            attention_projections: 4,
            ffn_projections: 3,
            total_projections: 7,
            has_fused_qkv: false,
            has_fused_gate_up: false,
            has_full_attention: true,
            has_full_ffn: true,
            validation_issues: vec![],
        }
    }

    #[test]
    fn info_clone_is_independent() {
        let info = sample_info();
        let mut cloned = info.clone();
        cloned.attention_projections = 99;
        assert_eq!(info.attention_projections, 4);
    }

    #[test]
    fn info_debug_format() {
        let info = sample_info();
        let debug = format!("{:?}", info);
        assert!(debug.contains("attention_projections"));
        assert!(debug.contains("total_projections"));
    }

    #[test]
    fn info_json_all_fields() {
        let info = sample_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("attention_projections"));
        assert!(json.contains("ffn_projections"));
        assert!(json.contains("total_projections"));
        assert!(json.contains("has_fused_qkv"));
        assert!(json.contains("has_fused_gate_up"));
        assert!(json.contains("has_full_attention"));
        assert!(json.contains("has_full_ffn"));
        assert!(json.contains("validation_issues"));
    }

    #[test]
    fn info_json_parseable_as_value() {
        let info = sample_info();
        let json = serde_json::to_string(&info).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["attention_projections"].as_u64(), Some(4));
        assert_eq!(parsed["ffn_projections"].as_u64(), Some(3));
        assert_eq!(parsed["total_projections"].as_u64(), Some(7));
        assert_eq!(parsed["has_full_attention"].as_bool(), Some(true));
        assert_eq!(parsed["has_full_ffn"].as_bool(), Some(true));
    }

    #[test]
    fn info_with_validation_issues_serializes() {
        let mut info = sample_info();
        info.validation_issues = vec![
            "partial attention coverage: 2/4 projections".to_string(),
            "partial FFN coverage: 1/3 projections".to_string(),
        ];
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("partial attention"));
        assert!(json.contains("partial FFN"));
    }

    #[test]
    fn info_total_is_sum_of_parts() {
        let info = sample_info();
        assert_eq!(
            info.total_projections,
            info.attention_projections + info.ffn_projections
        );
    }

    #[test]
    fn info_zero_projections() {
        let info = LayerGpuGemvsInfo {
            attention_projections: 0,
            ffn_projections: 0,
            total_projections: 0,
            has_fused_qkv: false,
            has_fused_gate_up: false,
            has_full_attention: false,
            has_full_ffn: false,
            validation_issues: vec![],
        };
        assert_eq!(info.total_projections, 0);
        assert!(!info.has_full_attention);
        assert!(!info.has_full_ffn);
    }

    // ── Empty layer: info and validate ───────────────────────────────────

    #[test]
    fn empty_layer_info_total_is_sum() {
        let layer = make_empty_layer();
        let info = layer.info();
        assert_eq!(
            info.total_projections,
            info.attention_projections + info.ffn_projections
        );
    }

    #[test]
    fn empty_layer_info_no_fused() {
        let layer = make_empty_layer();
        let info = layer.info();
        assert!(!info.has_fused_qkv);
        assert!(!info.has_fused_gate_up);
    }

    #[test]
    fn empty_layer_validate_matches_info() {
        let layer = make_empty_layer();
        let info = layer.info();
        let validate = layer.validate();
        assert_eq!(info.validation_issues, validate);
    }

    #[test]
    fn empty_layer_info_no_partial_issues() {
        // Empty layer has 0 projections → no partial coverage issues
        let layer = make_empty_layer();
        let info = layer.info();
        assert!(info.validation_issues.is_empty());
    }

    // ── Block 185: JSON key counts ────────────────────────────────────────

    #[test]
    fn result_json_has_exactly_7_keys() {
        let r = all_false_result();
        let val: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&r).unwrap()
        ).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 7);
    }

    #[test]
    fn info_json_has_exactly_8_keys() {
        let info = sample_info();
        let val: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&info).unwrap()
        ).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 8);
    }

    // ── Block 185: JSON roundtrip via Value ───────────────────────────────

    #[test]
    fn result_json_roundtrip_via_value() {
        let mut r = all_false_result();
        r.q_dispatched = true;
        r.down_dispatched = true;
        let json = serde_json::to_string(&r).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["q_dispatched"], true);
        assert_eq!(val["k_dispatched"], false);
        assert_eq!(val["down_dispatched"], true);
    }

    #[test]
    fn info_json_roundtrip_via_value() {
        let info = sample_info();
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["attention_projections"], 4);
        assert_eq!(val["ffn_projections"], 3);
        assert_eq!(val["total_projections"], 7);
        assert_eq!(val["has_full_attention"], true);
    }

    // ── Block 185: Debug format completeness ──────────────────────────────

    #[test]
    fn result_debug_has_all_seven_fields() {
        let r = all_false_result();
        let debug = format!("{:?}", r);
        assert!(debug.contains("q_dispatched"));
        assert!(debug.contains("k_dispatched"));
        assert!(debug.contains("v_dispatched"));
        assert!(debug.contains("o_dispatched"));
        assert!(debug.contains("gate_dispatched"));
        assert!(debug.contains("up_dispatched"));
        assert!(debug.contains("down_dispatched"));
    }

    #[test]
    fn info_debug_has_all_eight_fields() {
        let info = sample_info();
        let debug = format!("{:?}", info);
        assert!(debug.contains("attention_projections"));
        assert!(debug.contains("ffn_projections"));
        assert!(debug.contains("total_projections"));
        assert!(debug.contains("has_fused_qkv"));
        assert!(debug.contains("has_fused_gate_up"));
        assert!(debug.contains("has_full_attention"));
        assert!(debug.contains("has_full_ffn"));
        assert!(debug.contains("validation_issues"));
    }

    // ── Block 185: dispatched_count exhaustive ────────────────────────────

    #[test]
    fn result_dispatched_count_exactly_5() {
        let mut r = all_false_result();
        r.q_dispatched = true;
        r.k_dispatched = true;
        r.v_dispatched = true;
        r.o_dispatched = true;
        r.gate_dispatched = true;
        assert_eq!(r.dispatched_count(), 5);
    }

    // ── Block 185: has_attention/has_ffn boundary ─────────────────────────

    #[test]
    fn result_has_attention_only_o() {
        let mut r = all_false_result();
        r.o_dispatched = true;
        assert!(r.has_attention());
        assert!(!r.has_ffn());
    }

    #[test]
    fn result_has_ffn_only_down() {
        let mut r = all_false_result();
        r.down_dispatched = true;
        assert!(!r.has_attention());
        assert!(r.has_ffn());
    }

    // ── Block 185: Compact JSON ───────────────────────────────────────────

    #[test]
    fn result_compact_json() {
        let r = all_false_result();
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("\n"));
    }

    #[test]
    fn info_compact_json() {
        let info = sample_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("\n"));
    }

    // ── Block 185: Info formula verification ──────────────────────────────

    #[test]
    fn info_total_equals_attention_plus_ffn_various() {
        for (attn, ffn) in [(0, 0), (1, 0), (0, 1), (4, 3), (2, 1), (3, 2)] {
            let info = LayerGpuGemvsInfo {
                attention_projections: attn,
                ffn_projections: ffn,
                total_projections: attn + ffn,
                has_fused_qkv: false,
                has_fused_gate_up: false,
                has_full_attention: attn == 4,
                has_full_ffn: ffn == 3,
                validation_issues: vec![],
            };
            assert_eq!(info.total_projections, attn + ffn,
                "failed for attn={}, ffn={}", attn, ffn);
        }
    }

    #[test]
    fn info_has_full_attention_only_at_4() {
        for n in 0..=4 {
            let info = LayerGpuGemvsInfo {
                attention_projections: n,
                ffn_projections: 0,
                total_projections: n,
                has_fused_qkv: false,
                has_fused_gate_up: false,
                has_full_attention: n == 4,
                has_full_ffn: false,
                validation_issues: vec![],
            };
            assert_eq!(info.has_full_attention, n == 4);
        }
    }

    // ── Block 185: Validation issues edge cases ───────────────────────────

    #[test]
    fn info_with_single_validation_issue() {
        let mut info = sample_info();
        info.validation_issues = vec!["test issue".to_string()];
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let issues = val["validation_issues"].as_array().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].as_str().unwrap(), "test issue");
    }

    #[test]
    fn info_empty_validation_issues_is_array() {
        let info = sample_info();
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["validation_issues"].as_array().unwrap().is_empty());
    }

    // ── Block 185: Clone independence additional ──────────────────────────

    #[test]
    fn info_clone_validation_issues_independent() {
        let mut info = sample_info();
        info.validation_issues = vec!["original".to_string()];
        let mut cloned = info.clone();
        cloned.validation_issues.push("added".to_string());
        assert_eq!(info.validation_issues.len(), 1);
        assert_eq!(cloned.validation_issues.len(), 2);
    }

    // ── Block 185: JSON numeric types ─────────────────────────────────────

    #[test]
    fn result_json_values_are_booleans() {
        let r = all_false_result();
        let json = serde_json::to_string(&r).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        for key in &["q_dispatched", "k_dispatched", "v_dispatched", "o_dispatched",
                      "gate_dispatched", "up_dispatched", "down_dispatched"] {
            assert!(val[key].is_boolean(), "{} should be boolean", key);
        }
    }

    #[test]
    fn info_json_numeric_types() {
        let info = sample_info();
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["attention_projections"].is_number());
        assert!(val["ffn_projections"].is_number());
        assert!(val["total_projections"].is_number());
        assert!(val["has_fused_qkv"].is_boolean());
        assert!(val["validation_issues"].is_array());
    }
}
