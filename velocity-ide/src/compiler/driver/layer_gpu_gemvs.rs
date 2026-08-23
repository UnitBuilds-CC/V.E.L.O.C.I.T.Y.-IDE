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
}
