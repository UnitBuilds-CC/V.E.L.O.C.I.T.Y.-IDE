// GPU infrastructure — retained for future Qwen model support.
#![allow(dead_code)]
//! Vulkan Qwen (RoPE-based) transformer layer dispatch.
//!
//! # Safety Invariants
//!
//! All `unsafe` blocks wrap Vulkan API calls via `ash`. Handles are valid from the
//! `VulkanDriver` parameter. Buffers use `create_coherent_buffer`/`create_device_local_buffer`.
//! Descriptor sets, pipelines, and command buffers follow standard Vulkan lifecycle patterns.
//! The `Drop` impl tears down resources in reverse dependency order.

use super::vulkan_init::*;
use ash::vk;
use ash::Device;
use serde::Serialize;
use std::ffi::CString;
use std::time::Instant;

/// Qwen layer model dimensions.
#[derive(Debug, Clone, Serialize)]
pub struct QwenLayerConfig {
    pub hidden_size: usize,
    pub ffn_size: usize,
    pub n_heads: usize,
    pub n_kv_heads: usize,
    pub head_dim: usize,
}

/// Describes the compute dispatches for a Qwen layer.
#[derive(Debug, Clone, Serialize)]
pub struct QwenDispatchPlan {
    pub int4_dispatches: usize,
    pub activation_dispatches: usize,
    pub total_dispatches: usize,
    pub descriptor_sets: usize,
}

/// Diagnostic info about a Qwen layer configuration.
#[derive(Debug, Clone, Serialize)]
pub struct QwenLayerInfo {
    pub config: QwenLayerConfig,
    pub dispatch_plan: QwenDispatchPlan,
    pub weight_buffers: usize,
    pub total_weight_bytes_estimate: usize,
    pub validation_issues: Vec<String>,
}

/// Validate Qwen layer dimensions.
pub fn validate_qwen_config(cfg: &QwenLayerConfig) -> Vec<String> {
    let mut issues = Vec::new();
    if cfg.hidden_size == 0 {
        issues.push("hidden_size must be > 0".into());
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
    if cfg.n_kv_heads != 0 && cfg.n_heads != 0 && cfg.n_heads % cfg.n_kv_heads != 0 {
        issues.push(format!(
            "n_heads ({}) must be divisible by n_kv_heads ({})",
            cfg.n_heads, cfg.n_kv_heads
        ));
    }
    issues
}

/// Compute the dispatch plan for a Qwen layer (pure function).
pub fn qwen_dispatch_plan() -> QwenDispatchPlan {
    let int4_dispatches = 7; // Q, K, V, O, gate, up, down
    let activation_dispatches = 1; // SiLU activation
    let total_dispatches = int4_dispatches + activation_dispatches;
    let descriptor_sets = 8;
    QwenDispatchPlan {
        int4_dispatches,
        activation_dispatches,
        total_dispatches,
        descriptor_sets,
    }
}

/// Build diagnostic info for a Qwen layer configuration.
pub fn qwen_layer_info(cfg: &QwenLayerConfig) -> QwenLayerInfo {
    let issues = validate_qwen_config(cfg);
    let plan = qwen_dispatch_plan();
    let weight_buffers = 7;
    let weight_bytes = cfg.hidden_size * cfg.hidden_size * 4 * 4
        + cfg.hidden_size * cfg.ffn_size * 4 * 2
        + cfg.ffn_size * cfg.hidden_size * 4;
    QwenLayerInfo {
        config: cfg.clone(),
        dispatch_plan: plan,
        weight_buffers,
        total_weight_bytes_estimate: weight_bytes,
        validation_issues: issues,
    }
}

pub struct VulkanQwenLayer {
    pub device: Device,
    pub queue: vk::Queue,

    pub shader_int4: vk::ShaderModule,
    pub shader_act: vk::ShaderModule,

    pub desc_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout: vk::PipelineLayout,

    pub pipeline_int4: vk::Pipeline,
    pub pipeline_act: vk::Pipeline,

    pub inputs_2304_buffer: vk::Buffer,
    pub inputs_2304_memory: vk::DeviceMemory,
    pub inputs_2304_ptr: *mut std::ffi::c_void,

    pub out_2304_a_buffer: vk::Buffer,
    pub out_2304_a_memory: vk::DeviceMemory,
    pub out_2304_a_ptr: *mut std::ffi::c_void,

    pub out_2304_b_buffer: vk::Buffer,
    pub out_2304_b_memory: vk::DeviceMemory,

    pub out_256_k_buffer: vk::Buffer,
    pub out_256_k_memory: vk::DeviceMemory,

    pub out_256_v_buffer: vk::Buffer,
    pub out_256_v_memory: vk::DeviceMemory,

    pub out_11008_gate_buffer: vk::Buffer,
    pub out_11008_gate_memory: vk::DeviceMemory,

    pub out_11008_up_buffer: vk::Buffer,
    pub out_11008_up_memory: vk::DeviceMemory,

    pub inputs_11008_buffer: vk::Buffer,
    pub inputs_11008_memory: vk::DeviceMemory,

    pub weight_q_buffer: vk::Buffer,
    pub weight_q_memory: vk::DeviceMemory,
    pub weight_k_buffer: vk::Buffer,
    pub weight_k_memory: vk::DeviceMemory,
    pub weight_v_buffer: vk::Buffer,
    pub weight_v_memory: vk::DeviceMemory,
    pub weight_o_buffer: vk::Buffer,
    pub weight_o_memory: vk::DeviceMemory,
    pub weight_gate_buffer: vk::Buffer,
    pub weight_gate_memory: vk::DeviceMemory,
    pub weight_up_buffer: vk::Buffer,
    pub weight_up_memory: vk::DeviceMemory,
    pub weight_down_buffer: vk::Buffer,
    pub weight_down_memory: vk::DeviceMemory,

    pub desc_pool: vk::DescriptorPool,
    pub desc_sets: Vec<vk::DescriptorSet>,

    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
}

impl VulkanQwenLayer {
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

        let shader_info_int4 =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::INT4_SPV);
        // SAFETY: create_shader_module with valid INT4 SPIR-V bytecode.
        let shader_int4 = unsafe { device.create_shader_module(&shader_info_int4, None)? };

        let shader_info_act =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ACT_QWEN_SPV);
        // SAFETY: create_shader_module with valid activation SPIR-V bytecode.
        let shader_act = unsafe { device.create_shader_module(&shader_info_act, None)? };

        let bindings = [
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
        let layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
        // SAFETY: create_descriptor_set_layout with storage buffer bindings.
        let desc_set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };

        let push_constant_ranges = [vk::PushConstantRange::builder()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(8)
            .build()];
        let layouts = [desc_set_layout];
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&layouts)
            .push_constant_ranges(&push_constant_ranges);
        // SAFETY: create_pipeline_layout with descriptor set layout and push constants.
        let pipeline_layout =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

        let main_entry = CString::new("main")?;

        let stage_info_int4 = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_int4)
            .name(&main_entry);
        let pipeline_create_info_int4 = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_int4.build())
            .layout(pipeline_layout);

        let stage_info_act = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_act)
            .name(&main_entry);
        let pipeline_create_info_act = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info_act.build())
            .layout(pipeline_layout);

        // SAFETY: create_compute_pipelines for int4 and activation pipelines.
        let pipelines = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[
                        pipeline_create_info_int4.build(),
                        pipeline_create_info_act.build(),
                    ],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let pipeline_int4 = pipelines[0];
        let pipeline_act = pipelines[1];

        let (inputs_2304_buffer, inputs_2304_memory, inputs_2304_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            2304 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_2304_a_buffer, out_2304_a_memory, out_2304_a_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            2304 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let (out_2304_b_buffer, out_2304_b_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            2304 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_256_k_buffer, out_256_k_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            256 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_256_v_buffer, out_256_v_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            256 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_11008_gate_buffer, out_11008_gate_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            11008 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (out_11008_up_buffer, out_11008_up_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            11008 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (inputs_11008_buffer, inputs_11008_memory, _) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            11008 * 4,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let (weight_q_buffer, weight_q_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_q.len() as vk::DeviceSize,
            weight_q,
        )?;
        let (weight_k_buffer, weight_k_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_k.len() as vk::DeviceSize,
            weight_k,
        )?;
        let (weight_v_buffer, weight_v_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_v.len() as vk::DeviceSize,
            weight_v,
        )?;
        let (weight_o_buffer, weight_o_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_o.len() as vk::DeviceSize,
            weight_o,
        )?;
        let (weight_gate_buffer, weight_gate_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_gate.len() as vk::DeviceSize,
            weight_gate,
        )?;
        let (weight_up_buffer, weight_up_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_up.len() as vk::DeviceSize,
            weight_up,
        )?;
        let (weight_down_buffer, weight_down_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            weight_down.len() as vk::DeviceSize,
            weight_down,
        )?;

        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(24)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(8)
            .pool_sizes(&pool_sizes);
        // SAFETY: create_descriptor_pool with capacity for storage buffer sets.
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let layouts_vec = vec![desc_set_layout; 8];
        let alloc_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(desc_pool)
            .set_layouts(&layouts_vec);
        // SAFETY: allocate_descriptor_sets from the pool.
        let desc_sets = unsafe { device.allocate_descriptor_sets(&alloc_info)? };

        let set_configs = [
            (inputs_2304_buffer, weight_q_buffer, out_2304_a_buffer),
            (inputs_2304_buffer, weight_k_buffer, out_256_k_buffer),
            (inputs_2304_buffer, weight_v_buffer, out_256_v_buffer),
            (inputs_2304_buffer, weight_o_buffer, out_2304_b_buffer),
            (
                inputs_2304_buffer,
                weight_gate_buffer,
                out_11008_gate_buffer,
            ),
            (inputs_2304_buffer, weight_up_buffer, out_11008_up_buffer),
            (
                out_11008_gate_buffer,
                out_11008_up_buffer,
                inputs_11008_buffer,
            ),
            (inputs_11008_buffer, weight_down_buffer, out_2304_a_buffer),
        ];

        for (i, (b0, b1, b2)) in set_configs.iter().enumerate() {
            let buffer_infos = [
                vk::DescriptorBufferInfo::builder()
                    .buffer(*b0)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*b1)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(*b2)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build(),
            ];
            let writes = [
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets[i])
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[0..1])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets[i])
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[1..2])
                    .build(),
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_sets[i])
                    .dst_binding(2)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[2..3])
                    .build(),
            ];
            // SAFETY: update_descriptor_sets binds buffer info to descriptor sets.
            unsafe { device.update_descriptor_sets(&writes, &[]) };
        }

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
        // SAFETY: Record compute dispatch commands into the command buffer.
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;

            let dispatch_int4 = |cmd: vk::CommandBuffer, set: vk::DescriptorSet, k: u32, n: u32| {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline_int4);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline_layout,
                    0,
                    &[set],
                    &[],
                );
                let params = [k, n];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(
                    cmd,
                    pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    params_bytes,
                );
                let workgroups = n.div_ceil(64u32);
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            let dispatch_act = |cmd: vk::CommandBuffer, set: vk::DescriptorSet, n: u32| {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline_act);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline_layout,
                    0,
                    &[set],
                    &[],
                );
                let params = [n, 0u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
                device.cmd_push_constants(
                    cmd,
                    pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    params_bytes,
                );
                let workgroups = n.div_ceil(64u32);
                device.cmd_dispatch(cmd, workgroups, 1, 1);
            };

            dispatch_int4(command_buffer, desc_sets[0], 2304, 2304);
            dispatch_int4(command_buffer, desc_sets[1], 2304, 256);
            dispatch_int4(command_buffer, desc_sets[2], 2304, 256);
            dispatch_int4(command_buffer, desc_sets[3], 2304, 2304);
            dispatch_int4(command_buffer, desc_sets[4], 2304, 11008);
            dispatch_int4(command_buffer, desc_sets[5], 2304, 11008);
            dispatch_act(command_buffer, desc_sets[6], 11008);
            dispatch_int4(command_buffer, desc_sets[7], 11008, 2304);

            device.end_command_buffer(command_buffer)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        // SAFETY: create_fence for GPU synchronization.
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            shader_int4,
            shader_act,
            desc_set_layout,
            pipeline_layout,
            pipeline_int4,
            pipeline_act,
            inputs_2304_buffer,
            inputs_2304_memory,
            inputs_2304_ptr,
            out_2304_a_buffer,
            out_2304_a_memory,
            out_2304_a_ptr,
            out_2304_b_buffer,
            out_2304_b_memory,
            out_256_k_buffer,
            out_256_k_memory,
            out_256_v_buffer,
            out_256_v_memory,
            out_11008_gate_buffer,
            out_11008_gate_memory,
            out_11008_up_buffer,
            out_11008_up_memory,
            inputs_11008_buffer,
            inputs_11008_memory,
            weight_q_buffer,
            weight_q_memory,
            weight_k_buffer,
            weight_k_memory,
            weight_v_buffer,
            weight_v_memory,
            weight_o_buffer,
            weight_o_memory,
            weight_gate_buffer,
            weight_gate_memory,
            weight_up_buffer,
            weight_up_memory,
            weight_down_buffer,
            weight_down_memory,
            desc_pool,
            desc_sets,
            command_pool,
            command_buffer,
            fence,
        })
    }

    pub fn run(
        &self,
        input_bytes: &[u8],
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        // SAFETY: copy input bytes to coherent GPU buffer via mapped pointer.
        unsafe {
            std::ptr::copy_nonoverlapping(
                input_bytes.as_ptr(),
                self.inputs_2304_ptr as *mut u8,
                input_bytes.len(),
            );
        }

        let start = Instant::now();
        let command_buffers = [self.command_buffer];
        let submit_info = vk::SubmitInfo::builder().command_buffers(&command_buffers);
        // SAFETY: reset fence, submit command buffer, wait for completion.
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device
                .queue_submit(self.queue, &[submit_info.build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        // SAFETY: copy output floats from coherent GPU buffer via mapped pointer.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.out_2304_a_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }
}

impl Drop for VulkanQwenLayer {
    fn drop(&mut self) {
        // SAFETY: All Vulkan handles (fence, command_pool, desc_pool, buffers, memories)
        // were created by `self` in `new()` and are valid. device_wait_idle ensures no
        // GPU work is in flight before destroying resources. Destruction order:
        // wait → fence/pool → unmap+free+destroy buffers. Allocator is the same device.
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);

            self.device.unmap_memory(self.inputs_2304_memory);
            self.device.free_memory(self.inputs_2304_memory, None);
            self.device.destroy_buffer(self.inputs_2304_buffer, None);

            self.device.unmap_memory(self.out_2304_a_memory);
            self.device.free_memory(self.out_2304_a_memory, None);
            self.device.destroy_buffer(self.out_2304_a_buffer, None);

            self.device.free_memory(self.out_2304_b_memory, None);
            self.device.destroy_buffer(self.out_2304_b_buffer, None);

            self.device.free_memory(self.out_256_k_memory, None);
            self.device.destroy_buffer(self.out_256_k_buffer, None);

            self.device.free_memory(self.out_256_v_memory, None);
            self.device.destroy_buffer(self.out_256_v_buffer, None);

            self.device.free_memory(self.out_11008_gate_memory, None);
            self.device.destroy_buffer(self.out_11008_gate_buffer, None);

            self.device.free_memory(self.out_11008_up_memory, None);
            self.device.destroy_buffer(self.out_11008_up_buffer, None);

            self.device.free_memory(self.inputs_11008_memory, None);
            self.device.destroy_buffer(self.inputs_11008_buffer, None);

            self.device.free_memory(self.weight_q_memory, None);
            self.device.destroy_buffer(self.weight_q_buffer, None);

            self.device.free_memory(self.weight_k_memory, None);
            self.device.destroy_buffer(self.weight_k_buffer, None);

            self.device.free_memory(self.weight_v_memory, None);
            self.device.destroy_buffer(self.weight_v_buffer, None);

            self.device.free_memory(self.weight_o_memory, None);
            self.device.destroy_buffer(self.weight_o_buffer, None);

            self.device.free_memory(self.weight_gate_memory, None);
            self.device.destroy_buffer(self.weight_gate_buffer, None);

            self.device.free_memory(self.weight_up_memory, None);
            self.device.destroy_buffer(self.weight_up_buffer, None);

            self.device.free_memory(self.weight_down_memory, None);
            self.device.destroy_buffer(self.weight_down_buffer, None);

            self.device.destroy_pipeline(self.pipeline_int4, None);
            self.device.destroy_pipeline(self.pipeline_act, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_set_layout, None);

            self.device.destroy_shader_module(self.shader_int4, None);
            self.device.destroy_shader_module(self.shader_act, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_qwen_config() -> QwenLayerConfig {
        QwenLayerConfig {
            hidden_size: 2304,
            ffn_size: 11008,
            n_heads: 18,
            n_kv_heads: 2,
            head_dim: 128,
        }
    }

    #[test]
    fn validate_qwen_config_valid() {
        let cfg = default_qwen_config();
        assert!(validate_qwen_config(&cfg).is_empty());
    }

    #[test]
    fn validate_qwen_config_zero_hidden() {
        let mut cfg = default_qwen_config();
        cfg.hidden_size = 0;
        assert!(validate_qwen_config(&cfg).iter().any(|i| i.contains("hidden_size")));
    }

    #[test]
    fn validate_qwen_config_heads_not_divisible() {
        let mut cfg = default_qwen_config();
        cfg.n_heads = 7;
        cfg.n_kv_heads = 2;
        assert!(validate_qwen_config(&cfg).iter().any(|i| i.contains("divisible")));
    }

    #[test]
    fn validate_qwen_config_zero_ffn() {
        let mut cfg = default_qwen_config();
        cfg.ffn_size = 0;
        assert!(validate_qwen_config(&cfg).iter().any(|i| i.contains("ffn_size")));
    }

    #[test]
    fn qwen_dispatch_plan_works() {
        let plan = qwen_dispatch_plan();
        assert_eq!(plan.int4_dispatches, 7);
        assert_eq!(plan.activation_dispatches, 1);
        assert_eq!(plan.total_dispatches, 8);
        assert_eq!(plan.descriptor_sets, 8);
    }

    #[test]
    fn qwen_layer_info_works() {
        let cfg = default_qwen_config();
        let info = qwen_layer_info(&cfg);
        assert!(info.validation_issues.is_empty());
        assert_eq!(info.weight_buffers, 7);
        assert!(info.total_weight_bytes_estimate > 0);
        assert_eq!(info.dispatch_plan.total_dispatches, 8);
    }

    #[test]
    fn qwen_layer_info_with_issues() {
        let mut cfg = default_qwen_config();
        cfg.hidden_size = 0;
        cfg.ffn_size = 0;
        let info = qwen_layer_info(&cfg);
        assert!(info.validation_issues.len() >= 2);
    }

    #[test]
    fn qwen_config_serializes() {
        let json = serde_json::to_string(&default_qwen_config()).unwrap();
        assert!(json.contains("hidden_size"));
        assert!(json.contains("2304"));
    }

    #[test]
    fn qwen_layer_info_serializes() {
        let info = qwen_layer_info(&default_qwen_config());
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("dispatch_plan"));
        assert!(json.contains("weight_buffers"));
    }

    #[test]
    fn qwen_dispatch_plan_serializes() {
        let json = serde_json::to_string(&qwen_dispatch_plan()).unwrap();
        assert!(json.contains("int4_dispatches"));
    }

    // ── Validation: individual fields ────────────────────────────────────

    #[test]
    fn validate_zero_n_heads() {
        let mut cfg = default_qwen_config();
        cfg.n_heads = 0;
        let issues = validate_qwen_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("n_heads")));
    }

    #[test]
    fn validate_zero_n_kv_heads() {
        let mut cfg = default_qwen_config();
        cfg.n_kv_heads = 0;
        let issues = validate_qwen_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("n_kv_heads")));
    }

    #[test]
    fn validate_zero_head_dim() {
        let mut cfg = default_qwen_config();
        cfg.head_dim = 0;
        let issues = validate_qwen_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("head_dim")));
    }

    // ── Validation: multiple issues ──────────────────────────────────────

    #[test]
    fn validate_all_zeros_triggers_five_issues() {
        let cfg = QwenLayerConfig {
            hidden_size: 0,
            ffn_size: 0,
            n_heads: 0,
            n_kv_heads: 0,
            head_dim: 0,
        };
        let issues = validate_qwen_config(&cfg);
        // 5 zero-field issues; divisibility check is guarded so no panic
        assert_eq!(issues.len(), 5);
    }

    #[test]
    fn validate_zero_kv_heads_no_remainder_panic() {
        // Regression: n_heads % n_kv_heads used to be evaluated before the zero guard.
        let cfg = QwenLayerConfig {
            hidden_size: 2304,
            ffn_size: 11008,
            n_heads: 18,
            n_kv_heads: 0,
            head_dim: 128,
        };
        let issues = validate_qwen_config(&cfg);
        // Only "n_kv_heads must be > 0"; no divisibility issue since guard protects it
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("n_kv_heads"));
    }

    #[test]
    fn validate_divisibility_issue_text_contains_values() {
        let mut cfg = default_qwen_config();
        cfg.n_heads = 7;
        cfg.n_kv_heads = 2;
        let issues = validate_qwen_config(&cfg);
        let div_issue = issues.iter().find(|i| i.contains("divisible")).unwrap();
        assert!(div_issue.contains("7"));
        assert!(div_issue.contains("2"));
    }

    #[test]
    fn validate_heads_divisible_no_issue() {
        let mut cfg = default_qwen_config();
        cfg.n_heads = 18;
        cfg.n_kv_heads = 2;
        let issues = validate_qwen_config(&cfg);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_heads_equal_no_issue() {
        let mut cfg = default_qwen_config();
        cfg.n_heads = 2;
        cfg.n_kv_heads = 2;
        let issues = validate_qwen_config(&cfg);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_kv_heads_one_always_valid() {
        let mut cfg = default_qwen_config();
        cfg.n_heads = 17;
        cfg.n_kv_heads = 1;
        let issues = validate_qwen_config(&cfg);
        assert!(issues.is_empty());
    }

    // ── Dispatch plan ────────────────────────────────────────────────────

    #[test]
    fn dispatch_plan_is_deterministic() {
        let p1 = qwen_dispatch_plan();
        let p2 = qwen_dispatch_plan();
        assert_eq!(p1.int4_dispatches, p2.int4_dispatches);
        assert_eq!(p1.activation_dispatches, p2.activation_dispatches);
        assert_eq!(p1.total_dispatches, p2.total_dispatches);
        assert_eq!(p1.descriptor_sets, p2.descriptor_sets);
    }

    #[test]
    fn dispatch_plan_total_is_sum() {
        let plan = qwen_dispatch_plan();
        assert_eq!(
            plan.total_dispatches,
            plan.int4_dispatches + plan.activation_dispatches
        );
    }

    // ── Layer info: weight calculations ──────────────────────────────────

    #[test]
    fn info_weight_bytes_formula() {
        let cfg = default_qwen_config();
        let info = qwen_layer_info(&cfg);
        let expected = cfg.hidden_size * cfg.hidden_size * 4 * 4
            + cfg.hidden_size * cfg.ffn_size * 4 * 2
            + cfg.ffn_size * cfg.hidden_size * 4;
        assert_eq!(info.total_weight_bytes_estimate, expected);
    }

    #[test]
    fn info_weight_buffers_is_seven() {
        let cfg = default_qwen_config();
        let info = qwen_layer_info(&cfg);
        assert_eq!(info.weight_buffers, 7);
    }

    #[test]
    fn info_config_matches_input() {
        let cfg = default_qwen_config();
        let info = qwen_layer_info(&cfg);
        assert_eq!(info.config.hidden_size, cfg.hidden_size);
        assert_eq!(info.config.ffn_size, cfg.ffn_size);
        assert_eq!(info.config.n_heads, cfg.n_heads);
        assert_eq!(info.config.n_kv_heads, cfg.n_kv_heads);
        assert_eq!(info.config.head_dim, cfg.head_dim);
    }

    #[test]
    fn info_dispatch_plan_matches() {
        let cfg = default_qwen_config();
        let info = qwen_layer_info(&cfg);
        let plan = qwen_dispatch_plan();
        assert_eq!(info.dispatch_plan.int4_dispatches, plan.int4_dispatches);
        assert_eq!(info.dispatch_plan.total_dispatches, plan.total_dispatches);
    }

    #[test]
    fn info_large_config_weight_estimate() {
        let cfg = QwenLayerConfig {
            hidden_size: 4096,
            ffn_size: 11008,
            n_heads: 32,
            n_kv_heads: 8,
            head_dim: 128,
        };
        let info = qwen_layer_info(&cfg);
        assert!(info.total_weight_bytes_estimate > 500_000_000);
    }

    #[test]
    fn info_small_config_weight_estimate() {
        let cfg = QwenLayerConfig {
            hidden_size: 64,
            ffn_size: 128,
            n_heads: 4,
            n_kv_heads: 2,
            head_dim: 16,
        };
        let info = qwen_layer_info(&cfg);
        // 64*64*4*4 + 64*128*4*2 + 128*64*4 = 65536 + 65536 + 32768 = 163840
        assert_eq!(info.total_weight_bytes_estimate, 163_840);
    }

    #[test]
    fn info_zero_hidden_still_produces_info() {
        let mut cfg = default_qwen_config();
        cfg.hidden_size = 0;
        let info = qwen_layer_info(&cfg);
        assert!(!info.validation_issues.is_empty());
        // weight_bytes should be 0 when hidden_size is 0
        assert_eq!(info.total_weight_bytes_estimate, 0);
    }

    // ── Struct derives ───────────────────────────────────────────────────

    #[test]
    fn config_clone_is_independent() {
        let cfg = default_qwen_config();
        let mut cloned = cfg.clone();
        cloned.hidden_size = 9999;
        assert_eq!(cfg.hidden_size, 2304);
    }

    #[test]
    fn config_debug_format_contains_field() {
        let cfg = default_qwen_config();
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("hidden_size"));
        assert!(debug.contains("2304"));
    }

    #[test]
    fn dispatch_plan_clone_is_independent() {
        let plan = qwen_dispatch_plan();
        let mut cloned = plan.clone();
        cloned.int4_dispatches = 999;
        assert_eq!(plan.int4_dispatches, 7);
    }

    #[test]
    fn dispatch_plan_debug_format() {
        let plan = qwen_dispatch_plan();
        let debug = format!("{:?}", plan);
        assert!(debug.contains("int4_dispatches"));
        assert!(debug.contains("8"));
    }

    #[test]
    fn info_clone_is_independent() {
        let cfg = default_qwen_config();
        let info = qwen_layer_info(&cfg);
        let mut cloned = info.clone();
        cloned.weight_buffers = 999;
        assert_eq!(info.weight_buffers, 7);
    }

    // ── Serialization ────────────────────────────────────────────────────

    #[test]
    fn config_json_contains_all_fields() {
        let cfg = default_qwen_config();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("hidden_size"));
        assert!(json.contains("ffn_size"));
        assert!(json.contains("n_heads"));
        assert!(json.contains("n_kv_heads"));
        assert!(json.contains("head_dim"));
    }

    #[test]
    fn dispatch_plan_json_contains_all_fields() {
        let plan = qwen_dispatch_plan();
        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("int4_dispatches"));
        assert!(json.contains("activation_dispatches"));
        assert!(json.contains("total_dispatches"));
        assert!(json.contains("descriptor_sets"));
    }

    #[test]
    fn info_json_contains_validation_issues() {
        let mut cfg = default_qwen_config();
        cfg.hidden_size = 0;
        let info = qwen_layer_info(&cfg);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("validation_issues"));
        assert!(json.contains("hidden_size"));
    }

    #[test]
    fn config_json_parseable_as_value() {
        let cfg = default_qwen_config();
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["hidden_size"].as_u64().unwrap(), 2304);
        assert_eq!(parsed["ffn_size"].as_u64().unwrap(), 11008);
        assert_eq!(parsed["n_heads"].as_u64().unwrap(), 18);
        assert_eq!(parsed["n_kv_heads"].as_u64().unwrap(), 2);
        assert_eq!(parsed["head_dim"].as_u64().unwrap(), 128);
    }

    // ── Boundary / edge cases ────────────────────────────────────────────

    #[test]
    fn validate_hidden_size_one_not_multiple_of_anything_special() {
        let mut cfg = default_qwen_config();
        cfg.hidden_size = 1;
        cfg.n_heads = 1;
        cfg.n_kv_heads = 1;
        cfg.head_dim = 1;
        let issues = validate_qwen_config(&cfg);
        assert!(issues.is_empty());
    }

    #[test]
    fn info_with_only_ffn_nonzero() {
        let cfg = QwenLayerConfig {
            hidden_size: 0,
            ffn_size: 100,
            n_heads: 0,
            n_kv_heads: 0,
            head_dim: 0,
        };
        let info = qwen_layer_info(&cfg);
        // All terms have hidden_size as factor → 0
        assert_eq!(info.total_weight_bytes_estimate, 0);
    }

    #[test]
    fn info_weight_bytes_grows_quadratically_with_hidden() {
        let small = QwenLayerConfig {
            hidden_size: 256,
            ffn_size: 512,
            n_heads: 4,
            n_kv_heads: 2,
            head_dim: 64,
        };
        let large = QwenLayerConfig {
            hidden_size: 512,
            ffn_size: 1024,
            n_heads: 8,
            n_kv_heads: 4,
            head_dim: 64,
        };
        let info_small = qwen_layer_info(&small);
        let info_large = qwen_layer_info(&large);
        assert!(info_large.total_weight_bytes_estimate > info_small.total_weight_bytes_estimate);
    }

    // ── JSON key count verification ──────────────────────────────────────

    #[test]
    fn config_json_has_exactly_5_keys() {
        let json = serde_json::to_string(&default_qwen_config()).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 5);
    }

    #[test]
    fn dispatch_plan_json_has_exactly_4_keys() {
        let json = serde_json::to_string(&qwen_dispatch_plan()).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 4);
    }

    #[test]
    fn info_json_has_exactly_5_keys() {
        let info = qwen_layer_info(&default_qwen_config());
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 5);
        assert!(val.get("config").is_some());
        assert!(val.get("dispatch_plan").is_some());
        assert!(val.get("weight_buffers").is_some());
        assert!(val.get("total_weight_bytes_estimate").is_some());
        assert!(val.get("validation_issues").is_some());
    }

    // ── JSON roundtrip ───────────────────────────────────────────────────

    #[test]
    fn config_json_roundtrip_via_value() {
        let cfg = default_qwen_config();
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["hidden_size"].as_u64().unwrap(), cfg.hidden_size as u64);
        assert_eq!(parsed["ffn_size"].as_u64().unwrap(), cfg.ffn_size as u64);
        assert_eq!(parsed["n_heads"].as_u64().unwrap(), cfg.n_heads as u64);
        assert_eq!(parsed["n_kv_heads"].as_u64().unwrap(), cfg.n_kv_heads as u64);
        assert_eq!(parsed["head_dim"].as_u64().unwrap(), cfg.head_dim as u64);
    }

    #[test]
    fn dispatch_plan_json_roundtrip_via_value() {
        let plan = qwen_dispatch_plan();
        let json = serde_json::to_string(&plan).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["int4_dispatches"].as_u64().unwrap(), plan.int4_dispatches as u64);
        assert_eq!(parsed["activation_dispatches"].as_u64().unwrap(), plan.activation_dispatches as u64);
        assert_eq!(parsed["total_dispatches"].as_u64().unwrap(), plan.total_dispatches as u64);
        assert_eq!(parsed["descriptor_sets"].as_u64().unwrap(), plan.descriptor_sets as u64);
    }

    #[test]
    fn info_json_roundtrip_preserves_values() {
        let cfg = default_qwen_config();
        let info = qwen_layer_info(&cfg);
        let json = serde_json::to_string(&info).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["config"]["hidden_size"].as_u64().unwrap(), info.config.hidden_size as u64);
        assert_eq!(parsed["weight_buffers"].as_u64().unwrap(), info.weight_buffers as u64);
        assert_eq!(parsed["total_weight_bytes_estimate"].as_u64().unwrap(), info.total_weight_bytes_estimate as u64);
        assert!(parsed["validation_issues"].is_array());
    }

    // ── Validation: combined issues ──────────────────────────────────────

    #[test]
    fn validate_multiple_zero_fields_combined() {
        let cfg = QwenLayerConfig {
            hidden_size: 0,
            ffn_size: 0,
            n_heads: 0,
            n_kv_heads: 0,
            head_dim: 0,
        };
        let issues = validate_qwen_config(&cfg);
        // 5 zero-field issues (divisibility guard prevents 6th)
        assert_eq!(issues.len(), 5);
    }

    #[test]
    fn validate_issue_messages_are_descriptive() {
        let cfg = QwenLayerConfig {
            hidden_size: 0,
            ffn_size: 0,
            n_heads: 0,
            n_kv_heads: 0,
            head_dim: 0,
        };
        let issues = validate_qwen_config(&cfg);
        for issue in &issues {
            assert!(issue.contains("must be"), "issue should contain 'must be': {}", issue);
        }
    }

    #[test]
    fn validate_valid_config_returns_empty_vec() {
        let cfg = QwenLayerConfig {
            hidden_size: 1024,
            ffn_size: 4096,
            n_heads: 16,
            n_kv_heads: 4,
            head_dim: 64,
        };
        let issues = validate_qwen_config(&cfg);
        assert!(issues.is_empty());
        assert_eq!(issues.len(), 0);
    }

    // ── Weight byte formula edge cases ───────────────────────────────────

    #[test]
    fn weight_bytes_all_zeros_is_zero() {
        let cfg = QwenLayerConfig {
            hidden_size: 0,
            ffn_size: 0,
            n_heads: 1,
            n_kv_heads: 1,
            head_dim: 1,
        };
        let info = qwen_layer_info(&cfg);
        assert_eq!(info.total_weight_bytes_estimate, 0);
    }

    #[test]
    fn weight_bytes_hidden_one_ffn_one() {
        let cfg = QwenLayerConfig {
            hidden_size: 1,
            ffn_size: 1,
            n_heads: 1,
            n_kv_heads: 1,
            head_dim: 1,
        };
        let info = qwen_layer_info(&cfg);
        // 1*1*4*4 + 1*1*4*2 + 1*1*4 = 16 + 8 + 4 = 28
        assert_eq!(info.total_weight_bytes_estimate, 28);
    }

    #[test]
    fn weight_bytes_scales_linearly_with_ffn() {
        let cfg1 = QwenLayerConfig {
            hidden_size: 256,
            ffn_size: 512,
            n_heads: 4,
            n_kv_heads: 2,
            head_dim: 64,
        };
        let cfg2 = QwenLayerConfig {
            hidden_size: 256,
            ffn_size: 1024,
            n_heads: 4,
            n_kv_heads: 2,
            head_dim: 64,
        };
        let info1 = qwen_layer_info(&cfg1);
        let info2 = qwen_layer_info(&cfg2);
        // ffn terms: hidden*ffn*4*2 + ffn*hidden*4 = hidden*ffn*(8+4) = hidden*ffn*12
        // Doubling ffn should increase ffn-related terms by 2x
        let ffn_term_1 = cfg1.hidden_size * cfg1.ffn_size * 4 * 2 + cfg1.ffn_size * cfg1.hidden_size * 4;
        let ffn_term_2 = cfg2.hidden_size * cfg2.ffn_size * 4 * 2 + cfg2.ffn_size * cfg2.hidden_size * 4;
        assert_eq!(ffn_term_2, ffn_term_1 * 2);
        assert!(info2.total_weight_bytes_estimate > info1.total_weight_bytes_estimate);
    }

    // ── Dispatch plan constants ──────────────────────────────────────────

    #[test]
    fn dispatch_plan_int4_count_is_seven() {
        // Q, K, V, O, gate, up, down = 7
        assert_eq!(qwen_dispatch_plan().int4_dispatches, 7);
    }

    #[test]
    fn dispatch_plan_activation_count_is_one() {
        // SiLU activation = 1
        assert_eq!(qwen_dispatch_plan().activation_dispatches, 1);
    }

    #[test]
    fn dispatch_plan_descriptor_sets_is_eight() {
        assert_eq!(qwen_dispatch_plan().descriptor_sets, 8);
    }

    // ── Info struct consistency ──────────────────────────────────────────

    #[test]
    fn info_validation_issues_match_standalone_validation() {
        let mut cfg = default_qwen_config();
        cfg.hidden_size = 0;
        cfg.n_heads = 7;
        cfg.n_kv_heads = 2;
        let info = qwen_layer_info(&cfg);
        let standalone = validate_qwen_config(&cfg);
        assert_eq!(info.validation_issues.len(), standalone.len());
        for (a, b) in info.validation_issues.iter().zip(standalone.iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn info_dispatch_plan_matches_standalone() {
        let cfg = default_qwen_config();
        let info = qwen_layer_info(&cfg);
        let standalone = qwen_dispatch_plan();
        assert_eq!(info.dispatch_plan.int4_dispatches, standalone.int4_dispatches);
        assert_eq!(info.dispatch_plan.activation_dispatches, standalone.activation_dispatches);
        assert_eq!(info.dispatch_plan.total_dispatches, standalone.total_dispatches);
        assert_eq!(info.dispatch_plan.descriptor_sets, standalone.descriptor_sets);
    }

    #[test]
    fn info_weight_buffers_always_seven() {
        // Regardless of config, weight_buffers is always 7
        for hidden in [1, 64, 2304, 4096] {
            let cfg = QwenLayerConfig {
                hidden_size: hidden,
                ffn_size: hidden * 4,
                n_heads: 4,
                n_kv_heads: 2,
                head_dim: hidden / 4,
            };
            let info = qwen_layer_info(&cfg);
            assert_eq!(info.weight_buffers, 7);
        }
    }

    // ── Config edge cases ────────────────────────────────────────────────

    #[test]
    fn config_with_very_large_values() {
        let cfg = QwenLayerConfig {
            hidden_size: 16384,
            ffn_size: 65536,
            n_heads: 128,
            n_kv_heads: 16,
            head_dim: 128,
        };
        let issues = validate_qwen_config(&cfg);
        assert!(issues.is_empty());
        let info = qwen_layer_info(&cfg);
        assert!(info.total_weight_bytes_estimate > 1_000_000_000);
    }

    #[test]
    fn config_mha_n_heads_equals_kv_heads() {
        let cfg = QwenLayerConfig {
            hidden_size: 512,
            ffn_size: 2048,
            n_heads: 8,
            n_kv_heads: 8, // MHA: n_kv_heads == n_heads
            head_dim: 64,
        };
        let issues = validate_qwen_config(&cfg);
        assert!(issues.is_empty());
    }

    #[test]
    fn config_gqa_various_ratios() {
        // GQA with 4:1 ratio
        let cfg = QwenLayerConfig {
            hidden_size: 2048,
            ffn_size: 8192,
            n_heads: 32,
            n_kv_heads: 8,
            head_dim: 64,
        };
        assert!(validate_qwen_config(&cfg).is_empty());
    }

    // ── Debug format ─────────────────────────────────────────────────────

    #[test]
    fn info_debug_format_contains_fields() {
        let info = qwen_layer_info(&default_qwen_config());
        let debug = format!("{:?}", info);
        assert!(debug.contains("QwenLayerInfo"));
        assert!(debug.contains("weight_buffers"));
        assert!(debug.contains("validation_issues"));
    }

    #[test]
    fn config_eq_derived() {
        let cfg1 = default_qwen_config();
        let cfg2 = default_qwen_config();
        // QwenLayerConfig derives Debug, Clone, Serialize but not PartialEq.
        // Compare via JSON roundtrip.
        let j1 = serde_json::to_string(&cfg1).unwrap();
        let j2 = serde_json::to_string(&cfg2).unwrap();
        assert_eq!(j1, j2);
    }

    // ── Validate: n_heads=0 with n_kv_heads=0 guard ─────────────────────

    #[test]
    fn validate_both_heads_zero_no_divisibility_issue() {
        let cfg = QwenLayerConfig {
            hidden_size: 100,
            ffn_size: 200,
            n_heads: 0,
            n_kv_heads: 0,
            head_dim: 10,
        };
        let issues = validate_qwen_config(&cfg);
        // n_heads=0 and n_kv_heads=0 → 2 issues; divisibility check guarded
        assert_eq!(issues.len(), 2);
        assert!(!issues.iter().any(|i| i.contains("divisible")));
    }

    #[test]
    fn validate_n_heads_one_n_kv_heads_one() {
        let mut cfg = default_qwen_config();
        cfg.n_heads = 1;
        cfg.n_kv_heads = 1;
        let issues = validate_qwen_config(&cfg);
        assert!(issues.is_empty());
    }
}
