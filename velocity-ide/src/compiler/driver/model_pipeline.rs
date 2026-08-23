//! Vulkan model pipeline: initializes GPU resources for transformer model inference.
//!
//! # Safety Invariants
//!
//! All `unsafe` blocks wrap Vulkan API calls via `ash`. This module creates and manages
//! Vulkan buffers, descriptor pools, pipelines, and per-layer GEMV dispatchers.
//! All handles are derived from a valid `VulkanDriver`. The `Drop` impl cleans up
//! all allocated resources.

use super::layer_gpu_gemvs::LayerGpuGemvs;
use super::vulkan_init::*;
use ash::vk;
use ash::Device;
use serde::Serialize;

/// Model dimensions used to set up the Vulkan pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct ModelPipelineConfig {
    pub n_layers: usize,
    pub hidden_size: usize,
    pub ffn_size: usize,
    pub n_heads: usize,
    pub n_kv_heads: usize,
    pub head_dim: usize,
    pub max_seq_len: usize,
    pub vocab_size: usize,
}

/// Diagnostic info about a model pipeline setup.
#[derive(Debug, Clone, Serialize)]
pub struct ModelPipelineInfo {
    pub config: ModelPipelineConfig,
    pub shader_count: usize,
    pub pipeline_count: usize,
    pub buffer_count: usize,
    pub per_layer_descriptor_sets: usize,
    pub total_descriptor_sets_estimate: usize,
    pub validation_issues: Vec<String>,
}

/// Validate model pipeline configuration.
pub fn validate_model_pipeline_config(cfg: &ModelPipelineConfig) -> Vec<String> {
    let mut issues = Vec::new();
    if cfg.n_layers == 0 {
        issues.push("n_layers must be > 0".into());
    }
    if cfg.hidden_size == 0 {
        issues.push("hidden_size must be > 0".into());
    }
    if cfg.hidden_size % 4 != 0 {
        issues.push(format!(
            "hidden_size ({}) must be a multiple of 4 (float buffer alignment)",
            cfg.hidden_size
        ));
    }
    if cfg.ffn_size == 0 {
        issues.push("ffn_size must be > 0".into());
    }
    if cfg.n_heads == 0 {
        issues.push("n_heads must be > 0".into());
    }
    if cfg.n_kv_heads == 0 {
        issues.push("n_kv_heads must be > 0".into());
    }
    if cfg.head_dim == 0 {
        issues.push("head_dim must be > 0".into());
    }
    if cfg.max_seq_len == 0 {
        issues.push("max_seq_len must be > 0".into());
    }
    if cfg.vocab_size == 0 {
        issues.push("vocab_size must be > 0".into());
    }
    if cfg.n_heads % cfg.n_kv_heads != 0 && cfg.n_kv_heads != 0 && cfg.n_heads != 0 {
        issues.push(format!(
            "n_heads ({}) must be divisible by n_kv_heads ({})",
            cfg.n_heads, cfg.n_kv_heads
        ));
    }
    issues
}

/// Build diagnostic info for a model pipeline configuration.
pub fn model_pipeline_info(cfg: &ModelPipelineConfig) -> ModelPipelineInfo {
    let issues = validate_model_pipeline_config(cfg);
    let shader_count = 7; // rms_norm, rope, kv_write, attn_softmax, swiglu, residual_add, bias_add
    let pipeline_count = 7;
    let buffer_count = 5; // x_residual, attn_out, gated, lm_head, x_final
    let per_layer_descriptor_sets = 10; // rms_norm_attn, rms_norm_ffn, rope, kv_write, attn_softmax, residual_add_attn, residual_add_ffn, swiglu, bias_q/k/v
    let total_desc = per_layer_descriptor_sets * cfg.n_layers + 1; // +1 for final norm
    ModelPipelineInfo {
        config: cfg.clone(),
        shader_count,
        pipeline_count,
        buffer_count,
        per_layer_descriptor_sets,
        total_descriptor_sets_estimate: total_desc,
        validation_issues: issues,
    }
}

pub struct VulkanModelPipeline {
    pub device: Device,
    pub queue: vk::Queue,

    pub shader_rms_norm: vk::ShaderModule,
    pub shader_rope: vk::ShaderModule,
    pub shader_kv_write: vk::ShaderModule,
    pub shader_attn_softmax: vk::ShaderModule,
    pub shader_swiglu: vk::ShaderModule,
    pub shader_residual_add: vk::ShaderModule,
    pub shader_bias_add: vk::ShaderModule,

    pub layout_rms_norm: vk::PipelineLayout,
    pub layout_rope: vk::PipelineLayout,
    pub layout_kv_write: vk::PipelineLayout,
    pub layout_attn_softmax: vk::PipelineLayout,
    pub layout_swiglu: vk::PipelineLayout,
    pub layout_residual_add: vk::PipelineLayout,
    pub layout_bias_add: vk::PipelineLayout,

    pub desc_layout_2: vk::DescriptorSetLayout,
    pub desc_layout_3: vk::DescriptorSetLayout,

    pub pipeline_rms_norm: vk::Pipeline,
    pub pipeline_rope: vk::Pipeline,
    pub pipeline_kv_write: vk::Pipeline,
    pub pipeline_attn_softmax: vk::Pipeline,
    pub pipeline_swiglu: vk::Pipeline,
    pub pipeline_residual_add: vk::Pipeline,
    pub pipeline_bias_add: vk::Pipeline,

    pub x_residual_buffer: vk::Buffer,
    pub x_residual_memory: vk::DeviceMemory,
    pub x_residual_ptr: *mut std::ffi::c_void,

    pub attn_out_buffer: vk::Buffer,
    pub attn_out_memory: vk::DeviceMemory,
    pub gated_buffer: vk::Buffer,
    pub gated_memory: vk::DeviceMemory,

    pub layer_attn_norms: Vec<(vk::Buffer, vk::DeviceMemory)>,
    pub layer_ffn_norms: Vec<(vk::Buffer, vk::DeviceMemory)>,
    pub layer_q_biases: Vec<Option<(vk::Buffer, vk::DeviceMemory)>>,
    pub layer_k_biases: Vec<Option<(vk::Buffer, vk::DeviceMemory)>>,
    pub layer_v_biases: Vec<Option<(vk::Buffer, vk::DeviceMemory)>>,
    pub final_norm_buf: vk::Buffer,
    pub final_norm_mem: vk::DeviceMemory,

    pub layer_kv_caches: Vec<(vk::Buffer, vk::DeviceMemory)>,

    pub desc_pool: vk::DescriptorPool,
    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,

    pub desc_sets_rms_norm_attn: Vec<vk::DescriptorSet>,
    pub desc_sets_rms_norm_ffn: Vec<vk::DescriptorSet>,
    pub desc_sets_bias_q: Vec<Option<vk::DescriptorSet>>,
    pub desc_sets_bias_k: Vec<Option<vk::DescriptorSet>>,
    pub desc_sets_bias_v: Vec<Option<vk::DescriptorSet>>,
    pub desc_sets_rope: Vec<vk::DescriptorSet>,
    pub desc_sets_kv_write: Vec<vk::DescriptorSet>,
    pub desc_sets_attn_softmax: Vec<vk::DescriptorSet>,
    pub desc_sets_residual_add_attn: Vec<vk::DescriptorSet>,
    pub desc_sets_swiglu: Vec<vk::DescriptorSet>,
    pub desc_sets_residual_add_ffn: Vec<vk::DescriptorSet>,
    pub desc_set_final_norm: vk::DescriptorSet,
}

impl VulkanModelPipeline {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        driver: &VulkanDriver,
        n_layers: usize,
        hidden_size: usize,
        ffn_size: usize,
        _n_heads: usize,
        n_kv_heads: usize,
        head_dim: usize,
        max_seq_len: usize,
        attn_norm_weights: &[&[f32]],
        ffn_norm_weights: &[&[f32]],
        q_biases: &[Option<&[f32]>],
        k_biases: &[Option<&[f32]>],
        v_biases: &[Option<&[f32]>],
        final_norm_weight: &[f32],
        layers_gpu: &[&LayerGpuGemvs],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = driver.device.clone();
        let physical_device = driver.physical_device;
        let instance = driver.instance.clone();
        let queue = driver.compute_queue;

        let shader_rms_norm =
            create_shader_module(&device, crate::compiler::shaders::RMS_NORM_SPV)?;
        let shader_rope = create_shader_module(&device, crate::compiler::shaders::ROPE_SPV)?;
        let shader_kv_write =
            create_shader_module(&device, crate::compiler::shaders::KV_WRITE_SPV)?;
        let shader_attn_softmax =
            create_shader_module(&device, crate::compiler::shaders::ATTN_SOFTMAX_SPV)?;
        let shader_swiglu = create_shader_module(&device, crate::compiler::shaders::SWIGLU_SPV)?;
        let shader_residual_add =
            create_shader_module(&device, crate::compiler::shaders::RESIDUAL_ADD_SPV)?;
        let shader_bias_add =
            create_shader_module(&device, crate::compiler::shaders::BIAS_ADD_SPV)?;

        let bindings_2 = [
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
        ];
        let desc_layout_2 = create_desc_layout(&device, &bindings_2)?;

        let bindings_3 = [
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
        ];
        let desc_layout_3 = create_desc_layout(&device, &bindings_3)?;

        let layout_rms_norm = create_pipeline_layout(&device, desc_layout_2, 8)?;
        let layout_rope = create_pipeline_layout(&device, desc_layout_2, 20)?;
        let layout_kv_write = create_pipeline_layout(&device, desc_layout_3, 12)?;
        let layout_attn_softmax = create_pipeline_layout(&device, desc_layout_3, 24)?;
        let layout_swiglu = create_pipeline_layout(&device, desc_layout_3, 4)?;
        let layout_residual_add = create_pipeline_layout(&device, desc_layout_2, 4)?;
        let layout_bias_add = create_pipeline_layout(&device, desc_layout_2, 4)?;

        let pipeline_rms_norm = create_compute_pipeline(&device, shader_rms_norm, layout_rms_norm)?;
        let pipeline_rope = create_compute_pipeline(&device, shader_rope, layout_rope)?;
        let pipeline_kv_write = create_compute_pipeline(&device, shader_kv_write, layout_kv_write)?;
        let pipeline_attn_softmax =
            create_compute_pipeline(&device, shader_attn_softmax, layout_attn_softmax)?;
        let pipeline_swiglu = create_compute_pipeline(&device, shader_swiglu, layout_swiglu)?;
        let pipeline_residual_add =
            create_compute_pipeline(&device, shader_residual_add, layout_residual_add)?;
        let pipeline_bias_add = create_compute_pipeline(&device, shader_bias_add, layout_bias_add)?;

        let (x_residual_buffer, x_residual_memory, x_residual_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (hidden_size * 4) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (attn_out_buffer, attn_out_memory) = create_uninitialized_device_local_buffer(
            &device,
            &instance,
            physical_device,
            (hidden_size * 4) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (gated_buffer, gated_memory) = create_uninitialized_device_local_buffer(
            &device,
            &instance,
            physical_device,
            (ffn_size * 4) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let mut layer_attn_norms = Vec::with_capacity(n_layers);
        let mut layer_ffn_norms = Vec::with_capacity(n_layers);
        let mut layer_q_biases = Vec::with_capacity(n_layers);
        let mut layer_k_biases = Vec::with_capacity(n_layers);
        let mut layer_v_biases = Vec::with_capacity(n_layers);
        let mut layer_kv_caches = Vec::with_capacity(n_layers);

        let kv_dim = n_kv_heads * head_dim;

        for i in 0..n_layers {
            // SAFETY: Reinterpret Vec<u32> attn norm weights as bytes. Length checked via checked_mul.
            let bytes_attn = unsafe {
                let byte_len = attn_norm_weights[i]
                    .len()
                    .checked_mul(4)
                    .expect("attn_norm overflow");
                std::slice::from_raw_parts(attn_norm_weights[i].as_ptr() as *const u8, byte_len)
            };
            let (attn_buf, attn_mem) = create_device_local_buffer(
                &device,
                &instance,
                physical_device,
                queue,
                driver.queue_family_index,
                bytes_attn.len() as vk::DeviceSize,
                bytes_attn,
            )?;
            layer_attn_norms.push((attn_buf, attn_mem));

            // SAFETY: Reinterpret Vec<u32> ffn norm weights as bytes. Length checked via checked_mul.
            let bytes_ffn = unsafe {
                let byte_len = ffn_norm_weights[i]
                    .len()
                    .checked_mul(4)
                    .expect("ffn_norm overflow");
                std::slice::from_raw_parts(ffn_norm_weights[i].as_ptr() as *const u8, byte_len)
            };
            let (ffn_buf, ffn_mem) = create_device_local_buffer(
                &device,
                &instance,
                physical_device,
                queue,
                driver.queue_family_index,
                bytes_ffn.len() as vk::DeviceSize,
                bytes_ffn,
            )?;
            layer_ffn_norms.push((ffn_buf, ffn_mem));

            if let Some(qb) = q_biases[i] {
                // SAFETY: Reinterpret Vec<u32> q_bias as bytes. Length checked via checked_mul.
                let bytes = unsafe {
                    let byte_len = qb.len().checked_mul(4).expect("q_bias overflow");
                    std::slice::from_raw_parts(qb.as_ptr() as *const u8, byte_len)
                };
                let (buf, mem) = create_device_local_buffer(
                    &device,
                    &instance,
                    physical_device,
                    queue,
                    driver.queue_family_index,
                    bytes.len() as vk::DeviceSize,
                    bytes,
                )?;
                layer_q_biases.push(Some((buf, mem)));
            } else {
                layer_q_biases.push(None);
            }

            if let Some(kb) = k_biases[i] {
                // SAFETY: Reinterpret Vec<u32> k_bias as bytes. Length checked via checked_mul.
                let bytes = unsafe {
                    let byte_len = kb.len().checked_mul(4).expect("k_bias overflow");
                    std::slice::from_raw_parts(kb.as_ptr() as *const u8, byte_len)
                };
                let (buf, mem) = create_device_local_buffer(
                    &device,
                    &instance,
                    physical_device,
                    queue,
                    driver.queue_family_index,
                    bytes.len() as vk::DeviceSize,
                    bytes,
                )?;
                layer_k_biases.push(Some((buf, mem)));
            } else {
                layer_k_biases.push(None);
            }

            if let Some(vb) = v_biases[i] {
                // SAFETY: Reinterpret Vec<u32> v_bias as bytes. Length checked via checked_mul.
                let bytes = unsafe {
                    let byte_len = vb.len().checked_mul(4).expect("v_bias overflow");
                    std::slice::from_raw_parts(vb.as_ptr() as *const u8, byte_len)
                };
                let (buf, mem) = create_device_local_buffer(
                    &device,
                    &instance,
                    physical_device,
                    queue,
                    driver.queue_family_index,
                    bytes.len() as vk::DeviceSize,
                    bytes,
                )?;
                layer_v_biases.push(Some((buf, mem)));
            } else {
                layer_v_biases.push(None);
            }

            let cache_size = 2usize
                .checked_mul(max_seq_len)
                .and_then(|v| v.checked_mul(kv_dim))
                .and_then(|v| v.checked_mul(4))
                .expect("KV cache size overflow");
            let (cache_buf, cache_mem) = create_uninitialized_device_local_buffer(
                &device,
                &instance,
                physical_device,
                cache_size as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?;
            layer_kv_caches.push((cache_buf, cache_mem));
        }

        // SAFETY: Reinterpret Vec<u32> final norm weight as bytes. Length checked via checked_mul.
        let bytes_final = unsafe {
            let byte_len = final_norm_weight
                .len()
                .checked_mul(4)
                .expect("final_norm overflow");
            std::slice::from_raw_parts(final_norm_weight.as_ptr() as *const u8, byte_len)
        };
        let (final_norm_buf, final_norm_mem) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            bytes_final.len() as vk::DeviceSize,
            bytes_final,
        )?;

        let mut total_sets_2 = n_layers * 5 + 1;
        for i in 0..n_layers {
            if q_biases[i].is_some() {
                total_sets_2 += 1;
            }
            if k_biases[i].is_some() {
                total_sets_2 += 1;
            }
            if v_biases[i].is_some() {
                total_sets_2 += 1;
            }
        }
        let total_sets_3 = n_layers * 3;
        let total_sets = total_sets_2 + total_sets_3;

        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count((total_sets * 3) as u32)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&pool_sizes)
            .max_sets(total_sets as u32);
        // SAFETY: Create Vulkan descriptor pool for binding GPU buffers to shader pipelines.
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let layouts_2 = vec![desc_layout_2; total_sets_2];
        let alloc_info_2 = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(desc_pool)
            .set_layouts(&layouts_2);
        // SAFETY: Allocate descriptor sets from pool for 2-buffer shader bindings.
        let sets_2 = unsafe { device.allocate_descriptor_sets(&alloc_info_2)? };

        let layouts_3 = vec![desc_layout_3; total_sets_3];
        let alloc_info_3 = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(desc_pool)
            .set_layouts(&layouts_3);
        // SAFETY: Allocate descriptor sets from pool for 3-buffer shader bindings.
        let sets_3 = unsafe { device.allocate_descriptor_sets(&alloc_info_3)? };

        let mut sets_2_iter = sets_2.into_iter();
        let mut sets_3_iter = sets_3.into_iter();

        let mut desc_sets_rms_norm_attn = Vec::with_capacity(n_layers);
        let mut desc_sets_rms_norm_ffn = Vec::with_capacity(n_layers);
        let mut desc_sets_bias_q = Vec::with_capacity(n_layers);
        let mut desc_sets_bias_k = Vec::with_capacity(n_layers);
        let mut desc_sets_bias_v = Vec::with_capacity(n_layers);
        let mut desc_sets_rope = Vec::with_capacity(n_layers);
        let mut desc_sets_kv_write = Vec::with_capacity(n_layers);
        let mut desc_sets_attn_softmax = Vec::with_capacity(n_layers);
        let mut desc_sets_residual_add_attn = Vec::with_capacity(n_layers);
        let mut desc_sets_swiglu = Vec::with_capacity(n_layers);
        let mut desc_sets_residual_add_ffn = Vec::with_capacity(n_layers);

        let mut buffer_infos = Vec::with_capacity(total_sets * 3);
        let mut writes = Vec::new();

        let push_write_2 =
            |set: vk::DescriptorSet,
             b0: vk::Buffer,
             b1: vk::Buffer,
             infos: &mut Vec<vk::DescriptorBufferInfo>,
             writes_list: &mut Vec<vk::WriteDescriptorSet>| {
                let idx = infos.len();
                infos.push(
                    vk::DescriptorBufferInfo::builder()
                        .buffer(b0)
                        .offset(0)
                        .range(vk::WHOLE_SIZE)
                        .build(),
                );
                infos.push(
                    vk::DescriptorBufferInfo::builder()
                        .buffer(b1)
                        .offset(0)
                        .range(vk::WHOLE_SIZE)
                        .build(),
                );

                writes_list.push(
                    vk::WriteDescriptorSet::builder()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .buffer_info(&infos[idx..idx + 1])
                        .build(),
                );
                writes_list.push(
                    vk::WriteDescriptorSet::builder()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .buffer_info(&infos[idx + 1..idx + 2])
                        .build(),
                );
            };

        let push_write_3 =
            |set: vk::DescriptorSet,
             b0: vk::Buffer,
             b1: vk::Buffer,
             b2: vk::Buffer,
             infos: &mut Vec<vk::DescriptorBufferInfo>,
             writes_list: &mut Vec<vk::WriteDescriptorSet>| {
                let idx = infos.len();
                infos.push(
                    vk::DescriptorBufferInfo::builder()
                        .buffer(b0)
                        .offset(0)
                        .range(vk::WHOLE_SIZE)
                        .build(),
                );
                infos.push(
                    vk::DescriptorBufferInfo::builder()
                        .buffer(b1)
                        .offset(0)
                        .range(vk::WHOLE_SIZE)
                        .build(),
                );
                infos.push(
                    vk::DescriptorBufferInfo::builder()
                        .buffer(b2)
                        .offset(0)
                        .range(vk::WHOLE_SIZE)
                        .build(),
                );

                writes_list.push(
                    vk::WriteDescriptorSet::builder()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .buffer_info(&infos[idx..idx + 1])
                        .build(),
                );
                writes_list.push(
                    vk::WriteDescriptorSet::builder()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .buffer_info(&infos[idx + 1..idx + 2])
                        .build(),
                );
                writes_list.push(
                    vk::WriteDescriptorSet::builder()
                        .dst_set(set)
                        .dst_binding(2)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .buffer_info(&infos[idx + 2..idx + 3])
                        .build(),
                );
            };

        for i in 0..n_layers {
            let lg = layers_gpu[i];
            let q_buf = lg
                .q_proj_gpu
                .as_ref()
                .map(|g| g.output_buffer)
                .unwrap_or(vk::Buffer::null());
            let k_buf = lg
                .k_proj_gpu
                .as_ref()
                .map(|g| g.output_buffer)
                .unwrap_or(vk::Buffer::null());
            let v_buf = lg
                .v_proj_gpu
                .as_ref()
                .map(|g| g.output_buffer)
                .unwrap_or(vk::Buffer::null());
            let o_buf = lg
                .o_proj_gpu
                .as_ref()
                .map(|g| g.output_buffer)
                .unwrap_or(vk::Buffer::null());
            let gate_buf = lg
                .gate_proj_gpu
                .as_ref()
                .map(|g| g.output_buffer)
                .unwrap_or(vk::Buffer::null());
            let up_buf = lg
                .up_proj_gpu
                .as_ref()
                .map(|g| g.output_buffer)
                .unwrap_or(vk::Buffer::null());
            let down_buf = lg
                .down_proj_gpu
                .as_ref()
                .map(|g| g.output_buffer)
                .unwrap_or(vk::Buffer::null());

            let set_rms_attn = sets_2_iter.next().unwrap();
            push_write_2(
                set_rms_attn,
                driver.shared_input_buffer,
                layer_attn_norms[i].0,
                &mut buffer_infos,
                &mut writes,
            );
            desc_sets_rms_norm_attn.push(set_rms_attn);

            if let Some(ref bias) = layer_q_biases[i] {
                let set = sets_2_iter.next().unwrap();
                push_write_2(set, q_buf, bias.0, &mut buffer_infos, &mut writes);
                desc_sets_bias_q.push(Some(set));
            } else {
                desc_sets_bias_q.push(None);
            }

            if let Some(ref bias) = layer_k_biases[i] {
                let set = sets_2_iter.next().unwrap();
                push_write_2(set, k_buf, bias.0, &mut buffer_infos, &mut writes);
                desc_sets_bias_k.push(Some(set));
            } else {
                desc_sets_bias_k.push(None);
            }

            if let Some(ref bias) = layer_v_biases[i] {
                let set = sets_2_iter.next().unwrap();
                push_write_2(set, v_buf, bias.0, &mut buffer_infos, &mut writes);
                desc_sets_bias_v.push(Some(set));
            } else {
                desc_sets_bias_v.push(None);
            }

            let set_rope = sets_2_iter.next().unwrap();
            push_write_2(set_rope, q_buf, k_buf, &mut buffer_infos, &mut writes);
            desc_sets_rope.push(set_rope);

            let set_kv_write = sets_3_iter.next().unwrap();
            push_write_3(
                set_kv_write,
                k_buf,
                v_buf,
                layer_kv_caches[i].0,
                &mut buffer_infos,
                &mut writes,
            );
            desc_sets_kv_write.push(set_kv_write);

            let set_attn = sets_3_iter.next().unwrap();
            push_write_3(
                set_attn,
                q_buf,
                layer_kv_caches[i].0,
                attn_out_buffer,
                &mut buffer_infos,
                &mut writes,
            );
            desc_sets_attn_softmax.push(set_attn);

            let set_res_attn = sets_2_iter.next().unwrap();
            push_write_2(
                set_res_attn,
                x_residual_buffer,
                o_buf,
                &mut buffer_infos,
                &mut writes,
            );
            desc_sets_residual_add_attn.push(set_res_attn);

            let set_rms_ffn = sets_2_iter.next().unwrap();
            push_write_2(
                set_rms_ffn,
                driver.shared_input_buffer,
                layer_ffn_norms[i].0,
                &mut buffer_infos,
                &mut writes,
            );
            desc_sets_rms_norm_ffn.push(set_rms_ffn);

            let set_swiglu = sets_3_iter.next().unwrap();
            push_write_3(
                set_swiglu,
                gate_buf,
                up_buf,
                gated_buffer,
                &mut buffer_infos,
                &mut writes,
            );
            desc_sets_swiglu.push(set_swiglu);

            let set_res_ffn = sets_2_iter.next().unwrap();
            push_write_2(
                set_res_ffn,
                x_residual_buffer,
                down_buf,
                &mut buffer_infos,
                &mut writes,
            );
            desc_sets_residual_add_ffn.push(set_res_ffn);
        }

        let desc_set_final_norm = sets_2_iter.next().unwrap();
        push_write_2(
            desc_set_final_norm,
            x_residual_buffer,
            final_norm_buf,
            &mut buffer_infos,
            &mut writes,
        );

        // SAFETY: Update descriptor sets with buffer bindings. All buffers and sets are valid.
        unsafe { device.update_descriptor_sets(&writes, &[]) };

        let command_pool_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(driver.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        // SAFETY: Create command pool for recording GPU dispatch commands.
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        // SAFETY: Allocate primary command buffer from the pool.
        let command_buffers = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::builder()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?
        };
        let command_buffer = command_buffers[0];

        let fence_info = vk::FenceCreateInfo::builder();
        // SAFETY: Create fence for CPU-GPU synchronization.
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            shader_rms_norm,
            shader_rope,
            shader_kv_write,
            shader_attn_softmax,
            shader_swiglu,
            shader_residual_add,
            shader_bias_add,
            layout_rms_norm,
            layout_rope,
            layout_kv_write,
            layout_attn_softmax,
            layout_swiglu,
            layout_residual_add,
            layout_bias_add,
            desc_layout_2,
            desc_layout_3,
            pipeline_rms_norm,
            pipeline_rope,
            pipeline_kv_write,
            pipeline_attn_softmax,
            pipeline_swiglu,
            pipeline_residual_add,
            pipeline_bias_add,
            x_residual_buffer,
            x_residual_memory,
            x_residual_ptr,
            attn_out_buffer,
            attn_out_memory,
            gated_buffer,
            gated_memory,
            layer_attn_norms,
            layer_ffn_norms,
            layer_q_biases,
            layer_k_biases,
            layer_v_biases,
            final_norm_buf,
            final_norm_mem,
            layer_kv_caches,
            desc_pool,
            command_pool,
            command_buffer,
            fence,
            desc_sets_rms_norm_attn,
            desc_sets_rms_norm_ffn,
            desc_sets_bias_q,
            desc_sets_bias_k,
            desc_sets_bias_v,
            desc_sets_rope,
            desc_sets_kv_write,
            desc_sets_attn_softmax,
            desc_sets_residual_add_attn,
            desc_sets_swiglu,
            desc_sets_residual_add_ffn,
            desc_set_final_norm,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_and_execute_token(
        &self,
        driver: &VulkanDriver,
        n_layers: usize,
        hidden_size: usize,
        ffn_size: usize,
        n_heads: usize,
        n_kv_heads: usize,
        head_dim: usize,
        max_seq_len: usize,
        rope_theta: f32,
        scale: f32,
        pos: u32,
        layers_gpu: &[&LayerGpuGemvs],
    ) -> Result<(), Box<dyn std::error::Error>> {
        super::pipeline_execution::record_and_execute_token(
            self,
            driver,
            n_layers,
            hidden_size,
            ffn_size,
            n_heads,
            n_kv_heads,
            head_dim,
            max_seq_len,
            rope_theta,
            scale,
            pos,
            layers_gpu,
        )
    }
}

impl Drop for VulkanModelPipeline {
    fn drop(&mut self) {
        // SAFETY: Wait for GPU idle, then destroy all Vulkan resources (fence, command pool,
        // descriptor pool, buffers, pipelines, layouts, shader modules). All handles are valid
        // from pipeline creation and owned by this struct.
        unsafe {
            let _ = self.device.device_wait_idle();

            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);

            let destroy_buffer_fn =
                |device: &Device, buffer: vk::Buffer, memory: vk::DeviceMemory, mapped: bool| {
                    if mapped {
                        device.unmap_memory(memory);
                    }
                    device.free_memory(memory, None);
                    device.destroy_buffer(buffer, None);
                };

            destroy_buffer_fn(
                &self.device,
                self.x_residual_buffer,
                self.x_residual_memory,
                true,
            );
            destroy_buffer_fn(
                &self.device,
                self.attn_out_buffer,
                self.attn_out_memory,
                false,
            );
            destroy_buffer_fn(&self.device, self.gated_buffer, self.gated_memory, false);

            for (buf, mem) in &self.layer_attn_norms {
                destroy_buffer_fn(&self.device, *buf, *mem, false);
            }
            for (buf, mem) in &self.layer_ffn_norms {
                destroy_buffer_fn(&self.device, *buf, *mem, false);
            }
            for (buf, mem) in self.layer_q_biases.iter().flatten() {
                destroy_buffer_fn(&self.device, *buf, *mem, false);
            }
            for (buf, mem) in self.layer_k_biases.iter().flatten() {
                destroy_buffer_fn(&self.device, *buf, *mem, false);
            }
            for (buf, mem) in self.layer_v_biases.iter().flatten() {
                destroy_buffer_fn(&self.device, *buf, *mem, false);
            }
            destroy_buffer_fn(
                &self.device,
                self.final_norm_buf,
                self.final_norm_mem,
                false,
            );

            for (buf, mem) in &self.layer_kv_caches {
                destroy_buffer_fn(&self.device, *buf, *mem, false);
            }

            self.device.destroy_pipeline(self.pipeline_rms_norm, None);
            self.device.destroy_pipeline(self.pipeline_rope, None);
            self.device.destroy_pipeline(self.pipeline_kv_write, None);
            self.device
                .destroy_pipeline(self.pipeline_attn_softmax, None);
            self.device.destroy_pipeline(self.pipeline_swiglu, None);
            self.device
                .destroy_pipeline(self.pipeline_residual_add, None);
            self.device.destroy_pipeline(self.pipeline_bias_add, None);

            self.device
                .destroy_pipeline_layout(self.layout_rms_norm, None);
            self.device.destroy_pipeline_layout(self.layout_rope, None);
            self.device
                .destroy_pipeline_layout(self.layout_kv_write, None);
            self.device
                .destroy_pipeline_layout(self.layout_attn_softmax, None);
            self.device
                .destroy_pipeline_layout(self.layout_swiglu, None);
            self.device
                .destroy_pipeline_layout(self.layout_residual_add, None);
            self.device
                .destroy_pipeline_layout(self.layout_bias_add, None);

            self.device
                .destroy_descriptor_set_layout(self.desc_layout_2, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_layout_3, None);

            self.device
                .destroy_shader_module(self.shader_rms_norm, None);
            self.device.destroy_shader_module(self.shader_rope, None);
            self.device
                .destroy_shader_module(self.shader_kv_write, None);
            self.device
                .destroy_shader_module(self.shader_attn_softmax, None);
            self.device.destroy_shader_module(self.shader_swiglu, None);
            self.device
                .destroy_shader_module(self.shader_residual_add, None);
            self.device
                .destroy_shader_module(self.shader_bias_add, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_model_config() -> ModelPipelineConfig {
        ModelPipelineConfig {
            n_layers: 4,
            hidden_size: 256,
            ffn_size: 1024,
            n_heads: 8,
            n_kv_heads: 2,
            head_dim: 32,
            max_seq_len: 512,
            vocab_size: 32000,
        }
    }

    #[test]
    fn validate_model_config_valid() {
        assert!(validate_model_pipeline_config(&default_model_config()).is_empty());
    }

    #[test]
    fn validate_model_config_zero_layers() {
        let mut cfg = default_model_config();
        cfg.n_layers = 0;
        assert!(validate_model_pipeline_config(&cfg).iter().any(|i| i.contains("n_layers")));
    }

    #[test]
    fn validate_model_config_bad_hidden() {
        let mut cfg = default_model_config();
        cfg.hidden_size = 255; // not multiple of 4
        assert!(validate_model_pipeline_config(&cfg).iter().any(|i| i.contains("multiple of 4")));
    }

    #[test]
    fn validate_model_config_heads_not_divisible() {
        let mut cfg = default_model_config();
        cfg.n_heads = 7;
        cfg.n_kv_heads = 2;
        assert!(validate_model_pipeline_config(&cfg).iter().any(|i| i.contains("divisible")));
    }

    #[test]
    fn validate_model_config_zero_vocab() {
        let mut cfg = default_model_config();
        cfg.vocab_size = 0;
        assert!(validate_model_pipeline_config(&cfg).iter().any(|i| i.contains("vocab_size")));
    }

    #[test]
    fn model_pipeline_info_works() {
        let cfg = default_model_config();
        let info = model_pipeline_info(&cfg);
        assert!(info.validation_issues.is_empty());
        assert_eq!(info.shader_count, 7);
        assert_eq!(info.pipeline_count, 7);
        assert_eq!(info.buffer_count, 5);
        assert_eq!(info.total_descriptor_sets_estimate, 41); // 10*4 + 1
    }

    #[test]
    fn model_pipeline_info_with_issues() {
        let mut cfg = default_model_config();
        cfg.n_layers = 0;
        cfg.hidden_size = 0;
        let info = model_pipeline_info(&cfg);
        assert!(info.validation_issues.len() >= 2);
    }

    #[test]
    fn model_config_serializes() {
        let json = serde_json::to_string(&default_model_config()).unwrap();
        assert!(json.contains("n_layers"));
        assert!(json.contains("hidden_size"));
    }

    #[test]
    fn model_pipeline_info_serializes() {
        let info = model_pipeline_info(&default_model_config());
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("shader_count"));
        assert!(json.contains("total_descriptor_sets_estimate"));
    }
}
