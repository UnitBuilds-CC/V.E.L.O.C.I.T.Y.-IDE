// GPU infrastructure — retained for future BitNet NDA model support.
#![allow(dead_code)]
//! Vulkan NDA BitNet (1-bit quantized) transformer layer implementation.
//!
//! # Safety Invariants
//!
//! All `unsafe` blocks wrap Vulkan API calls via `ash`. The following invariants hold:
//! - All handles (Device, Queue, Buffers, Pipelines, DescriptorSets) are valid and initialized.
//! - Buffers are created via `create_coherent_buffer` / `create_device_local_buffer`.
//! - Descriptor sets are allocated from pools with sufficient capacity.
//! - Command buffers are recorded within valid scopes and submitted to the correct queue.
//! - Push constants match the pipeline layout ranges.
//! - `Drop` tears down resources in reverse dependency order.

use super::packing::*;
use super::vulkan_init::*;
use ash::vk;
use ash::Device;
use serde::Serialize;
use std::ffi::CString;
use std::time::Instant;

/// NDA BitNet layer model dimensions.
#[derive(Debug, Clone, Serialize)]
pub struct NdaBitNetLayerConfig {
    pub hidden_size: usize,
    pub ffn_size: usize,
    pub n_heads: usize,
    pub head_dim: usize,
}

/// Diagnostic info about an NDA BitNet layer.
#[derive(Debug, Clone, Serialize)]
pub struct NdaBitNetLayerInfo {
    pub config: NdaBitNetLayerConfig,
    pub nda_shader_count: usize,
    pub pipeline_count: usize,
    pub weight_buffers: usize,
    pub total_weight_bytes_estimate: usize,
    pub validation_issues: Vec<String>,
}

/// Validate NDA BitNet layer dimensions.
pub fn validate_nda_bitnet_config(cfg: &NdaBitNetLayerConfig) -> Vec<String> {
    let mut issues = Vec::new();
    if cfg.hidden_size == 0 {
        issues.push("hidden_size must be > 0".into());
    }
    if cfg.hidden_size % 128 != 0 {
        issues.push(format!(
            "hidden_size ({}) must be a multiple of 128 for NDA packing",
            cfg.hidden_size
        ));
    }
    if cfg.ffn_size == 0 {
        issues.push("ffn_size must be > 0".into());
    }
    if cfg.n_heads == 0 {
        issues.push("n_heads must be > 0".into());
    }
    if cfg.head_dim == 0 {
        issues.push("head_dim must be > 0".into());
    }
    issues
}

/// Build diagnostic info for an NDA BitNet layer.
pub fn nda_bitnet_layer_info(cfg: &NdaBitNetLayerConfig) -> NdaBitNetLayerInfo {
    let issues = validate_nda_bitnet_config(cfg);
    let weight_buffers = 7; // Q, K, V, O, gate, up, down (each has active + pos = 14 buffers)
    let weight_bytes = cfg.hidden_size * cfg.hidden_size * 4 * 4
        + cfg.hidden_size * cfg.ffn_size * 4 * 2
        + cfg.ffn_size * cfg.hidden_size * 4;
    NdaBitNetLayerInfo {
        config: cfg.clone(),
        nda_shader_count: 2,
        pipeline_count: 2,
        weight_buffers,
        total_weight_bytes_estimate: weight_bytes,
        validation_issues: issues,
    }
}

pub struct VulkanNdaBitNetLayer {
    pub device: Device,
    pub queue: vk::Queue,

    pub shader_nda: vk::ShaderModule,
    pub shader_act: vk::ShaderModule,

    pub desc_set_layout_nda: vk::DescriptorSetLayout,
    pub desc_set_layout_act: vk::DescriptorSetLayout,

    pub pipeline_layout_nda: vk::PipelineLayout,
    pub pipeline_layout_act: vk::PipelineLayout,

    pub pipeline_nda: vk::Pipeline,
    pub pipeline_act: vk::Pipeline,

    pub inputs_3200_active_buffer: vk::Buffer,
    pub inputs_3200_active_memory: vk::DeviceMemory,
    pub inputs_3200_active_ptr: *mut std::ffi::c_void,

    pub inputs_3200_pos_buffer: vk::Buffer,
    pub inputs_3200_pos_memory: vk::DeviceMemory,
    pub inputs_3200_pos_ptr: *mut std::ffi::c_void,

    pub out_3200_q_buffer: vk::Buffer,
    pub out_3200_q_memory: vk::DeviceMemory,

    pub out_3200_k_buffer: vk::Buffer,
    pub out_3200_k_memory: vk::DeviceMemory,

    pub out_3200_v_buffer: vk::Buffer,
    pub out_3200_v_memory: vk::DeviceMemory,

    pub out_3200_o_buffer: vk::Buffer,
    pub out_3200_o_memory: vk::DeviceMemory,

    pub out_8640_gate_buffer: vk::Buffer,
    pub out_8640_gate_memory: vk::DeviceMemory,

    pub out_8640_up_buffer: vk::Buffer,
    pub out_8640_up_memory: vk::DeviceMemory,

    pub inputs_8640_active_buffer: vk::Buffer,
    pub inputs_8640_active_memory: vk::DeviceMemory,

    pub inputs_8640_pos_buffer: vk::Buffer,
    pub inputs_8640_pos_memory: vk::DeviceMemory,

    pub out_3200_down_buffer: vk::Buffer,
    pub out_3200_down_memory: vk::DeviceMemory,
    pub out_3200_down_ptr: *mut std::ffi::c_void,

    pub weight_q_active_buffer: vk::Buffer,
    pub weight_q_active_memory: vk::DeviceMemory,
    pub weight_q_pos_buffer: vk::Buffer,
    pub weight_q_pos_memory: vk::DeviceMemory,

    pub weight_k_active_buffer: vk::Buffer,
    pub weight_k_active_memory: vk::DeviceMemory,
    pub weight_k_pos_buffer: vk::Buffer,
    pub weight_k_pos_memory: vk::DeviceMemory,

    pub weight_v_active_buffer: vk::Buffer,
    pub weight_v_active_memory: vk::DeviceMemory,
    pub weight_v_pos_buffer: vk::Buffer,
    pub weight_v_pos_memory: vk::DeviceMemory,

    pub weight_o_active_buffer: vk::Buffer,
    pub weight_o_active_memory: vk::DeviceMemory,
    pub weight_o_pos_buffer: vk::Buffer,
    pub weight_o_pos_memory: vk::DeviceMemory,

    pub weight_gate_active_buffer: vk::Buffer,
    pub weight_gate_active_memory: vk::DeviceMemory,
    pub weight_gate_pos_buffer: vk::Buffer,
    pub weight_gate_pos_memory: vk::DeviceMemory,

    pub weight_up_active_buffer: vk::Buffer,
    pub weight_up_active_memory: vk::DeviceMemory,
    pub weight_up_pos_buffer: vk::Buffer,
    pub weight_up_pos_memory: vk::DeviceMemory,

    pub weight_down_active_buffer: vk::Buffer,
    pub weight_down_active_memory: vk::DeviceMemory,
    pub weight_down_pos_buffer: vk::Buffer,
    pub weight_down_pos_memory: vk::DeviceMemory,

    pub desc_pool: vk::DescriptorPool,
    pub desc_sets_nda: Vec<vk::DescriptorSet>,
    pub desc_set_act: vk::DescriptorSet,

    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
}

impl VulkanNdaBitNetLayer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        driver: &VulkanDriver,
        weight_q: &[u8],
        weight_k: &[u8],
        weight_v: &[u8],
        weight_o: &[u8],
        weight_gate: &[u8],
        weight_up: &[u8],
        weight_down: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = driver.device.clone();
        let physical_device = driver.physical_device;
        let instance = driver.instance.clone();
        let queue = driver.compute_queue;

        let shader_info_nda =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::NDA_SPV);
        // SAFETY: create_shader_module with valid NDA SPIR-V bytecode.
        let shader_nda = unsafe { device.create_shader_module(&shader_info_nda, None)? };

        let shader_info_act =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ACT_NDA_SPV);
        // SAFETY: create_shader_module with valid activation SPIR-V bytecode.
        let shader_act = unsafe { device.create_shader_module(&shader_info_act, None)? };

        let bindings_nda = [
            vk::DescriptorSetLayoutBinding::builder()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(4)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
        ];
        let layout_info_nda = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_nda);
        // SAFETY: create_descriptor_set_layout for NDA compute bindings (5 storage buffers).
        let desc_set_layout_nda =
            unsafe { device.create_descriptor_set_layout(&layout_info_nda, None)? };

        let bindings_act = [
            vk::DescriptorSetLayoutBinding::builder()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .build(),
        ];
        let layout_info_act = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_act);
        // SAFETY: create_descriptor_set_layout for activation function bindings (4 storage buffers).
        let desc_set_layout_act =
            unsafe { device.create_descriptor_set_layout(&layout_info_act, None)? };

        let push_constant_ranges = [vk::PushConstantRange::builder()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(8)
            .build()];

        let layouts_nda = [desc_set_layout_nda];
        let pipeline_layout_info_nda = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&layouts_nda)
            .push_constant_ranges(&push_constant_ranges);
        // SAFETY: create_pipeline_layout for NDA and activation pipelines (8-byte push constants).
        let pipeline_layout_nda =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info_nda, None)? };

        let layouts_act = [desc_set_layout_act];
        let pipeline_layout_info_act = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&layouts_act)
            .push_constant_ranges(&push_constant_ranges);
        // SAFETY: Create pipeline layout for activation shader with desc layout and push constants.
        let pipeline_layout_act =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info_act, None)? };

        let main_entry = CString::new("main")?;

        let stage_info_nda = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_nda)
            .name(&main_entry);
        let pipeline_create_info_nda = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_nda.build())
            .layout(pipeline_layout_nda);

        let stage_info_act = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_act)
            .name(&main_entry);
        let pipeline_create_info_act = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_act.build())
            .layout(pipeline_layout_act);

        // SAFETY: create_compute_pipelines for NDA and activation pipelines with valid shaders.
        let pipelines_nda = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[pipeline_create_info_nda.build()],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let pipeline_nda = pipelines_nda[0];

        // SAFETY: create_compute_pipelines for the activation pipeline.
        let pipelines_act = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[pipeline_create_info_act.build()],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let pipeline_act = pipelines_act[0];

        let in_3200_bytes = (3200 / 32) * 4;
        let (inputs_3200_active_buffer, inputs_3200_active_memory, inputs_3200_active_ptr) =
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                in_3200_bytes as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?;
        let (inputs_3200_pos_buffer, inputs_3200_pos_memory, inputs_3200_pos_ptr) =
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                in_3200_bytes as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?;

        let out_3200_bytes = 3200 * 4;
        let (out_3200_q_buffer, out_3200_q_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_3200_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_3200_k_buffer, out_3200_k_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_3200_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_3200_v_buffer, out_3200_v_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_3200_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_3200_o_buffer, out_3200_o_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_3200_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let out_8640_bytes = 8640 * 4;
        let (out_8640_gate_buffer, out_8640_gate_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_8640_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_8640_up_buffer, out_8640_up_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            out_8640_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let in_8640_bytes = (8640 / 32) * 4;
        let (inputs_8640_active_buffer, inputs_8640_active_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            in_8640_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (inputs_8640_pos_buffer, inputs_8640_pos_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            in_8640_bytes as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let (out_3200_down_buffer, out_3200_down_memory, out_3200_down_ptr) =
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                out_3200_bytes as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?;

        let pack = |w: &[u8], k, n| pack_weights_nda(w, k, n);

        let (wq_a, wq_p) = pack(weight_q, 3200, 3200);
        let (weight_q_active_buffer, weight_q_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wq_a.len() as vk::DeviceSize,
            &wq_a,
        )?;
        let (weight_q_pos_buffer, weight_q_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wq_p.len() as vk::DeviceSize,
            &wq_p,
        )?;

        let (wk_a, wk_p) = pack(weight_k, 3200, 3200);
        let (weight_k_active_buffer, weight_k_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wk_a.len() as vk::DeviceSize,
            &wk_a,
        )?;
        let (weight_k_pos_buffer, weight_k_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wk_p.len() as vk::DeviceSize,
            &wk_p,
        )?;

        let (wv_a, wv_p) = pack(weight_v, 3200, 3200);
        let (weight_v_active_buffer, weight_v_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wv_a.len() as vk::DeviceSize,
            &wv_a,
        )?;
        let (weight_v_pos_buffer, weight_v_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wv_p.len() as vk::DeviceSize,
            &wv_p,
        )?;

        let (wo_a, wo_p) = pack(weight_o, 3200, 3200);
        let (weight_o_active_buffer, weight_o_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wo_a.len() as vk::DeviceSize,
            &wo_a,
        )?;
        let (weight_o_pos_buffer, weight_o_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wo_p.len() as vk::DeviceSize,
            &wo_p,
        )?;

        let (wgate_a, wgate_p) = pack(weight_gate, 3200, 8640);
        let (weight_gate_active_buffer, weight_gate_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wgate_a.len() as vk::DeviceSize,
            &wgate_a,
        )?;
        let (weight_gate_pos_buffer, weight_gate_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wgate_p.len() as vk::DeviceSize,
            &wgate_p,
        )?;

        let (wup_a, wup_p) = pack(weight_up, 3200, 8640);
        let (weight_up_active_buffer, weight_up_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wup_a.len() as vk::DeviceSize,
            &wup_a,
        )?;
        let (weight_up_pos_buffer, weight_up_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wup_p.len() as vk::DeviceSize,
            &wup_p,
        )?;

        let (wdown_a, wdown_p) = pack(weight_down, 8640, 3200);
        let (weight_down_active_buffer, weight_down_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wdown_a.len() as vk::DeviceSize,
            &wdown_a,
        )?;
        let (weight_down_pos_buffer, weight_down_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            wdown_p.len() as vk::DeviceSize,
            &wdown_p,
        )?;

        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(40)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(8)
            .pool_sizes(&pool_sizes);
        // SAFETY: create_descriptor_pool with capacity for 8 sets of 40 storage buffers.
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let layouts_nda = vec![desc_set_layout_nda; 7];
        // SAFETY: allocate_descriptor_sets allocates 7 NDA sets + 1 activation set from pool.
        let desc_sets_nda = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&layouts_nda),
            )?
        };

        // SAFETY: allocate one activation descriptor set from the pool.
        let desc_set_act = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&[desc_set_layout_act]),
            )?[0]
        };

        let set_configs_nda = [
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_q_active_buffer,
                weight_q_pos_buffer,
                out_3200_q_buffer,
            ),
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_k_active_buffer,
                weight_k_pos_buffer,
                out_3200_k_buffer,
            ),
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_v_active_buffer,
                weight_v_pos_buffer,
                out_3200_v_buffer,
            ),
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_o_active_buffer,
                weight_o_pos_buffer,
                out_3200_o_buffer,
            ),
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_gate_active_buffer,
                weight_gate_pos_buffer,
                out_8640_gate_buffer,
            ),
            (
                inputs_3200_active_buffer,
                inputs_3200_pos_buffer,
                weight_up_active_buffer,
                weight_up_pos_buffer,
                out_8640_up_buffer,
            ),
            (
                inputs_8640_active_buffer,
                inputs_8640_pos_buffer,
                weight_down_active_buffer,
                weight_down_pos_buffer,
                out_3200_down_buffer,
            ),
        ];

        for (i, (in_a, in_p, w_a, w_p, out_b)) in set_configs_nda.iter().enumerate() {
            let buffer_infos = [
                vk::DescriptorBufferInfo::builder()
                    .buffer(*in_a)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*in_p)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*w_a)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*w_p)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*out_b)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
            ];
            let writes = [
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets_nda[i])
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[0..1])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets_nda[i])
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[1..2])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets_nda[i])
                    .dst_binding(2)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[2..3])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets_nda[i])
                    .dst_binding(3)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[3..4])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets_nda[i])
                    .dst_binding(4)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[4..5])
                    .build(),
            ];
            // SAFETY: update_descriptor_sets binds buffer info to NDA descriptor sets.
            unsafe { device.update_descriptor_sets(&writes, &[]) };
        }

        let buffer_infos_act = [
            vk::DescriptorBufferInfo::builder()
                .buffer(out_8640_gate_buffer)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(out_8640_up_buffer)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(inputs_8640_active_buffer)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(inputs_8640_pos_buffer)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build(),
        ];
        let writes_act = [
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_act)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_act[0..1])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_act)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_act[1..2])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_act)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_act[2..3])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set_act)
                .dst_binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos_act[3..4])
                .build(),
        ];
        // SAFETY: update_descriptor_sets binds buffer info to activation descriptor set.
        unsafe { device.update_descriptor_sets(&writes_act, &[]) };

        let pool_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(driver.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        // SAFETY: create_command_pool for the compute queue family.
        let command_pool = unsafe { device.create_command_pool(&pool_info, None)? };

        let alloc_info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: allocate_command_buffers allocates one primary command buffer.
        let command_buffers = unsafe { device.allocate_command_buffers(&alloc_info)? };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        // SAFETY: Record all NDA dispatches into command buffer: begin, bind, dispatch, end.
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;

            let dispatch_nda = |cmd: vk::CommandBuffer, set: vk::DescriptorSet, k: u32, n: u32| {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline_nda);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline_layout_nda,
                    0,
                    &[set],
                    &[],
                );
                let params = [k, n];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(
                    cmd,
                    pipeline_layout_nda,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    params_bytes,
                );
                let workgroups = n.div_ceil(256u32);
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            let dispatch_act = |cmd: vk::CommandBuffer, set: vk::DescriptorSet, n: u32| {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline_act);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline_layout_act,
                    0,
                    &[set],
                    &[],
                );
                let params = [n, 0u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(
                    cmd,
                    pipeline_layout_act,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    params_bytes,
                );
                let workgroups = n.div_ceil(256u32);
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            dispatch_nda(command_buffer, desc_sets_nda[0], 3200, 3200);
            dispatch_nda(command_buffer, desc_sets_nda[1], 3200, 3200);
            dispatch_nda(command_buffer, desc_sets_nda[2], 3200, 3200);
            dispatch_nda(command_buffer, desc_sets_nda[3], 3200, 3200);
            dispatch_nda(command_buffer, desc_sets_nda[4], 3200, 8640);
            dispatch_nda(command_buffer, desc_sets_nda[5], 3200, 8640);
            dispatch_act(command_buffer, desc_set_act, 8640);
            dispatch_nda(command_buffer, desc_sets_nda[6], 8640, 3200);

            device.end_command_buffer(command_buffer)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        // SAFETY: Create fence for NDA layer GPU synchronization.
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            shader_nda,
            shader_act,
            desc_set_layout_nda,
            desc_set_layout_act,
            pipeline_layout_nda,
            pipeline_layout_act,
            pipeline_nda,
            pipeline_act,
            inputs_3200_active_buffer,
            inputs_3200_active_memory,
            inputs_3200_active_ptr,
            inputs_3200_pos_buffer,
            inputs_3200_pos_memory,
            inputs_3200_pos_ptr,
            out_3200_q_buffer,
            out_3200_q_memory,
            out_3200_k_buffer,
            out_3200_k_memory,
            out_3200_v_buffer,
            out_3200_v_memory,
            out_3200_o_buffer,
            out_3200_o_memory,
            out_8640_gate_buffer,
            out_8640_gate_memory,
            out_8640_up_buffer,
            out_8640_up_memory,
            inputs_8640_active_buffer,
            inputs_8640_active_memory,
            inputs_8640_pos_buffer,
            inputs_8640_pos_memory,
            out_3200_down_buffer,
            out_3200_down_memory,
            out_3200_down_ptr,
            weight_q_active_buffer,
            weight_q_active_memory,
            weight_q_pos_buffer,
            weight_q_pos_memory,
            weight_k_active_buffer,
            weight_k_active_memory,
            weight_k_pos_buffer,
            weight_k_pos_memory,
            weight_v_active_buffer,
            weight_v_active_memory,
            weight_v_pos_buffer,
            weight_v_pos_memory,
            weight_o_active_buffer,
            weight_o_active_memory,
            weight_o_pos_buffer,
            weight_o_pos_memory,
            weight_gate_active_buffer,
            weight_gate_active_memory,
            weight_gate_pos_buffer,
            weight_gate_pos_memory,
            weight_up_active_buffer,
            weight_up_active_memory,
            weight_up_pos_buffer,
            weight_up_pos_memory,
            weight_down_active_buffer,
            weight_down_active_memory,
            weight_down_pos_buffer,
            weight_down_pos_memory,
            desc_pool,
            desc_sets_nda,
            desc_set_act,
            command_pool,
            command_buffer,
            fence,
        })
    }

    pub fn run(
        &self,
        inputs_active_bytes: &[u8],
        inputs_pos_bytes: &[u8],
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        // SAFETY: Copy packed input bytes into HOST_VISIBLE mapped buffers.
        // `inputs_3200_active_ptr` and `inputs_3200_pos_ptr` are valid from create_coherent_buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(
                inputs_active_bytes.as_ptr(),
                self.inputs_3200_active_ptr as *mut u8,
                inputs_active_bytes.len(),
            );
            std::ptr::copy_nonoverlapping(
                inputs_pos_bytes.as_ptr(),
                self.inputs_3200_pos_ptr as *mut u8,
                inputs_pos_bytes.len(),
            );
        }

        let start = Instant::now();
        let command_buffers = [self.command_buffer];
        let submit_info = vk::SubmitInfo::builder().command_buffers(&command_buffers);
        // SAFETY: Reset fence, submit NDA dispatch, wait for completion.
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device
                .queue_submit(self.queue, &[submit_info.build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        // SAFETY: Copy output floats from GPU-mapped buffer to caller slice.
        // `out_3200_down_ptr` is a valid mapped pointer from create_coherent_buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.out_3200_down_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }
}

impl Drop for VulkanNdaBitNetLayer {
    fn drop(&mut self) {
        // SAFETY: Vulkan resource teardown in correct dependency order:
        // device_wait_idle → fence → command_pool → descriptor_pool → all buffers/memory →
        // pipelines → pipeline layouts → descriptor set layouts → shader modules.
        // All handles are valid and owned by this struct.
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);

            self.device.unmap_memory(self.inputs_3200_active_memory);
            self.device
                .free_memory(self.inputs_3200_active_memory, None);
            self.device
                .destroy_buffer(self.inputs_3200_active_buffer, None);

            self.device.unmap_memory(self.inputs_3200_pos_memory);
            self.device.free_memory(self.inputs_3200_pos_memory, None);
            self.device
                .destroy_buffer(self.inputs_3200_pos_buffer, None);

            self.device.unmap_memory(self.out_3200_down_memory);
            self.device.free_memory(self.out_3200_down_memory, None);
            self.device.destroy_buffer(self.out_3200_down_buffer, None);

            self.device.free_memory(self.out_3200_q_memory, None);
            self.device.destroy_buffer(self.out_3200_q_buffer, None);

            self.device.free_memory(self.out_3200_k_memory, None);
            self.device.destroy_buffer(self.out_3200_k_buffer, None);

            self.device.free_memory(self.out_3200_v_memory, None);
            self.device.destroy_buffer(self.out_3200_v_buffer, None);

            self.device.free_memory(self.out_3200_o_memory, None);
            self.device.destroy_buffer(self.out_3200_o_buffer, None);

            self.device.free_memory(self.out_8640_gate_memory, None);
            self.device.destroy_buffer(self.out_8640_gate_buffer, None);

            self.device.free_memory(self.out_8640_up_memory, None);
            self.device.destroy_buffer(self.out_8640_up_buffer, None);

            self.device
                .free_memory(self.inputs_8640_active_memory, None);
            self.device
                .destroy_buffer(self.inputs_8640_active_buffer, None);

            self.device.free_memory(self.inputs_8640_pos_memory, None);
            self.device
                .destroy_buffer(self.inputs_8640_pos_buffer, None);

            self.device.free_memory(self.weight_q_active_memory, None);
            self.device
                .destroy_buffer(self.weight_q_active_buffer, None);
            self.device.free_memory(self.weight_q_pos_memory, None);
            self.device.destroy_buffer(self.weight_q_pos_buffer, None);

            self.device.free_memory(self.weight_k_active_memory, None);
            self.device
                .destroy_buffer(self.weight_k_active_buffer, None);
            self.device.free_memory(self.weight_k_pos_memory, None);
            self.device.destroy_buffer(self.weight_k_pos_buffer, None);

            self.device.free_memory(self.weight_v_active_memory, None);
            self.device
                .destroy_buffer(self.weight_v_active_buffer, None);
            self.device.free_memory(self.weight_v_pos_memory, None);
            self.device.destroy_buffer(self.weight_v_pos_buffer, None);

            self.device.free_memory(self.weight_o_active_memory, None);
            self.device
                .destroy_buffer(self.weight_o_active_buffer, None);
            self.device.free_memory(self.weight_o_pos_memory, None);
            self.device.destroy_buffer(self.weight_o_pos_buffer, None);

            self.device
                .free_memory(self.weight_gate_active_memory, None);
            self.device
                .destroy_buffer(self.weight_gate_active_buffer, None);
            self.device.free_memory(self.weight_gate_pos_memory, None);
            self.device
                .destroy_buffer(self.weight_gate_pos_buffer, None);

            self.device.free_memory(self.weight_up_active_memory, None);
            self.device
                .destroy_buffer(self.weight_up_active_buffer, None);
            self.device.free_memory(self.weight_up_pos_memory, None);
            self.device.destroy_buffer(self.weight_up_pos_buffer, None);

            self.device
                .free_memory(self.weight_down_active_memory, None);
            self.device
                .destroy_buffer(self.weight_down_active_buffer, None);
            self.device.free_memory(self.weight_down_pos_memory, None);
            self.device
                .destroy_buffer(self.weight_down_pos_buffer, None);

            self.device.destroy_pipeline(self.pipeline_nda, None);
            self.device.destroy_pipeline(self.pipeline_act, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout_nda, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout_act, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_set_layout_nda, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_set_layout_act, None);

            self.device.destroy_shader_module(self.shader_nda, None);
            self.device.destroy_shader_module(self.shader_act, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_nda_bitnet_config() -> NdaBitNetLayerConfig {
        NdaBitNetLayerConfig {
            hidden_size: 3200,
            ffn_size: 8640,
            n_heads: 50,
            head_dim: 64,
        }
    }

    #[test]
    fn validate_nda_bitnet_valid() {
        // 3200 is not multiple of 128, so this should have issues
        let cfg = default_nda_bitnet_config();
        let issues = validate_nda_bitnet_config(&cfg);
        // 3200 % 128 = 0, so it's valid
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn validate_nda_bitnet_zero_hidden() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 0;
        assert!(validate_nda_bitnet_config(&cfg).iter().any(|i| i.contains("hidden_size")));
    }

    #[test]
    fn validate_nda_bitnet_bad_hidden() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 256; // 256 % 128 == 0, valid
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
        cfg.hidden_size = 300; // not multiple of 128
        assert!(validate_nda_bitnet_config(&cfg).iter().any(|i| i.contains("multiple of 128")));
    }

    #[test]
    fn validate_nda_bitnet_zero_ffn() {
        let mut cfg = default_nda_bitnet_config();
        cfg.ffn_size = 0;
        assert!(validate_nda_bitnet_config(&cfg).iter().any(|i| i.contains("ffn_size")));
    }

    #[test]
    fn nda_bitnet_layer_info_works() {
        let cfg = default_nda_bitnet_config();
        let info = nda_bitnet_layer_info(&cfg);
        assert!(info.validation_issues.is_empty());
        assert_eq!(info.weight_buffers, 7);
        assert_eq!(info.nda_shader_count, 2);
        assert!(info.total_weight_bytes_estimate > 0);
    }

    #[test]
    fn nda_bitnet_layer_info_with_issues() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 0;
        cfg.ffn_size = 0;
        let info = nda_bitnet_layer_info(&cfg);
        assert!(info.validation_issues.len() >= 2);
    }

    #[test]
    fn nda_bitnet_config_serializes() {
        let json = serde_json::to_string(&default_nda_bitnet_config()).unwrap();
        assert!(json.contains("hidden_size"));
        assert!(json.contains("3200"));
    }

    #[test]
    fn nda_bitnet_layer_info_serializes() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("nda_shader_count"));
        assert!(json.contains("weight_buffers"));
    }

    // ── Validation: individual fields ────────────────────────────────────

    #[test]
    fn validate_zero_n_heads() {
        let mut cfg = default_nda_bitnet_config();
        cfg.n_heads = 0;
        assert!(validate_nda_bitnet_config(&cfg).iter().any(|i| i.contains("n_heads")));
    }

    #[test]
    fn validate_zero_head_dim() {
        let mut cfg = default_nda_bitnet_config();
        cfg.head_dim = 0;
        assert!(validate_nda_bitnet_config(&cfg).iter().any(|i| i.contains("head_dim")));
    }

    #[test]
    fn validate_hidden_128_valid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 128;
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
    }

    #[test]
    fn validate_hidden_256_valid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 256;
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
    }

    #[test]
    fn validate_hidden_1024_valid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 1024;
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
    }

    #[test]
    fn validate_hidden_1_invalid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 1;
        let issues = validate_nda_bitnet_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("multiple of 128")));
    }

    #[test]
    fn validate_hidden_64_invalid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 64;
        let issues = validate_nda_bitnet_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("multiple of 128")));
    }

    #[test]
    fn validate_zero_hidden_only_one_issue() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 0;
        let issues = validate_nda_bitnet_config(&cfg);
        // 0 % 128 == 0, so only "hidden_size must be > 0" triggers
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0], "hidden_size must be > 0");
    }

    // ── Validation: multiple issues ──────────────────────────────────────

    #[test]
    fn validate_all_zeros() {
        let cfg = NdaBitNetLayerConfig {
            hidden_size: 0,
            ffn_size: 0,
            n_heads: 0,
            head_dim: 0,
        };
        let issues = validate_nda_bitnet_config(&cfg);
        // hidden_size=0: "hidden_size must be > 0" (0%128==0, no modulo issue)
        // ffn_size=0: "ffn_size must be > 0"
        // n_heads=0: "n_heads must be > 0"
        // head_dim=0: "head_dim must be > 0"
        assert_eq!(issues.len(), 4);
    }

    #[test]
    fn validate_bad_hidden_plus_zero_ffn() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 100;
        cfg.ffn_size = 0;
        let issues = validate_nda_bitnet_config(&cfg);
        assert_eq!(issues.len(), 2);
        assert!(issues.iter().any(|i| i.contains("multiple of 128")));
        assert!(issues.iter().any(|i| i.contains("ffn_size")));
    }

    #[test]
    fn validate_issues_order_deterministic() {
        let cfg = NdaBitNetLayerConfig {
            hidden_size: 0,
            ffn_size: 0,
            n_heads: 0,
            head_dim: 0,
        };
        let i1 = validate_nda_bitnet_config(&cfg);
        let i2 = validate_nda_bitnet_config(&cfg);
        assert_eq!(i1, i2);
    }

    // ── Validation issue text ────────────────────────────────────────────

    #[test]
    fn validate_hidden_zero_issue_text() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 0;
        assert_eq!(validate_nda_bitnet_config(&cfg)[0], "hidden_size must be > 0");
    }

    #[test]
    fn validate_bad_hidden_includes_value() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 300;
        let issues = validate_nda_bitnet_config(&cfg);
        assert!(issues[0].contains("300"));
    }

    #[test]
    fn validate_ffn_zero_issue_text() {
        let mut cfg = default_nda_bitnet_config();
        cfg.ffn_size = 0;
        assert_eq!(validate_nda_bitnet_config(&cfg)[0], "ffn_size must be > 0");
    }

    #[test]
    fn validate_n_heads_zero_issue_text() {
        let mut cfg = default_nda_bitnet_config();
        cfg.n_heads = 0;
        assert_eq!(validate_nda_bitnet_config(&cfg)[0], "n_heads must be > 0");
    }

    #[test]
    fn validate_head_dim_zero_issue_text() {
        let mut cfg = default_nda_bitnet_config();
        cfg.head_dim = 0;
        assert_eq!(validate_nda_bitnet_config(&cfg)[0], "head_dim must be > 0");
    }

    // ── Info calculations ────────────────────────────────────────────────

    #[test]
    fn info_weight_bytes_formula() {
        let cfg = default_nda_bitnet_config();
        let info = nda_bitnet_layer_info(&cfg);
        let expected = cfg.hidden_size * cfg.hidden_size * 4 * 4
            + cfg.hidden_size * cfg.ffn_size * 4 * 2
            + cfg.ffn_size * cfg.hidden_size * 4;
        assert_eq!(info.total_weight_bytes_estimate, expected);
    }

    #[test]
    fn info_shader_count_is_2() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        assert_eq!(info.nda_shader_count, 2);
    }

    #[test]
    fn info_pipeline_count_is_2() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        assert_eq!(info.pipeline_count, 2);
    }

    #[test]
    fn info_weight_buffers_is_7() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        assert_eq!(info.weight_buffers, 7);
    }

    #[test]
    fn info_preserves_config() {
        let cfg = default_nda_bitnet_config();
        let info = nda_bitnet_layer_info(&cfg);
        assert_eq!(info.config.hidden_size, cfg.hidden_size);
        assert_eq!(info.config.ffn_size, cfg.ffn_size);
        assert_eq!(info.config.n_heads, cfg.n_heads);
        assert_eq!(info.config.head_dim, cfg.head_dim);
    }

    #[test]
    fn info_minimal_config() {
        let cfg = NdaBitNetLayerConfig {
            hidden_size: 128,
            ffn_size: 1,
            n_heads: 1,
            head_dim: 1,
        };
        let info = nda_bitnet_layer_info(&cfg);
        assert!(info.validation_issues.is_empty());
        assert!(info.total_weight_bytes_estimate > 0);
    }

    #[test]
    fn info_large_config() {
        let cfg = NdaBitNetLayerConfig {
            hidden_size: 4096,
            ffn_size: 11008,
            n_heads: 32,
            head_dim: 128,
        };
        let info = nda_bitnet_layer_info(&cfg);
        assert!(info.validation_issues.is_empty());
        assert!(info.total_weight_bytes_estimate > 500_000_000);
    }

    #[test]
    fn info_with_invalid_config() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 0;
        let info = nda_bitnet_layer_info(&cfg);
        assert!(!info.validation_issues.is_empty());
        // weight_bytes = 0*0*16 + 0*8640*8 + 8640*0*4 = 0
        assert_eq!(info.total_weight_bytes_estimate, 0);
    }

    // ── Struct derives ───────────────────────────────────────────────────

    #[test]
    fn config_clone() {
        let cfg = default_nda_bitnet_config();
        let cloned = cfg.clone();
        assert_eq!(cloned.hidden_size, cfg.hidden_size);
        assert_eq!(cloned.ffn_size, cfg.ffn_size);
        assert_eq!(cloned.n_heads, cfg.n_heads);
        assert_eq!(cloned.head_dim, cfg.head_dim);
    }

    #[test]
    fn config_clone_independent() {
        let cfg = default_nda_bitnet_config();
        let mut cloned = cfg.clone();
        cloned.hidden_size = 999;
        assert_ne!(cfg.hidden_size, cloned.hidden_size);
    }

    #[test]
    fn config_debug_format() {
        let cfg = default_nda_bitnet_config();
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("NdaBitNetLayerConfig"));
        assert!(debug.contains("3200"));
        assert!(debug.contains("8640"));
    }

    #[test]
    fn info_clone() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let cloned = info.clone();
        assert_eq!(cloned.nda_shader_count, info.nda_shader_count);
        assert_eq!(cloned.pipeline_count, info.pipeline_count);
        assert_eq!(cloned.weight_buffers, info.weight_buffers);
        assert_eq!(cloned.total_weight_bytes_estimate, info.total_weight_bytes_estimate);
    }

    #[test]
    fn info_debug_format() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let debug = format!("{:?}", info);
        assert!(debug.contains("NdaBitNetLayerInfo"));
        assert!(debug.contains("nda_shader_count"));
        assert!(debug.contains("weight_buffers"));
    }

    // ── Serialization ────────────────────────────────────────────────────

    #[test]
    fn config_json_all_fields() {
        let cfg = default_nda_bitnet_config();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"hidden_size\""));
        assert!(json.contains("\"ffn_size\""));
        assert!(json.contains("\"n_heads\""));
        assert!(json.contains("\"head_dim\""));
    }

    #[test]
    fn info_json_all_fields() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("config"));
        assert!(json.contains("nda_shader_count"));
        assert!(json.contains("pipeline_count"));
        assert!(json.contains("weight_buffers"));
        assert!(json.contains("total_weight_bytes_estimate"));
        assert!(json.contains("validation_issues"));
    }

    #[test]
    fn info_json_parseable_as_value() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let json = serde_json::to_string(&info).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["nda_shader_count"], 2);
        assert_eq!(value["pipeline_count"], 2);
        assert_eq!(value["weight_buffers"], 7);
        assert!(value["validation_issues"].is_array());
    }

    #[test]
    fn info_pretty_json() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let pretty = serde_json::to_string_pretty(&info).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));
    }

    #[test]
    fn info_json_with_issues() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 0;
        let info = nda_bitnet_layer_info(&cfg);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("validation_issues"));
        assert!(json.contains("hidden_size"));
    }

    // ── Boundary values ──────────────────────────────────────────────────

    #[test]
    fn validate_n_heads_1_valid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.n_heads = 1;
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
    }

    #[test]
    fn validate_head_dim_1_valid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.head_dim = 1;
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
    }

    #[test]
    fn validate_ffn_1_valid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.ffn_size = 1;
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
    }

    #[test]
    fn validate_max_values() {
        let mut cfg = default_nda_bitnet_config();
        cfg.ffn_size = usize::MAX;
        cfg.n_heads = usize::MAX;
        cfg.head_dim = usize::MAX;
        // hidden_size=3200 is valid (multiple of 128)
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
    }

    // ── JSON key counts ──────────────────────────────────────────────────

    #[test]
    fn config_json_has_exactly_4_keys() {
        let json = serde_json::to_string(&default_nda_bitnet_config()).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 4);
    }

    #[test]
    fn info_json_has_exactly_6_keys() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 6);
    }

    // ── Weight bytes calculation specifics ───────────────────────────────

    #[test]
    fn info_weight_bytes_minimal() {
        let cfg = NdaBitNetLayerConfig {
            hidden_size: 128, ffn_size: 1, n_heads: 1, head_dim: 1,
        };
        let info = nda_bitnet_layer_info(&cfg);
        // attn: 128*128*16 = 262144
        // ffn gate+up: 128*1*8 = 1024
        // ffn down: 1*128*4 = 512
        // total = 263680
        assert_eq!(info.total_weight_bytes_estimate, 263680);
    }

    #[test]
    fn info_weight_bytes_hidden_256_ffn_64() {
        let cfg = NdaBitNetLayerConfig {
            hidden_size: 256, ffn_size: 64, n_heads: 4, head_dim: 64,
        };
        let info = nda_bitnet_layer_info(&cfg);
        // attn: 256*256*16 = 1048576
        // ffn gate+up: 256*64*8 = 131072
        // ffn down: 64*256*4 = 65536
        // total = 1245184
        assert_eq!(info.total_weight_bytes_estimate, 1245184);
    }

    #[test]
    fn info_weight_bytes_default_config() {
        let cfg = default_nda_bitnet_config();
        let info = nda_bitnet_layer_info(&cfg);
        // attn: 3200*3200*16 = 163840000
        // ffn gate+up: 3200*8640*8 = 221184000
        // ffn down: 8640*3200*4 = 110592000
        // total = 495616000
        assert_eq!(info.total_weight_bytes_estimate, 495616000);
    }

    #[test]
    fn info_weight_bytes_zero_ffn() {
        let mut cfg = default_nda_bitnet_config();
        cfg.ffn_size = 0;
        let info = nda_bitnet_layer_info(&cfg);
        // attn: 3200*3200*16 = 163840000
        // ffn: all zero
        assert_eq!(info.total_weight_bytes_estimate, 163840000);
    }

    // ── Info clone independence ──────────────────────────────────────────

    #[test]
    fn info_clone_independent_issues() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 100;
        let info = nda_bitnet_layer_info(&cfg);
        let mut cloned = info.clone();
        cloned.validation_issues.push("extra".to_string());
        assert_ne!(info.validation_issues.len(), cloned.validation_issues.len());
    }

    #[test]
    fn info_clone_independent_config() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let mut cloned = info.clone();
        cloned.config.hidden_size = 999;
        assert_ne!(info.config.hidden_size, cloned.config.hidden_size);
    }

    // ── Validation issues propagation ────────────────────────────────────

    #[test]
    fn info_issues_match_validate_function() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 0;
        cfg.ffn_size = 0;
        let info = nda_bitnet_layer_info(&cfg);
        let direct = validate_nda_bitnet_config(&cfg);
        assert_eq!(info.validation_issues, direct);
    }

    #[test]
    fn info_issues_count_all_zeros() {
        let cfg = NdaBitNetLayerConfig {
            hidden_size: 0, ffn_size: 0, n_heads: 0, head_dim: 0,
        };
        let info = nda_bitnet_layer_info(&cfg);
        assert_eq!(info.validation_issues.len(), 4);
    }

    // ── JSON value verification ──────────────────────────────────────────

    #[test]
    fn config_json_numeric_values() {
        let json = serde_json::to_string(&default_nda_bitnet_config()).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["hidden_size"], 3200);
        assert_eq!(val["ffn_size"], 8640);
        assert_eq!(val["n_heads"], 50);
        assert_eq!(val["head_dim"], 64);
    }

    #[test]
    fn info_json_nested_config() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["config"].is_object());
        assert_eq!(val["config"]["hidden_size"], 3200);
        assert_eq!(val["config"]["ffn_size"], 8640);
    }

    #[test]
    fn info_json_validation_issues_content() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 0;
        cfg.n_heads = 0;
        let info = nda_bitnet_layer_info(&cfg);
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let issues = val["validation_issues"].as_array().unwrap();
        assert_eq!(issues.len(), 2);
        assert!(issues[0].as_str().unwrap().contains("hidden_size"));
        assert!(issues[1].as_str().unwrap().contains("n_heads"));
    }

    #[test]
    fn info_json_weight_bytes_value() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["total_weight_bytes_estimate"], 495616000);
    }

    // ── Debug format details ─────────────────────────────────────────────

    #[test]
    fn config_debug_all_fields() {
        let cfg = default_nda_bitnet_config();
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("hidden_size: 3200"));
        assert!(debug.contains("ffn_size: 8640"));
        assert!(debug.contains("n_heads: 50"));
        assert!(debug.contains("head_dim: 64"));
    }

    #[test]
    fn info_debug_includes_all_fields() {
        let info = nda_bitnet_layer_info(&default_nda_bitnet_config());
        let debug = format!("{:?}", info);
        assert!(debug.contains("nda_shader_count: 2"));
        assert!(debug.contains("pipeline_count: 2"));
        assert!(debug.contains("weight_buffers: 7"));
        assert!(debug.contains("validation_issues"));
    }

    #[test]
    fn info_debug_with_issues() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 0;
        let info = nda_bitnet_layer_info(&cfg);
        let debug = format!("{:?}", info);
        assert!(debug.contains("hidden_size must"));
    }

    // ── Validation edge cases ────────────────────────────────────────────

    #[test]
    fn validate_hidden_127_invalid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 127;
        let issues = validate_nda_bitnet_config(&cfg);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("multiple of 128"));
    }

    #[test]
    fn validate_hidden_129_invalid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 129;
        let issues = validate_nda_bitnet_config(&cfg);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("multiple of 128"));
    }

    #[test]
    fn validate_hidden_4096_valid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.hidden_size = 4096;
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
    }

    #[test]
    fn validate_multiple_calls_same_result() {
        let cfg = default_nda_bitnet_config();
        for _ in 0..10 {
            assert!(validate_nda_bitnet_config(&cfg).is_empty());
        }
    }

    #[test]
    fn validate_n_heads_max_valid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.n_heads = usize::MAX;
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
    }

    #[test]
    fn validate_head_dim_max_valid() {
        let mut cfg = default_nda_bitnet_config();
        cfg.head_dim = usize::MAX;
        assert!(validate_nda_bitnet_config(&cfg).is_empty());
    }

    // ── Scaling behavior ─────────────────────────────────────────────────

    #[test]
    fn info_weight_scales_quadratically_with_hidden() {
        let cfg1 = NdaBitNetLayerConfig { hidden_size: 128, ffn_size: 0, n_heads: 1, head_dim: 1 };
        let cfg2 = NdaBitNetLayerConfig { hidden_size: 256, ffn_size: 0, n_heads: 1, head_dim: 1 };
        let info1 = nda_bitnet_layer_info(&cfg1);
        let info2 = nda_bitnet_layer_info(&cfg2);
        // With ffn=0, weight = hidden*hidden*16, so doubling hidden → 4x weight
        assert_eq!(info2.total_weight_bytes_estimate, info1.total_weight_bytes_estimate * 4);
    }

    #[test]
    fn info_weight_scales_linearly_with_ffn() {
        let cfg1 = NdaBitNetLayerConfig { hidden_size: 128, ffn_size: 100, n_heads: 1, head_dim: 1 };
        let cfg2 = NdaBitNetLayerConfig { hidden_size: 128, ffn_size: 200, n_heads: 1, head_dim: 1 };
        let info1 = nda_bitnet_layer_info(&cfg1);
        let info2 = nda_bitnet_layer_info(&cfg2);
        // FFN part: hidden*ffn*8 + ffn*hidden*4 = hidden*ffn*12
        // Doubling ffn → FFN part doubles
        let ffn1 = cfg1.hidden_size * cfg1.ffn_size * 12;
        let ffn2 = cfg2.hidden_size * cfg2.ffn_size * 12;
        assert_eq!(ffn2, ffn1 * 2);
        // Total difference should equal FFN difference
        assert_eq!(
            info2.total_weight_bytes_estimate - info1.total_weight_bytes_estimate,
            ffn2 - ffn1
        );
    }

    // ── JSON roundtrip via Value ─────────────────────────────────────────

    #[test]
    fn config_json_roundtrip_via_value() {
        let cfg = NdaBitNetLayerConfig {
            hidden_size: 256, ffn_size: 512, n_heads: 8, head_dim: 32,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["hidden_size"], 256);
        assert_eq!(val["ffn_size"], 512);
        assert_eq!(val["n_heads"], 8);
        assert_eq!(val["head_dim"], 32);
    }

    #[test]
    fn info_json_roundtrip_via_value() {
        let cfg = NdaBitNetLayerConfig {
            hidden_size: 256, ffn_size: 512, n_heads: 8, head_dim: 32,
        };
        let info = nda_bitnet_layer_info(&cfg);
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["config"]["hidden_size"], 256);
        assert_eq!(val["nda_shader_count"], 2);
        assert_eq!(val["pipeline_count"], 2);
        assert_eq!(val["weight_buffers"], 7);
        assert!(val["validation_issues"].as_array().unwrap().is_empty());
    }
}
