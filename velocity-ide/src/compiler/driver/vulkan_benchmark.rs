use std::ffi::CString;
use std::time::Instant;

use ash::vk;

use super::vulkan_init::*;

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
    let shader_module_contig =
        unsafe { device.create_shader_module(&shader_info_contig, None)? };

    let shader_info_ndakv =
        vk::ShaderModuleCreateInfo::builder().code(crate::compiler::shaders::ATTN_NDAKV_SPV);
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
    let layout_info_ndakv =
        vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings_ndakv);
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
    let pipeline_layout_contig =
        unsafe { device.create_pipeline_layout(&pipeline_layout_info_contig, None)? };

    let layouts_ndakv = [desc_set_layout_ndakv];
    let pipeline_layout_info_ndakv = vk::PipelineLayoutCreateInfo::builder()
        .set_layouts(&layouts_ndakv)
        .push_constant_ranges(&push_constant_ranges);
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

    let (q_buffer, q_memory, q_ptr) =
        create_coherent_buffer(device, instance, physical_device, q_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
    let (k_buffer, k_memory, k_ptr) =
        create_coherent_buffer(device, instance, physical_device, k_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
    let (v_buffer, v_memory, v_ptr) =
        create_coherent_buffer(device, instance, physical_device, v_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
    let (block_buffer, block_memory, block_ptr) =
        create_coherent_buffer(device, instance, physical_device, block_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;
    let (out_buffer, out_memory, _out_ptr) =
        create_coherent_buffer(device, instance, physical_device, out_size, vk::BufferUsageFlags::STORAGE_BUFFER)?;

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
    let desc_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

    let desc_set_contig = unsafe {
        device.allocate_descriptor_sets(
            &vk::DescriptorSetAllocateInfo::builder()
                .descriptor_pool(desc_pool)
                .set_layouts(&[desc_set_layout_contig]),
        )?[0]
    };
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
    unsafe { device.update_descriptor_sets(&writes_ndakv, &[]) };

    let command_pool_info = vk::CommandPoolCreateInfo::builder()
        .queue_family_index(driver.queue_family_index)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
    let command_pool = unsafe { device.create_command_pool(&command_pool_info, None)? };

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
    let fence = unsafe { device.create_fence(&fence_info, None)? };

    let iterations = 500;

    let mut total_contig = 0.0;
    for _ in 0..iterations {
        let start = Instant::now();
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
