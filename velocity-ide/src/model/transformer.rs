// model/transformer.rs — V.E.L.O.C.I.T.Y.-IDE
//
// Full autoregressive transformer forward pass for BitNet b1.58-3B.
//
// Execution model
// ───────────────
//   • NDA-GEMV  (all weight projections) : CPU, parallelised over rows via rayon
//   • RMSNorm, RoPE                      : CPU, FP32 scalar
//   • Q quantisation (post-RoPE)         : FP32 → v2 quad {-2,-1,+1,+2} 2-bit bitmaps
//   • Q·K attention dot product          : pure bitwise popcount (no FP32 in hot loop)
//   • KV-cache                           : v2 quad bitmaps — O(seq × d_kv/4) memory
//
//! # Safety Invariants
//!
//! `unsafe` blocks in this module perform GPU buffer operations:
//! - `copy_nonoverlapping`: copies between CPU slices and GPU-resident buffers
//!   (`x_residual_ptr`). Pointers come from `GpuPipeline` which allocates and owns
//!   the buffer for the lifetime of the pipeline. Copy length is `hidden_size` which
//!   is guaranteed to fit within both source and destination.
//! - `from_raw_parts_mut`: creates mutable slices from `driver.shared_input_ptr`
//!   (a Vulkan coherent buffer mapping). The pointer is valid for `hidden_size` elements
//!   because the buffer was created with that exact size. The driver outlives the slice.

use rand::Rng;
use rayon::prelude::*;
use serde::Serialize;
use std::time::Instant;

use crate::compiler::driver::VulkanNdaGemv;
use crate::model::{config::ModelConfig, weights::ModelWeights};
use crate::nda::{nda_gemv, NdaMatrix};

/// Pack a float32 vector to v2 quad sign+extra bitmaps (same as quantize_activations_v2_quad
/// but operates on a slice of `len` elements and can zero-pad to `len` if needed).
fn pack_vector_impl(v: &[f32], scale: f32, len: usize) -> (Vec<u8>, Vec<u8>) {
    let mut sign_buf = vec![0u8; len.div_ceil(8)];
    let mut extra_buf = vec![0u8; len.div_ceil(8)];

    let actual_scale = if scale < 1e-8 { 1.0 } else { scale };
    let inv_scale = 1.0 / actual_scale;

    for (i, &val) in v.iter().enumerate() {
        if i >= len {
            break;
        }
        let val_scaled = val * inv_scale;
        let is_large = val_scaled.abs() >= 1.5;
        let is_pos = val >= 0.0;

        let sign_bit = if is_pos { 1u8 } else { 0 };
        let large_bit = if is_large { 1u8 } else { 0 };
        // XNOR(sign, large) = extra
        let extra_bit = !(sign_bit ^ large_bit) & 1;

        let byte_idx = i / 8;
        let bit_idx = i % 8;

        if sign_bit == 1 {
            sign_buf[byte_idx] |= 1 << bit_idx;
        }
        if extra_bit == 1 {
            extra_buf[byte_idx] |= 1 << bit_idx;
        }
    }
    (sign_buf, extra_buf)
}

fn pack_vector_padded(v: &[f32], scale: f32, padded_len: usize) -> (Vec<u8>, Vec<u8>) {
    pack_vector_impl(v, scale, padded_len)
}

fn nda_gemv_gpu_or_cpu(
    gpu_gemv: &Option<VulkanNdaGemv>,
    cpu_gemv: &NdaMatrix,
    x: &[f32],
    out: &mut [f32],
) {
    if let Some(ref gpu) = gpu_gemv {
        if gpu.version == crate::nda::NDA_VERSION_FP4 as u32
            || gpu.version == crate::nda::NDA_VERSION_FP2 as u32
        {
            if gpu.run_float(x, out).is_ok() {
                let s = cpu_gemv.scale;
                out.iter_mut().for_each(|val| *val *= s);
                return;
            }
        } else {
            // 1. Calculate input scale
            let scale = x.iter().map(|&val| val.abs()).fold(0.0_f32, f32::max);

            // 2. Pad column count (only legacy quad format needs padding to 128-element boundaries)
            let cols_padded = if cpu_gemv.version == crate::nda::NDA_V2_QUAD {
                let num_col_words_padded = (cpu_gemv.cols / 32).div_ceil(4) * 4;
                num_col_words_padded * 32
            } else {
                cpu_gemv.cols
            };

            // 3. Pack vector to active/pos bitmaps (padded to cols_padded)
            let (active, pos) = pack_vector_padded(x, scale, cols_padded);

            // 4. Run on GPU
            if gpu.run(&active, &pos, out).is_ok() {
                // 5. Scale output by activation scale * weight scale
                let s = scale * cpu_gemv.scale;
                out.iter_mut().for_each(|val| *val *= s);
                return;
            }
        }
    }
    // Fallback to CPU
    let temp = nda_gemv(cpu_gemv, x);
    out.copy_from_slice(&temp);
}

#[allow(clippy::too_many_arguments)]
fn nda_gemv_gpu_or_cpu_batch_3(
    w0_gpu: &Option<VulkanNdaGemv>,
    w0_cpu: &NdaMatrix,
    w1_gpu: &Option<VulkanNdaGemv>,
    w1_cpu: &NdaMatrix,
    w2_gpu: &Option<VulkanNdaGemv>,
    w2_cpu: &NdaMatrix,
    x: &[f32],
    out0: &mut [f32],
    out1: &mut [f32],
    out2: &mut [f32],
) {
    if let (Some(g0), Some(g1), Some(g2)) = (w0_gpu, w1_gpu, w2_gpu) {
        if (g0.version == crate::nda::NDA_VERSION_FP4 as u32
            || g0.version == crate::nda::NDA_VERSION_FP2 as u32)
            && (g1.version == crate::nda::NDA_VERSION_FP4 as u32
                || g1.version == crate::nda::NDA_VERSION_FP2 as u32)
            && (g2.version == crate::nda::NDA_VERSION_FP4 as u32
                || g2.version == crate::nda::NDA_VERSION_FP2 as u32)
        {
            // Note: Since the input has already been written directly to the driver's
            // shared input buffer (via rms_norm_to), we dispatch with NO CPU copy!
            let _ = g0.submit_async_float_no_copy();
            let _ = g1.submit_async_float_no_copy();
            let _ = g2.submit_async_float_no_copy();

            let _ = g0.wait_and_copy_float(out0);
            let _ = g1.wait_and_copy_float(out1);
            let _ = g2.wait_and_copy_float(out2);

            out0.iter_mut().for_each(|val| *val *= w0_cpu.scale);
            out1.iter_mut().for_each(|val| *val *= w1_cpu.scale);
            out2.iter_mut().for_each(|val| *val *= w2_cpu.scale);
            return;
        }
    }

    // Fallback: sequential
    nda_gemv_gpu_or_cpu(w0_gpu, w0_cpu, x, out0);
    nda_gemv_gpu_or_cpu(w1_gpu, w1_cpu, x, out1);
    nda_gemv_gpu_or_cpu(w2_gpu, w2_cpu, x, out2);
}

fn nda_gemv_gpu_or_cpu_batch_2(
    w0_gpu: &Option<VulkanNdaGemv>,
    w0_cpu: &NdaMatrix,
    w1_gpu: &Option<VulkanNdaGemv>,
    w1_cpu: &NdaMatrix,
    x: &[f32],
    out0: &mut [f32],
    out1: &mut [f32],
) {
    if let (Some(g0), Some(g1)) = (w0_gpu, w1_gpu) {
        if (g0.version == crate::nda::NDA_VERSION_FP4 as u32
            || g0.version == crate::nda::NDA_VERSION_FP2 as u32)
            && (g1.version == crate::nda::NDA_VERSION_FP4 as u32
                || g1.version == crate::nda::NDA_VERSION_FP2 as u32)
        {
            // Note: Since the input has already been written directly to the driver's
            // shared input buffer (via rms_norm_to), we dispatch with NO CPU copy!
            let _ = g0.submit_async_float_no_copy();
            let _ = g1.submit_async_float_no_copy();

            let _ = g0.wait_and_copy_float(out0);
            let _ = g1.wait_and_copy_float(out1);

            out0.iter_mut().for_each(|val| *val *= w0_cpu.scale);
            out1.iter_mut().for_each(|val| *val *= w1_cpu.scale);
            return;
        }
    }

    nda_gemv_gpu_or_cpu(w0_gpu, w0_cpu, x, out0);
    nda_gemv_gpu_or_cpu(w1_gpu, w1_cpu, x, out1);
}

// ─── KV cache ──────────────────────────────────────────────────────────────

use sha2::{Digest, Sha256};

pub struct NdaKvBlock {
    pub prev_hash: [u8; 32],
    pub hash: [u8; 32], // Precomputed SHA-256 hash of this block
    pub k_scale: f32,
    pub v_scale: f32,
    /// v2 sign bitmap: bit=1 → K element is positive
    pub k_sign: Vec<u8>,
    /// v2 extra bitmap: magnitude via XNOR(sign,extra) → {-2,-1,+1,+2}
    pub k_extra: Vec<u8>,
    /// v2 sign bitmap: bit=1 → V element is positive
    pub v_sign: Vec<u8>,
    /// v2 extra bitmap: magnitude via XNOR(sign,extra) → {-2,-1,+1,+2}
    pub v_extra: Vec<u8>,
    pub k_raw: Vec<f32>,
    pub v_raw: Vec<f32>,
}

impl NdaKvBlock {
    pub fn compute_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.prev_hash);
        hasher.update(self.k_scale.to_le_bytes());
        hasher.update(self.v_scale.to_le_bytes());
        hasher.update(&self.k_sign);
        hasher.update(&self.k_extra);
        hasher.update(&self.v_sign);
        hasher.update(&self.v_extra);
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }
}

pub struct KvLayer {
    pub blocks: Vec<NdaKvBlock>,
}

fn pack_vector(v: &[f32], scale: f32) -> (Vec<u8>, Vec<u8>) {
    pack_vector_impl(v, scale, v.len())
}

impl KvLayer {
    fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    fn push(&mut self, k: &[f32], v: &[f32]) {
        let k_scale = k.iter().map(|&x| x.abs()).fold(0.0_f32, f32::max);
        let v_scale = v.iter().map(|&x| x.abs()).fold(0.0_f32, f32::max);

        // pack_vector uses v2 quad encoding → produces (sign_bitmap, extra_bitmap)
        let (k_sign, k_extra) = pack_vector(k, k_scale);
        let (v_sign, v_extra) = pack_vector(v, v_scale);

        let prev_hash = if let Some(last_block) = self.blocks.last() {
            last_block.hash
        } else {
            [0u8; 32]
        };

        let mut block = NdaKvBlock {
            prev_hash,
            hash: [0u8; 32],
            k_scale,
            v_scale,
            k_sign,
            k_extra,
            v_sign,
            v_extra,
            k_raw: k.to_vec(),
            v_raw: v.to_vec(),
        };
        block.hash = block.compute_hash();
        self.blocks.push(block);
    }
}

// ─── Math primitives ───────────────────────────────────────────────────────

/// In-place RMSNorm: x ← x / rms(x) * weight
fn rms_norm(x: &mut [f32], weight: &[f32], eps: f32) {
    let mean_sq = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
    let inv_rms = (mean_sq + eps).sqrt().recip();
    x.iter_mut()
        .zip(weight.iter())
        .for_each(|(xi, &wi)| *xi *= inv_rms * wi);
}

/// Out-of-place RMSNorm: out ← x / rms(x) * weight
fn rms_norm_to(x: &[f32], out: &mut [f32], weight: &[f32], eps: f32) {
    let mean_sq = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
    let inv_rms = (mean_sq + eps).sqrt().recip();
    out.iter_mut()
        .zip(x.iter())
        .zip(weight.iter())
        .for_each(|((outi, &xi), &wi)| *outi = xi * inv_rms * wi);
}

/// Apply rotary positional embedding to a single head's Q or K slice (in-place).
///
/// Uses the "interleaved" RoPE convention: pairs [i, i+half_dim].
fn apply_rope_head(head: &mut [f32], pos: usize, head_dim: usize, theta: f32) {
    let half = head_dim / 2;
    for i in 0..half {
        let freq = theta.powf(-2.0 * i as f32 / head_dim as f32);
        let angle = pos as f32 * freq;
        let (s, c) = angle.sin_cos();
        let x0 = head[i];
        let x1 = head[i + half];
        head[i] = x0 * c - x1 * s;
        head[i + half] = x0 * s + x1 * c;
    }
}

/// SiLU activation: x * σ(x)
#[inline]
fn silu(x: f32) -> f32 {
    x / (1.0 + (-x).exp())
}

/// Bitwise-popcount Q·K attention for one head (causal).
///
/// Q is supplied as pre-quantised v2 quad bitmaps (q_sign, q_extra, q_scale).
/// K is retrieved from the NDA-KV cache (also v2 quad bitmaps).
/// The dot product is computed entirely with integer popcount — zero FP32 in the hot loop.
///
/// Returns the attention output vector of length `head_dim`.
#[allow(dead_code)]
#[allow(clippy::needless_range_loop)]
fn attention_head(
    q_sign: &[u8],
    q_extra: &[u8],
    q_scale: f32,
    kv_layer: &KvLayer,
    h_start: usize,
    h_end: usize,
    scale: f32,
) -> Vec<f32> {
    let head_dim = h_end - h_start;
    let head_bytes = head_dim.div_ceil(8);
    // Byte offset into the full-width KV bitmaps where this head starts
    let head_byte_start = h_start / 8;
    // Precompute q_scale × attn_scale (constant for entire head)
    // → only one multiplication remains per KV block (block.k_scale)
    let qk_scale = q_scale * scale;

    // Attention scores: e_t = (q · k_t) * q_scale * k_scale * attn_scale
    let mut scores: Vec<f32> = kv_layer
        .blocks
        .iter()
        .enumerate()
        .map(|(t, block)| {
            // O(1) Merkle hash chain verification (cached hashes)
            if t > 0 {
                let prev_hash = kv_layer.blocks[t - 1].hash;
                if block.prev_hash != prev_hash {
                    log::error!("Security Fault: Hash chain broken at block {}!", t);
                    return 0.0; // safe default: zero attention score
                }
            }

            // Pure bitwise Q·K dot product (matches nda_gemv_v2_quad_quantized exactly)
            let mut acc = 0_i32;
            for b in 0..head_bytes {
                let qs = q_sign[b];
                let qe = q_extra[b];
                let ks = block.k_sign[head_byte_start + b];
                let ke = block.k_extra[head_byte_start + b];

                // same_sign  = XNOR(q_sign, k_sign) = !(qs ^ ks)
                let same_sign = !(qs ^ ks);
                let diff_sign = qs ^ ks;

                // large = XNOR(sign, extra)  for each operand
                let q_large = !(qs ^ qe);
                let k_large = !(ks ^ ke);

                // Positive contributions (same sign): base + magnitude extras
                let pos = same_sign.count_ones()
                    + (same_sign & q_large).count_ones()
                    + (same_sign & k_large).count_ones()
                    + (same_sign & q_large & k_large).count_ones();

                // Negative contributions (different sign)
                let neg = diff_sign.count_ones()
                    + (diff_sign & q_large).count_ones()
                    + (diff_sign & k_large).count_ones()
                    + (diff_sign & q_large & k_large).count_ones();

                acc += pos as i32 - neg as i32;
            }
            // Dequantize: one multiplication (k_scale only; q_scale×attn_scale precomputed)
            acc as f32 * block.k_scale * qk_scale
        })
        .collect();

    // Numerically-stable softmax
    let max_s = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    scores.iter_mut().for_each(|s| *s = (*s - max_s).exp());
    let sum_s: f32 = scores.iter().sum();
    let inv_sum = (sum_s + 1e-10).recip();
    scores.iter_mut().for_each(|s| *s *= inv_sum);

    // Weighted sum of values: pure-additive v2 quad decode
    let mut out = vec![0.0_f32; head_dim];
    for (&score, block) in scores.iter().zip(kv_layer.blocks.iter()) {
        let s = score * block.v_scale;
        if s == 0.0 {
            continue;
        }
        for i in 0..head_dim {
            let global_idx = h_start + i;
            let byte_idx = global_idx / 8;
            let bit_idx = global_idx % 8;
            let mask = 1 << bit_idx;
            let sign = (block.v_sign[byte_idx] & mask) != 0;
            let extra = (block.v_extra[byte_idx] & mask) != 0;
            // v2 XNOR decode: pure-additive (no multiplication)
            let val = if sign { s } else { -s };
            out[i] += val; // always add once
            if sign == extra {
                out[i] += val;
            } // large → add again
        }
    }
    out
}

fn attention_head_float(
    q: &[f32],
    kv_layer: &KvLayer,
    h_start: usize,
    h_end: usize,
    scale: f32,
    out: &mut [f32],
) {
    let head_dim = h_end - h_start;
    let mut scores: Vec<f32> = kv_layer
        .blocks
        .iter()
        .map(|block| {
            let k_slice = &block.k_raw[h_start..h_end];
            let dot = q
                .iter()
                .zip(k_slice.iter())
                .map(|(&qi, &ki)| qi * ki)
                .sum::<f32>();
            dot * scale
        })
        .collect();

    let max_s = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    scores.iter_mut().for_each(|s| *s = (*s - max_s).exp());
    let sum_s: f32 = scores.iter().sum();
    let inv_sum = (sum_s + 1e-10).recip();
    scores.iter_mut().for_each(|s| *s *= inv_sum);

    out.fill(0.0);
    for (&score, block) in scores.iter().zip(kv_layer.blocks.iter()) {
        let v_slice = &block.v_raw[h_start..h_end];
        for i in 0..head_dim {
            out[i] += score * v_slice[i];
        }
    }
}

/// LM-head FP32 matmul: logits[v] = weight_row[v] · hidden (parallel over vocab).
/// Writes results in-place into `out_logits` which must have length `vocab_size`.
fn lm_head(
    hidden: &[f32],
    weights: &[f32],
    _vocab_size: usize,
    hidden_size: usize,
    out_logits: &mut [f32],
) {
    out_logits
        .par_iter_mut()
        .enumerate()
        .for_each(|(v, logit)| {
            let offset = v * hidden_size;
            let mut sum = 0.0f32;
            unsafe {
                let w_ptr = weights.as_ptr().add(offset);
                let h_ptr = hidden.as_ptr();
                for i in 0..hidden_size {
                    sum += (*w_ptr.add(i)) * (*h_ptr.add(i));
                }
            }
            *logit = sum;
        });
}

/// Sample the next token from `logits` given temperature and top-p.
fn sample_token(logits: &[f32], temperature: f32, top_p: f32, rng: &mut impl Rng) -> u32 {
    // Greedy at temperature ≤ 0 (or very small)
    if temperature < 1e-6 {
        return logits
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i as u32)
            .unwrap_or(0);
    }

    // Temperature-scaled softmax
    let max_l = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let mut probs: Vec<f32> = logits
        .iter()
        .map(|&l| ((l - max_l) / temperature).exp())
        .collect();
    let sum_p: f32 = probs.iter().sum();
    probs.iter_mut().for_each(|p| *p /= sum_p);

    // Top-p nucleus: sort descending, cumulate, truncate
    let mut indexed: Vec<(usize, f32)> = probs.into_iter().enumerate().collect();
    indexed
        .sort_unstable_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    let mut cumulative = 0.0_f32;
    let mut cutoff = indexed.len();
    for (i, (_, p)) in indexed.iter().enumerate() {
        cumulative += p;
        if cumulative >= top_p {
            cutoff = i + 1;
            break;
        }
    }
    indexed.truncate(cutoff);

    // Renormalise nucleus
    let nucleus_sum: f32 = indexed.iter().map(|(_, p)| p).sum();
    let inv = nucleus_sum.recip();

    // Weighted random draw
    let target: f32 = rng.gen::<f32>() * nucleus_sum;
    let mut acc = 0.0_f32;
    for (idx, p) in &indexed {
        acc += p * inv * nucleus_sum; // = p
        if acc >= target {
            return *idx as u32;
        }
    }
    // Fallback to most probable token (numerical edge case)
    indexed[0].0 as u32
}

/// Sample the next token using top-k filtering before top-p.
#[cfg(test)]
fn sample_token_top_k(
    logits: &[f32],
    temperature: f32,
    top_p: f32,
    top_k: usize,
    rng: &mut impl Rng,
) -> u32 {
    if top_k == 0 || top_k >= logits.len() {
        return sample_token(logits, temperature, top_p, rng);
    }
    // Keep only top-k logits
    let mut indexed: Vec<(usize, f32)> = logits.iter().copied().enumerate().collect();
    indexed
        .sort_unstable_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    indexed.truncate(top_k);
    // Rebuild filtered logits
    let max_l = indexed
        .iter()
        .map(|(_, l)| *l)
        .fold(f32::NEG_INFINITY, f32::max);
    let mut probs: Vec<f32> = indexed
        .iter()
        .map(|(_, l)| ((*l - max_l) / temperature).exp())
        .collect();
    let sum_p: f32 = probs.iter().sum();
    probs.iter_mut().for_each(|p| *p /= sum_p);
    // Top-p on filtered
    let mut cum = 0.0_f32;
    let mut cutoff = probs.len();
    for (i, &p) in probs.iter().enumerate() {
        cum += p;
        if cum >= top_p {
            cutoff = i + 1;
            break;
        }
    }
    probs.truncate(cutoff);
    let nucleus_sum: f32 = probs.iter().sum();
    let inv = nucleus_sum.recip();
    let target: f32 = rng.gen::<f32>() * nucleus_sum;
    let mut acc = 0.0_f32;
    for (idx, p) in indexed[..cutoff].iter().zip(probs.iter()) {
        acc += p * inv * nucleus_sum;
        if acc >= target {
            return idx.0 as u32;
        }
    }
    indexed[0].0 as u32
}

/// Apply frequency penalty to logits based on token occurrence counts.
#[cfg(test)]
fn apply_frequency_penalty(
    logits: &mut [f32],
    token_counts: &std::collections::HashMap<u32, usize>,
    penalty: f32,
) {
    for (&tok, &count) in token_counts.iter() {
        let idx = tok as usize;
        if idx < logits.len() {
            logits[idx] -= penalty * count as f32;
        }
    }
}

/// Apply presence penalty to logits based on token occurrence.
#[cfg(test)]
fn apply_presence_penalty(
    logits: &mut [f32],
    token_counts: &std::collections::HashMap<u32, usize>,
    penalty: f32,
) {
    for (&tok, _) in token_counts.iter() {
        let idx = tok as usize;
        if idx < logits.len() {
            logits[idx] -= penalty;
        }
    }
}

// ─── FP32 Transformer Metrics & Reports ────────────────────────────────────

/// Metrics from a single FP32 forward pass.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Fp32ForwardMetrics {
    /// Position index of this forward pass.
    pub position: usize,
    /// Whether the GPU pipeline was used (true) or CPU fallback (false).
    pub gpu_active: bool,
    /// Number of transformer layers executed.
    pub layers_executed: usize,
    /// Total KV cache blocks across all layers after this forward.
    pub total_kv_blocks: usize,
    /// Elapsed time for this forward pass (microseconds).
    pub elapsed_us: u64,
}

/// Structured report from an FP32 autoregressive generation run.
#[derive(Debug, Clone, Serialize)]
pub struct Fp32GenerationReport {
    /// Number of tokens in the prompt.
    pub prompt_tokens: usize,
    /// Number of new tokens generated (excluding prompt).
    pub tokens_generated: usize,
    /// Whether generation stopped due to EOS token.
    pub stopped_at_eos: bool,
    /// Whether generation was truncated at max_seq_len.
    pub truncated: bool,
    /// Total KV cache blocks at end of generation (sum across layers).
    pub final_kv_blocks: usize,
    /// Per-layer KV cache sizes (blocks per layer).
    pub kv_cache_sizes: Vec<usize>,
    /// Estimated KV cache memory footprint in bytes.
    pub kv_memory_bytes: usize,
    /// Whether the GPU pipeline was active.
    pub gpu_pipeline_active: bool,
    /// Total elapsed time for generation (microseconds).
    pub elapsed_us: u64,
    /// Throughput in tokens per second.
    pub tokens_per_second: f64,
    /// All generated token IDs (excluding prompt).
    pub token_ids: Vec<u32>,
    /// Per-forward metrics (prefill + decode steps).
    pub forward_metrics: Vec<Fp32ForwardMetrics>,
}

// ─── Transformer ───────────────────────────────────────────────────────────

pub struct TransformerScratch {
    pub x: Vec<f32>,
    pub x_norm: Vec<f32>,
    pub q: Vec<f32>,
    pub k: Vec<f32>,
    pub v: Vec<f32>,
    pub attn_out: Vec<f32>,
    pub gate_out: Vec<f32>,
    pub up_out: Vec<f32>,
    pub gated: Vec<f32>,
    pub logits: Vec<f32>,
}

impl TransformerScratch {
    pub fn new(cfg: &ModelConfig) -> Self {
        let h = cfg.hidden_size;
        let q_size = cfg.n_heads * cfg.head_dim;
        let kv_size = cfg.n_kv_heads * cfg.head_dim;
        let mlp_size = cfg.ffn_size;
        Self {
            x: vec![0.0; h],
            x_norm: vec![0.0; h],
            q: vec![0.0; q_size],
            k: vec![0.0; kv_size],
            v: vec![0.0; kv_size],
            attn_out: vec![0.0; h],
            gate_out: vec![0.0; mlp_size],
            up_out: vec![0.0; mlp_size],
            gated: vec![0.0; mlp_size],
            logits: vec![0.0; cfg.vocab_size],
        }
    }
}

pub struct Transformer {
    config: ModelConfig,
    weights: ModelWeights,
    kv_cache: Vec<KvLayer>,
    scratch: TransformerScratch,
    gpu_pipeline: Option<crate::compiler::driver::VulkanModelPipeline>,
}

impl Transformer {
    pub fn new(config: ModelConfig, weights: ModelWeights) -> Self {
        let kv_cache = (0..config.n_layers).map(|_| KvLayer::new()).collect();
        let scratch = TransformerScratch::new(&config);

        // Enable the fused Vulkan pipeline only when a GPU driver initialised
        // and every layer has its projection weights uploaded to GPU buffers.
        // Any missing piece (no driver, CPU-only weights) falls back to the
        // CPU path in `forward_one`.
        // The fused GPU pipeline records a full transformer forward pass as a single
        // Vulkan command buffer. For FP4/FP2 weights, the pipeline shaders don't
        // account for the per-matrix global_scale (applied externally in the
        // individual GEMV path via nda_gemv_gpu_or_cpu), so we skip the fused
        // pipeline and use individual GPU GEMV dispatches instead.
        //
        // TODO(opt): Fix the fused pipeline for FP4/FP2 by either:
        //   (a) Adding global_scale as a push constant to the GEMV shader, or
        //   (b) Recording a scale-multiply compute pass after each FP4 GEMV dispatch.
        // This would enable GPU-side attention for FP4 models (currently CPU-bound).
        let has_fp_weights = !weights.layers.is_empty()
            && weights.layers.iter().any(|l| {
                l.q_proj.version == crate::nda::NDA_VERSION_FP4
                    || l.q_proj.version == crate::nda::NDA_VERSION_FP2
            });
        let gpu_pipeline = if has_fp_weights {
            eprintln!("[transformer] FP4/FP2 weights detected — using individual GPU GEMVs (CPU attention)");
            None
        } else {
            Self::try_build_gpu_pipeline(&config, &weights)
        };

        Self {
            config,
            weights,
            kv_cache,
            scratch,
            gpu_pipeline,
        }
    }

    /// Attempt to construct the fused GPU pipeline from the loaded weights.
    /// Returns `None` (CPU fallback) when the GPU driver is absent, any layer
    /// lacks uploaded GPU buffers, or pipeline creation fails.
    fn try_build_gpu_pipeline(
        config: &ModelConfig,
        weights: &ModelWeights,
    ) -> Option<crate::compiler::driver::VulkanModelPipeline> {
        let driver = weights.vulkan.as_ref()?;

        let all_gpu_ready = !weights.layers.is_empty()
            && weights.layers.iter().all(|l| {
                l.q_proj_gpu.is_some()
                    && l.k_proj_gpu.is_some()
                    && l.v_proj_gpu.is_some()
                    && l.o_proj_gpu.is_some()
                    && l.gate_proj_gpu.is_some()
                    && l.up_proj_gpu.is_some()
                    && l.down_proj_gpu.is_some()
            });
        if !all_gpu_ready {
            return None;
        }

        let attn_norms: Vec<&[f32]> = weights
            .layers
            .iter()
            .map(|l| l.attn_norm.as_slice())
            .collect();
        let ffn_norms: Vec<&[f32]> = weights
            .layers
            .iter()
            .map(|l| l.ffn_norm.as_slice())
            .collect();
        let q_biases: Vec<Option<&[f32]>> = weights
            .layers
            .iter()
            .map(|l| l.q_proj_bias.as_deref())
            .collect();
        let k_biases: Vec<Option<&[f32]>> = weights
            .layers
            .iter()
            .map(|l| l.k_proj_bias.as_deref())
            .collect();
        let v_biases: Vec<Option<&[f32]>> = weights
            .layers
            .iter()
            .map(|l| l.v_proj_bias.as_deref())
            .collect();

        let layers_gpu: Vec<crate::compiler::driver::LayerGpuGemvs> = weights
            .layers
            .iter()
            .map(|l| crate::compiler::driver::LayerGpuGemvs {
                qkv_proj_gpu: &l.qkv_proj_gpu,
                gate_up_proj_gpu: &l.gate_up_proj_gpu,
                q_proj_gpu: &l.q_proj_gpu,
                k_proj_gpu: &l.k_proj_gpu,
                v_proj_gpu: &l.v_proj_gpu,
                o_proj_gpu: &l.o_proj_gpu,
                gate_proj_gpu: &l.gate_proj_gpu,
                up_proj_gpu: &l.up_proj_gpu,
                down_proj_gpu: &l.down_proj_gpu,
            })
            .collect();
        let layers_gpu_refs: Vec<&crate::compiler::driver::LayerGpuGemvs> =
            layers_gpu.iter().collect();

        match crate::compiler::driver::VulkanModelPipeline::new(
            driver,
            config.n_layers,
            config.hidden_size,
            config.ffn_size,
            config.n_heads,
            config.n_kv_heads,
            config.head_dim,
            config.max_seq_len,
            &attn_norms,
            &ffn_norms,
            &q_biases,
            &k_biases,
            &v_biases,
            &weights.final_norm,
            &layers_gpu_refs,
        ) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!(
                    "[transformer] GPU pipeline init failed, using CPU path: {}",
                    e
                );
                None
            }
        }
    }

    /// Get a reference to the model configuration.
    pub fn config(&self) -> &ModelConfig {
        &self.config
    }

    /// Whether the GPU (Vulkan) pipeline is active.
    pub fn gpu_pipeline_active(&self) -> bool {
        self.gpu_pipeline.is_some()
    }

    /// Get per-layer KV cache sizes (number of blocks per layer).
    pub fn kv_cache_sizes(&self) -> Vec<usize> {
        self.kv_cache.iter().map(|layer| layer.blocks.len()).collect()
    }

    /// Total KV cache blocks across all layers.
    pub fn total_kv_blocks(&self) -> usize {
        self.kv_cache.iter().map(|layer| layer.blocks.len()).sum()
    }

    /// Estimate KV cache memory footprint in bytes.
    /// Each block stores: k_raw + v_raw (f32 vectors) + k_sign + k_extra + v_sign + v_extra (bitmap vectors).
    pub fn kv_memory_bytes(&self) -> usize {
        let kv_dim = self.config.n_kv_heads * self.config.head_dim;
        let raw_bytes = kv_dim * std::mem::size_of::<f32>(); // per raw vector
        let bitmap_bytes = kv_dim.div_ceil(8); // per bitmap vector
        let per_block = 2 * raw_bytes + 4 * bitmap_bytes; // k_raw + v_raw + 4 bitmaps
        self.total_kv_blocks() * per_block
    }

    /// Human-readable layer summary for diagnostics.
    pub fn layer_summary(&self) -> String {
        let cfg = &self.config;
        format!(
            "Transformer: {} layers, hidden={}, heads={}×{}, kv_heads={}, ffn={}, gpu={}",
            cfg.n_layers,
            cfg.hidden_size,
            cfg.n_heads,
            cfg.head_dim,
            cfg.n_kv_heads,
            cfg.ffn_size,
            if self.gpu_pipeline_active() { "Vulkan" } else { "CPU" },
        )
    }

    /// Reset KV cache (call between independent prompts).
    pub fn reset_cache(&mut self) {
        for layer in &mut self.kv_cache {
            layer.blocks.clear();
        }
    }

    /// Process one token at position `pos` and return a reference to the logit vector.
    fn forward_one(&mut self, token: u32, pos: usize) -> &[f32] {
        let cfg = &self.config;
        let h = cfg.hidden_size;

        if let Some(ref pipeline) = self.gpu_pipeline {
            // 1. Copy initial token embeddings to x_residual_buffer on GPU
            let embed_src =
                &self.weights.embed_tokens[token as usize * h..(token as usize + 1) * h];
            // SAFETY: `embed_src` has exactly `h` elements. `pipeline.x_residual_ptr`
            // points to a GPU buffer of at least `h` f32 elements. Non-overlapping.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    embed_src.as_ptr(),
                    pipeline.x_residual_ptr as *mut f32,
                    h,
                );
            }

            // 2. Build LayerGpuGemvs list
            let layers_gpu: Vec<crate::compiler::driver::LayerGpuGemvs> = self
                .weights
                .layers
                .iter()
                .map(|l| crate::compiler::driver::LayerGpuGemvs {
                    qkv_proj_gpu: &l.qkv_proj_gpu,
                    gate_up_proj_gpu: &l.gate_up_proj_gpu,
                    q_proj_gpu: &l.q_proj_gpu,
                    k_proj_gpu: &l.k_proj_gpu,
                    v_proj_gpu: &l.v_proj_gpu,
                    o_proj_gpu: &l.o_proj_gpu,
                    gate_proj_gpu: &l.gate_proj_gpu,
                    up_proj_gpu: &l.up_proj_gpu,
                    down_proj_gpu: &l.down_proj_gpu,
                })
                .collect();
            let layers_gpu_refs: Vec<&crate::compiler::driver::LayerGpuGemvs> =
                layers_gpu.iter().collect();

            // 3. Run the GPU execution pipeline
            let attn_scale = (cfg.head_dim as f32).sqrt().recip();
            pipeline
                .record_and_execute_token(
                    self.weights
                        .vulkan
                        .as_ref()
                        .expect("gpu_pipeline requires Vulkan driver (invariant: try_build_gpu_pipeline returns None if vulkan is absent)"),
                    cfg.n_layers,
                    h,
                    cfg.ffn_size,
                    cfg.n_heads,
                    cfg.n_kv_heads,
                    cfg.head_dim,
                    cfg.max_seq_len,
                    cfg.rope_theta,
                    attn_scale,
                    pos as u32,
                    &layers_gpu_refs,
                )
                .expect("Vulkan pipeline execution failed");

            // 4. Read back the final hidden state from x_residual_buffer to CPU self.scratch.x
            // SAFETY: `pipeline.x_residual_ptr` and `self.scratch.x.as_mut_ptr()` are
            // valid for `h` f32 elements. Non-overlapping (GPU buffer vs CPU Vec).
            unsafe {
                std::ptr::copy_nonoverlapping(
                    pipeline.x_residual_ptr as *const f32,
                    self.scratch.x.as_mut_ptr(),
                    h,
                );
            }

            // 5. Evaluate the LM Head on the CPU (in-place into pre-allocated scratch)
            lm_head(
                &self.scratch.x,
                &self.weights.lm_head,
                cfg.vocab_size,
                h,
                &mut self.scratch.logits,
            );
            return &self.scratch.logits;
        }

        // 1. Copy embed tokens to scratch.x in-place (no allocation!)
        self.scratch.x.copy_from_slice(
            &self.weights.embed_tokens[token as usize * h..(token as usize + 1) * h],
        );

        // ── 26 transformer layers ───────────────────────────────────────────
        for layer_idx in 0..cfg.n_layers {
            let lw = &self.weights.layers[layer_idx];

            // 1. Attention pre-norm (zero-copy straight to mapped GPU buffer if Vulkan is used!)
            let x_input: &[f32] = if let Some(ref driver) = self.weights.vulkan {
                // SAFETY: `driver.shared_input_ptr` is a valid coherent buffer mapping
                // for `h` f32 elements. The driver outlives this mutable borrow.
                unsafe {
                    let dest_slice =
                        std::slice::from_raw_parts_mut(driver.shared_input_ptr as *mut f32, h);
                    rms_norm_to(&self.scratch.x, dest_slice, &lw.attn_norm, cfg.rms_eps);
                    dest_slice
                }
            } else {
                rms_norm_to(
                    &self.scratch.x,
                    &mut self.scratch.x_norm,
                    &lw.attn_norm,
                    cfg.rms_eps,
                );
                &self.scratch.x_norm[..]
            };

            // 2. QKV projections (in batch asynchronously writing directly to scratch)
            nda_gemv_gpu_or_cpu_batch_3(
                &lw.q_proj_gpu,
                &lw.q_proj,
                &lw.k_proj_gpu,
                &lw.k_proj,
                &lw.v_proj_gpu,
                &lw.v_proj,
                x_input,
                &mut self.scratch.q,
                &mut self.scratch.k,
                &mut self.scratch.v,
            );

            if let Some(ref qb) = lw.q_proj_bias {
                self.scratch
                    .q
                    .iter_mut()
                    .zip(qb.iter())
                    .for_each(|(qi, &bi)| *qi += bi);
            }
            if let Some(ref kb) = lw.k_proj_bias {
                self.scratch
                    .k
                    .iter_mut()
                    .zip(kb.iter())
                    .for_each(|(ki, &bi)| *ki += bi);
            }
            if let Some(ref vb) = lw.v_proj_bias {
                self.scratch
                    .v
                    .iter_mut()
                    .zip(vb.iter())
                    .for_each(|(vi, &bi)| *vi += bi);
            }

            // 3. Per-head RoPE on Q and K
            let hd = cfg.head_dim;
            for head in 0..cfg.n_heads {
                let s = head * hd;
                let e = s + hd;
                apply_rope_head(&mut self.scratch.q[s..e], pos, hd, cfg.rope_theta);
            }
            for kv_head in 0..cfg.n_kv_heads {
                let s = kv_head * hd;
                let e = s + hd;
                apply_rope_head(&mut self.scratch.k[s..e], pos, hd, cfg.rope_theta);
            }

            // 4. Append K, V to cache
            self.kv_cache[layer_idx].push(&self.scratch.k, &self.scratch.v);

            // 5. Multi-head causal self-attention
            let attn_scale = (hd as f32).sqrt().recip();
            let kv_layer = &self.kv_cache[layer_idx];
            let heads_per_kv = cfg.n_heads / cfg.n_kv_heads;
            for head in 0..cfg.n_heads {
                let hs = head * hd;
                let he = hs + hd;
                let kv_head_idx = head / heads_per_kv;
                let hs_kv = kv_head_idx * hd;
                let he_kv = hs_kv + hd;

                attention_head_float(
                    &self.scratch.q[hs..he],
                    kv_layer,
                    hs_kv,
                    he_kv,
                    attn_scale,
                    &mut self.scratch.attn_out[hs..he],
                );
            }

            // 6. Output projection + residual
            // We use self.scratch.q temporarily to hold the output projection result
            nda_gemv_gpu_or_cpu(
                &lw.o_proj_gpu,
                &lw.o_proj,
                &self.scratch.attn_out,
                &mut self.scratch.q,
            );
            self.scratch
                .x
                .iter_mut()
                .zip(self.scratch.q.iter())
                .for_each(|(xi, &oi)| *xi += oi);

            // 7. FFN pre-norm (zero-copy straight to mapped GPU buffer if Vulkan is used!)
            let x_ffn_input: &[f32] = if let Some(ref driver) = self.weights.vulkan {
                // SAFETY: `driver.shared_input_ptr` is a valid coherent buffer mapping
                // for `h` f32 elements. The driver outlives this mutable borrow.
                unsafe {
                    let dest_slice =
                        std::slice::from_raw_parts_mut(driver.shared_input_ptr as *mut f32, h);
                    rms_norm_to(&self.scratch.x, dest_slice, &lw.ffn_norm, cfg.rms_eps);
                    dest_slice
                }
            } else {
                rms_norm_to(
                    &self.scratch.x,
                    &mut self.scratch.x_norm,
                    &lw.ffn_norm,
                    cfg.rms_eps,
                );
                &self.scratch.x_norm[..]
            };

            // 8. SwiGLU: down( SiLU(gate(x)) ⊙ up(x) )
            nda_gemv_gpu_or_cpu_batch_2(
                &lw.gate_proj_gpu,
                &lw.gate_proj,
                &lw.up_proj_gpu,
                &lw.up_proj,
                x_ffn_input,
                &mut self.scratch.gate_out,
                &mut self.scratch.up_out,
            );

            // Calculate gated state in-place in self.scratch.gated
            self.scratch
                .gated
                .iter_mut()
                .zip(self.scratch.gate_out.iter())
                .zip(self.scratch.up_out.iter())
                .for_each(|((gated_i, &g), &u)| *gated_i = silu(g) * u);

            // Output of down projection written to self.scratch.q
            nda_gemv_gpu_or_cpu(
                &lw.down_proj_gpu,
                &lw.down_proj,
                &self.scratch.gated,
                &mut self.scratch.q,
            );

            // 9. FFN residual
            self.scratch
                .x
                .iter_mut()
                .zip(self.scratch.q.iter())
                .for_each(|(xi, &fi)| *xi += fi);
        }

        // ── Final RMSNorm ───────────────────────────────────────────────────
        rms_norm(&mut self.scratch.x, &self.weights.final_norm, cfg.rms_eps);

        // ── LM head (FP32 matmul, in-place into pre-allocated scratch) ───────
        lm_head(
            &self.scratch.x,
            &self.weights.lm_head,
            cfg.vocab_size,
            h,
            &mut self.scratch.logits,
        );
        &self.scratch.logits
    }

    /// Run autoregressive generation.
    ///
    /// Processes the full prompt token-by-token (prefill), then decodes up to
    /// `max_new_tokens` additional tokens. Calls `on_token` with each decoded
    /// piece for streaming output.
    pub fn generate(
        &mut self,
        prompt_tokens: &[u32],
        max_new_tokens: usize,
        temperature: f32,
        top_p: f32,
        mut on_token: impl FnMut(u32),
    ) {
        let mut rng = rand::thread_rng();
        self.reset_cache();

        // ── Prefill ─────────────────────────────────────────────────────────
        let n_prompt = prompt_tokens.len();
        debug_assert_ne!(n_prompt, 0, "prompt must not be empty");
        debug_assert_eq!(
            prompt_tokens[0], self.config.bos_token_id,
            "first prompt token should be BOS"
        );
        // Clamp max_new_tokens to the model's supported context window
        let max_new_tokens = max_new_tokens.min(self.config.max_seq_len.saturating_sub(n_prompt));
        for (pos, &tok) in prompt_tokens[..n_prompt - 1].iter().enumerate() {
            self.forward_one(tok, pos);
        }
        let logits = self.forward_one(prompt_tokens[n_prompt - 1], n_prompt - 1);
        let mut next = sample_token(logits, temperature, top_p, &mut rng);

        // ── Decode ──────────────────────────────────────────────────────────
        for step in 0..max_new_tokens {
            if next == self.config.eos_token_id {
                break;
            }
            on_token(next);
            let logits = self.forward_one(next, n_prompt + step);
            next = sample_token(logits, temperature, top_p, &mut rng);
        }
    }

    /// Run autoregressive generation and return a structured report with metrics.
    ///
    /// Same as `generate()` but collects timing, KV cache stats, and token IDs.
    pub fn generate_with_report(
        &mut self,
        prompt_tokens: &[u32],
        max_new_tokens: usize,
        temperature: f32,
        top_p: f32,
    ) -> Fp32GenerationReport {
        let mut rng = rand::thread_rng();
        self.reset_cache();
        let t_start = Instant::now();

        let n_prompt = prompt_tokens.len();
        debug_assert_ne!(n_prompt, 0, "prompt must not be empty");
        debug_assert_eq!(
            prompt_tokens[0], self.config.bos_token_id,
            "first prompt token should be BOS"
        );
        let max_new_tokens = max_new_tokens.min(self.config.max_seq_len.saturating_sub(n_prompt));
        let gpu_active = self.gpu_pipeline_active();
        let n_layers = self.config.n_layers;

        let mut forward_metrics = Vec::with_capacity(n_prompt + max_new_tokens);
        let mut token_ids = Vec::with_capacity(max_new_tokens);
        let mut stopped_at_eos = false;
        // Track KV blocks manually to avoid borrow conflict with forward_one's return ref
        let mut kv_blocks = 0usize;

        // ── Prefill ─────────────────────────────────────────────────────────
        for (pos, &tok) in prompt_tokens[..n_prompt - 1].iter().enumerate() {
            let step_start = Instant::now();
            self.forward_one(tok, pos);
            kv_blocks += n_layers;
            forward_metrics.push(Fp32ForwardMetrics {
                position: pos,
                gpu_active,
                layers_executed: n_layers,
                total_kv_blocks: kv_blocks,
                elapsed_us: step_start.elapsed().as_micros() as u64,
            });
        }
        let step_start = Instant::now();
        let logits = self.forward_one(prompt_tokens[n_prompt - 1], n_prompt - 1);
        kv_blocks += n_layers;
        let mut next = sample_token(logits, temperature, top_p, &mut rng);
        forward_metrics.push(Fp32ForwardMetrics {
            position: n_prompt - 1,
            gpu_active,
            layers_executed: n_layers,
            total_kv_blocks: kv_blocks,
            elapsed_us: step_start.elapsed().as_micros() as u64,
        });

        // ── Decode ──────────────────────────────────────────────────────────
        for step in 0..max_new_tokens {
            if next == self.config.eos_token_id {
                stopped_at_eos = true;
                break;
            }
            token_ids.push(next);
            let step_start = Instant::now();
            let logits = self.forward_one(next, n_prompt + step);
            kv_blocks += n_layers;
            next = sample_token(logits, temperature, top_p, &mut rng);
            forward_metrics.push(Fp32ForwardMetrics {
                position: n_prompt + step,
                gpu_active,
                layers_executed: n_layers,
                total_kv_blocks: kv_blocks,
                elapsed_us: step_start.elapsed().as_micros() as u64,
            });
        }

        let elapsed_us = t_start.elapsed().as_micros() as u64;
        let tokens_generated = token_ids.len();
        let tokens_per_second = if elapsed_us > 0 {
            tokens_generated as f64 * 1_000_000.0 / elapsed_us as f64
        } else {
            0.0
        };

        Fp32GenerationReport {
            prompt_tokens: n_prompt,
            tokens_generated,
            stopped_at_eos,
            truncated: !stopped_at_eos && tokens_generated >= max_new_tokens,
            final_kv_blocks: self.total_kv_blocks(),
            kv_cache_sizes: self.kv_cache_sizes(),
            kv_memory_bytes: self.kv_memory_bytes(),
            gpu_pipeline_active: gpu_active,
            elapsed_us,
            tokens_per_second,
            token_ids,
            forward_metrics,
        }
    }

    /// Get the final conditioning hidden state from natural language prompt processing.
    pub fn get_conditioning_hidden_state(&mut self, prompt_tokens: &[u32]) -> Vec<f32> {
        self.reset_cache();
        for (pos, &tok) in prompt_tokens.iter().enumerate() {
            self.forward_one(tok, pos);
        }
        self.scratch.x.clone()
    }

    /// Validate the transformer for consistency.
    /// Returns a list of warnings (empty = all good).
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let cfg = &self.config;

        if cfg.n_layers == 0 {
            warnings.push("n_layers is zero".to_string());
        }
        if cfg.hidden_size == 0 {
            warnings.push("hidden_size is zero".to_string());
        }
        if cfg.n_heads == 0 {
            warnings.push("n_heads is zero".to_string());
        }
        if cfg.head_dim == 0 {
            warnings.push("head_dim is zero".to_string());
        }
        if cfg.vocab_size == 0 {
            warnings.push("vocab_size is zero".to_string());
        }
        if cfg.hidden_size != cfg.n_heads * cfg.head_dim {
            warnings.push(format!(
                "hidden_size ({}) != n_heads * head_dim ({} * {} = {})",
                cfg.hidden_size,
                cfg.n_heads,
                cfg.head_dim,
                cfg.n_heads * cfg.head_dim
            ));
        }
        if cfg.n_heads % cfg.n_kv_heads != 0 {
            warnings.push(format!(
                "n_heads ({}) not divisible by n_kv_heads ({})",
                cfg.n_heads, cfg.n_kv_heads
            ));
        }
        if self.weights.layers.len() != cfg.n_layers {
            warnings.push(format!(
                "weight layers count ({}) != config n_layers ({})",
                self.weights.layers.len(),
                cfg.n_layers
            ));
        }
        if self.kv_cache.len() != cfg.n_layers {
            warnings.push(format!(
                "kv_cache layers ({}) != config n_layers ({})",
                self.kv_cache.len(),
                cfg.n_layers
            ));
        }

        warnings
    }

    /// Return a diagnostic snapshot of the transformer.
    pub fn info(&self) -> TransformerInfo {
        TransformerInfo {
            n_layers: self.config.n_layers,
            hidden_size: self.config.hidden_size,
            n_heads: self.config.n_heads,
            n_kv_heads: self.config.n_kv_heads,
            head_dim: self.config.head_dim,
            ffn_size: self.config.ffn_size,
            vocab_size: self.config.vocab_size,
            max_seq_len: self.config.max_seq_len,
            gpu_pipeline_active: self.gpu_pipeline_active(),
            total_kv_blocks: self.total_kv_blocks(),
            kv_memory_bytes: self.kv_memory_bytes(),
            weight_layers: self.weights.layers.len(),
            validation_issues: self.validate(),
        }
    }

    /// Batch forward: process multiple tokens and collect all logit vectors.
    /// Useful for evaluation or scoring.
    pub fn forward_batch(&mut self, tokens: &[u32]) -> Vec<Vec<f32>> {
        self.reset_cache();
        let mut outputs = Vec::with_capacity(tokens.len());
        for (pos, &tok) in tokens.iter().enumerate() {
            let logits = self.forward_one(tok, pos);
            outputs.push(logits.to_vec());
        }
        outputs
    }
}

/// Diagnostic snapshot of a Transformer.
#[derive(Debug, Clone, Serialize)]
pub struct TransformerInfo {
    pub n_layers: usize,
    pub hidden_size: usize,
    pub n_heads: usize,
    pub n_kv_heads: usize,
    pub head_dim: usize,
    pub ffn_size: usize,
    pub vocab_size: usize,
    pub max_seq_len: usize,
    pub gpu_pipeline_active: bool,
    pub total_kv_blocks: usize,
    pub kv_memory_bytes: usize,
    pub weight_layers: usize,
    pub validation_issues: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_norm_unit_rms() {
        let mut x = vec![1.0, 2.0, 3.0, 4.0];
        let weight = vec![1.0; 4];
        rms_norm(&mut x, &weight, 1e-6);
        let rms = (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        assert!((rms - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_rms_norm_preserves_direction() {
        let mut x = vec![2.0, 2.0, 2.0, 2.0];
        let weight = vec![1.0; 4];
        rms_norm(&mut x, &weight, 1e-6);
        // All elements should be equal after normalization with uniform weights
        for &xi in &x {
            assert!((xi - x[0]).abs() < 1e-5);
        }
    }

    #[test]
    fn test_rms_norm_to() {
        let x = vec![1.0, 2.0, 3.0, 4.0];
        let mut out = vec![0.0; 4];
        let weight = vec![1.0; 4];
        rms_norm_to(&x, &mut out, &weight, 1e-6);
        let rms = (out.iter().map(|v| v * v).sum::<f32>() / out.len() as f32).sqrt();
        assert!((rms - 1.0).abs() < 0.01);
        // Original should be unchanged
        assert_eq!(x, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_silu_zero() {
        assert!((silu(0.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_silu_positive() {
        let y = silu(1.0);
        assert!(y > 0.7 && y < 0.8);
    }

    #[test]
    fn test_silu_large() {
        let y = silu(10.0);
        assert!((y - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_apply_rope_head_pos_zero() {
        let mut head = vec![1.0, 0.0, 1.0, 0.0];
        apply_rope_head(&mut head, 0, 4, 10000.0);
        // At pos=0, angle=0, so cos=1, sin=0 -> identity transform
        assert!((head[0] - 1.0).abs() < 1e-5);
        assert!((head[1] - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_sample_token_greedy() {
        let logits = vec![1.0, 5.0, 3.0, 2.0];
        let mut rng = rand::thread_rng();
        let tok = sample_token(&logits, 0.0, 1.0, &mut rng);
        assert_eq!(tok, 1);
    }

    #[test]
    fn test_sample_token_temperature() {
        let logits = vec![0.0, 10.0, 0.0, 0.0];
        let mut rng = rand::thread_rng();
        // With very high temperature, distribution is more uniform
        let tok = sample_token(&logits, 100.0, 1.0, &mut rng);
        assert!(tok < 4);
    }

    #[test]
    fn test_sample_token_top_p() {
        let logits = vec![0.0, 10.0, 0.0, 0.0];
        let mut rng = rand::thread_rng();
        // With top_p=0.5, only the highest logit token should be selected
        let tok = sample_token(&logits, 0.1, 0.5, &mut rng);
        assert_eq!(tok, 1);
    }

    #[test]
    fn test_sample_token_top_k() {
        let logits = vec![0.0, 10.0, 5.0, 0.0];
        let mut rng = rand::thread_rng();
        let tok = sample_token_top_k(&logits, 0.1, 1.0, 2, &mut rng);
        assert!(tok == 1 || tok == 2);
    }

    #[test]
    fn test_frequency_penalty() {
        let mut logits = vec![5.0, 5.0, 5.0];
        let mut counts = std::collections::HashMap::new();
        counts.insert(0, 3);
        apply_frequency_penalty(&mut logits, &counts, 1.0);
        assert!((logits[0] - 2.0).abs() < 1e-5);
        assert!((logits[1] - 5.0).abs() < 1e-5);
    }

    #[test]
    fn test_presence_penalty() {
        let mut logits = vec![5.0, 5.0, 5.0];
        let mut counts = std::collections::HashMap::new();
        counts.insert(1, 10);
        apply_presence_penalty(&mut logits, &counts, 2.0);
        assert!((logits[0] - 5.0).abs() < 1e-5);
        assert!((logits[1] - 3.0).abs() < 1e-5);
    }

    #[test]
    fn test_lm_head_small() {
        let hidden = vec![1.0, 0.0, 0.0];
        let weights = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let mut logits = vec![0.0; 2];
        lm_head(&hidden, &weights, 2, 3, &mut logits);
        assert_eq!(logits.len(), 2);
        assert!((logits[0] - 1.0).abs() < 1e-5);
        assert!((logits[1] - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_pack_vector_impl() {
        let v = vec![1.0, -1.0, 2.0, -2.0];
        let (sign, extra) = pack_vector_impl(&v, 2.0, 4);
        assert_eq!(sign.len(), 1);
        assert_eq!(extra.len(), 1);
    }

    #[test]
    fn test_fp32_forward_metrics_default() {
        let m = Fp32ForwardMetrics::default();
        assert_eq!(m.position, 0);
        assert!(!m.gpu_active);
        assert_eq!(m.layers_executed, 0);
        assert_eq!(m.total_kv_blocks, 0);
        assert_eq!(m.elapsed_us, 0);
    }

    #[test]
    fn test_fp32_forward_metrics_serialize() {
        let m = Fp32ForwardMetrics {
            position: 5,
            gpu_active: true,
            layers_executed: 26,
            total_kv_blocks: 130,
            elapsed_us: 1500,
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"gpu_active\":true"));
        assert!(json.contains("\"layers_executed\":26"));
    }

    #[test]
    fn test_fp32_generation_report_serialize() {
        let report = Fp32GenerationReport {
            prompt_tokens: 10,
            tokens_generated: 5,
            stopped_at_eos: true,
            truncated: false,
            final_kv_blocks: 390,
            kv_cache_sizes: vec![15; 26],
            kv_memory_bytes: 50000,
            gpu_pipeline_active: false,
            elapsed_us: 10000,
            tokens_per_second: 500.0,
            token_ids: vec![1, 2, 3, 4, 5],
            forward_metrics: vec![],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"stopped_at_eos\":true"));
        assert!(json.contains("\"tokens_generated\":5"));
        assert!(json.contains("\"tokens_per_second\":500.0"));
    }

    #[test]
    fn test_kv_cache_empty_initially() {
        // Verify KV cache introspection on a fresh transformer
        // We can't build a full Transformer without weights, but we can test
        // the KvLayer directly
        let layer = KvLayer::new();
        assert_eq!(layer.blocks.len(), 0);
    }

    #[test]
    fn test_kv_layer_push_increments_blocks() {
        let mut layer = KvLayer::new();
        let k = vec![1.0; 8];
        let v = vec![0.5; 8];
        layer.push(&k, &v);
        assert_eq!(layer.blocks.len(), 1);
        layer.push(&k, &v);
        assert_eq!(layer.blocks.len(), 2);
    }

    #[test]
    fn test_kv_block_hash_chain() {
        let mut layer = KvLayer::new();
        let k1 = vec![1.0, 2.0, 3.0, 4.0];
        let v1 = vec![0.5, 1.0, 1.5, 2.0];
        layer.push(&k1, &v1);
        let k2 = vec![0.1, 0.2, 0.3, 0.4];
        let v2 = vec![0.05, 0.1, 0.15, 0.2];
        layer.push(&k2, &v2);

        // First block's prev_hash should be all zeros (genesis)
        assert_eq!(layer.blocks[0].prev_hash, [0u8; 32]);
        // Second block's prev_hash should match first block's hash
        assert_eq!(layer.blocks[1].prev_hash, layer.blocks[0].hash);
        // Hashes should not be zero (extremely unlikely for SHA-256)
        assert_ne!(layer.blocks[0].hash, [0u8; 32]);
        assert_ne!(layer.blocks[1].hash, [0u8; 32]);
    }

    #[test]
    fn test_kv_block_compute_hash_deterministic() {
        let block = NdaKvBlock {
            prev_hash: [0u8; 32],
            hash: [0u8; 32],
            k_scale: 1.0,
            v_scale: 2.0,
            k_sign: vec![0xFF],
            k_extra: vec![0xAA],
            v_sign: vec![0x55],
            v_extra: vec![0x00],
            k_raw: vec![1.0],
            v_raw: vec![2.0],
        };
        let h1 = block.compute_hash();
        let h2 = block.compute_hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_transformer_scratch_sizes() {
        let cfg = ModelConfig {
            n_layers: 2,
            hidden_size: 64,
            ffn_size: 128,
            n_heads: 4,
            n_kv_heads: 2,
            head_dim: 16,
            vocab_size: 100,
            max_seq_len: 32,
            rope_theta: 10000.0,
            alibi_shifts: vec![],
            rms_eps: 1e-6,
            eos_token_id: 2,
            bos_token_id: 1,
        };
        let scratch = TransformerScratch::new(&cfg);
        assert_eq!(scratch.x.len(), 64);
        assert_eq!(scratch.x_norm.len(), 64);
        assert_eq!(scratch.q.len(), 64); // n_heads * head_dim = 4*16
        assert_eq!(scratch.k.len(), 32); // n_kv_heads * head_dim = 2*16
        assert_eq!(scratch.v.len(), 32);
        assert_eq!(scratch.attn_out.len(), 64);
        assert_eq!(scratch.gate_out.len(), 128);
        assert_eq!(scratch.up_out.len(), 128);
        assert_eq!(scratch.gated.len(), 128);
        assert_eq!(scratch.logits.len(), 100);
    }

    #[test]
    fn test_transformer_info_serializes() {
        let info = TransformerInfo {
            n_layers: 26,
            hidden_size: 3200,
            n_heads: 32,
            n_kv_heads: 8,
            head_dim: 100,
            ffn_size: 8640,
            vocab_size: 32002,
            max_seq_len: 4096,
            gpu_pipeline_active: false,
            total_kv_blocks: 0,
            kv_memory_bytes: 0,
            weight_layers: 26,
            validation_issues: vec![],
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"n_layers\":26"));
        assert!(json.contains("\"hidden_size\":3200"));
        assert!(json.contains("\"gpu_pipeline_active\":false"));
    }

    // ─── Block 88: comprehensive tests ─────────────────────────────────────

    // ── silu edge cases ──────────────────────────────────────────────────────

    #[test]
    fn silu_negative() {
        let y = silu(-5.0);
        assert!(y.abs() < 0.05, "silu(-5) should be near zero, got {}", y);
    }

    #[test]
    fn silu_large_negative() {
        let y = silu(-20.0);
        assert!(y.abs() < 1e-6, "silu(-20) should be effectively zero, got {}", y);
    }

    #[test]
    fn silu_symmetry() {
        // silu(x) ≈ x for large x, silu(-x) ≈ 0 for large x
        let y_pos = silu(10.0);
        let y_neg = silu(-10.0);
        assert!((y_pos - 10.0).abs() < 0.01);
        assert!(y_neg.abs() < 0.01);
    }

    // ── rms_norm edge cases ──────────────────────────────────────────────────

    #[test]
    fn rms_norm_zero_input() {
        let mut x = vec![0.0; 4];
        let weight = vec![1.0; 4];
        rms_norm(&mut x, &weight, 1e-6);
        // Zero input with eps → small but finite output
        for &xi in &x {
            assert!(xi.is_finite());
        }
    }

    #[test]
    fn rms_norm_single_element() {
        let mut x = vec![5.0];
        let weight = vec![2.0];
        rms_norm(&mut x, &weight, 1e-6);
        // rms = sqrt(25/1) = 5, inv_rms = 0.2, x = 5 * 0.2 * 2 = 2.0
        assert!((x[0] - 2.0).abs() < 0.01, "got {}", x[0]);
    }

    #[test]
    fn rms_norm_non_uniform_weights() {
        let mut x = vec![1.0, 1.0, 1.0, 1.0];
        let weight = vec![2.0, 1.0, 1.0, 0.5];
        rms_norm(&mut x, &weight, 1e-6);
        // After normalization, ratios should match weight ratios
        assert!(x[0] > x[1], "higher weight should give higher value");
        assert!(x[3] < x[1], "lower weight should give lower value");
    }

    #[test]
    fn rms_norm_to_matches_in_place() {
        let x = vec![1.0, 2.0, 3.0, 4.0];
        let weight = vec![1.0; 4];
        let mut out = vec![0.0; 4];
        rms_norm_to(&x, &mut out, &weight, 1e-6);
        let mut x_copy = x.clone();
        rms_norm(&mut x_copy, &weight, 1e-6);
        for (a, b) in out.iter().zip(x_copy.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    // ── apply_rope_head edge cases ───────────────────────────────────────────

    #[test]
    fn rope_head_nonzero_position() {
        let mut head = vec![1.0, 0.0, 1.0, 0.0];
        apply_rope_head(&mut head, 5, 4, 10000.0);
        // Should be rotated — not identity anymore
        assert!((head[0] - 1.0).abs() > 0.01, "should be rotated at pos=5");
    }

    #[test]
    fn rope_head_preserves_norm() {
        let mut head = vec![3.0, 4.0, 1.0, 2.0];
        let norm_before: f32 = head.iter().map(|v| v * v).sum::<f32>().sqrt();
        apply_rope_head(&mut head, 10, 4, 10000.0);
        let norm_after: f32 = head.iter().map(|v| v * v).sum::<f32>().sqrt();
        // RoPE is a rotation — should preserve L2 norm
        assert!((norm_before - norm_after).abs() < 0.01,
            "norm before={}, after={}", norm_before, norm_after);
    }

    // ── sample_token edge cases ──────────────────────────────────────────────

    #[test]
    fn sample_token_all_equal() {
        let logits = vec![1.0, 1.0, 1.0, 1.0];
        let mut rng = rand::thread_rng();
        let tok = sample_token(&logits, 1.0, 1.0, &mut rng);
        assert!(tok < 4, "should pick a valid token");
    }

    #[test]
    fn sample_token_single_logit() {
        let logits = vec![5.0];
        let mut rng = rand::thread_rng();
        let tok = sample_token(&logits, 1.0, 1.0, &mut rng);
        assert_eq!(tok, 0, "only one token available");
    }

    // ── sample_token_top_k edge cases ────────────────────────────────────────

    #[test]
    fn sample_top_k_one_is_greedy() {
        let logits = vec![1.0, 5.0, 3.0, 2.0];
        let mut rng = rand::thread_rng();
        let tok = sample_token_top_k(&logits, 0.01, 1.0, 1, &mut rng);
        assert_eq!(tok, 1, "k=1 should be greedy");
    }

    #[test]
    fn sample_top_k_larger_than_vocab() {
        let logits = vec![1.0, 5.0, 3.0];
        let mut rng = rand::thread_rng();
        let tok = sample_token_top_k(&logits, 0.01, 1.0, 100, &mut rng);
        assert_eq!(tok, 1, "k > vocab should still pick highest");
    }

    // ── penalty functions ────────────────────────────────────────────────────

    #[test]
    fn frequency_penalty_no_counts() {
        let mut logits = vec![5.0, 3.0, 1.0];
        let counts = std::collections::HashMap::new();
        apply_frequency_penalty(&mut logits, &counts, 1.0);
        assert_eq!(logits, vec![5.0, 3.0, 1.0], "no counts → no change");
    }

    #[test]
    fn frequency_penalty_multiple_tokens() {
        let mut logits = vec![5.0, 5.0, 5.0];
        let mut counts = std::collections::HashMap::new();
        counts.insert(0, 2);
        counts.insert(1, 3);
        apply_frequency_penalty(&mut logits, &counts, 0.5);
        assert!((logits[0] - 4.0).abs() < 1e-5); // 5 - 2*0.5
        assert!((logits[1] - 3.5).abs() < 1e-5); // 5 - 3*0.5
        assert!((logits[2] - 5.0).abs() < 1e-5); // unchanged
    }

    #[test]
    fn presence_penalty_no_counts() {
        let mut logits = vec![5.0, 3.0, 1.0];
        let counts = std::collections::HashMap::new();
        apply_presence_penalty(&mut logits, &counts, 1.0);
        assert_eq!(logits, vec![5.0, 3.0, 1.0], "no counts → no change");
    }

    #[test]
    fn presence_penalty_out_of_range_token() {
        let mut logits = vec![5.0, 3.0, 1.0];
        let mut counts = std::collections::HashMap::new();
        counts.insert(99, 1); // token 99 is out of range
        apply_presence_penalty(&mut logits, &counts, 1.0);
        assert_eq!(logits, vec![5.0, 3.0, 1.0], "out-of-range token → no change");
    }

    // ── pack_vector_impl ─────────────────────────────────────────────────────

    #[test]
    fn pack_vector_impl_known_values() {
        let v = vec![2.0, -2.0, 1.0, -1.0, 0.0, 0.5, -0.5, 1.5];
        let (sign, extra) = pack_vector_impl(&v, 2.0, 8);
        assert_eq!(sign.len(), 1);
        assert_eq!(extra.len(), 1);
        // Positive values: sign bit = 1
        // Negative values: sign bit = 0
    }

    #[test]
    fn pack_vector_impl_padding() {
        let v = vec![1.0, -1.0];
        let (sign, extra) = pack_vector_impl(&v, 1.0, 8); // padded to 8
        assert_eq!(sign.len(), 1);
        assert_eq!(extra.len(), 1);
    }

    // ── NdaKvBlock hash ──────────────────────────────────────────────────────

    #[test]
    fn kv_block_hash_differs_for_different_data() {
        let b1 = NdaKvBlock {
            prev_hash: [0u8; 32], hash: [0u8; 32],
            k_scale: 1.0, v_scale: 1.0,
            k_sign: vec![0xFF], k_extra: vec![0xAA],
            v_sign: vec![0x55], v_extra: vec![0x00],
            k_raw: vec![1.0], v_raw: vec![2.0],
        };
        let b2 = NdaKvBlock {
            prev_hash: [0u8; 32], hash: [0u8; 32],
            k_scale: 2.0, v_scale: 1.0, // different scale
            k_sign: vec![0xFF], k_extra: vec![0xAA],
            v_sign: vec![0x55], v_extra: vec![0x00],
            k_raw: vec![1.0], v_raw: vec![2.0],
        };
        assert_ne!(b1.compute_hash(), b2.compute_hash());
    }

    #[test]
    fn kv_block_hash_differs_for_different_prev_hash() {
        let b1 = NdaKvBlock {
            prev_hash: [0u8; 32], hash: [0u8; 32],
            k_scale: 1.0, v_scale: 1.0,
            k_sign: vec![0xFF], k_extra: vec![0xAA],
            v_sign: vec![0x55], v_extra: vec![0x00],
            k_raw: vec![1.0], v_raw: vec![2.0],
        };
        let b2 = NdaKvBlock {
            prev_hash: [1u8; 32], hash: [0u8; 32],
            k_scale: 1.0, v_scale: 1.0,
            k_sign: vec![0xFF], k_extra: vec![0xAA],
            v_sign: vec![0x55], v_extra: vec![0x00],
            k_raw: vec![1.0], v_raw: vec![2.0],
        };
        assert_ne!(b1.compute_hash(), b2.compute_hash());
    }

    // ── KvLayer ──────────────────────────────────────────────────────────────

    #[test]
    fn kv_layer_push_bitmap_sizes() {
        let mut layer = KvLayer::new();
        let k = vec![1.0; 16];
        let v = vec![0.5; 16];
        layer.push(&k, &v);
        assert_eq!(layer.blocks.len(), 1);
        let block = &layer.blocks[0];
        // 16 elements → 2 bytes per bitmap
        assert_eq!(block.k_sign.len(), 2);
        assert_eq!(block.k_extra.len(), 2);
        assert_eq!(block.v_sign.len(), 2);
        assert_eq!(block.v_extra.len(), 2);
    }

    #[test]
    fn kv_layer_multiple_pushes() {
        let mut layer = KvLayer::new();
        for i in 0..5 {
            let k = vec![i as f32; 8];
            let v = vec![(i as f32) * 0.5; 8];
            layer.push(&k, &v);
        }
        assert_eq!(layer.blocks.len(), 5);
        // Hash chain integrity
        for i in 1..5 {
            assert_eq!(layer.blocks[i].prev_hash, layer.blocks[i - 1].hash);
        }
    }

    // ── lm_head ──────────────────────────────────────────────────────────────

    #[test]
    fn lm_head_identity_like() {
        // 3×3 identity weight matrix
        let hidden = vec![1.0, 2.0, 3.0];
        let weights = vec![
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
        ];
        let mut logits = vec![0.0; 3];
        lm_head(&hidden, &weights, 3, 3, &mut logits);
        assert!((logits[0] - 1.0).abs() < 1e-5);
        assert!((logits[1] - 2.0).abs() < 1e-5);
        assert!((logits[2] - 3.0).abs() < 1e-5);
    }

    #[test]
    fn lm_head_zero_hidden() {
        let hidden = vec![0.0; 4];
        let weights = vec![1.0; 8]; // 2 vocab × 4 hidden
        let mut logits = vec![0.0; 2];
        lm_head(&hidden, &weights, 2, 4, &mut logits);
        assert_eq!(logits, vec![0.0, 0.0]);
    }

    // ── Fp32GenerationReport extras ──────────────────────────────────────────

    #[test]
    fn generation_report_with_metrics() {
        let report = Fp32GenerationReport {
            prompt_tokens: 5,
            tokens_generated: 3,
            stopped_at_eos: false,
            truncated: true,
            final_kv_blocks: 100,
            kv_cache_sizes: vec![4; 2],
            kv_memory_bytes: 8000,
            gpu_pipeline_active: true,
            elapsed_us: 5000,
            tokens_per_second: 600.0,
            token_ids: vec![10, 20, 30],
            forward_metrics: vec![
                Fp32ForwardMetrics {
                    position: 0, gpu_active: true, layers_executed: 2,
                    total_kv_blocks: 2, elapsed_us: 1000,
                },
                Fp32ForwardMetrics {
                    position: 1, gpu_active: true, layers_executed: 2,
                    total_kv_blocks: 4, elapsed_us: 800,
                },
            ],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"truncated\":true"));
        assert!(json.contains("\"gpu_pipeline_active\":true"));
        assert!(json.contains("\"tokens_per_second\":600.0"));
        assert!(json.contains("\"forward_metrics\""));
    }

    // ── TransformerInfo with validation issues ───────────────────────────────

    #[test]
    fn transformer_info_with_issues() {
        let info = TransformerInfo {
            n_layers: 2,
            hidden_size: 64,
            n_heads: 4,
            n_kv_heads: 2,
            head_dim: 16,
            ffn_size: 128,
            vocab_size: 100,
            max_seq_len: 512,
            gpu_pipeline_active: false,
            total_kv_blocks: 0,
            kv_memory_bytes: 0,
            weight_layers: 2,
            validation_issues: vec!["layer 0 q_proj mismatch".into()],
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("validation_issues"));
        assert!(json.contains("q_proj mismatch"));
    }

    // ── TransformerScratch different configs ─────────────────────────────────

    #[test]
    fn scratch_gqa_config() {
        let cfg = ModelConfig {
            n_layers: 1,
            hidden_size: 64,
            ffn_size: 128,
            n_heads: 8,      // 8 heads
            n_kv_heads: 2,   // 2 KV heads (GQA)
            head_dim: 8,
            vocab_size: 50,
            max_seq_len: 32,
            rope_theta: 10000.0,
            alibi_shifts: vec![],
            rms_eps: 1e-6,
            eos_token_id: 2,
            bos_token_id: 1,
        };
        let scratch = TransformerScratch::new(&cfg);
        assert_eq!(scratch.q.len(), 64); // 8 * 8
        assert_eq!(scratch.k.len(), 16); // 2 * 8
        assert_eq!(scratch.v.len(), 16); // 2 * 8
        assert_eq!(scratch.logits.len(), 50);
    }
}
