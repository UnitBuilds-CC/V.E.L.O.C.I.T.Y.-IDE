//! Vulkan pipeline execution for transformer model forward pass.
//!
//! # Safety Invariants
//!
//! All `unsafe` blocks wrap Vulkan command-buffer recording calls via `ash`.
//! - `pipeline` and `driver` contain valid, initialized Vulkan handles from init.
//! - `cmd` is a valid command buffer in recording state throughout.
//! - Descriptor sets, pipeline layouts, and push constant ranges are pre-validated.
//! - Buffer copies use sizes derived from model dimensions (always positive multiples of 4).
//! - Barriers ensure correct memory visibility between pipeline stages.

use super::layer_gpu_gemvs::LayerGpuGemvs;
use super::model_pipeline::VulkanModelPipeline;
use super::vulkan_init::*;
use ash::vk;

#[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
pub fn record_and_execute_token(
    pipeline: &VulkanModelPipeline,
    driver: &VulkanDriver,
    n_layers: usize,
    hidden_size: usize,
    _ffn_size: usize,
    n_heads: usize,
    n_kv_heads: usize,
    head_dim: usize,
    max_seq_len: usize,
    rope_theta: f32,
    scale: f32,
    pos: u32,
    layers_gpu: &[&LayerGpuGemvs],
) -> Result<(), Box<dyn std::error::Error>> {
    let device = &pipeline.device;
    let cmd = pipeline.command_buffer;

    let begin_info =
        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    // SAFETY: begin_command_buffer starts recording into a valid command buffer.
    unsafe {
        device.begin_command_buffer(cmd, &begin_info)?;
    }

    let kv_dim = n_kv_heads * head_dim;

    for i in 0..n_layers {
        let lg = layers_gpu[i];

        cmd_compute_to_transfer_barrier(device, cmd);
        // SAFETY: cmd_copy_buffer copies residual state between valid GPU buffers.
        unsafe {
            device.cmd_copy_buffer(
                cmd,
                pipeline.x_residual_buffer,
                driver.shared_input_buffer,
                &[vk::BufferCopy::builder()
                    .size((hidden_size * 4) as vk::DeviceSize)
                    .build()],
            );
        }
        cmd_transfer_to_compute_barrier(device, cmd);

        // SAFETY: Record compute dispatch — bind pipeline, descriptor sets, push constants,
        // dispatch. All handles are valid from pipeline initialization.
        unsafe {
            device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.pipeline_rms_norm,
            );
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.layout_rms_norm,
                0,
                &[pipeline.desc_sets_rms_norm_attn[i]],
                &[],
            );

            let params = [hidden_size as u32, 1e-6f32.to_bits()];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(
                cmd,
                pipeline.layout_rms_norm,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );
            device.cmd_dispatch(cmd, 1, 1, 1);
        }
        cmd_compute_barrier(device, cmd);

        if let Some(ref g) = lg.q_proj_gpu {
            g.record_dispatch(cmd);
        }
        if let Some(ref g) = lg.k_proj_gpu {
            g.record_dispatch(cmd);
        }
        if let Some(ref g) = lg.v_proj_gpu {
            g.record_dispatch(cmd);
        }
        cmd_compute_barrier(device, cmd);

        if let Some(ref bias_q_set) = pipeline.desc_sets_bias_q[i] {
            // SAFETY: Bias add dispatch — bind bias_add pipeline, descriptor set, push constant.
            unsafe {
                device.cmd_bind_pipeline(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline.pipeline_bias_add,
                );
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline.layout_bias_add,
                    0,
                    &[*bias_q_set],
                    &[],
                );
                let params = [hidden_size as u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
                device.cmd_push_constants(
                    cmd,
                    pipeline.layout_bias_add,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    params_bytes,
                );
                device.cmd_dispatch(cmd, (hidden_size as u32).div_ceil(256), 1, 1);
            }
        }
        if let Some(ref bias_k_set) = pipeline.desc_sets_bias_k[i] {
            // SAFETY: Bias add dispatch for K projection.
            unsafe {
                device.cmd_bind_pipeline(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline.pipeline_bias_add,
                );
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline.layout_bias_add,
                    0,
                    &[*bias_k_set],
                    &[],
                );
                let params = [kv_dim as u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
                device.cmd_push_constants(
                    cmd,
                    pipeline.layout_bias_add,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    params_bytes,
                );
                device.cmd_dispatch(cmd, (kv_dim as u32).div_ceil(256), 1, 1);
            }
        }
        if let Some(ref bias_v_set) = pipeline.desc_sets_bias_v[i] {
            // SAFETY: Bias add dispatch for V projection.
            unsafe {
                device.cmd_bind_pipeline(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline.pipeline_bias_add,
                );
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    pipeline.layout_bias_add,
                    0,
                    &[*bias_v_set],
                    &[],
                );
                let params = [kv_dim as u32];
                let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
                device.cmd_push_constants(
                    cmd,
                    pipeline.layout_bias_add,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    params_bytes,
                );
                device.cmd_dispatch(cmd, (kv_dim as u32).div_ceil(256), 1, 1);
            }
        }
        if pipeline.desc_sets_bias_q[i].is_some()
            || pipeline.desc_sets_bias_k[i].is_some()
            || pipeline.desc_sets_bias_v[i].is_some()
        {
            cmd_compute_barrier(device, cmd);
        }

        // SAFETY: RoPE (Rotary Position Embedding) dispatch — bind rope pipeline,
        // descriptor set, push constants (pos, rope_theta, scale, head_dim).
        unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline.pipeline_rope);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.layout_rope,
                0,
                &[pipeline.desc_sets_rope[i]],
                &[],
            );

            let params = [
                pos,
                head_dim as u32,
                n_heads as u32,
                n_kv_heads as u32,
                rope_theta.to_bits(),
            ];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 20);
            device.cmd_push_constants(
                cmd,
                pipeline.layout_rope,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );

            let total_q_pairs = n_heads * (head_dim / 2);
            device.cmd_dispatch(cmd, (total_q_pairs as u32).div_ceil(64), 1, 1);
        }
        cmd_compute_barrier(device, cmd);

        // SAFETY: Record compute dispatch — bind pipeline, descriptor sets, push constants,
        // dispatch. All handles are valid from pipeline initialization.
        unsafe {
            device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.pipeline_kv_write,
            );
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.layout_kv_write,
                0,
                &[pipeline.desc_sets_kv_write[i]],
                &[],
            );

            let params = [pos, kv_dim as u32, max_seq_len as u32];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 12);
            device.cmd_push_constants(
                cmd,
                pipeline.layout_kv_write,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );
            device.cmd_dispatch(cmd, (kv_dim as u32).div_ceil(64), 1, 1);
        }
        cmd_compute_barrier(device, cmd);

        // SAFETY: Record compute dispatch — bind pipeline, descriptor sets, push constants,
        // dispatch. All handles are valid from pipeline initialization.
        unsafe {
            device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.pipeline_attn_softmax,
            );
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.layout_attn_softmax,
                0,
                &[pipeline.desc_sets_attn_softmax[i]],
                &[],
            );

            let params = [
                pos,
                head_dim as u32,
                n_heads as u32,
                n_kv_heads as u32,
                max_seq_len as u32,
                scale.to_bits(),
            ];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 24);
            device.cmd_push_constants(
                cmd,
                pipeline.layout_attn_softmax,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );
            device.cmd_dispatch(cmd, n_heads as u32, 1, 1);
        }
        cmd_compute_barrier(device, cmd);

        cmd_compute_to_transfer_barrier(device, cmd);
        // SAFETY: cmd_copy_buffer copies residual state between valid GPU buffers.
        unsafe {
            device.cmd_copy_buffer(
                cmd,
                pipeline.attn_out_buffer,
                driver.shared_input_buffer,
                &[vk::BufferCopy::builder()
                    .size((hidden_size * 4) as vk::DeviceSize)
                    .build()],
            );
        }
        cmd_transfer_to_compute_barrier(device, cmd);

        if let Some(ref g) = lg.o_proj_gpu {
            g.record_dispatch(cmd);
        }
        cmd_compute_barrier(device, cmd);

        // SAFETY: Record compute dispatch — bind pipeline, descriptor sets, push constants,
        // dispatch. All handles are valid from pipeline initialization.
        unsafe {
            device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.pipeline_residual_add,
            );
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.layout_residual_add,
                0,
                &[pipeline.desc_sets_residual_add_attn[i]],
                &[],
            );
            let params = [hidden_size as u32];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
            device.cmd_push_constants(
                cmd,
                pipeline.layout_residual_add,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );
            device.cmd_dispatch(cmd, (hidden_size as u32).div_ceil(256), 1, 1);
        }
        cmd_compute_barrier(device, cmd);

        cmd_compute_to_transfer_barrier(device, cmd);
        // SAFETY: cmd_copy_buffer copies residual state between valid GPU buffers.
        unsafe {
            device.cmd_copy_buffer(
                cmd,
                pipeline.x_residual_buffer,
                driver.shared_input_buffer,
                &[vk::BufferCopy::builder()
                    .size((hidden_size * 4) as vk::DeviceSize)
                    .build()],
            );
        }
        cmd_transfer_to_compute_barrier(device, cmd);

        // SAFETY: Record compute dispatch — bind pipeline, descriptor sets, push constants,
        // dispatch. All handles are valid from pipeline initialization.
        unsafe {
            device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.pipeline_rms_norm,
            );
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.layout_rms_norm,
                0,
                &[pipeline.desc_sets_rms_norm_ffn[i]],
                &[],
            );
            let params = [hidden_size as u32, 1e-6f32.to_bits()];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
            device.cmd_push_constants(
                cmd,
                pipeline.layout_rms_norm,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );
            device.cmd_dispatch(cmd, 1, 1, 1);
        }
        cmd_compute_barrier(device, cmd);

        if let Some(ref g) = lg.gate_proj_gpu {
            g.record_dispatch(cmd);
        }
        if let Some(ref g) = lg.up_proj_gpu {
            g.record_dispatch(cmd);
        }
        cmd_compute_barrier(device, cmd);

        // SAFETY: Record compute dispatch — bind pipeline, descriptor sets, push constants,
        // dispatch. All handles are valid from pipeline initialization.
        unsafe {
            device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.pipeline_swiglu,
            );
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.layout_swiglu,
                0,
                &[pipeline.desc_sets_swiglu[i]],
                &[],
            );
            let params = [_ffn_size as u32];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
            device.cmd_push_constants(
                cmd,
                pipeline.layout_swiglu,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );
            device.cmd_dispatch(cmd, (_ffn_size as u32).div_ceil(256), 1, 1);
        }
        cmd_compute_barrier(device, cmd);

        cmd_compute_to_transfer_barrier(device, cmd);
        // SAFETY: cmd_copy_buffer copies residual state between valid GPU buffers.
        unsafe {
            device.cmd_copy_buffer(
                cmd,
                pipeline.gated_buffer,
                driver.shared_input_buffer,
                &[vk::BufferCopy::builder()
                    .size((_ffn_size * 4) as vk::DeviceSize)
                    .build()],
            );
        }
        cmd_transfer_to_compute_barrier(device, cmd);

        if let Some(ref g) = lg.down_proj_gpu {
            g.record_dispatch(cmd);
        }
        cmd_compute_barrier(device, cmd);

        // SAFETY: Record compute dispatch — bind pipeline, descriptor sets, push constants,
        // dispatch. All handles are valid from pipeline initialization.
        unsafe {
            device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.pipeline_residual_add,
            );
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipeline.layout_residual_add,
                0,
                &[pipeline.desc_sets_residual_add_ffn[i]],
                &[],
            );
            let params = [hidden_size as u32];
            let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 4);
            device.cmd_push_constants(
                cmd,
                pipeline.layout_residual_add,
                vk::ShaderStageFlags::COMPUTE,
                0,
                params_bytes,
            );
            device.cmd_dispatch(cmd, (hidden_size as u32).div_ceil(256), 1, 1);
        }
        cmd_compute_barrier(device, cmd);
    }

    // SAFETY: Record final RMS norm dispatch — bind pipeline, descriptor sets, push constants.
    // All handles (pipeline, layout, desc_set_final_norm) are valid from pipeline initialization.
    unsafe {
        device.cmd_bind_pipeline(
            cmd,
            vk::PipelineBindPoint::COMPUTE,
            pipeline.pipeline_rms_norm,
        );
        device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::COMPUTE,
            pipeline.layout_rms_norm,
            0,
            &[pipeline.desc_set_final_norm],
            &[],
        );
        let params = [hidden_size as u32, 1e-6f32.to_bits()];
        let params_bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 8);
        device.cmd_push_constants(
            cmd,
            pipeline.layout_rms_norm,
            vk::ShaderStageFlags::COMPUTE,
            0,
            params_bytes,
        );
        device.cmd_dispatch(cmd, 1, 1, 1);
    }
    cmd_compute_to_host_barrier(device, cmd);

    // SAFETY: `cmd` is a valid recording command buffer; end_command_buffer finishes recording.
    unsafe {
        device.end_command_buffer(cmd)?;
    }

    // SAFETY: `pipeline.fence`, `pipeline.queue` are valid handles from initialization.
    // reset_fences/queue_submit/wait_for_fences are standard Vulkan synchronization operations.
    unsafe {
        device.reset_fences(&[pipeline.fence])?;
        device.queue_submit(
            pipeline.queue,
            &[vk::SubmitInfo::builder().command_buffers(&[cmd]).build()],
            pipeline.fence,
        )?;
        device.wait_for_fences(&[pipeline.fence], true, u64::MAX)?;
    }

    Ok(())
}
