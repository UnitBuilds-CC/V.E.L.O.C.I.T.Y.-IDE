//! Vulkan NDA (Nested Dissection Architecture) GEMV compute kernel dispatch.
//!
//! # Safety Invariants
//!
//! All `unsafe` blocks in this module wrap Vulkan API calls via the `ash` crate.
//! The following invariants are maintained throughout:
//! - `Device`, `Instance`, `Queue`, and all handle fields are valid and initialized in `new()`.
//! - Buffers are created via `create_coherent_buffer` / `create_device_local_buffer` which
//!   guarantee valid buffer+memory+pointer triples.
//! - Descriptor sets are allocated from a pool with sufficient capacity (5 storage buffers).
//! - Command buffers are recorded within a valid recording scope and submitted to the
//!   correct queue family. Fences synchronize completion before host reads.
//! - The `Drop` impl tears down resources in reverse dependency order.

use super::packing::*;
use super::vulkan_init::*;
use ash::vk;
use ash::Device;
use serde::Serialize;
use std::ffi::CString;
use std::time::Instant;

/// Configuration for an NDA GEMV kernel.
#[derive(Debug, Clone, Serialize)]
pub struct NdaGemvConfig {
    pub k: usize,
    pub n: usize,
    pub version: u32,
    pub scales: [f32; 3],
}

/// Diagnostic info about an NDA GEMV kernel setup.
#[derive(Debug, Clone, Serialize)]
pub struct NdaGemvInfo {
    pub config: NdaGemvConfig,
    pub input_active_bytes: usize,
    pub input_pos_bytes: usize,
    pub weight_active_bytes: usize,
    pub weight_pos_bytes: usize,
    pub output_bytes: usize,
    pub total_gpu_memory_estimate: usize,
    pub validation_issues: Vec<String>,
}

/// Validate NDA GEMV configuration.
pub fn validate_nda_gemv_config(cfg: &NdaGemvConfig) -> Vec<String> {
    let mut issues = Vec::new();
    if cfg.k == 0 {
        issues.push("k must be > 0".into());
    }
    if cfg.n == 0 {
        issues.push("n must be > 0".into());
    }
    if cfg.k % 128 != 0 {
        issues.push(format!(
            "k ({}) must be a multiple of 128 for NDA packing",
            cfg.k
        ));
    }
    issues
}

/// Build diagnostic info for an NDA GEMV kernel.
pub fn nda_gemv_info(cfg: &NdaGemvConfig) -> NdaGemvInfo {
    let issues = validate_nda_gemv_config(cfg);
    let k_words = cfg.k / 16;
    let input_bytes = k_words * 4;
    let weight_bytes = (cfg.k / 128) * cfg.n * 4 * 4;
    let output_bytes = cfg.n * 4;
    let total = input_bytes * 2 + weight_bytes * 2 + output_bytes;
    NdaGemvInfo {
        config: cfg.clone(),
        input_active_bytes: input_bytes,
        input_pos_bytes: input_bytes,
        weight_active_bytes: weight_bytes,
        weight_pos_bytes: weight_bytes,
        output_bytes,
        total_gpu_memory_estimate: total,
        validation_issues: issues,
    }
}

pub struct VulkanNdaGemv {
    pub device: Device,
    pub queue: vk::Queue,
    pub k: u32,
    pub n: u32,
    pub version: u32,
    /// Per-matrix scales for fused GEMV dispatch.
    /// [0] = primary scale, [1..2] = secondary/tertiary scales (fused projections).
    /// Currently stored for infrastructure; push-constant shader upgrade will use these.
    #[allow(dead_code)]
    pub scales: [f32; 3],

    pub shader_module: vk::ShaderModule,
    pub desc_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout: vk::PipelineLayout,
    pub compute_pipeline: vk::Pipeline,

    pub input_active_buffer: vk::Buffer,
    pub input_active_memory: vk::DeviceMemory,
    pub input_active_ptr: *mut std::ffi::c_void,

    pub input_pos_buffer: vk::Buffer,
    pub input_pos_memory: vk::DeviceMemory,
    pub input_pos_ptr: *mut std::ffi::c_void,

    pub weight_active_buffer: vk::Buffer,
    pub weight_active_memory: vk::DeviceMemory,

    pub weight_pos_buffer: vk::Buffer,
    pub weight_pos_memory: vk::DeviceMemory,

    pub output_buffer: vk::Buffer,
    pub output_memory: vk::DeviceMemory,
    pub output_ptr: *mut std::ffi::c_void,

    pub desc_pool: vk::DescriptorPool,
    pub desc_set: vk::DescriptorSet,
    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
}

impl VulkanNdaGemv {
    pub fn record_dispatch(&self, cmd: vk::CommandBuffer) {
        // SAFETY: All unsafe calls below are Vulkan command-buffer recording functions.
        // `cmd` is a valid command buffer in recording state. Pipeline, descriptor set,
        // and pipeline layout handles are valid from new(). Push constants are 8 bytes
        // (k, n as u32), matching the pipeline layout's push constant range.
        unsafe {
            self.device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.compute_pipeline,
            );
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline_layout,
                0,
                &[self.desc_set],
                &[],
            );

            let params = [self.k, self.n];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            self.device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );

            let workgroup_count_y = self.n.div_ceil(16);
            self.device.cmd_dispatch(cmd, 1, workgroup_count_y, 1);
        }
    }

    #[allow(dead_code)]
    pub fn new(
        driver: &VulkanDriver,
        k: u32,
        n: u32,
        weight_bytes: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = driver.device.clone();
        let physical_device = driver.physical_device;
        let instance = driver.instance.clone();
        let queue = driver.compute_queue;

        let shader_info =
            vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::NDA_SPV);
        // SAFETY: create_shader_module with valid NDA SPIR-V bytecode.
        let shader_module = unsafe { device.create_shader_module(&shader_info, None)? };

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
        let layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
        // SAFETY: create_descriptor_set_layout with 5 storage buffer bindings.
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
        // SAFETY: create_pipeline_layout with one descriptor set layout and 8-byte push constants.
        let pipeline_layout =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

        let main_entry = CString::new("main")?;
        let stage_info = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module)
            .name(&main_entry);
        let pipeline_create_info = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info.build())
            .layout(pipeline_layout);
        // SAFETY: create_compute_pipelines with valid shader module and pipeline layout.
        let compute_pipelines = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[pipeline_create_info.build()],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let compute_pipeline = compute_pipelines[0];

        let (active_w_bytes, pos_w_bytes) = pack_weights_nda(weight_bytes, k as usize, n as usize);

        let (input_active_buffer, input_active_memory, input_active_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (k / 8) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (input_pos_buffer, input_pos_memory, input_pos_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (k / 8) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let (weight_active_buffer, weight_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            active_w_bytes.len() as vk::DeviceSize,
            &active_w_bytes,
        )?;
        let (weight_pos_buffer, weight_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            pos_w_bytes.len() as vk::DeviceSize,
            &pos_w_bytes,
        )?;

        let (output_buffer, output_memory, output_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (n * 4) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(5)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(1)
            .pool_sizes(&pool_sizes);
        // SAFETY: create_descriptor_pool with capacity for 1 set of 5 storage buffers.
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        // SAFETY: allocate_descriptor_sets allocates one set from the pool with the layout.
        let desc_set = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&[desc_set_layout]),
            )?[0]
        };

        let buffer_infos = [
            vk::DescriptorBufferInfo::builder()
                .buffer(input_active_buffer)
                .offset(0)
                .range((k / 8) as vk::DeviceSize)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(input_pos_buffer)
                .offset(0)
                .range((k / 8) as vk::DeviceSize)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(weight_active_buffer)
                .offset(0)
                .range(active_w_bytes.len() as vk::DeviceSize)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(weight_pos_buffer)
                .offset(0)
                .range(pos_w_bytes.len() as vk::DeviceSize)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(output_buffer)
                .offset(0)
                .range((n * 4) as vk::DeviceSize)
                .build(),
        ];
        let writes = [
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos[0..1])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos[1..2])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos[2..3])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set)
                .dst_binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos[3..4])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(desc_set)
                .dst_binding(4)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_infos[4..5])
                .build(),
        ];
        // SAFETY: update_descriptor_sets writes buffer bindings to the descriptor set.
        // All buffer handles and ranges are valid from the create_coherent/device_local calls.
        unsafe { device.update_descriptor_sets(&writes, &[]) };

        let command_pool_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(driver.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        // SAFETY: create_command_pool for the compute queue family with RESET flag.
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        // SAFETY: allocate_command_buffers allocates one primary command buffer.
        let command_buffers = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::builder()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?
        };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        // SAFETY: Record compute dispatch: bind pipeline, descriptor set, push constants,
        // dispatch workgroups, end recording. All handles are valid.
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;
            device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                compute_pipeline,
            );
            device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                pipeline_layout,
                0,
                &[desc_set],
                &[],
            );

            let params = [k, n];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(
                command_buffer,
                pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );

            let workgroup_count = n.div_ceil(64);
            device.cmd_dispatch(command_buffer, workgroup_count, 1, 1);
            device.end_command_buffer(command_buffer)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        // SAFETY: create_fence for synchronizing dispatch completion.
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            k,
            n,
            version: 2,
            scales: [1.0, 0.0, 0.0],
            shader_module,
            desc_set_layout,
            pipeline_layout,
            compute_pipeline,
            input_active_buffer,
            input_active_memory,
            input_active_ptr,
            input_pos_buffer,
            input_pos_memory,
            input_pos_ptr,
            weight_active_buffer,
            weight_active_memory,
            weight_pos_buffer,
            weight_pos_memory,
            output_buffer,
            output_memory,
            output_ptr,
            desc_pool,
            desc_set,
            command_pool,
            command_buffer,
            fence,
        })
    }

    pub fn new_direct(
        driver: &VulkanDriver,
        version: u32,
        k: u32,
        n: u32,
        scales: [f32; 3],
        active_w_bytes: &[u8],
        pos_w_bytes: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = driver.device.clone();
        let physical_device = driver.physical_device;
        let instance = driver.instance.clone();
        let queue = driver.compute_queue;

        let is_float_act = version == 3 || version == 4;

        let spv_code = match version {
            3 => crate::compiler::shaders::FP4_SPV,
            4 => crate::compiler::shaders::FP2_SPV,
            _ => crate::compiler::shaders::NDA_SPV,
        };
        let shader_info = vk::ShaderModuleCreateInfo::builder().code(spv_code);
        // SAFETY: Create shader module from valid NDA SPIR-V bytecode.
        let shader_module = unsafe { device.create_shader_module(&shader_info, None)? };

        let bindings = if is_float_act {
            vec![
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
            ]
        } else {
            vec![
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
            ]
        };
        let layout_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
        // SAFETY: create_descriptor_set_layout with 5 storage buffer bindings.
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
        // SAFETY: create_pipeline_layout with one descriptor set layout and 8-byte push constants.
        let pipeline_layout =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

        let main_entry = CString::new("main")?;
        let stage_info = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module)
            .name(&main_entry);
        let pipeline_create_info = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info.build())
            .layout(pipeline_layout);
        // SAFETY: create_compute_pipelines with valid shader module and pipeline layout.
        let compute_pipelines = unsafe {
            device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[pipeline_create_info.build()],
                    None,
                )
                .map_err(|(_, e)| e)?
        };
        let compute_pipeline = compute_pipelines[0];

        let input_active_size = if is_float_act {
            (k * 4) as vk::DeviceSize
        } else {
            (k / 8) as vk::DeviceSize
        };
        let (input_active_buffer, input_active_memory, input_active_ptr) = if is_float_act {
            (
                driver.shared_input_buffer,
                driver.shared_input_memory,
                driver.shared_input_ptr,
            )
        } else {
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                input_active_size,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?
        };

        let (input_pos_buffer, input_pos_memory, input_pos_ptr) = if is_float_act {
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                4 as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?
        } else {
            create_coherent_buffer(
                &device,
                &instance,
                physical_device,
                (k / 8) as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?
        };

        let (weight_active_buffer, weight_active_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            active_w_bytes.len() as vk::DeviceSize,
            active_w_bytes,
        )?;
        let (weight_pos_buffer, weight_pos_memory) = create_device_local_buffer(
            &device,
            &instance,
            physical_device,
            queue,
            driver.queue_family_index,
            pos_w_bytes.len() as vk::DeviceSize,
            pos_w_bytes,
        )?;

        let (output_buffer, output_memory, output_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            (n * 4) as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let num_bindings = if is_float_act { 4 } else { 5 };
        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(num_bindings)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(1)
            .pool_sizes(&pool_sizes);
        // SAFETY: create_descriptor_pool with capacity for 1 set of 5 storage buffers.
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        // SAFETY: allocate_descriptor_sets allocates one set from the pool with the layout.
        let desc_set = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::builder()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&[desc_set_layout]),
            )?[0]
        };

        let buffer_infos = if is_float_act {
            vec![
                vk::DescriptorBufferInfo::builder()
                    .buffer(input_active_buffer)
                    .offset(0)
                    .range(input_active_size)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(weight_active_buffer)
                    .offset(0)
                    .range(active_w_bytes.len() as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(weight_pos_buffer)
                    .offset(0)
                    .range(pos_w_bytes.len() as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(output_buffer)
                    .offset(0)
                    .range((n * 4) as vk::DeviceSize)
                    .build(),
            ]
        } else {
            vec![
                vk::DescriptorBufferInfo::builder()
                    .buffer(input_active_buffer)
                    .offset(0)
                    .range((k / 8) as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(input_pos_buffer)
                    .offset(0)
                    .range((k / 8) as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(weight_active_buffer)
                    .offset(0)
                    .range(active_w_bytes.len() as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(weight_pos_buffer)
                    .offset(0)
                    .range(pos_w_bytes.len() as vk::DeviceSize)
                    .build(),
                vk::DescriptorBufferInfo::builder()
                    .buffer(output_buffer)
                    .offset(0)
                    .range((n * 4) as vk::DeviceSize)
                    .build(),
            ]
        };

        let mut writes = Vec::new();
        for i in 0..num_bindings as usize {
            writes.push(
                vk::WriteDescriptorSet::builder()
                    .dst_set(desc_set)
                    .dst_binding(i as u32)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_infos[i..i + 1])
                    .build(),
            );
        }
        // SAFETY: Update descriptor sets with NDA buffer bindings. All handles valid.
        unsafe { device.update_descriptor_sets(&writes, &[]) };

        let command_pool_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(driver.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        // SAFETY: Create command pool for NDA dispatch recording.
        let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };
        // SAFETY: Allocate primary command buffer from pool.
        let command_buffers = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::builder()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?
        };
        let command_buffer = command_buffers[0];

        let begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);
        // SAFETY: Record compute dispatch: bind pipeline, descriptor set, push constants,
        // dispatch workgroups, end recording. All handles are valid.
        unsafe {
            device.begin_command_buffer(command_buffer, &begin_info)?;
            device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                compute_pipeline,
            );
            device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                pipeline_layout,
                0,
                &[desc_set],
                &[],
            );

            let params = [k, n];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(
                command_buffer,
                pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );

            let workgroup_count_y = n.div_ceil(16);
            device.cmd_dispatch(command_buffer, 1, workgroup_count_y, 1);
            device.end_command_buffer(command_buffer)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        // SAFETY: create_fence for synchronizing dispatch completion.
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            k,
            n,
            version,
            scales,
            shader_module,
            desc_set_layout,
            pipeline_layout,
            compute_pipeline,
            input_active_buffer,
            input_active_memory,
            input_active_ptr,
            input_pos_buffer,
            input_pos_memory,
            input_pos_ptr,
            weight_active_buffer,
            weight_active_memory,
            weight_pos_buffer,
            weight_pos_memory,
            output_buffer,
            output_memory,
            output_ptr,
            desc_pool,
            desc_set,
            command_pool,
            command_buffer,
            fence,
        })
    }

    pub fn run_float(
        &self,
        input_floats: &[f32],
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        // SAFETY: Copy input floats into the HOST_VISIBLE mapped input buffers.
        // Pointers are valid from create_coherent_buffer; lengths fit within buffer sizes.
        unsafe {
            std::ptr::copy_nonoverlapping(
                input_floats.as_ptr(),
                self.input_active_ptr as *mut f32,
                input_floats.len(),
            );
        }

        let start = Instant::now();
        // SAFETY: Reset fence, submit command buffer to compute queue, wait for completion.
        // The command buffer was recorded in new() with SIMULTANEOUS_USE flag.
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[self.command_buffer])
                    .build()],
                self.fence,
            )?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        // SAFETY: Copy output floats from the HOST_VISIBLE mapped output buffer.
        // Pointer is valid from create_coherent_buffer; length fits within buffer size.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.output_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }

    #[allow(dead_code)]
    pub fn submit_async_float(
        &self,
        input_floats: &[f32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        // SAFETY: Copy input floats to GPU-mapped buffer, reset fence, and submit dispatch.
        // `input_active_ptr` is a valid mapped pointer from create_coherent_buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(
                input_floats.as_ptr(),
                self.input_active_ptr as *mut f32,
                input_floats.len(),
            );
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[self.command_buffer])
                    .build()],
                self.fence,
            )?;
        }
        Ok(())
    }

    pub fn wait_and_copy_float(
        &self,
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        let start = Instant::now();
        // SAFETY: Wait for fence to signal GPU completion.
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        // SAFETY: Copy output floats from GPU-mapped buffer to caller slice.
        // `output_ptr` is a valid mapped pointer from create_coherent_buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.output_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }

    pub fn submit_async_float_no_copy(&self) -> Result<(), Box<dyn std::error::Error>> {
        // SAFETY: Reset fence and submit NDA dispatch command buffer (no-copy variant).
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[self.command_buffer])
                    .build()],
                self.fence,
            )?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn run_float_no_copy(
        &self,
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        let start = Instant::now();
        // SAFETY: Reset fence, submit dispatch, and wait for fence completion.
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[self.command_buffer])
                    .build()],
                self.fence,
            )?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        // SAFETY: Copy output floats from GPU-mapped buffer to caller slice.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.output_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }

    pub fn run(
        &self,
        input_active: &[u8],
        input_pos: &[u8],
        output_floats: &mut [f32],
    ) -> Result<f64, Box<dyn std::error::Error>> {
        // SAFETY: Copy packed input bytes into HOST_VISIBLE mapped buffers.
        // Pointers are valid from create_coherent_buffer; lengths fit within buffer sizes.
        unsafe {
            std::ptr::copy_nonoverlapping(
                input_active.as_ptr(),
                self.input_active_ptr as *mut u8,
                input_active.len(),
            );
            std::ptr::copy_nonoverlapping(
                input_pos.as_ptr(),
                self.input_pos_ptr as *mut u8,
                input_pos.len(),
            );
        }

        let start = Instant::now();
        // SAFETY: Reset fence, submit command buffer, wait for GPU completion.
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[self.command_buffer])
                    .build()],
                self.fence,
            )?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        // SAFETY: Copy output floats from HOST_VISIBLE mapped output buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.output_ptr as *const f32,
                output_floats.as_mut_ptr(),
                output_floats.len(),
            );
        }

        Ok(duration_us)
    }
}

impl Drop for VulkanNdaGemv {
    fn drop(&mut self) {
        // SAFETY: Vulkan resource teardown in correct dependency order:
        // wait_idle → destroy fence → destroy command pool → destroy descriptor pool →
        // destroy buffers/memory → destroy pipeline → destroy pipeline layout →
        // destroy descriptor set layout → destroy shader module.
        // All handles are valid and owned by this struct.
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);

            let destroy_buffer =
                |device: &Device, buffer: vk::Buffer, memory: vk::DeviceMemory, mapped: bool| {
                    if mapped {
                        device.unmap_memory(memory);
                    }
                    device.free_memory(memory, None);
                    device.destroy_buffer(buffer, None);
                };

            let is_float_act = self.version == 3 || self.version == 4;
            if !is_float_act {
                destroy_buffer(
                    &self.device,
                    self.input_active_buffer,
                    self.input_active_memory,
                    true,
                );
            }
            destroy_buffer(
                &self.device,
                self.input_pos_buffer,
                self.input_pos_memory,
                true,
            );
            destroy_buffer(
                &self.device,
                self.weight_active_buffer,
                self.weight_active_memory,
                false,
            );
            destroy_buffer(
                &self.device,
                self.weight_pos_buffer,
                self.weight_pos_memory,
                false,
            );
            destroy_buffer(&self.device, self.output_buffer, self.output_memory, true);

            self.device.destroy_pipeline(self.compute_pipeline, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.desc_set_layout, None);
            self.device.destroy_shader_module(self.shader_module, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_gemv_config() -> NdaGemvConfig {
        NdaGemvConfig {
            k: 128,
            n: 3200,
            version: 1,
            scales: [1.0, 1.0, 1.0],
        }
    }

    #[test]
    fn validate_gemv_config_valid() {
        assert!(validate_nda_gemv_config(&default_gemv_config()).is_empty());
    }

    #[test]
    fn validate_gemv_config_zero_k() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        let issues = validate_nda_gemv_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("k must")));
    }

    #[test]
    fn validate_gemv_config_bad_k() {
        let mut cfg = default_gemv_config();
        cfg.k = 64; // not multiple of 128
        assert!(validate_nda_gemv_config(&cfg).iter().any(|i| i.contains("multiple of 128")));
    }

    #[test]
    fn validate_gemv_config_zero_n() {
        let mut cfg = default_gemv_config();
        cfg.n = 0;
        assert!(validate_nda_gemv_config(&cfg).iter().any(|i| i.contains("n must")));
    }

    #[test]
    fn nda_gemv_info_works() {
        let cfg = default_gemv_config();
        let info = nda_gemv_info(&cfg);
        assert!(info.validation_issues.is_empty());
        assert_eq!(info.input_active_bytes, 32); // 128/16 * 4
        assert_eq!(info.output_bytes, 12800); // 3200 * 4
        assert!(info.total_gpu_memory_estimate > 0);
    }

    #[test]
    fn nda_gemv_info_with_issues() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        cfg.n = 0;
        let info = nda_gemv_info(&cfg);
        assert!(info.validation_issues.len() >= 2);
    }

    #[test]
    fn nda_gemv_config_serializes() {
        let json = serde_json::to_string(&default_gemv_config()).unwrap();
        assert!(json.contains("\"k\":128"));
        assert!(json.contains("scales"));
    }

    #[test]
    fn nda_gemv_info_serializes() {
        let info = nda_gemv_info(&default_gemv_config());
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("total_gpu_memory_estimate"));
        assert!(json.contains("input_active_bytes"));
    }

    // ── Validation: multiple issues ──────────────────────────────────────

    #[test]
    fn validate_zero_k_triggers_only_k_must() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        let issues = validate_nda_gemv_config(&cfg);
        // k=0 triggers "k must be > 0" only; 0 % 128 == 0 so no "multiple" issue
        assert!(issues.iter().any(|i| i.contains("k must")));
        assert!(!issues.iter().any(|i| i.contains("multiple of 128")));
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn validate_all_three_issues() {
        let mut cfg = default_gemv_config();
        cfg.k = 64; // not multiple of 128, but > 0
        cfg.n = 0;
        let issues = validate_nda_gemv_config(&cfg);
        // k=64: "k (64) must be a multiple of 128"
        // n=0: "n must be > 0"
        assert_eq!(issues.len(), 2);
        assert!(issues.iter().any(|i| i.contains("multiple of 128")));
        assert!(issues.iter().any(|i| i.contains("n must")));
    }

    #[test]
    fn validate_k_not_multiple_of_128_nonzero() {
        let mut cfg = default_gemv_config();
        cfg.k = 256 + 64; // 320, not a multiple of 128
        let issues = validate_nda_gemv_config(&cfg);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("multiple of 128"));
    }

    #[test]
    fn validate_k_128_valid() {
        let mut cfg = default_gemv_config();
        cfg.k = 128;
        assert!(validate_nda_gemv_config(&cfg).is_empty());
    }

    #[test]
    fn validate_k_256_valid() {
        let mut cfg = default_gemv_config();
        cfg.k = 256;
        assert!(validate_nda_gemv_config(&cfg).is_empty());
    }

    #[test]
    fn validate_k_4096_valid() {
        let mut cfg = default_gemv_config();
        cfg.k = 4096;
        assert!(validate_nda_gemv_config(&cfg).is_empty());
    }

    #[test]
    fn validate_k_1_invalid() {
        let mut cfg = default_gemv_config();
        cfg.k = 1;
        let issues = validate_nda_gemv_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("multiple of 128")));
    }

    #[test]
    fn validate_n_1_valid() {
        let mut cfg = default_gemv_config();
        cfg.n = 1;
        assert!(validate_nda_gemv_config(&cfg).is_empty());
    }

    #[test]
    fn validate_n_max_valid() {
        let mut cfg = default_gemv_config();
        cfg.n = usize::MAX;
        assert!(validate_nda_gemv_config(&cfg).is_empty());
    }

    // ── Validation issue text ────────────────────────────────────────────

    #[test]
    fn validate_k_zero_issue_text() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        let issues = validate_nda_gemv_config(&cfg);
        assert_eq!(issues[0], "k must be > 0");
    }

    #[test]
    fn validate_n_zero_issue_text() {
        let mut cfg = default_gemv_config();
        cfg.n = 0;
        let issues = validate_nda_gemv_config(&cfg);
        assert_eq!(issues[0], "n must be > 0");
    }

    #[test]
    fn validate_k_multiple_issue_includes_value() {
        let mut cfg = default_gemv_config();
        cfg.k = 64;
        let issues = validate_nda_gemv_config(&cfg);
        assert!(issues[0].contains("64"));
    }

    #[test]
    fn validate_issues_order_deterministic() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        cfg.n = 0;
        let i1 = validate_nda_gemv_config(&cfg);
        let i2 = validate_nda_gemv_config(&cfg);
        assert_eq!(i1, i2);
    }

    // ── Info calculations ────────────────────────────────────────────────

    #[test]
    fn info_input_active_bytes_formula() {
        let cfg = default_gemv_config();
        let info = nda_gemv_info(&cfg);
        // k/16 * 4 = 128/16 * 4 = 32
        assert_eq!(info.input_active_bytes, cfg.k / 16 * 4);
    }

    #[test]
    fn info_input_pos_bytes_equals_active() {
        let info = nda_gemv_info(&default_gemv_config());
        assert_eq!(info.input_pos_bytes, info.input_active_bytes);
    }

    #[test]
    fn info_weight_active_bytes_formula() {
        let cfg = default_gemv_config();
        let info = nda_gemv_info(&cfg);
        // (k/128) * n * 4 * 4 = (128/128) * 3200 * 16 = 51200
        let expected = (cfg.k / 128) * cfg.n * 4 * 4;
        assert_eq!(info.weight_active_bytes, expected);
    }

    #[test]
    fn info_weight_pos_bytes_equals_active() {
        let info = nda_gemv_info(&default_gemv_config());
        assert_eq!(info.weight_pos_bytes, info.weight_active_bytes);
    }

    #[test]
    fn info_output_bytes_formula() {
        let cfg = default_gemv_config();
        let info = nda_gemv_info(&cfg);
        assert_eq!(info.output_bytes, cfg.n * 4);
    }

    #[test]
    fn info_total_memory_formula() {
        let cfg = default_gemv_config();
        let info = nda_gemv_info(&cfg);
        let expected = info.input_active_bytes * 2
            + info.weight_active_bytes * 2
            + info.output_bytes;
        assert_eq!(info.total_gpu_memory_estimate, expected);
    }

    #[test]
    fn info_minimal_config() {
        let cfg = NdaGemvConfig {
            k: 128,
            n: 1,
            version: 1,
            scales: [1.0, 0.5, 0.25],
        };
        let info = nda_gemv_info(&cfg);
        assert!(info.validation_issues.is_empty());
        assert_eq!(info.input_active_bytes, 32); // 128/16*4
        assert_eq!(info.output_bytes, 4); // 1*4
        assert_eq!(info.weight_active_bytes, 16); // (128/128)*1*4*4
        assert!(info.total_gpu_memory_estimate > 0);
    }

    #[test]
    fn info_large_config() {
        let cfg = NdaGemvConfig {
            k: 8192,
            n: 3200,
            version: 1,
            scales: [1.0, 1.0, 1.0],
        };
        let info = nda_gemv_info(&cfg);
        assert!(info.validation_issues.is_empty());
        assert_eq!(info.input_active_bytes, 8192 / 16 * 4);
        assert_eq!(info.weight_active_bytes, (8192 / 128) * 3200 * 4 * 4);
        assert!(info.total_gpu_memory_estimate > 1_000_000);
    }

    #[test]
    fn info_preserves_config() {
        let cfg = default_gemv_config();
        let info = nda_gemv_info(&cfg);
        assert_eq!(info.config.k, cfg.k);
        assert_eq!(info.config.n, cfg.n);
        assert_eq!(info.config.version, cfg.version);
        assert_eq!(info.config.scales, cfg.scales);
    }

    #[test]
    fn info_with_invalid_config_has_issues() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        cfg.n = 0;
        let info = nda_gemv_info(&cfg);
        assert!(!info.validation_issues.is_empty());
        // Memory calculations still proceed even with invalid config
        assert_eq!(info.output_bytes, 0); // n=0 → 0*4=0
    }

    // ── Struct derives ───────────────────────────────────────────────────

    #[test]
    fn config_clone() {
        let cfg = default_gemv_config();
        let cloned = cfg.clone();
        assert_eq!(cloned.k, cfg.k);
        assert_eq!(cloned.n, cfg.n);
        assert_eq!(cloned.version, cfg.version);
        assert_eq!(cloned.scales, cfg.scales);
    }

    #[test]
    fn config_clone_independent() {
        let cfg = default_gemv_config();
        let mut cloned = cfg.clone();
        cloned.k = 999;
        assert_ne!(cfg.k, cloned.k);
    }

    #[test]
    fn config_debug_format() {
        let cfg = default_gemv_config();
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("NdaGemvConfig"));
        assert!(debug.contains("k: 128"));
        assert!(debug.contains("n: 3200"));
    }

    #[test]
    fn info_clone() {
        let info = nda_gemv_info(&default_gemv_config());
        let cloned = info.clone();
        assert_eq!(cloned.input_active_bytes, info.input_active_bytes);
        assert_eq!(cloned.output_bytes, info.output_bytes);
        assert_eq!(cloned.total_gpu_memory_estimate, info.total_gpu_memory_estimate);
        assert_eq!(cloned.validation_issues, info.validation_issues);
    }

    #[test]
    fn info_debug_format() {
        let info = nda_gemv_info(&default_gemv_config());
        let debug = format!("{:?}", info);
        assert!(debug.contains("NdaGemvInfo"));
        assert!(debug.contains("input_active_bytes"));
        assert!(debug.contains("output_bytes"));
    }

    // ── Serialization ────────────────────────────────────────────────────

    #[test]
    fn config_json_all_fields() {
        let cfg = default_gemv_config();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"k\""));
        assert!(json.contains("\"n\""));
        assert!(json.contains("\"version\""));
        assert!(json.contains("\"scales\""));
    }

    #[test]
    fn config_json_values_correct() {
        let cfg = default_gemv_config();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("128"));
        assert!(json.contains("3200"));
    }

    #[test]
    fn info_json_all_fields() {
        let info = nda_gemv_info(&default_gemv_config());
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("config"));
        assert!(json.contains("input_active_bytes"));
        assert!(json.contains("input_pos_bytes"));
        assert!(json.contains("weight_active_bytes"));
        assert!(json.contains("weight_pos_bytes"));
        assert!(json.contains("output_bytes"));
        assert!(json.contains("total_gpu_memory_estimate"));
        assert!(json.contains("validation_issues"));
    }

    #[test]
    fn info_pretty_json() {
        let info = nda_gemv_info(&default_gemv_config());
        let pretty = serde_json::to_string_pretty(&info).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));
    }

    #[test]
    fn info_json_parseable_as_value() {
        let info = nda_gemv_info(&default_gemv_config());
        let json = serde_json::to_string(&info).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["input_active_bytes"], 32);
        assert_eq!(value["output_bytes"], 12800);
        assert!(value["validation_issues"].is_array());
        assert!(value["validation_issues"].as_array().unwrap().is_empty());
    }

    #[test]
    fn info_json_with_issues() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        let info = nda_gemv_info(&cfg);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("validation_issues"));
        assert!(json.contains("k must"));
    }

    // ── Scales variations ────────────────────────────────────────────────

    #[test]
    fn config_custom_scales() {
        let cfg = NdaGemvConfig {
            k: 256,
            n: 64,
            version: 2,
            scales: [0.5, 0.25, 0.125],
        };
        assert!(validate_nda_gemv_config(&cfg).is_empty());
        let info = nda_gemv_info(&cfg);
        assert_eq!(info.config.scales[0], 0.5);
        assert_eq!(info.config.scales[1], 0.25);
        assert_eq!(info.config.scales[2], 0.125);
    }

    #[test]
    fn config_zero_scales_still_valid() {
        let mut cfg = default_gemv_config();
        cfg.scales = [0.0, 0.0, 0.0];
        // Validation only checks k and n, not scales
        assert!(validate_nda_gemv_config(&cfg).is_empty());
    }

    #[test]
    fn config_negative_scales_still_valid() {
        let mut cfg = default_gemv_config();
        cfg.scales = [-1.0, -0.5, 0.0];
        assert!(validate_nda_gemv_config(&cfg).is_empty());
    }

    #[test]
    fn config_version_variations() {
        for v in [0, 1, 42, u32::MAX] {
            let mut cfg = default_gemv_config();
            cfg.version = v;
            assert!(validate_nda_gemv_config(&cfg).is_empty());
        }
    }

    // ── JSON key counts ──────────────────────────────────────────────────

    #[test]
    fn config_json_has_exactly_4_keys() {
        let json = serde_json::to_string(&default_gemv_config()).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 4);
    }

    #[test]
    fn info_json_has_exactly_8_keys() {
        let info = nda_gemv_info(&default_gemv_config());
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 8);
    }

    // ── Memory calculations for various k/n ──────────────────────────────

    #[test]
    fn info_k256_n64_memory() {
        let cfg = NdaGemvConfig {
            k: 256, n: 64, version: 1, scales: [1.0; 3],
        };
        let info = nda_gemv_info(&cfg);
        // k_words = 256/16 = 16, input = 16*4 = 64
        assert_eq!(info.input_active_bytes, 64);
        // weight = (256/128)*64*4*4 = 2*64*16 = 2048
        assert_eq!(info.weight_active_bytes, 2048);
        // output = 64*4 = 256
        assert_eq!(info.output_bytes, 256);
        // total = 64*2 + 2048*2 + 256 = 128 + 4096 + 256 = 4480
        assert_eq!(info.total_gpu_memory_estimate, 4480);
    }

    #[test]
    fn info_k512_n128_memory() {
        let cfg = NdaGemvConfig {
            k: 512, n: 128, version: 1, scales: [1.0; 3],
        };
        let info = nda_gemv_info(&cfg);
        // k_words = 512/16 = 32, input = 32*4 = 128
        assert_eq!(info.input_active_bytes, 128);
        // weight = (512/128)*128*4*4 = 4*128*16 = 8192
        assert_eq!(info.weight_active_bytes, 8192);
        // output = 128*4 = 512
        assert_eq!(info.output_bytes, 512);
        // total = 128*2 + 8192*2 + 512 = 256 + 16384 + 512 = 17152
        assert_eq!(info.total_gpu_memory_estimate, 17152);
    }

    #[test]
    fn info_k128_n1_minimal_memory() {
        let cfg = NdaGemvConfig {
            k: 128, n: 1, version: 1, scales: [1.0; 3],
        };
        let info = nda_gemv_info(&cfg);
        // input = 128/16*4 = 32
        assert_eq!(info.input_active_bytes, 32);
        // weight = 1*1*4*4 = 16
        assert_eq!(info.weight_active_bytes, 16);
        // output = 1*4 = 4
        assert_eq!(info.output_bytes, 4);
        // total = 32*2 + 16*2 + 4 = 64 + 32 + 4 = 100
        assert_eq!(info.total_gpu_memory_estimate, 100);
    }

    #[test]
    fn info_k0_produces_zero_input() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        let info = nda_gemv_info(&cfg);
        // k=0: k_words=0/16=0, input=0*4=0
        assert_eq!(info.input_active_bytes, 0);
        assert_eq!(info.input_pos_bytes, 0);
        // weight = (0/128)*n*4*4 = 0
        assert_eq!(info.weight_active_bytes, 0);
    }

    #[test]
    fn info_n0_produces_zero_output() {
        let mut cfg = default_gemv_config();
        cfg.n = 0;
        let info = nda_gemv_info(&cfg);
        assert_eq!(info.output_bytes, 0);
        // weight = (128/128)*0*4*4 = 0
        assert_eq!(info.weight_active_bytes, 0);
        // total = 32*2 + 0*2 + 0 = 64
        assert_eq!(info.total_gpu_memory_estimate, 64);
    }

    #[test]
    fn info_both_zero_all_zero() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        cfg.n = 0;
        let info = nda_gemv_info(&cfg);
        assert_eq!(info.input_active_bytes, 0);
        assert_eq!(info.weight_active_bytes, 0);
        assert_eq!(info.output_bytes, 0);
        assert_eq!(info.total_gpu_memory_estimate, 0);
    }

    // ── Info clone independence ──────────────────────────────────────────

    #[test]
    fn info_clone_independent_validation_issues() {
        let mut cfg = default_gemv_config();
        cfg.k = 64;
        let info = nda_gemv_info(&cfg);
        let mut cloned = info.clone();
        cloned.validation_issues.push("extra".to_string());
        assert_ne!(info.validation_issues.len(), cloned.validation_issues.len());
    }

    #[test]
    fn info_clone_independent_config() {
        let info = nda_gemv_info(&default_gemv_config());
        let mut cloned = info.clone();
        cloned.config.k = 999;
        assert_ne!(info.config.k, cloned.config.k);
    }

    // ── Symmetry invariants ──────────────────────────────────────────────

    #[test]
    fn info_input_symmetry_multiple_configs() {
        for k in [128, 256, 512, 1024, 4096] {
            let mut cfg = default_gemv_config();
            cfg.k = k;
            let info = nda_gemv_info(&cfg);
            assert_eq!(info.input_active_bytes, info.input_pos_bytes,
                "input symmetry broken for k={}", k);
        }
    }

    #[test]
    fn info_weight_symmetry_multiple_configs() {
        for n in [1, 16, 64, 128, 3200] {
            let mut cfg = default_gemv_config();
            cfg.n = n;
            let info = nda_gemv_info(&cfg);
            assert_eq!(info.weight_active_bytes, info.weight_pos_bytes,
                "weight symmetry broken for n={}", n);
        }
    }

    // ── Validation issues propagation ────────────────────────────────────

    #[test]
    fn info_issues_match_validate_function() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        cfg.n = 0;
        let info = nda_gemv_info(&cfg);
        let direct = validate_nda_gemv_config(&cfg);
        assert_eq!(info.validation_issues, direct);
    }

    #[test]
    fn info_issues_count_for_k_not_multiple() {
        let mut cfg = default_gemv_config();
        cfg.k = 64;
        let info = nda_gemv_info(&cfg);
        assert_eq!(info.validation_issues.len(), 1);
    }

    #[test]
    fn info_issues_empty_for_valid() {
        let info = nda_gemv_info(&default_gemv_config());
        assert!(info.validation_issues.is_empty());
    }

    // ── JSON value verification ──────────────────────────────────────────

    #[test]
    fn config_json_scales_array() {
        let cfg = NdaGemvConfig {
            k: 128, n: 64, version: 1, scales: [0.5, 0.25, 0.125],
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let scales = val["scales"].as_array().unwrap();
        assert_eq!(scales.len(), 3);
        assert!((scales[0].as_f64().unwrap() - 0.5).abs() < 1e-6);
        assert!((scales[1].as_f64().unwrap() - 0.25).abs() < 1e-6);
        assert!((scales[2].as_f64().unwrap() - 0.125).abs() < 1e-6);
    }

    #[test]
    fn config_json_numeric_types() {
        let val: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&default_gemv_config()).unwrap()
        ).unwrap();
        assert!(val["k"].is_number());
        assert!(val["n"].is_number());
        assert!(val["version"].is_number());
        assert!(val["scales"].is_array());
    }

    #[test]
    fn info_json_validation_issues_array_content() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        cfg.n = 0;
        let info = nda_gemv_info(&cfg);
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let issues = val["validation_issues"].as_array().unwrap();
        assert_eq!(issues.len(), 2);
        assert!(issues[0].as_str().unwrap().contains("k must"));
        assert!(issues[1].as_str().unwrap().contains("n must"));
    }

    #[test]
    fn info_json_config_nested() {
        let info = nda_gemv_info(&default_gemv_config());
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["config"].is_object());
        assert_eq!(val["config"]["k"], 128);
        assert_eq!(val["config"]["n"], 3200);
    }

    #[test]
    fn info_json_numeric_values() {
        let json = serde_json::to_string(&nda_gemv_info(&default_gemv_config())).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["input_active_bytes"], 32);
        assert_eq!(val["input_pos_bytes"], 32);
        assert_eq!(val["weight_active_bytes"], 51200);
        assert_eq!(val["weight_pos_bytes"], 51200);
        assert_eq!(val["output_bytes"], 12800);
    }

    // ── Total memory breakdown ───────────────────────────────────────────

    #[test]
    fn info_total_equals_sum_of_parts() {
        for (k, n) in [(128, 1), (128, 3200), (256, 64), (512, 128), (8192, 3200)] {
            let cfg = NdaGemvConfig { k, n, version: 1, scales: [1.0; 3] };
            let info = nda_gemv_info(&cfg);
            let expected = info.input_active_bytes * 2
                + info.weight_active_bytes * 2
                + info.output_bytes;
            assert_eq!(info.total_gpu_memory_estimate, expected,
                "total breakdown wrong for k={}, n={}", k, n);
        }
    }

    #[test]
    fn info_total_doubles_input_and_weight() {
        let cfg = default_gemv_config();
        let info = nda_gemv_info(&cfg);
        // total = input*2 + weight*2 + output
        // So total - output should equal input*2 + weight*2
        let double_part = info.total_gpu_memory_estimate - info.output_bytes;
        assert_eq!(double_part, info.input_active_bytes * 2 + info.weight_active_bytes * 2);
    }

    // ── Debug format details ─────────────────────────────────────────────

    #[test]
    fn config_debug_includes_scales() {
        let cfg = NdaGemvConfig {
            k: 128, n: 64, version: 3, scales: [0.5, 0.25, 0.125],
        };
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("scales:"));
        assert!(debug.contains("version: 3"));
    }

    #[test]
    fn info_debug_includes_validation_issues() {
        let mut cfg = default_gemv_config();
        cfg.k = 0;
        let info = nda_gemv_info(&cfg);
        let debug = format!("{:?}", info);
        assert!(debug.contains("validation_issues"));
        assert!(debug.contains("k must"));
    }

    #[test]
    fn info_debug_includes_all_memory_fields() {
        let info = nda_gemv_info(&default_gemv_config());
        let debug = format!("{:?}", info);
        assert!(debug.contains("input_active_bytes"));
        assert!(debug.contains("input_pos_bytes"));
        assert!(debug.contains("weight_active_bytes"));
        assert!(debug.contains("weight_pos_bytes"));
        assert!(debug.contains("output_bytes"));
        assert!(debug.contains("total_gpu_memory_estimate"));
    }

    // ── Scaling behavior ─────────────────────────────────────────────────

    #[test]
    fn info_weight_scales_linearly_with_n() {
        let cfg1 = NdaGemvConfig { k: 128, n: 100, version: 1, scales: [1.0; 3] };
        let cfg2 = NdaGemvConfig { k: 128, n: 200, version: 1, scales: [1.0; 3] };
        let info1 = nda_gemv_info(&cfg1);
        let info2 = nda_gemv_info(&cfg2);
        assert_eq!(info2.weight_active_bytes, info1.weight_active_bytes * 2);
        assert_eq!(info2.output_bytes, info1.output_bytes * 2);
    }

    #[test]
    fn info_weight_scales_linearly_with_k_groups() {
        let cfg1 = NdaGemvConfig { k: 128, n: 64, version: 1, scales: [1.0; 3] };
        let cfg2 = NdaGemvConfig { k: 256, n: 64, version: 1, scales: [1.0; 3] };
        let info1 = nda_gemv_info(&cfg1);
        let info2 = nda_gemv_info(&cfg2);
        // k doubled → k/128 doubled → weight doubled
        assert_eq!(info2.weight_active_bytes, info1.weight_active_bytes * 2);
        // input also doubled (k/16 doubled)
        assert_eq!(info2.input_active_bytes, info1.input_active_bytes * 2);
    }

    #[test]
    fn info_output_scales_with_n_only() {
        let cfg1 = NdaGemvConfig { k: 128, n: 100, version: 1, scales: [1.0; 3] };
        let cfg2 = NdaGemvConfig { k: 256, n: 100, version: 1, scales: [1.0; 3] };
        let info1 = nda_gemv_info(&cfg1);
        let info2 = nda_gemv_info(&cfg2);
        // Same n → same output_bytes
        assert_eq!(info1.output_bytes, info2.output_bytes);
    }

    // ── Validation edge cases ────────────────────────────────────────────

    #[test]
    fn validate_k_127_invalid() {
        let mut cfg = default_gemv_config();
        cfg.k = 127;
        let issues = validate_nda_gemv_config(&cfg);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("multiple of 128"));
    }

    #[test]
    fn validate_k_129_invalid() {
        let mut cfg = default_gemv_config();
        cfg.k = 129;
        let issues = validate_nda_gemv_config(&cfg);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("multiple of 128"));
    }

    #[test]
    fn validate_k_1024_valid() {
        let mut cfg = default_gemv_config();
        cfg.k = 1024;
        assert!(validate_nda_gemv_config(&cfg).is_empty());
    }

    #[test]
    fn validate_k_16384_valid() {
        let mut cfg = default_gemv_config();
        cfg.k = 16384;
        assert!(validate_nda_gemv_config(&cfg).is_empty());
    }

    #[test]
    fn validate_multiple_calls_same_result() {
        let cfg = default_gemv_config();
        for _ in 0..10 {
            assert!(validate_nda_gemv_config(&cfg).is_empty());
        }
    }

    // ── Scales preserved through info ────────────────────────────────────

    #[test]
    fn info_preserves_custom_scales() {
        let cfg = NdaGemvConfig {
            k: 128, n: 64, version: 1, scales: [3.14, 2.71, 1.41],
        };
        let info = nda_gemv_info(&cfg);
        assert_eq!(info.config.scales[0], 3.14);
        assert_eq!(info.config.scales[1], 2.71);
        assert_eq!(info.config.scales[2], 1.41);
    }

    #[test]
    fn info_preserves_version() {
        let cfg = NdaGemvConfig {
            k: 128, n: 64, version: 42, scales: [1.0; 3],
        };
        let info = nda_gemv_info(&cfg);
        assert_eq!(info.config.version, 42);
    }

    // ── JSON roundtrip via Value ─────────────────────────────────────────

    #[test]
    fn config_json_roundtrip_via_value() {
        let cfg = NdaGemvConfig {
            k: 256, n: 128, version: 3, scales: [0.5, 0.25, 0.125],
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["k"], 256);
        assert_eq!(val["n"], 128);
        assert_eq!(val["version"], 3);
    }

    #[test]
    fn info_json_roundtrip_via_value() {
        let cfg = NdaGemvConfig {
            k: 512, n: 256, version: 2, scales: [1.0, 0.5, 0.25],
        };
        let info = nda_gemv_info(&cfg);
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["config"]["k"], 512);
        assert_eq!(val["config"]["n"], 256);
        assert_eq!(val["output_bytes"], 256 * 4);
        assert!(val["validation_issues"].as_array().unwrap().is_empty());
    }

    // ── Large config stress ──────────────────────────────────────────────

    #[test]
    fn info_very_large_config() {
        let cfg = NdaGemvConfig {
            k: 16384, n: 10000, version: 1, scales: [1.0; 3],
        };
        let info = nda_gemv_info(&cfg);
        assert!(info.validation_issues.is_empty());
        assert!(info.total_gpu_memory_estimate > 10_000_000);
        // input = 16384/16*4 = 4096
        assert_eq!(info.input_active_bytes, 4096);
        // weight = (16384/128)*10000*4*4 = 128*10000*16 = 20_480_000
        assert_eq!(info.weight_active_bytes, 20_480_000);
        // output = 10000*4 = 40000
        assert_eq!(info.output_bytes, 40000);
    }

    #[test]
    #[should_panic(expected = "multiply with overflow")]
    fn info_n_usize_max_overflows() {
        let mut cfg = default_gemv_config();
        cfg.n = usize::MAX;
        // Weight calculation overflows: (k/128) * usize::MAX * 4 * 4
        let _info = nda_gemv_info(&cfg);
    }

    #[test]
    fn info_n_large_but_safe() {
        let mut cfg = default_gemv_config();
        cfg.n = 1_000_000;
        let info = nda_gemv_info(&cfg);
        assert!(info.validation_issues.is_empty());
        assert_eq!(info.output_bytes, 4_000_000);
        assert!(info.total_gpu_memory_estimate > 0);
    }
}
