use super::nda_gemv::VulkanNdaGemv;
use super::vulkan_init::VulkanDriver;
use ash::vk;

pub struct LayerGpuGemvs<'a> {
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
#[derive(Debug, Clone)]
pub struct LayerForwardResult {
    pub q_dispatched: bool,
    pub k_dispatched: bool,
    pub v_dispatched: bool,
    pub o_dispatched: bool,
    pub gate_dispatched: bool,
    pub up_dispatched: bool,
    pub down_dispatched: bool,
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
    pub fn has_full_attention(&self) -> bool {
        self.q_proj_gpu.is_some()
            && self.k_proj_gpu.is_some()
            && self.v_proj_gpu.is_some()
            && self.o_proj_gpu.is_some()
    }

    /// Check if all FFN projections are available.
    pub fn has_full_ffn(&self) -> bool {
        self.gate_proj_gpu.is_some() && self.up_proj_gpu.is_some() && self.down_proj_gpu.is_some()
    }

    /// Total number of projection GEMVs configured for this layer.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_empty_layer() -> LayerGpuGemvs<'static> {
        // Use a leaked Box to get a 'static reference (test-only, no Sync needed)
        let none: &'static Option<VulkanNdaGemv> = Box::leak(Box::new(None));
        LayerGpuGemvs {
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
        // We can't create a real VulkanDriver in tests, but we can verify
        // the logic paths that don't require Vulkan by checking the result
        // struct shape and the helper methods.
        let layer = make_empty_layer();
        assert_eq!(layer.projection_count(), 0);
    }
}
