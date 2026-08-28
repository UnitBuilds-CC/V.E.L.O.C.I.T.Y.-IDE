//! Vulkan benchmark: NDA attention vs contiguous attention performance comparison.
//!
//! # Safety Invariants
//!
//! All `unsafe` blocks wrap Vulkan API calls via `ash`. Handles are valid from the
//! `VulkanDriver` parameter. Buffers, descriptor sets, and pipelines follow the same
//! creation/validation patterns as the main pipeline code. Resources are cleaned up
//! before the function returns.

use std::ffi::CString;
use std::time::Instant;

use ash::vk;
use serde::Serialize;

use super::vulkan_init::*;

/// Configuration for the attention benchmark.
#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkConfig {
    pub num_tokens: u32,
    pub head_dim: u32,
    pub num_heads: u32,
    pub iterations: u32,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            num_tokens: 256,
            head_dim: 32,
            num_heads: 32,
            iterations: 500,
        }
    }
}

/// Results from the attention benchmark comparison.
#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkReport {
    pub config: BenchmarkConfig,
    pub contig_avg_us: f64,
    pub ndakv_avg_us: f64,
    pub speedup_ratio: f64,
    pub faster_method: String,
    pub validation_issues: Vec<String>,
}

/// Validate benchmark configuration.
pub fn validate_benchmark_config(cfg: &BenchmarkConfig) -> Vec<String> {
    let mut issues = Vec::new();
    if cfg.num_tokens == 0 {
        issues.push("num_tokens must be > 0".into());
    }
    if cfg.head_dim == 0 {
        issues.push("head_dim must be > 0".into());
    }
    if cfg.num_heads == 0 {
        issues.push("num_heads must be > 0".into());
    }
    if cfg.iterations == 0 {
        issues.push("iterations must be > 0".into());
    }
    if cfg.iterations < 10 {
        issues.push("iterations should be >= 10 for meaningful results".into());
    }
    issues
}

/// Build a benchmark report from timing results.
pub fn build_benchmark_report(
    cfg: &BenchmarkConfig,
    contig_avg_us: f64,
    ndakv_avg_us: f64,
) -> BenchmarkReport {
    let issues = validate_benchmark_config(cfg);
    let (ratio, faster) = if contig_avg_us > 0.0 && ndakv_avg_us > 0.0 {
        if contig_avg_us < ndakv_avg_us {
            (ndakv_avg_us / contig_avg_us, "contig".to_string())
        } else {
            (contig_avg_us / ndakv_avg_us, "ndakv".to_string())
        }
    } else {
        (0.0, "unknown".to_string())
    };
    BenchmarkReport {
        config: cfg.clone(),
        contig_avg_us,
        ndakv_avg_us,
        speedup_ratio: ratio,
        faster_method: faster,
        validation_issues: issues,
    }
}

#[allow(clippy::needless_range_loop)]
pub fn benchmark_attention_nda_vs_contig(
    driver: &VulkanDriver,
) -> Result<(f64, f64), Box<dyn std::error::Error>> {
    let device = &driver.device;
    let physical_device = driver.physical_device;
    let instance = &driver.instance;
    let queue = driver.compute_queue;

    let shader_info_contig =
        vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ATTN_CONTIG_SPV);
    // SAFETY: create_shader_module with valid attention SPIR-V bytecodes.
    let shader_module_contig = unsafe { device.create_shader_module(&shader_info_contig, None)? };

    let shader_info_ndakv =
        vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ATTN_NDAKV_SPV);
    // SAFETY: All following unsafe blocks create Vulkan pipeline resources (descriptor set
    // layouts, pipeline layouts, compute pipelines) with valid handles from the driver.
    let shader_module_ndakv = unsafe { device.create_shader_module(&shader_info_ndakv, None)? };

    let bindings_contig = [
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
    let layout_info_contig =
        vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_contig);
    // SAFETY: Create descriptor set layout with 1 storage buffer binding (contig attention).
    let desc_set_layout_contig =
        unsafe { device.create_descriptor_set_layout(&layout_info_contig, None)? };

    let bindings_ndakv = [
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
    let layout_info_ndakv = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_ndakv);
    // SAFETY: Create descriptor set layout with 3 storage buffer bindings (NDA KV attention).
    let desc_set_layout_ndakv =
        unsafe { device.create_descriptor_set_layout(&layout_info_ndakv, None)? };

    let push_constant_ranges = [vk::PushConstantRange::builder()
        .stage_flags(vk::ShaderStageFlags::COMPUTE)
        .offset(0)
        .size(8)
        .build()];
    let layouts_contig = [desc_set_layout_contig];
    let pipeline_layout_info_contig = vk::PipelineLayoutCreateInfo::builder()
        .set_layouts(&layouts_contig)
        .push_constant_ranges(&push_constant_ranges);
    // SAFETY: Create pipeline layout with contig descriptor set layout and push constants.
    let pipeline_layout_contig =
        unsafe { device.create_pipeline_layout(&pipeline_layout_info_contig, None)? };

    let layouts_ndakv = [desc_set_layout_ndakv];
    let pipeline_layout_info_ndakv = vk::PipelineLayoutCreateInfo::builder()
        .set_layouts(&layouts_ndakv)
        .push_constant_ranges(&push_constant_ranges);
    // SAFETY: Create pipeline layout with ndakv descriptor set layout and push constants.
    let pipeline_layout_ndakv =
        unsafe { device.create_pipeline_layout(&pipeline_layout_info_ndakv, None)? };

    let main_entry = CString::new("main")?;

    let stage_info_contig = vk::PipelineShaderStageCreateInfo::builder()
        .stage(vk::ShaderStageFlags::COMPUTE)
        .module(shader_module_contig)
        .name(&main_entry);
    let pipeline_create_info_contig = vk::ComputePipelineCreateInfo::builder()
        .stage(stage_info_contig.build())
        .layout(pipeline_layout_contig);
    // SAFETY: Create compute pipeline for contig attention from valid shader and layout.
    let compute_pipelines_contig = unsafe {
        device
            .create_compute_pipelines(
                vk::PipelineCache::null(),
                &[pipeline_create_info_contig.build()],
                None,
            )
            .map_err(|(_, e)| e)?
    };
    let compute_pipeline_contig = compute_pipelines_contig[0];

    let stage_info_ndakv = vk::PipelineShaderStageCreateInfo::builder()
        .stage(vk::ShaderStageFlags::COMPUTE)
        .module(shader_module_ndakv)
        .name(&main_entry);
    let pipeline_create_info_ndakv = vk::ComputePipelineCreateInfo::builder()
        .stage(stage_info_ndakv.build())
        .layout(pipeline_layout_ndakv);
    // SAFETY: Create compute pipeline for NDA KV attention from valid shader and layout.
    let compute_pipelines_ndakv = unsafe {
        device
            .create_compute_pipelines(
                vk::PipelineCache::null(),
                &[pipeline_create_info_ndakv.build()],
                None,
            )
            .map_err(|(_, e)| e)?
    };
    let compute_pipeline_ndakv = compute_pipelines_ndakv[0];

    let num_tokens = 256u32;
    let head_dim = 32u32;
    let num_heads = 32u32;

    let q_size = 256 as vk::DeviceSize;
    let k_size = (num_tokens * num_heads * head_dim * 4) as vk::DeviceSize;
    let v_size = (num_tokens * num_heads * head_dim * 4) as vk::DeviceSize;

    #[repr(C)]
    struct NdaKvBlock {
        next_block_idx: u32,
        num_tokens: u32,
        hash_checksum: u32,
        padding: u32,
        keys_active: [u32; 512],
        keys_pos: [u32; 512],
        values_active: [u32; 512],
        values_pos: [u32; 512],
    }
    let block_size = (16 * std::mem::size_of::<NdaKvBlock>()) as vk::DeviceSize;
    let out_size = (num_heads * head_dim * 4) as vk::DeviceSize;

    let (q_buffer, q_memory, q_ptr) = create_coherent_buffer(
        device,
        instance,
        physical_device,
        q_size,
        vk::BufferUsageFlags::STORAGE_BUFFER,
    )?;
    let (k_buffer, k_memory, k_ptr) = create_coherent_buffer(
        device,
        instance,
        physical_device,
        k_size,
        vk::BufferUsageFlags::STORAGE_BUFFER,
    )?;
    let (v_buffer, v_memory, v_ptr) = create_coherent_buffer(
        device,
        instance,
        physical_device,
        v_size,
        vk::BufferUsageFlags::STORAGE_BUFFER,
    )?;
    let (block_buffer, block_memory, block_ptr) = create_coherent_buffer(
        device,
        instance,
        physical_device,
        block_size,
        vk::BufferUsageFlags::STORAGE_BUFFER,
    )?;
    let (out_buffer, out_memory, _out_ptr) = create_coherent_buffer(
        device,
        instance,
        physical_device,
        out_size,
        vk::BufferUsageFlags::STORAGE_BUFFER,
    )?;

    // SAFETY: Initialize benchmark input data via mapped HOST_VISIBLE pointers.
    // All pointers come from create_coherent_buffer; slice lengths fit within buffer sizes.
    unsafe {
        let q_slice = std::slice::from_raw_parts_mut(q_ptr as *mut u32, 64);
        q_slice[0..32].fill(0x55555555);
        q_slice[32..64].fill(0x33333333);

        let k_slice = std::slice::from_raw_parts_mut(
            k_ptr as *mut f32,
            (num_tokens * num_heads * head_dim) as usize,
        );
        k_slice.fill(0.1);
        let v_slice = std::slice::from_raw_parts_mut(
            v_ptr as *mut f32,
            (num_tokens * num_heads * head_dim) as usize,
        );
        v_slice.fill(0.1);

        let block_slice = std::slice::from_raw_parts_mut(block_ptr as *mut NdaKvBlock, 16);
        for i in 0..16 {
            block_slice[i] = NdaKvBlock {
                next_block_idx: if i == 15 { 999999 } else { (i + 1) as u32 },
                num_tokens: 16,
                hash_checksum: 12345,
                padding: 0,
                keys_active: [0x55555555; 512],
                keys_pos: [0x33333333; 512],
                values_active: [0x55555555; 512],
                values_pos: [0x33333333; 512],
            };
        }
    }

    let pool_sizes = [vk::DescriptorPoolSize::builder()
        .ty(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(10)
        .build()];
    let pool_info = vk::DescriptorPoolCreateInfo::builder()
        .max_sets(2)
        .pool_sizes(&pool_sizes);
    // SAFETY: create_descriptor_pool, allocate descriptor sets, update bindings,
    // create command pool and allocate command buffers — all with valid handles.
    let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

    // SAFETY: Allocate descriptor set for contig attention from pool.
    let desc_set_contig = unsafe {
        device.allocate_descriptor_sets(
            &vk::DescriptorSetAllocateInfo::builder()
                .descriptor_pool(desc_pool)
                .set_layouts(&[desc_set_layout_contig]),
        )?[0]
    };
    // SAFETY: Allocate descriptor set for NDA KV attention from pool.
    let desc_set_ndakv = unsafe {
        device.allocate_descriptor_sets(
            &vk::DescriptorSetAllocateInfo::builder()
                .descriptor_pool(desc_pool)
                .set_layouts(&[desc_set_layout_ndakv]),
        )?[0]
    };

    let buffer_infos_contig = [
        vk::DescriptorBufferInfo::builder()
            .buffer(q_buffer)
            .offset(0)
            .range(q_size)
            .build(),
        vk::DescriptorBufferInfo::builder()
            .buffer(k_buffer)
            .offset(0)
            .range(k_size)
            .build(),
        vk::DescriptorBufferInfo::builder()
            .buffer(v_buffer)
            .offset(0)
            .range(v_size)
            .build(),
        vk::DescriptorBufferInfo::builder()
            .buffer(out_buffer)
            .offset(0)
            .range(out_size)
            .build(),
    ];
    let writes_contig = [
        vk::WriteDescriptorSet::builder()
            .dst_set(desc_set_contig)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buffer_infos_contig[0..1])
            .build(),
        vk::WriteDescriptorSet::builder()
            .dst_set(desc_set_contig)
            .dst_binding(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buffer_infos_contig[1..2])
            .build(),
        vk::WriteDescriptorSet::builder()
            .dst_set(desc_set_contig)
            .dst_binding(2)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buffer_infos_contig[2..3])
            .build(),
        vk::WriteDescriptorSet::builder()
            .dst_set(desc_set_contig)
            .dst_binding(3)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buffer_infos_contig[3..4])
            .build(),
    ];
    // SAFETY: update_descriptor_sets and create command infrastructure.
    unsafe { device.update_descriptor_sets(&writes_contig, &[]) };

    let buffer_infos_ndakv = [
        vk::DescriptorBufferInfo::builder()
            .buffer(q_buffer)
            .offset(0)
            .range(q_size)
            .build(),
        vk::DescriptorBufferInfo::builder()
            .buffer(block_buffer)
            .offset(0)
            .range(block_size)
            .build(),
        vk::DescriptorBufferInfo::builder()
            .buffer(out_buffer)
            .offset(0)
            .range(out_size)
            .build(),
    ];
    let writes_ndakv = [
        vk::WriteDescriptorSet::builder()
            .dst_set(desc_set_ndakv)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buffer_infos_ndakv[0..1])
            .build(),
        vk::WriteDescriptorSet::builder()
            .dst_set(desc_set_ndakv)
            .dst_binding(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buffer_infos_ndakv[1..2])
            .build(),
        vk::WriteDescriptorSet::builder()
            .dst_set(desc_set_ndakv)
            .dst_binding(2)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buffer_infos_ndakv[2..3])
            .build(),
    ];
    // SAFETY: Update NDA KV descriptor set with buffer bindings.
    unsafe { device.update_descriptor_sets(&writes_ndakv, &[]) };

    let command_pool_info = vk::CommandPoolCreateInfo::builder()
        .queue_family_index(driver.queue_family_index)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
    // SAFETY: Create command pool for benchmark dispatch recording.
    let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };

    // SAFETY: allocate_command_buffers from the pool for recording benchmark dispatches.
    let command_buffers = unsafe {
        device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::builder()
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(2),
        )?
    };
    let cmd_contig = command_buffers[0];
    let cmd_ndakv = command_buffers[1];

    // SAFETY: Record contig attention dispatch: begin, bind pipeline/descriptors, push constants, dispatch, end.
    unsafe {
        device.begin_command_buffer(
            cmd_contig,
            &vk::CommandBufferBeginInfo::builder()
                .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE),
        )?;
        device.cmd_bind_pipeline(
            cmd_contig,
            vk::PipelineBindPoint::COMPUTE,
            compute_pipeline_contig,
        );
        device.cmd_bind_descriptor_sets(
            cmd_contig,
            vk::PipelineBindPoint::COMPUTE,
            pipeline_layout_contig,
            0,
            &[desc_set_contig],
            &[],
        );
        let params = [num_tokens, head_dim];
        let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
        device.cmd_push_constants(
            cmd_contig,
            pipeline_layout_contig,
            vk::ShaderStageFlags::COMPUTE,
            0,
            params_bytes,
        );
        device.cmd_dispatch(cmd_contig, 1, 1, 1);
        device.end_command_buffer(cmd_contig)?;
    }

    // SAFETY: Record NDA KV attention dispatch: begin, bind pipeline/descriptors, push constants, dispatch, end.
    unsafe {
        device.begin_command_buffer(
            cmd_ndakv,
            &vk::CommandBufferBeginInfo::builder()
                .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE),
        )?;
        device.cmd_bind_pipeline(
            cmd_ndakv,
            vk::PipelineBindPoint::COMPUTE,
            compute_pipeline_ndakv,
        );
        device.cmd_bind_descriptor_sets(
            cmd_ndakv,
            vk::PipelineBindPoint::COMPUTE,
            pipeline_layout_ndakv,
            0,
            &[desc_set_ndakv],
            &[],
        );
        let params = [0u32, head_dim];
        let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
        device.cmd_push_constants(
            cmd_ndakv,
            pipeline_layout_ndakv,
            vk::ShaderStageFlags::COMPUTE,
            0,
            params_bytes,
        );
        device.cmd_dispatch(cmd_ndakv, 1, 1, 1);
        device.end_command_buffer(cmd_ndakv)?;
    }

    let fence_info = vk::FenceCreateInfo::builder();
    // SAFETY: Create fence for benchmark synchronization.
    let fence = unsafe { device.create_fence(&fence_info, None)? };

    let iterations = 500;

    let mut total_contig = 0.0;
    for _ in 0..iterations {
        let start = Instant::now();
        // SAFETY: Submit contig benchmark command buffer and wait for completion.
        unsafe {
            device.reset_fences(&[fence])?;
            device.queue_submit(
                queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[cmd_contig])
                    .build()],
                fence,
            )?;
            device.wait_for_fences(&[fence], true, u64::MAX)?;
        }
        total_contig += start.elapsed().as_micros() as f64;
    }
    let contig_avg_us = total_contig / (iterations as f64);

    let mut total_ndakv = 0.0;
    for _ in 0..iterations {
        let start = Instant::now();
        // SAFETY: Submit NDA KV benchmark command buffer and wait for completion.
        unsafe {
            device.reset_fences(&[fence])?;
            device.queue_submit(
                queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[cmd_ndakv])
                    .build()],
                fence,
            )?;
            device.wait_for_fences(&[fence], true, u64::MAX)?;
        }
        total_ndakv += start.elapsed().as_micros() as f64;
    }
    let ndakv_avg_us = total_ndakv / (iterations as f64);

    // SAFETY: Cleanup — wait for idle, destroy all benchmark resources (fence, command buffers,
    // command pool, descriptor pool, buffers, pipelines, layouts, shader modules).
    unsafe {
        let _ = device.device_wait_idle();
        device.destroy_fence(fence, None);
        device.free_command_buffers(command_pool, &command_buffers);
        device.destroy_command_pool(command_pool, None);
        device.destroy_descriptor_pool(desc_pool, None);

        device.unmap_memory(q_memory);
        device.free_memory(q_memory, None);
        device.destroy_buffer(q_buffer, None);

        device.unmap_memory(k_memory);
        device.free_memory(k_memory, None);
        device.destroy_buffer(k_buffer, None);

        device.unmap_memory(v_memory);
        device.free_memory(v_memory, None);
        device.destroy_buffer(v_buffer, None);

        device.unmap_memory(block_memory);
        device.free_memory(block_memory, None);
        device.destroy_buffer(block_buffer, None);

        device.unmap_memory(out_memory);
        device.free_memory(out_memory, None);
        device.destroy_buffer(out_buffer, None);

        device.destroy_pipeline(compute_pipeline_contig, None);
        device.destroy_pipeline_layout(pipeline_layout_contig, None);
        device.destroy_descriptor_set_layout(desc_set_layout_contig, None);
        device.destroy_shader_module(shader_module_contig, None);

        device.destroy_pipeline(compute_pipeline_ndakv, None);
        device.destroy_pipeline_layout(pipeline_layout_ndakv, None);
        device.destroy_descriptor_set_layout(desc_set_layout_ndakv, None);
        device.destroy_shader_module(shader_module_ndakv, None);
    }

    Ok((contig_avg_us, ndakv_avg_us))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_valid() {
        let cfg = BenchmarkConfig::default();
        assert!(validate_benchmark_config(&cfg).is_empty());
    }

    #[test]
    fn validate_zero_tokens() {
        let mut cfg = BenchmarkConfig::default();
        cfg.num_tokens = 0;
        assert!(validate_benchmark_config(&cfg).iter().any(|i| i.contains("num_tokens")));
    }

    #[test]
    fn validate_zero_iterations() {
        let mut cfg = BenchmarkConfig::default();
        cfg.iterations = 0;
        let issues = validate_benchmark_config(&cfg);
        assert!(issues.iter().any(|i| i.contains("iterations")));
    }

    #[test]
    fn validate_low_iterations() {
        let mut cfg = BenchmarkConfig::default();
        cfg.iterations = 5;
        assert!(validate_benchmark_config(&cfg).iter().any(|i| i.contains(">= 10")));
    }

    #[test]
    fn benchmark_report_contig_faster() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        assert_eq!(report.faster_method, "contig");
        assert!((report.speedup_ratio - 2.0).abs() < 0.01);
        assert!(report.validation_issues.is_empty());
    }

    #[test]
    fn benchmark_report_ndakv_faster() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 300.0, 100.0);
        assert_eq!(report.faster_method, "ndakv");
        assert!((report.speedup_ratio - 3.0).abs() < 0.01);
    }

    #[test]
    fn benchmark_report_zero_times() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 0.0, 0.0);
        assert_eq!(report.faster_method, "unknown");
        assert_eq!(report.speedup_ratio, 0.0);
    }

    #[test]
    fn benchmark_config_serializes() {
        let cfg = BenchmarkConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("num_tokens"));
        assert!(json.contains("256"));
    }

    #[test]
    fn benchmark_report_serializes() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("speedup_ratio"));
        assert!(json.contains("faster_method"));
        assert!(json.contains("contig"));
    }

    // ── Validation: individual fields ────────────────────────────────────

    #[test]
    fn validate_zero_head_dim() {
        let mut cfg = BenchmarkConfig::default();
        cfg.head_dim = 0;
        assert!(validate_benchmark_config(&cfg).iter().any(|i| i.contains("head_dim")));
    }

    #[test]
    fn validate_zero_num_heads() {
        let mut cfg = BenchmarkConfig::default();
        cfg.num_heads = 0;
        assert!(validate_benchmark_config(&cfg).iter().any(|i| i.contains("num_heads")));
    }

    #[test]
    fn validate_zero_iterations_triggers_both() {
        let mut cfg = BenchmarkConfig::default();
        cfg.iterations = 0;
        let issues = validate_benchmark_config(&cfg);
        // 0 triggers both "must be > 0" and "should be >= 10"
        assert!(issues.iter().any(|i| i.contains("must be > 0")));
        assert!(issues.iter().any(|i| i.contains(">= 10")));
        assert_eq!(issues.len(), 2);
    }

    // ── Validation: iteration boundaries ─────────────────────────────────

    #[test]
    fn validate_iterations_9_warns() {
        let mut cfg = BenchmarkConfig::default();
        cfg.iterations = 9;
        assert!(validate_benchmark_config(&cfg).iter().any(|i| i.contains(">= 10")));
    }

    #[test]
    fn validate_iterations_10_valid() {
        let mut cfg = BenchmarkConfig::default();
        cfg.iterations = 10;
        assert!(validate_benchmark_config(&cfg).is_empty());
    }

    #[test]
    fn validate_iterations_1_warns() {
        let mut cfg = BenchmarkConfig::default();
        cfg.iterations = 1;
        let issues = validate_benchmark_config(&cfg);
        assert!(issues.iter().any(|i| i.contains(">= 10")));
        // 1 > 0, so no "must be > 0" issue
        assert!(!issues.iter().any(|i| i.contains("must be > 0")));
    }

    #[test]
    fn validate_iterations_max_valid() {
        let mut cfg = BenchmarkConfig::default();
        cfg.iterations = u32::MAX;
        assert!(validate_benchmark_config(&cfg).is_empty());
    }

    // ── Validation: multiple issues ──────────────────────────────────────

    #[test]
    fn validate_all_zeros() {
        let cfg = BenchmarkConfig {
            num_tokens: 0,
            head_dim: 0,
            num_heads: 0,
            iterations: 0,
        };
        let issues = validate_benchmark_config(&cfg);
        // num_tokens=0, head_dim=0, num_heads=0, iterations=0 (triggers 2 issues)
        assert_eq!(issues.len(), 5);
    }

    #[test]
    fn validate_issues_order_deterministic() {
        let cfg = BenchmarkConfig {
            num_tokens: 0, head_dim: 0, num_heads: 0, iterations: 0,
        };
        let i1 = validate_benchmark_config(&cfg);
        let i2 = validate_benchmark_config(&cfg);
        assert_eq!(i1, i2);
    }

    // ── Validation issue text ────────────────────────────────────────────

    #[test]
    fn validate_tokens_issue_text() {
        let mut cfg = BenchmarkConfig::default();
        cfg.num_tokens = 0;
        assert_eq!(validate_benchmark_config(&cfg)[0], "num_tokens must be > 0");
    }

    #[test]
    fn validate_head_dim_issue_text() {
        let mut cfg = BenchmarkConfig::default();
        cfg.head_dim = 0;
        assert_eq!(validate_benchmark_config(&cfg)[0], "head_dim must be > 0");
    }

    #[test]
    fn validate_num_heads_issue_text() {
        let mut cfg = BenchmarkConfig::default();
        cfg.num_heads = 0;
        assert_eq!(validate_benchmark_config(&cfg)[0], "num_heads must be > 0");
    }

    // ── Report calculations ──────────────────────────────────────────────

    #[test]
    fn report_equal_times() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 100.0);
        // contig >= ndakv is false (100 < 100 is false), so ndakv >= contig → ratio = 100/100 = 1.0
        // Actually: contig < ndakv → 100 < 100 → false → else branch → ratio = 100/100 = 1.0, "ndakv"
        assert_eq!(report.faster_method, "ndakv");
        assert!((report.speedup_ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn report_contig_zero_ndakv_nonzero() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 0.0, 100.0);
        assert_eq!(report.faster_method, "unknown");
        assert_eq!(report.speedup_ratio, 0.0);
    }

    #[test]
    fn report_contig_nonzero_ndakv_zero() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 0.0);
        assert_eq!(report.faster_method, "unknown");
        assert_eq!(report.speedup_ratio, 0.0);
    }

    #[test]
    fn report_very_small_times() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 0.001, 0.002);
        assert_eq!(report.faster_method, "contig");
        assert!((report.speedup_ratio - 2.0).abs() < 0.01);
    }

    #[test]
    fn report_very_large_times() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 1e12, 1e6);
        assert_eq!(report.faster_method, "ndakv");
        assert!((report.speedup_ratio - 1e6).abs() < 1.0);
    }

    #[test]
    fn report_preserves_config() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        assert_eq!(report.config.num_tokens, cfg.num_tokens);
        assert_eq!(report.config.head_dim, cfg.head_dim);
        assert_eq!(report.config.num_heads, cfg.num_heads);
        assert_eq!(report.config.iterations, cfg.iterations);
    }

    #[test]
    fn report_preserves_timing_values() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 123.456, 789.012);
        assert_eq!(report.contig_avg_us, 123.456);
        assert_eq!(report.ndakv_avg_us, 789.012);
    }

    #[test]
    fn report_with_invalid_config() {
        let mut cfg = BenchmarkConfig::default();
        cfg.num_tokens = 0;
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        assert!(!report.validation_issues.is_empty());
        // Report still computes even with invalid config
        assert_eq!(report.faster_method, "contig");
    }

    // ── Struct derives ───────────────────────────────────────────────────

    #[test]
    fn config_clone() {
        let cfg = BenchmarkConfig::default();
        let cloned = cfg.clone();
        assert_eq!(cloned.num_tokens, cfg.num_tokens);
        assert_eq!(cloned.head_dim, cfg.head_dim);
        assert_eq!(cloned.num_heads, cfg.num_heads);
        assert_eq!(cloned.iterations, cfg.iterations);
    }

    #[test]
    fn config_clone_independent() {
        let cfg = BenchmarkConfig::default();
        let mut cloned = cfg.clone();
        cloned.num_tokens = 999;
        assert_ne!(cfg.num_tokens, cloned.num_tokens);
    }

    #[test]
    fn config_debug_format() {
        let cfg = BenchmarkConfig::default();
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("BenchmarkConfig"));
        assert!(debug.contains("num_tokens: 256"));
    }

    #[test]
    fn report_clone() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let cloned = report.clone();
        assert_eq!(cloned.speedup_ratio, report.speedup_ratio);
        assert_eq!(cloned.faster_method, report.faster_method);
        assert_eq!(cloned.contig_avg_us, report.contig_avg_us);
    }

    #[test]
    fn report_debug_format() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let debug = format!("{:?}", report);
        assert!(debug.contains("BenchmarkReport"));
        assert!(debug.contains("speedup_ratio"));
        assert!(debug.contains("faster_method"));
    }

    // ── Serialization ────────────────────────────────────────────────────

    #[test]
    fn config_json_all_fields() {
        let cfg = BenchmarkConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"num_tokens\""));
        assert!(json.contains("\"head_dim\""));
        assert!(json.contains("\"num_heads\""));
        assert!(json.contains("\"iterations\""));
    }

    #[test]
    fn report_json_all_fields() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("config"));
        assert!(json.contains("contig_avg_us"));
        assert!(json.contains("ndakv_avg_us"));
        assert!(json.contains("speedup_ratio"));
        assert!(json.contains("faster_method"));
        assert!(json.contains("validation_issues"));
    }

    #[test]
    fn report_json_parseable_as_value() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let json = serde_json::to_string(&report).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["faster_method"], "contig");
        assert!(value["validation_issues"].is_array());
    }

    #[test]
    fn report_pretty_json() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let pretty = serde_json::to_string_pretty(&report).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));
    }

    // ── Default impl ────────────────────────────────────────────────────

    #[test]
    fn default_config_values() {
        let cfg = BenchmarkConfig::default();
        assert_eq!(cfg.num_tokens, 256);
        assert_eq!(cfg.head_dim, 32);
        assert_eq!(cfg.num_heads, 32);
        assert_eq!(cfg.iterations, 500);
    }

    #[test]
    fn default_config_is_valid() {
        let cfg = BenchmarkConfig::default();
        assert!(validate_benchmark_config(&cfg).is_empty());
    }

    // ── Boundary values ──────────────────────────────────────────────────

    #[test]
    fn validate_1_token_valid() {
        let mut cfg = BenchmarkConfig::default();
        cfg.num_tokens = 1;
        assert!(validate_benchmark_config(&cfg).is_empty());
    }

    #[test]
    fn validate_1_head_dim_valid() {
        let mut cfg = BenchmarkConfig::default();
        cfg.head_dim = 1;
        assert!(validate_benchmark_config(&cfg).is_empty());
    }

    #[test]
    fn validate_1_num_heads_valid() {
        let mut cfg = BenchmarkConfig::default();
        cfg.num_heads = 1;
        assert!(validate_benchmark_config(&cfg).is_empty());
    }

    // ── Block 181: additional comprehensive tests ─────────────────────────

    // ── JSON key counts ─────────────────────────────────────────────────

    #[test]
    fn config_json_has_exactly_4_keys() {
        let json = serde_json::to_string(&BenchmarkConfig::default()).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 4);
    }

    #[test]
    fn report_json_has_exactly_6_keys() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 6);
    }

    // ── JSON roundtrip via Value ─────────────────────────────────────────

    #[test]
    fn config_json_roundtrip_via_value() {
        let cfg = BenchmarkConfig {
            num_tokens: 512, head_dim: 64, num_heads: 16, iterations: 1000,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["num_tokens"], 512);
        assert_eq!(val["head_dim"], 64);
        assert_eq!(val["num_heads"], 16);
        assert_eq!(val["iterations"], 1000);
    }

    #[test]
    fn report_json_nested_config() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["config"]["num_tokens"], 256);
        assert_eq!(val["config"]["head_dim"], 32);
    }

    // ── Equality via JSON ───────────────────────────────────────────────

    #[test]
    fn config_eq_via_json() {
        let a = BenchmarkConfig::default();
        let b = BenchmarkConfig::default();
        assert_eq!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
    }

    #[test]
    fn config_neq_via_json() {
        let a = BenchmarkConfig::default();
        let mut b = BenchmarkConfig::default();
        b.num_tokens = 999;
        assert_ne!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
    }

    // ── Speedup ratio formula ───────────────────────────────────────────

    #[test]
    fn report_speedup_ratio_contig_faster() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 50.0, 200.0);
        // contig < ndakv → ratio = ndakv/contig = 200/50 = 4.0
        assert_eq!(report.faster_method, "contig");
        assert!((report.speedup_ratio - 4.0).abs() < 0.001);
    }

    #[test]
    fn report_speedup_ratio_ndakv_faster() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 500.0, 100.0);
        // contig >= ndakv → ratio = contig/ndakv = 500/100 = 5.0
        assert_eq!(report.faster_method, "ndakv");
        assert!((report.speedup_ratio - 5.0).abs() < 0.001);
    }

    #[test]
    fn report_speedup_ratio_exactly_equal() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 100.0);
        // contig < ndakv → 100 < 100 → false → else → ratio = 100/100 = 1.0, "ndakv"
        assert_eq!(report.faster_method, "ndakv");
        assert!((report.speedup_ratio - 1.0).abs() < 0.001);
    }

    // ── Report config independence ──────────────────────────────────────

    #[test]
    fn report_config_independent_from_original() {
        let mut cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        cfg.num_tokens = 9999;
        assert_eq!(report.config.num_tokens, 256);
        assert_eq!(cfg.num_tokens, 9999);
    }

    #[test]
    fn report_clone_independent_issues() {
        let mut cfg = BenchmarkConfig::default();
        cfg.num_tokens = 0;
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let mut cloned = report.clone();
        cloned.validation_issues.push("extra".to_string());
        assert_ne!(report.validation_issues.len(), cloned.validation_issues.len());
    }

    // ── Debug format details ────────────────────────────────────────────

    #[test]
    fn config_debug_contains_struct_name() {
        let debug = format!("{:?}", BenchmarkConfig::default());
        assert!(debug.contains("BenchmarkConfig"));
    }

    #[test]
    fn report_debug_contains_struct_name() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let debug = format!("{:?}", report);
        assert!(debug.contains("BenchmarkReport"));
    }

    #[test]
    fn report_debug_contains_timing() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 123.456, 789.012);
        let debug = format!("{:?}", report);
        assert!(debug.contains("123.456"));
        assert!(debug.contains("789.012"));
    }

    // ── JSON value verification ─────────────────────────────────────────

    #[test]
    fn report_json_speedup_value() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 300.0);
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["faster_method"], "contig");
        assert!((val["speedup_ratio"].as_f64().unwrap() - 3.0).abs() < 0.01);
    }

    #[test]
    fn report_json_validation_issues_empty_for_valid() {
        let cfg = BenchmarkConfig::default();
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["validation_issues"].as_array().unwrap().is_empty());
    }

    #[test]
    fn report_json_validation_issues_nonempty_for_invalid() {
        let mut cfg = BenchmarkConfig::default();
        cfg.num_tokens = 0;
        let report = build_benchmark_report(&cfg, 100.0, 200.0);
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(!val["validation_issues"].as_array().unwrap().is_empty());
    }

    // ── Config compact JSON ─────────────────────────────────────────────

    #[test]
    fn config_json_compact_no_whitespace() {
        let json = serde_json::to_string(&BenchmarkConfig::default()).unwrap();
        assert!(!json.contains(' '));
        assert!(!json.contains('\n'));
    }
}
