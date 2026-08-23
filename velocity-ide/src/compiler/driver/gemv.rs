// GPU infrastructure — retained for future use.
#![allow(dead_code)]
//! Vulkan GEMV (General Matrix-Vector) compute dispatch for contiguous weight layouts.
//!
//! # Safety Invariants
//!
//! All `unsafe` blocks wrap Vulkan API calls via `ash`. Handles are valid from the
//! `VulkanDriver` parameter. Buffers use `create_coherent_buffer`/`create_device_local_buffer`.
//! Descriptor sets, pipelines, and command buffers follow standard Vulkan lifecycle patterns.
//! The `Drop` impl tears down resources in reverse dependency order.

use super::packing::*;
use super::vulkan_init::*;
use ash::vk;
use ash::Device;
use serde::Serialize;
use std::ffi::CString;
use std::time::Instant;

/// Configuration for a GEMV dispatch.
#[derive(Debug, Clone, Serialize)]
pub struct GemvConfig {
    /// Number of columns (input vector length / weight matrix columns).
    pub k: usize,
    /// Number of rows (output vector length / weight matrix rows).
    pub n: usize,
    /// Whether this is a ternary (1.58-bit) weight matrix.
    pub is_ternary: bool,
    /// Total weight bytes to upload.
    pub weight_bytes: usize,
}

/// Diagnostic info about a GEMV dispatch without requiring Vulkan.
#[derive(Debug, Clone, Serialize)]
pub struct GemvDispatchInfo {
    pub config: GemvConfig,
    pub input_buffer_bytes: usize,
    pub weight_buffer_bytes: usize,
    pub output_buffer_bytes: usize,
    pub total_gpu_buffer_bytes: usize,
    pub workgroup_count: u32,
    pub workgroup_size: u32,
    pub descriptor_set_count: usize,
    pub push_constant_bytes: usize,
    pub validation_issues: Vec<String>,
}

/// Validate a GEMV configuration for correctness.
pub fn validate_gemv_config(cfg: &GemvConfig) -> Vec<String> {
    let mut issues = Vec::new();
    if cfg.k == 0 {
        issues.push("k (columns) is 0".into());
    }
    if cfg.n == 0 {
        issues.push("n (rows) is 0".into());
    }
    if cfg.is_ternary && cfg.k % 16 != 0 {
        issues.push(format!("ternary mode requires k ({}) to be a multiple of 16", cfg.k));
    }
    if cfg.weight_bytes == 0 {
        issues.push("weight_bytes is 0".into());
    }
    if cfg.is_ternary {
        // Ternary packs 16 weights into one u32 (4 bytes), so expected = (k/16)*4*n
        let expected = (cfg.k / 16) * 4 * cfg.n;
        if cfg.weight_bytes != expected && expected > 0 {
            issues.push(format!(
                "ternary weight_bytes {} != expected {} for k={}, n={}",
                cfg.weight_bytes, expected, cfg.k, cfg.n
            ));
        }
    } else {
        // INT4 packs 8 weights per byte, k values per row, n rows
        let expected = (cfg.k / 2) * cfg.n;
        if cfg.weight_bytes != expected && expected > 0 {
            issues.push(format!(
                "int4 weight_bytes {} != expected {} for k={}, n={}",
                cfg.weight_bytes, expected, cfg.k, cfg.n
            ));
        }
    }
    issues
}

/// Compute dispatch info for a GEMV operation without requiring Vulkan.
pub fn gemv_dispatch_info(cfg: &GemvConfig) -> GemvDispatchInfo {
    let validation_issues = validate_gemv_config(cfg);

    let input_buffer_bytes = if cfg.is_ternary {
        (cfg.k / 16) * 4
    } else {
        cfg.k * 4
    };

    let weight_buffer_bytes = cfg.weight_bytes;
    let output_buffer_bytes = cfg.n * 4;
    let total_gpu_buffer_bytes = input_buffer_bytes + weight_buffer_bytes + output_buffer_bytes;

    let (workgroup_size, workgroup_count) = if cfg.is_ternary {
        (256u32, (cfg.n as u32).div_ceil(256))
    } else {
        (64u32, (cfg.n as u32).div_ceil(64))
    };

    GemvDispatchInfo {
        config: cfg.clone(),
        input_buffer_bytes,
        weight_buffer_bytes,
        output_buffer_bytes,
        total_gpu_buffer_bytes,
        workgroup_count,
        workgroup_size,
        descriptor_set_count: 3,
        push_constant_bytes: 8,
        validation_issues,
    }
}

pub struct VulkanGemv {
    pub device: Device,
    pub queue: vk::Queue,
    pub k: u32,
    pub n: u32,
    pub is_ternary: bool,

    pub shader_module: vk::ShaderModule,
    pub desc_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout: vk::PipelineLayout,
    pub compute_pipeline: vk::Pipeline,

    pub input_buffer: vk::Buffer,
    pub input_memory: vk::DeviceMemory,
    pub input_ptr: *mut std::ffi::c_void,
    pub input_size: vk::DeviceSize,

    pub weight_buffer: vk::Buffer,
    pub weight_memory: vk::DeviceMemory,
    pub weight_ptr: *mut std::ffi::c_void,
    pub weight_size: vk::DeviceSize,

    pub output_buffer: vk::Buffer,
    pub output_memory: vk::DeviceMemory,
    pub output_ptr: *mut std::ffi::c_void,
    pub output_size: vk::DeviceSize,

    pub desc_pool: vk::DescriptorPool,
    pub desc_set: vk::DescriptorSet,

    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
}

impl VulkanGemv {
    pub fn new(
        driver: &VulkanDriver,
        is_ternary: bool,
        k: u32,
        n: u32,
        weight_bytes: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = driver.device.clone();
        let physical_device = driver.physical_device;
        let instance = driver.instance.clone();
        let queue = driver.compute_queue;

        let spv_code = if is_ternary {
            crate::compiler::shaders::TERNARY_SPV
        } else {
            crate::compiler::shaders::INT4_SPV
        };
        let shader_info = vk::ShaderModuleCreateInfo::builder().code(spv_code);
        // SAFETY: create_shader_module with valid SPIR-V bytecode.
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
        let stage_info = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module)
            .name(&main_entry);
        let pipeline_create_info = vk::ComputePipelineCreateInfo::builder()
            .stage(stage_info.build())
            .layout(pipeline_layout);
        // SAFETY: create_compute_pipelines with valid shader and pipeline layout.
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

        let input_size = if is_ternary {
            ((k / 16) * 4) as vk::DeviceSize
        } else {
            (k * 4) as vk::DeviceSize
        };
        let weight_size = weight_bytes.len() as vk::DeviceSize;
        let output_size = (n * 4) as vk::DeviceSize;

        let (input_buffer, input_memory, input_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            input_size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;
        let (weight_buffer, weight_memory) = if is_ternary {
            let packed = pack_weights_uvec4(weight_bytes, k as usize, n as usize);
            create_device_local_buffer(
                &device,
                &instance,
                physical_device,
                queue,
                driver.queue_family_index,
                weight_size,
                &packed,
            )?
        } else {
            create_device_local_buffer(
                &device,
                &instance,
                physical_device,
                queue,
                driver.queue_family_index,
                weight_size,
                weight_bytes,
            )?
        };
        let (output_buffer, output_memory, output_ptr) = create_coherent_buffer(
            &device,
            &instance,
            physical_device,
            output_size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let pool_sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(3)
            .build()];
        let pool_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(1)
            .pool_sizes(&pool_sizes);
        // SAFETY: create_descriptor_pool with capacity for storage buffer sets.
        let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let layouts = [desc_set_layout];
        let alloc_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(desc_pool)
            .set_layouts(&layouts);
        // SAFETY: allocate_descriptor_sets from the pool.
        let desc_sets = unsafe { device.allocate_descriptor_sets(&alloc_info)? };
        let desc_set = desc_sets[0];

        let buffer_infos = [
            vk::DescriptorBufferInfo::builder()
                .buffer(input_buffer)
                .offset(0)
                .range(input_size)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(weight_buffer)
                .offset(0)
                .range(weight_size)
                .build(),
            vk::DescriptorBufferInfo::builder()
                .buffer(output_buffer)
                .offset(0)
                .range(output_size)
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
        ];
        // SAFETY: update_descriptor_sets binds buffer info to descriptor set.
        unsafe { device.update_descriptor_sets(&writes, &[]) };

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
        // SAFETY: Record compute dispatch into the command buffer.
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

            let workgroup_count = if is_ternary {
                n.div_ceil(256u32)
            } else {
                n.div_ceil(64u32)
            };
            device.cmd_dispatch(command_buffer, workgroup_count, 1, 1);
            device.end_command_buffer(command_buffer)?;
        }

        let fence_info = vk::FenceCreateInfo::builder();
        // SAFETY: create_fence for GPU synchronization.
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        Ok(Self {
            device,
            queue,
            k,
            n,
            is_ternary,
            shader_module,
            desc_set_layout,
            pipeline_layout,
            compute_pipeline,
            input_buffer,
            input_memory,
            input_ptr,
            input_size,
            weight_buffer,
            weight_memory,
            weight_ptr: std::ptr::null_mut(),
            weight_size,
            output_buffer,
            output_memory,
            output_ptr,
            output_size,
            desc_pool,
            desc_set,
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
        // SAFETY: `input_bytes.as_ptr()` is valid for `input_bytes.len()` bytes.
        // `self.input_ptr` was mapped from a Vulkan buffer of sufficient size during `new()`.
        // Both pointers are properly aligned and non-overlapping (host vs GPU memory).
        unsafe {
            std::ptr::copy_nonoverlapping(
                input_bytes.as_ptr(),
                self.input_ptr as *mut u8,
                input_bytes.len(),
            );
        }

        let start = Instant::now();
        let command_buffers = [self.command_buffer];
        let submit_info = vk::SubmitInfo::builder().command_buffers(&command_buffers);
        // SAFETY: `self.fence`, `self.queue`, `self.device` are all valid handles from `new()`.
        // reset_fences/queue_submit/wait_for_fences are standard Vulkan queue operations.
        unsafe {
            self.device.reset_fences(&[self.fence])?;
            self.device
                .queue_submit(self.queue, &[submit_info.build()], self.fence)?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }
        let duration_us = start.elapsed().as_micros() as f64;

        // SAFETY: `self.output_ptr` was mapped from a Vulkan buffer of sufficient size.
        // `output_floats.as_mut_ptr()` is valid for `output_floats.len()` f32 elements.
        // Non-overlapping: GPU-mapped memory vs host slice.
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

impl Drop for VulkanGemv {
    fn drop(&mut self) {
        // SAFETY: All Vulkan handles were created by `self` in `new()` and are valid.
        // device_wait_idle ensures no GPU work in flight before resource destruction.
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);

            self.device.unmap_memory(self.input_memory);
            self.device.free_memory(self.input_memory, None);
            self.device.destroy_buffer(self.input_buffer, None);

            self.device.free_memory(self.weight_memory, None);
            self.device.destroy_buffer(self.weight_buffer, None);

            self.device.unmap_memory(self.output_memory);
            self.device.free_memory(self.output_memory, None);
            self.device.destroy_buffer(self.output_buffer, None);

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

    #[test]
    fn validate_gemv_config_valid_ternary() {
        let cfg = GemvConfig {
            k: 256,
            n: 128,
            is_ternary: true,
            weight_bytes: (256 / 16) * 4 * 128,
        };
        let issues = validate_gemv_config(&cfg);
        assert!(issues.is_empty(), "expected no issues, got: {:?}", issues);
    }

    #[test]
    fn validate_gemv_config_valid_int4() {
        let cfg = GemvConfig {
            k: 512,
            n: 256,
            is_ternary: false,
            weight_bytes: (512 / 2) * 256,
        };
        let issues = validate_gemv_config(&cfg);
        assert!(issues.is_empty(), "expected no issues, got: {:?}", issues);
    }

    #[test]
    fn validate_gemv_config_zero_k() {
        let cfg = GemvConfig {
            k: 0,
            n: 128,
            is_ternary: false,
            weight_bytes: 1024,
        };
        let issues = validate_gemv_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("k")));
    }

    #[test]
    fn validate_gemv_config_zero_n() {
        let cfg = GemvConfig {
            k: 256,
            n: 0,
            is_ternary: false,
            weight_bytes: 1024,
        };
        let issues = validate_gemv_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("n")));
    }

    #[test]
    fn validate_gemv_config_ternary_k_not_multiple_of_16() {
        let cfg = GemvConfig {
            k: 100, // not multiple of 16
            n: 64,
            is_ternary: true,
            weight_bytes: 1024,
        };
        let issues = validate_gemv_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("multiple of 16")));
    }

    #[test]
    fn validate_gemv_config_wrong_weight_bytes() {
        let cfg = GemvConfig {
            k: 256,
            n: 128,
            is_ternary: true,
            weight_bytes: 999, // wrong
        };
        let issues = validate_gemv_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("weight_bytes")));
    }

    #[test]
    fn dispatch_info_ternary() {
        let cfg = GemvConfig {
            k: 256,
            n: 512,
            is_ternary: true,
            weight_bytes: (256 / 16) * 4 * 512,
        };
        let info = gemv_dispatch_info(&cfg);
        assert_eq!(info.input_buffer_bytes, (256 / 16) * 4);
        assert_eq!(info.output_buffer_bytes, 512 * 4);
        assert_eq!(info.workgroup_size, 256);
        assert_eq!(info.workgroup_count, 512u32.div_ceil(256));
        assert_eq!(info.descriptor_set_count, 3);
        assert_eq!(info.push_constant_bytes, 8);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn dispatch_info_int4() {
        let cfg = GemvConfig {
            k: 512,
            n: 300,
            is_ternary: false,
            weight_bytes: (512 / 2) * 300,
        };
        let info = gemv_dispatch_info(&cfg);
        assert_eq!(info.input_buffer_bytes, 512 * 4);
        assert_eq!(info.output_buffer_bytes, 300 * 4);
        assert_eq!(info.workgroup_size, 64);
        assert_eq!(info.workgroup_count, 300u32.div_ceil(64));
        assert_eq!(info.descriptor_set_count, 3);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn dispatch_info_total_buffer_bytes() {
        let cfg = GemvConfig {
            k: 128,
            n: 64,
            is_ternary: false,
            weight_bytes: 1000,
        };
        let info = gemv_dispatch_info(&cfg);
        assert_eq!(
            info.total_gpu_buffer_bytes,
            info.input_buffer_bytes + info.weight_buffer_bytes + info.output_buffer_bytes
        );
    }

    #[test]
    fn dispatch_info_serializes() {
        let cfg = GemvConfig {
            k: 256,
            n: 128,
            is_ternary: true,
            weight_bytes: (256 / 16) * 4 * 128,
        };
        let info = gemv_dispatch_info(&cfg);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"workgroup_count\""));
        assert!(json.contains("\"input_buffer_bytes\""));
        assert!(json.contains("\"total_gpu_buffer_bytes\""));
    }
}
