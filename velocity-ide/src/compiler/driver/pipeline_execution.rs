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
    if cfg.n_kv_heads != 0 && cfg.n_heads != 0 && !cfg.n_heads.is_multiple_of(cfg.n_kv_heads) {
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
#[allow(clippy::too_many_arguments)]
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

    // ── Block 114: expanded tests ────────────────────────────────────────────

    #[test]
    fn validate_config_zero_n_heads() {
        let mut cfg = default_config();
        cfg.n_heads = 0;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("n_heads")));
    }

    #[test]
    fn validate_config_zero_n_kv_heads() {
        let mut cfg = default_config();
        cfg.n_kv_heads = 0;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("n_kv_heads")));
    }

    #[test]
    fn validate_config_zero_head_dim() {
        let mut cfg = default_config();
        cfg.head_dim = 0;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("head_dim")));
    }

    #[test]
    fn validate_config_zero_max_seq_len() {
        let mut cfg = default_config();
        cfg.max_seq_len = 0;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("max_seq_len")));
    }

    #[test]
    fn validate_config_negative_rope_theta() {
        let mut cfg = default_config();
        cfg.rope_theta = -100.0;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("rope_theta")));
    }

    #[test]
    fn validate_config_zero_scale() {
        let mut cfg = default_config();
        cfg.scale = 0.0;
        assert!(validate_pipeline_config(&cfg).iter().any(|i| i.contains("scale")));
    }

    #[test]
    fn validate_config_multiple_issues() {
        let mut cfg = default_config();
        cfg.n_layers = 0;
        cfg.hidden_size = 0;
        cfg.n_heads = 0;
        cfg.rope_theta = 0.0;
        let issues = validate_pipeline_config(&cfg);
        assert!(issues.len() >= 4, "expected >=4 issues, got {:?}", issues);
    }

    #[test]
    fn layer_dispatch_plan_partial_projections() {
        // Only Q and O projections (no K, V, gate, up, down)
        let plan = compute_layer_dispatch_plan(5, false, false, false, true, false, false, true, false, false, false);
        assert_eq!(plan.layer_index, 5);
        assert_eq!(plan.gemv_dispatches, 2);
        assert_eq!(plan.bias_add_dispatches, 0);
        assert_eq!(plan.buffer_copies, 3);
        assert_eq!(plan.rms_norm_dispatches, 2);
        assert_eq!(plan.rope_dispatches, 1);
    }

    #[test]
    fn layer_dispatch_plan_mixed_bias() {
        // Only bias_q and bias_v, no bias_k
        let plan = compute_layer_dispatch_plan(0, true, false, true, true, true, true, true, true, true, true);
        assert_eq!(plan.bias_add_dispatches, 2);
        assert_eq!(plan.gemv_dispatches, 7);
    }

    #[test]
    fn layer_dispatch_plan_total_accounting() {
        let plan = compute_layer_dispatch_plan(0, true, true, false, true, true, true, true, true, true, true);
        let expected = plan.rms_norm_dispatches + plan.gemv_dispatches + plan.bias_add_dispatches
            + plan.rope_dispatches + plan.kv_write_dispatches + plan.attn_softmax_dispatches
            + plan.residual_add_dispatches + plan.swiglu_dispatches;
        assert_eq!(plan.total_dispatches, expected);
    }

    #[test]
    fn layer_dispatch_plan_serializes() {
        let plan = compute_layer_dispatch_plan(3, true, false, true, true, true, true, true, true, true, true);
        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("layer_index"));
        assert!(json.contains("gemv_dispatches"));
        assert!(json.contains("total_dispatches"));
    }

    #[test]
    fn build_execution_plan_kv_dim() {
        let mut cfg = default_config();
        cfg.n_kv_heads = 4;
        cfg.head_dim = 32;
        let plan = build_execution_plan(&cfg, &[], &[]);
        assert_eq!(plan.kv_dim, 128);
    }

    #[test]
    fn build_execution_plan_fewer_entries_than_layers() {
        let cfg = default_config(); // 2 layers
        // Only provide bias/proj for 1 layer, second should use defaults
        let bias = vec![(true, true, true)];
        let projs = vec![(true, true, true, true, true, true, true)];
        let plan = build_execution_plan(&cfg, &bias, &projs);
        assert_eq!(plan.per_layer.len(), 2);
        // First layer has bias
        assert_eq!(plan.per_layer[0].bias_add_dispatches, 3);
        // Second layer defaults to no bias
        assert_eq!(plan.per_layer[1].bias_add_dispatches, 0);
    }

    #[test]
    fn build_execution_plan_single_layer() {
        let mut cfg = default_config();
        cfg.n_layers = 1;
        let bias = vec![(false, false, false)];
        let projs = vec![(true, true, true, true, true, true, true)];
        let plan = build_execution_plan(&cfg, &bias, &projs);
        assert_eq!(plan.per_layer.len(), 1);
        assert!(plan.final_norm_dispatch);
    }

    #[test]
    fn build_execution_plan_total_dispatch_includes_final_norm() {
        let cfg = default_config();
        let bias = vec![(false, false, false); 2];
        let projs = vec![(true, true, true, true, true, true, true); 2];
        let plan = build_execution_plan(&cfg, &bias, &projs);
        let layer_sum: usize = plan.per_layer.iter().map(|l| l.total_dispatches).sum::<usize>();
        // total = sum of layer dispatches + 1 (final norm)
        assert_eq!(plan.total_dispatches, layer_sum + 1);
    }

    #[test]
    fn build_execution_plan_no_layers() {
        let mut cfg = default_config();
        cfg.n_layers = 0;
        let plan = build_execution_plan(&cfg, &[], &[]);
        assert!(plan.per_layer.is_empty());
        assert!(plan.validation_issues.iter().any(|i| i.contains("n_layers")));
        // Still has final norm dispatch
        assert_eq!(plan.total_dispatches, 1);
    }

    #[test]
    fn pipeline_config_clone() {
        let cfg = default_config();
        let cloned = cfg.clone();
        assert_eq!(cloned.n_layers, cfg.n_layers);
        assert_eq!(cloned.hidden_size, cfg.hidden_size);
        assert_eq!(cloned.n_heads, cfg.n_heads);
        assert_eq!(cloned.rope_theta, cfg.rope_theta);
    }

    #[test]
    fn execution_plan_find_layer() {
        let cfg = default_config();
        let plan = build_execution_plan(&cfg, &[(false, false, false); 2], &[(true, true, true, true, true, true, true); 2]);
        assert_eq!(plan.per_layer[0].layer_index, 0);
        assert_eq!(plan.per_layer[1].layer_index, 1);
    }

    #[test]
    fn execution_plan_buffer_copies_accounting() {
        let cfg = default_config();
        let plan = build_execution_plan(&cfg, &[(false, false, false); 2], &[(true, true, true, true, true, true, true); 2]);
        let expected_copies: usize = plan.per_layer.iter().map(|l| l.buffer_copies).sum();
        assert_eq!(plan.total_buffer_copies, expected_copies);
    }

    // ── Block 127: comprehensive tests ──────────────────────────────────────

    #[test]
    fn validate_config_all_zeros() {
        let cfg = PipelineConfig {
            n_layers: 0, hidden_size: 0, ffn_size: 0, n_heads: 0,
            n_kv_heads: 0, head_dim: 0, max_seq_len: 0,
            rope_theta: 0.0, scale: 0.0,
        };
        let issues = validate_pipeline_config(&cfg);
        // n_layers, hidden_size, n_heads, n_kv_heads, head_dim, max_seq_len, rope_theta, scale = 8
        // divisibility and hidden_mismatch guards prevent those when heads/head_dim are 0
        assert!(issues.len() >= 8, "expected >=8 issues, got {}: {:?}", issues.len(), issues);
    }

    #[test]
    fn validate_config_divisibility_issue_contains_values() {
        let mut cfg = default_config();
        cfg.n_heads = 7;
        cfg.n_kv_heads = 3;
        cfg.hidden_size = 7 * 16;
        let issues = validate_pipeline_config(&cfg);
        let div_issue = issues.iter().find(|i| i.contains("divisible")).unwrap();
        assert!(div_issue.contains("7"), "issue should contain n_heads value: {}", div_issue);
        assert!(div_issue.contains("3"), "issue should contain n_kv_heads value: {}", div_issue);
    }

    #[test]
    fn validate_config_hidden_mismatch_contains_computed() {
        let mut cfg = default_config();
        cfg.hidden_size = 100; // 4 * 16 = 64 != 100
        let issues = validate_pipeline_config(&cfg);
        let mismatch = issues.iter().find(|i| i.contains("!=")).unwrap();
        assert!(mismatch.contains("100"), "should contain actual hidden_size: {}", mismatch);
        assert!(mismatch.contains("64"), "should contain computed n_heads*head_dim: {}", mismatch);
    }

    #[test]
    fn validate_config_ffn_size_zero_is_ok() {
        let mut cfg = default_config();
        cfg.ffn_size = 0;
        // ffn_size is NOT validated by validate_pipeline_config
        assert!(validate_pipeline_config(&cfg).is_empty());
    }

    #[test]
    fn validate_config_positive_boundary_values() {
        let cfg = PipelineConfig {
            n_layers: 1, hidden_size: 16, ffn_size: 64, n_heads: 1,
            n_kv_heads: 1, head_dim: 16, max_seq_len: 1,
            rope_theta: 0.001, scale: 0.001,
        };
        assert!(validate_pipeline_config(&cfg).is_empty());
    }

    #[test]
    fn validate_config_large_values() {
        let cfg = PipelineConfig {
            n_layers: 1000, hidden_size: 8192, ffn_size: 32768, n_heads: 64,
            n_kv_heads: 8, head_dim: 128, max_seq_len: 131072,
            rope_theta: 100000.0, scale: 1.0,
        };
        assert!(validate_pipeline_config(&cfg).is_empty());
    }

    #[test]
    fn validate_config_heads_equal_kv_heads() {
        let mut cfg = default_config();
        cfg.n_heads = 4;
        cfg.n_kv_heads = 4;
        cfg.hidden_size = 4 * 16;
        assert!(validate_pipeline_config(&cfg).is_empty());
    }

    #[test]
    fn validate_config_kv_heads_is_one() {
        let mut cfg = default_config();
        cfg.n_heads = 8;
        cfg.n_kv_heads = 1;
        cfg.hidden_size = 8 * 16;
        assert!(validate_pipeline_config(&cfg).is_empty());
    }

    // ── compute_layer_dispatch_plan: individual projections ─────────────

    #[test]
    fn dispatch_plan_only_q_proj() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, true, false, false, false, false, false, false);
        assert_eq!(plan.gemv_dispatches, 1);
        assert_eq!(plan.total_dispatches, 9); // 8 base + 1 gemv
    }

    #[test]
    fn dispatch_plan_only_k_proj() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, false, true, false, false, false, false, false);
        assert_eq!(plan.gemv_dispatches, 1);
    }

    #[test]
    fn dispatch_plan_only_v_proj() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, false, false, true, false, false, false, false);
        assert_eq!(plan.gemv_dispatches, 1);
    }

    #[test]
    fn dispatch_plan_only_o_proj() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, false, false, false, true, false, false, false);
        assert_eq!(plan.gemv_dispatches, 1);
    }

    #[test]
    fn dispatch_plan_only_gate_proj() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, false, false, false, false, true, false, false);
        assert_eq!(plan.gemv_dispatches, 1);
    }

    #[test]
    fn dispatch_plan_only_up_proj() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, false, false, false, false, false, true, false);
        assert_eq!(plan.gemv_dispatches, 1);
    }

    #[test]
    fn dispatch_plan_only_down_proj() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, false, false, false, false, false, false, true);
        assert_eq!(plan.gemv_dispatches, 1);
    }

    #[test]
    fn dispatch_plan_attention_only() {
        // Q, K, V, O projections, no FFN
        let plan = compute_layer_dispatch_plan(0, false, false, false, true, true, true, true, false, false, false);
        assert_eq!(plan.gemv_dispatches, 4);
        assert_eq!(plan.swiglu_dispatches, 1); // still present even without FFN projs
        let expected = plan.rms_norm_dispatches + plan.gemv_dispatches + plan.bias_add_dispatches
            + plan.rope_dispatches + plan.kv_write_dispatches + plan.attn_softmax_dispatches
            + plan.residual_add_dispatches + plan.swiglu_dispatches;
        assert_eq!(plan.total_dispatches, expected);
    }

    #[test]
    fn dispatch_plan_ffn_only() {
        // gate, up, down projections, no attention
        let plan = compute_layer_dispatch_plan(0, false, false, false, false, false, false, false, true, true, true);
        assert_eq!(plan.gemv_dispatches, 3);
        assert_eq!(plan.rope_dispatches, 1); // always 1
        assert_eq!(plan.kv_write_dispatches, 1); // always 1
    }

    #[test]
    fn dispatch_plan_only_bias_q() {
        let plan = compute_layer_dispatch_plan(0, true, false, false, false, false, false, false, false, false, false);
        assert_eq!(plan.bias_add_dispatches, 1);
        assert_eq!(plan.gemv_dispatches, 0);
    }

    #[test]
    fn dispatch_plan_only_bias_k() {
        let plan = compute_layer_dispatch_plan(0, false, true, false, false, false, false, false, false, false, false);
        assert_eq!(plan.bias_add_dispatches, 1);
    }

    #[test]
    fn dispatch_plan_only_bias_v() {
        let plan = compute_layer_dispatch_plan(0, false, false, true, false, false, false, false, false, false, false);
        assert_eq!(plan.bias_add_dispatches, 1);
    }

    #[test]
    fn dispatch_plan_fixed_fields() {
        // These fields are constant regardless of projection/bias config
        for bias_combo in 0..8u8 {
            let bq = bias_combo & 1 != 0;
            let bk = bias_combo & 2 != 0;
            let bv = bias_combo & 4 != 0;
            let plan = compute_layer_dispatch_plan(0, bq, bk, bv, false, false, false, false, false, false, false);
            assert_eq!(plan.buffer_copies, 3, "buffer_copies is always 3");
            assert_eq!(plan.rms_norm_dispatches, 2, "rms_norm is always 2");
            assert_eq!(plan.rope_dispatches, 1, "rope is always 1");
            assert_eq!(plan.kv_write_dispatches, 1, "kv_write is always 1");
            assert_eq!(plan.attn_softmax_dispatches, 1, "attn_softmax is always 1");
            assert_eq!(plan.residual_add_dispatches, 2, "residual_add is always 2");
            assert_eq!(plan.swiglu_dispatches, 1, "swiglu is always 1");
        }
    }

    #[test]
    fn dispatch_plan_layer_index_various() {
        for idx in [0, 1, 7, 42, 999, usize::MAX] {
            let plan = compute_layer_dispatch_plan(idx, false, false, false, false, false, false, false, false, false, false);
            assert_eq!(plan.layer_index, idx);
        }
    }

    #[test]
    fn dispatch_plan_total_sum_invariant_all_bias_combos() {
        // Test total = sum of parts for all 8 bias combinations with all projections
        for bias_combo in 0..8u8 {
            let bq = bias_combo & 1 != 0;
            let bk = bias_combo & 2 != 0;
            let bv = bias_combo & 4 != 0;
            let plan = compute_layer_dispatch_plan(0, bq, bk, bv, true, true, true, true, true, true, true);
            let parts = plan.rms_norm_dispatches + plan.gemv_dispatches + plan.bias_add_dispatches
                + plan.rope_dispatches + plan.kv_write_dispatches + plan.attn_softmax_dispatches
                + plan.residual_add_dispatches + plan.swiglu_dispatches;
            assert_eq!(plan.total_dispatches, parts, "failed for bias combo {}", bias_combo);
        }
    }

    #[test]
    fn dispatch_plan_max_total() {
        let plan = compute_layer_dispatch_plan(0, true, true, true, true, true, true, true, true, true, true);
        // 2 rms + 7 gemv + 3 bias + 1 rope + 1 kv + 1 softmax + 2 residual + 1 swiglu = 18
        assert_eq!(plan.total_dispatches, 18);
    }

    #[test]
    fn dispatch_plan_min_total() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, false, false, false, false, false, false, false);
        // 2 rms + 0 + 0 + 1 rope + 1 kv + 1 softmax + 2 residual + 1 swiglu = 8
        assert_eq!(plan.total_dispatches, 8);
    }

    // ── build_execution_plan: detailed tests ────────────────────────────

    #[test]
    fn execution_plan_default_fallback_projs_all_true() {
        // When layers_has_projs is empty, defaults to (true,true,true,true,true,true,true)
        let cfg = default_config();
        let plan = build_execution_plan(&cfg, &[], &[]);
        // Each layer should have 7 gemv_dispatches (all projections default to true)
        for layer in &plan.per_layer {
            assert_eq!(layer.gemv_dispatches, 7, "layer {} should default to all projs", layer.layer_index);
        }
    }

    #[test]
    fn execution_plan_default_fallback_bias_all_false() {
        // When layers_has_bias is empty, defaults to (false, false, false)
        let cfg = default_config();
        let plan = build_execution_plan(&cfg, &[], &[]);
        for layer in &plan.per_layer {
            assert_eq!(layer.bias_add_dispatches, 0, "layer {} should default to no bias", layer.layer_index);
        }
    }

    #[test]
    fn execution_plan_kv_dim_various() {
        let mut cfg = default_config();
        // 1 kv_head * 64 head_dim
        cfg.n_kv_heads = 1;
        cfg.head_dim = 64;
        cfg.hidden_size = 4 * 64;
        let plan = build_execution_plan(&cfg, &[], &[]);
        assert_eq!(plan.kv_dim, 64);

        // 16 kv_heads * 128 head_dim
        cfg.n_kv_heads = 16;
        cfg.head_dim = 128;
        cfg.n_heads = 16;
        cfg.hidden_size = 16 * 128;
        let plan = build_execution_plan(&cfg, &[], &[]);
        assert_eq!(plan.kv_dim, 2048);
    }

    #[test]
    fn execution_plan_per_layer_indices_sequential() {
        let mut cfg = default_config();
        cfg.n_layers = 5;
        let plan = build_execution_plan(&cfg, &[], &[]);
        for (i, layer) in plan.per_layer.iter().enumerate() {
            assert_eq!(layer.layer_index, i);
        }
    }

    #[test]
    fn execution_plan_config_cloned_into_plan() {
        let cfg = default_config();
        let plan = build_execution_plan(&cfg, &[], &[]);
        assert_eq!(plan.config.n_layers, cfg.n_layers);
        assert_eq!(plan.config.hidden_size, cfg.hidden_size);
        assert_eq!(plan.config.ffn_size, cfg.ffn_size);
        assert_eq!(plan.config.n_heads, cfg.n_heads);
        assert_eq!(plan.config.n_kv_heads, cfg.n_kv_heads);
        assert_eq!(plan.config.head_dim, cfg.head_dim);
        assert_eq!(plan.config.max_seq_len, cfg.max_seq_len);
        assert_eq!(plan.config.rope_theta, cfg.rope_theta);
        assert_eq!(plan.config.scale, cfg.scale);
    }

    #[test]
    fn execution_plan_validation_issues_propagate() {
        let mut cfg = default_config();
        cfg.n_heads = 0;
        cfg.rope_theta = -1.0;
        let plan = build_execution_plan(&cfg, &[], &[]);
        assert!(plan.validation_issues.iter().any(|i| i.contains("n_heads")));
        assert!(plan.validation_issues.iter().any(|i| i.contains("rope_theta")));
    }

    #[test]
    fn execution_plan_many_layers() {
        let mut cfg = default_config();
        cfg.n_layers = 10;
        cfg.hidden_size = 4 * 16;
        let bias = vec![(true, false, true); 10];
        let projs = vec![(true, true, true, true, true, true, true); 10];
        let plan = build_execution_plan(&cfg, &bias, &projs);
        assert_eq!(plan.per_layer.len(), 10);
        assert_eq!(plan.total_buffer_copies, 30); // 3 per layer * 10
        // Each layer: 2+7+1+1+1+1+2+1 = 16
        let layer_sum: usize = plan.per_layer.iter().map(|l| l.total_dispatches).sum();
        assert_eq!(plan.total_dispatches, layer_sum + 1);
    }

    #[test]
    fn execution_plan_mixed_per_layer_config() {
        let cfg = default_config(); // 2 layers
        let bias = vec![(true, true, true), (false, false, false)];
        let projs = vec![
            (true, true, true, true, true, true, true),  // layer 0: all projs
            (false, false, false, false, false, false, false), // layer 1: no projs
        ];
        let plan = build_execution_plan(&cfg, &bias, &projs);
        assert_eq!(plan.per_layer[0].gemv_dispatches, 7);
        assert_eq!(plan.per_layer[0].bias_add_dispatches, 3);
        assert_eq!(plan.per_layer[1].gemv_dispatches, 0);
        assert_eq!(plan.per_layer[1].bias_add_dispatches, 0);
    }

    #[test]
    fn execution_plan_final_norm_dispatch_always_true() {
        let mut cfg = default_config();
        cfg.n_layers = 0;
        let plan = build_execution_plan(&cfg, &[], &[]);
        assert!(plan.final_norm_dispatch, "final_norm_dispatch is always true");
    }

    // ── Struct derives ──────────────────────────────────────────────────

    #[test]
    fn pipeline_config_debug() {
        let cfg = default_config();
        let dbg = format!("{:?}", cfg);
        assert!(dbg.contains("PipelineConfig"));
        assert!(dbg.contains("n_layers"));
        assert!(dbg.contains("hidden_size"));
    }

    #[test]
    fn layer_dispatch_plan_debug() {
        let plan = compute_layer_dispatch_plan(0, false, false, false, false, false, false, false, false, false, false);
        let dbg = format!("{:?}", plan);
        assert!(dbg.contains("LayerDispatchPlan"));
        assert!(dbg.contains("layer_index"));
        assert!(dbg.contains("total_dispatches"));
    }

    #[test]
    fn execution_plan_debug() {
        let cfg = default_config();
        let plan = build_execution_plan(&cfg, &[], &[]);
        let dbg = format!("{:?}", plan);
        assert!(dbg.contains("PipelineExecutionPlan"));
        assert!(dbg.contains("kv_dim"));
        assert!(dbg.contains("per_layer"));
    }

    #[test]
    fn layer_dispatch_plan_clone_independence() {
        let plan = compute_layer_dispatch_plan(7, true, true, true, true, true, true, true, true, true, true);
        let mut cloned = plan.clone();
        cloned.layer_index = 999;
        cloned.total_dispatches = 0;
        assert_eq!(plan.layer_index, 7);
        assert_ne!(plan.total_dispatches, 0);
        assert_eq!(cloned.layer_index, 999);
        assert_eq!(cloned.total_dispatches, 0);
    }

    #[test]
    fn execution_plan_clone_independence() {
        let cfg = default_config();
        let plan = build_execution_plan(&cfg, &[(true, true, true); 2], &[(true, true, true, true, true, true, true); 2]);
        let mut cloned = plan.clone();
        cloned.kv_dim = 9999;
        cloned.per_layer.clear();
        assert_ne!(plan.kv_dim, 9999);
        assert_eq!(plan.per_layer.len(), 2);
    }

    // ── Serialization: deep JSON validation ─────────────────────────────

    #[test]
    fn pipeline_config_json_all_fields() {
        let cfg = default_config();
        let json = serde_json::to_string(&cfg).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["n_layers"], 2);
        assert_eq!(val["hidden_size"], 64);
        assert_eq!(val["ffn_size"], 256);
        assert_eq!(val["n_heads"], 4);
        assert_eq!(val["n_kv_heads"], 2);
        assert_eq!(val["head_dim"], 16);
        assert_eq!(val["max_seq_len"], 512);
        assert_eq!(val["rope_theta"], 10000.0);
        assert_eq!(val["scale"], 0.125);
    }

    #[test]
    fn layer_dispatch_plan_json_all_fields() {
        let plan = compute_layer_dispatch_plan(3, true, false, true, true, true, true, true, true, true, true);
        let json = serde_json::to_string(&plan).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["layer_index"], 3);
        assert_eq!(val["buffer_copies"], 3);
        assert_eq!(val["rms_norm_dispatches"], 2);
        assert_eq!(val["gemv_dispatches"], 7);
        assert_eq!(val["bias_add_dispatches"], 2);
        assert_eq!(val["rope_dispatches"], 1);
        assert_eq!(val["kv_write_dispatches"], 1);
        assert_eq!(val["attn_softmax_dispatches"], 1);
        assert_eq!(val["residual_add_dispatches"], 2);
        assert_eq!(val["swiglu_dispatches"], 1);
        assert_eq!(val["total_dispatches"], 17);
    }

    #[test]
    fn execution_plan_json_parseable_as_value() {
        let cfg = default_config();
        let plan = build_execution_plan(&cfg, &[(false, false, false); 2], &[(true, true, true, true, true, true, true); 2]);
        let json = serde_json::to_string(&plan).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["config"].is_object());
        assert!(val["per_layer"].is_array());
        assert_eq!(val["per_layer"].as_array().unwrap().len(), 2);
        assert!(val["validation_issues"].is_array());
        assert_eq!(val["kv_dim"].as_u64().unwrap(), 32);
        assert_eq!(val["final_norm_dispatch"].as_bool().unwrap(), true);
    }

    #[test]
    fn execution_plan_json_roundtrip_values() {
        let cfg = default_config();
        let plan = build_execution_plan(&cfg, &[(true, false, true); 2], &[(true, true, false, true, true, true, true); 2]);
        let json = serde_json::to_string(&plan).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Layer 0: bias_q=true, bias_k=false, bias_v=true => 2 bias dispatches
        assert_eq!(val["per_layer"][0]["bias_add_dispatches"].as_u64().unwrap(), 2);
        // Layer 0: q=true, k=true, v=false, o=true, gate=true, up=true, down=true => 6 gemv
        assert_eq!(val["per_layer"][0]["gemv_dispatches"].as_u64().unwrap(), 6);
    }

    #[test]
    fn pipeline_config_json_deterministic() {
        let cfg = default_config();
        let json1 = serde_json::to_string(&cfg).unwrap();
        let json2 = serde_json::to_string(&cfg).unwrap();
        assert_eq!(json1, json2, "serialization should be deterministic");
    }
}
