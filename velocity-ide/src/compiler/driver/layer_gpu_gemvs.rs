use super::nda_gemv::VulkanNdaGemv;

pub struct LayerGpuGemvs<'a> {
    pub q_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub k_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub v_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub o_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub gate_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub up_proj_gpu: &'a Option<VulkanNdaGemv>,
    pub down_proj_gpu: &'a Option<VulkanNdaGemv>,
}
