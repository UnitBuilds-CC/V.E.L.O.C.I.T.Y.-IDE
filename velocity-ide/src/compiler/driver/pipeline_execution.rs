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
use serde::Serialize;

/// Model dimensions used by the pipeline execution.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineConfig {
    pub n_layers: usize,
    pub hidden_size: usize,
    pub ffn_size: usize,
    pub n_heads: usize,
    pub n_kv_heads: usize,
    pub head_dim: usize,
    pub max_seq_len: usize,
    pub rope_theta: f32,
    pub scale: f32,
}

/// Describes the compute dispatches recorded for a single transformer layer.
#[derive(Debug, Clone, Serialize)]
pub struct LayerDispatchPlan {
    pub layer_index: usize,
    pub buffer_copies: usize,
    pub rms_norm_dispatches: usize,
    pub gemv_dispatches: usize,
    pub bias_add_dispatches: usize,
    pub rope_dispatches: usize,
    pub kv_write_dispatches: usize,
    pub attn_softmax_dispatches: usize,
    pub residual_add_dispatches: usize,
    pub swiglu_dispatches: usize,
    pub total_dispatches: usize,
}

/// Full execution plan for a token forward pass.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineExecutionPlan {
    pub config: PipelineConfig,
    pub kv_dim: usize,
    pub per_layer: Vec<LayerDispatchPlan>,
    pub final_norm_dispatch: bool,
    pub total_buffer_copies: usize,
    pub total_dispatches: usize,
    pub validation_issues: Vec<String>,
}

/// Validate model dimensions for consistency.
pub fn validate_pipeline_config(cfg: &PipelineConfig) -> Vec<String> {
    let mut issues = Vec::new();
    if cfg.n_layers == 0 {
        issues.push("n_layers must be > 0".into());
    }
    if cfg.hidden_size == 0 {
        issues.push("hidden_size must be > 0".into());
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
    if cfg.n_heads % cfg.n_kv_heads != 0 && cfg.n_kv_heads != 0 && cfg.n_heads != 0 {
        issues.push(format!(
            "n_heads ({}) must be divisible by n_kv_heads ({})",
            cfg.n_heads, cfg.n_kv_heads
        ));
    }
    if cfg.hidden_size != cfg.n_heads * cfg.head_dim && cfg.n_heads != 0 && cfg.head_dim != 0 {
        issues.push(format!(
            "hidden_size ({}) != n_heads * head_dim ({} * {} = {})",
            cfg.hidden_size,
            cfg.n_heads,
            cfg.head_dim,
            cfg.n_heads * cfg.head_dim
        ));
    }
    if cfg.max_seq_len == 0 {
        issues.push("max_seq_len must be > 0".into());
    }
    if cfg.rope_theta <= 0.0 {
        issues.push("rope_theta must be > 0".into());
    }
    if cfg.scale <= 0.0 {
        issues.push("scale must be > 0".into());
    }
    issues
}

/// Compute the dispatch plan for a single layer (pure function, no GPU needed).
pub fn compute_layer_dispatch_plan(
    layer_index: usize,
    has_bias_q: bool,
    has_bias_k: bool,
    has_bias_v: bool,
    has_q_proj: bool,
    has_k_proj: bool,
    has_v_proj: bool,
    has_o_proj: bool,
    has_gate_proj: bool,
    has_up_proj: bool,
    has_down_proj: bool,
) -> LayerDispatchPlan {
    let buffer_copies = 3;
    let rms_norm_dispatches = 2;
    let mut gemv_dispatches = 0;
    if has_q_proj { gemv_dispatches += 1; }
    if has_k_proj { gemv_dispatches += 1; }
    if has_v_proj { gemv_dispatches += 1; }
    if has_o_proj { gemv_dispatches += 1; }
    if has_gate_proj { gemv_dispatches += 1; }
    if has_up_proj { gemv_dispatches += 1; }
    if has_down_proj { gemv_dispatches += 1; }
    let bias_add_dispatches = [has_bias_q, has_bias_k, has_bias_v].iter().filter(|&&b| b).count();
    let rope_dispatches = 1;
    let kv_write_dispatches = 1;
    let attn_softmax_dispatches = 1;
    let residual_add_dispatches = 2;
    let swiglu_dispatches = 1;
    let total_dispatches = rms_norm_dispatches + gemv_dispatches + bias_add_dispatches
        + rope_dispatches + kv_write_dispatches + attn_softmax_dispatches
        + residual_add_dispatches + swiglu_dispatches;
    LayerDispatchPlan {
        layer_index,
        buffer_copies,
        rms_norm_dispatches,
        gemv_dispatches,
        bias_add_dispatches,
        rope_dispatches,
        kv_write_dispatches,
        attn_softmax_dispatches,
        residual_add_dispatches,
        swiglu_dispatches,
        total_dispatches,
    }
}

/// Build a full execution plan for a token forward pass.
pub fn build_execution_plan(
    cfg: &PipelineConfig,
    layers_has_bias: &[(bool, bool, bool)],
    layers_has_projs: &[(bool, bool, bool, bool, bool, bool, bool)],
) -> PipelineExecutionPlan {
    let kv_dim = cfg.n_kv_heads * cfg.head_dim;
    let validation_issues = validate_pipeline_config(cfg);
    let mut per_layer = Vec::new();
    let mut total_buffer_copies = 0;
    let mut total_dispatches = 0;
    for i in 0..cfg.n_layers {
        let (bq, bk, bv) = if i < layers_has_bias.len() { layers_has_bias[i] } else { (false, false, false) };
        let (qp, kp, vp, op, gp, up, dp) = if i < layers_has_projs.len() { layers_has_projs[i] } else { (true, true, true, true, true, true, true) };
        let plan = compute_layer_dispatch_plan(i, bq, bk, bv, qp, kp, vp, op, gp, up, dp);
        total_buffer_copies += plan.buffer_copies;
        total_dispatches += plan.total_dispatches;
        per_layer.push(plan);
    }
    total_dispatches += 1; // final RMS norm
    PipelineExecutionPlan {
        config: cfg.clone(),
        kv_dim,
        per_layer,
        final_norm_dispatch: true,
        total_buffer_copies,
        total_dispatches,
        validation_issues,
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> PipelineConfig {
        PipelineConfig {
            n_layers: 2,
            hidden_size: 64,
            ffn_size: 256,
            n_heads: 4,
            n_kv_heads: 2,
            head_dim: 16,
            max_seq_len: 512,
            rope_theta: 10000.0,
            scale: 0.125,
        }
    }

    #[test]
    fn validate_config_valid() {
        assert!(validate_pipeline_config(&default_config()).is_empty());
    }

    #[test]
    fn validate_config_zero_layers() {
        let mut cfg = default_config();
        cfg.n_layers = 0;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("n_layers")));
    }

    #[test]
    fn validate_config_zero_hidden() {
        let mut cfg = default_config();
        cfg.hidden_size = 0;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("hidden_size")));
    }

    #[test]
    fn validate_config_heads_not_divisible() {
        let mut cfg = default_config();
        cfg.n_heads = 5;
        cfg.n_kv_heads = 2;
        cfg.hidden_size = 5 * 16;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("divisible")));
    }

    #[test]
    fn validate_config_hidden_mismatch() {
        let mut cfg = default_config();
        cfg.hidden_size = 128;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("hidden_size")));
    }

    #[test]
    fn validate_config_bad_rope_theta() {
        let mut cfg = default_config();
        cfg.rope_theta = 0.0;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("rope_theta")));
    }

    #[test]
    fn validate_config_bad_scale() {
        let mut cfg = default_config();
        cfg.scale = -1.0;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("scale")));
    }

    #[test]
    fn layer_dispatch_plan_all_projs() {
        let plan = compute_layer_dispatch_plan(0, true, true, true, true, true, true, true, true, true, true);
        assert_eq!(plan.gemv_dispatches, 7);
        assert_eq!(plan.bias_add_dispatches, 3);
        assert_eq!(plan.total_dispatches, 18);
    }

    #[test]
    fn layer_dispatch_plan_no_bias() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, true, true, true, true, true, true, true);
        assert_eq!(plan.bias_add_dispatches, 0);
        assert_eq!(plan.total_dispatches, 15);
    }

    #[test]
    fn layer_dispatch_plan_minimal() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, false, false, false, false, false, false, false);
        assert_eq!(plan.gemv_dispatches, 0);
        assert_eq!(plan.total_dispatches, 8);
    }

    #[test]
    fn build_execution_plan_works() {
        let cfg = default_config();
        let bias = vec![(true, true, true); 2];
        let projs = vec![(true, true, true, true, true, true, true); 2];
        let plan = build_execution_plan(&cfg, &bias, &projs);
        assert_eq!(plan.per_layer.len(), 2);
        assert_eq!(plan.kv_dim, 32);
        assert_eq!(plan.total_buffer_copies, 6);
        assert!(plan.validation_issues.is_empty());
        assert_eq!(plan.total_dispatches, 37);
    }

    #[test]
    fn build_execution_plan_with_issues() {
        let mut cfg = default_config();
        cfg.n_layers = 0;
        cfg.hidden_size = 0;
        let plan = build_execution_plan(&cfg, &[], &[]);
        assert!(plan.validation_issues.len() >= 2);
    }

    #[test]
    fn execution_plan_serializes() {
        let cfg = default_config();
        let plan = build_execution_plan(&cfg, &[(false, false, false); 2], &[(true, true, true, true, true, true, true); 2]);
        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("kv_dim"));
        assert!(json.contains("per_layer"));
    }

    #[test]
    fn pipeline_config_serializes() {
        let json = serde_json::to_string(&default_config()).unwrap();
        assert!(json.contains("n_layers"));
        assert!(json.contains("hidden_size"));
    }
}
