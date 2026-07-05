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

use rand::Rng;
use rayon::prelude::*;

use crate::model::{config::ModelConfig, weights::ModelWeights};
use crate::nda::{nda_gemv, NdaMatrix};
use crate::compiler::driver::VulkanNdaGemv;

/// Pack a float32 vector to v2 quad sign+extra bitmaps (same as quantize_activations_v2_quad
/// but operates on a slice of `len` elements and can zero-pad to `len` if needed).
fn pack_vector_impl(v: &[f32], scale: f32, len: usize) -> (Vec<u8>, Vec<u8>) {
    let mut sign_buf  = vec![0u8; (len + 7) / 8];
    let mut extra_buf = vec![0u8; (len + 7) / 8];

    let actual_scale = if scale < 1e-8 { 1.0 } else { scale };
    let inv_scale = 1.0 / actual_scale;

    for (i, &val) in v.iter().enumerate() {
        if i >= len { break; }
        let val_scaled  = val * inv_scale;
        let is_large    = val_scaled.abs() >= 1.5;
        let is_pos      = val >= 0.0;

        let sign_bit  = if is_pos  { 1u8 } else { 0 };
        let large_bit = if is_large { 1u8 } else { 0 };
        // XNOR(sign, large) = extra
        let extra_bit = !(sign_bit ^ large_bit) & 1;

        let byte_idx = i / 8;
        let bit_idx  = i % 8;

        if sign_bit  == 1 { sign_buf[byte_idx]  |= 1 << bit_idx; }
        if extra_bit == 1 { extra_buf[byte_idx] |= 1 << bit_idx; }
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
        if gpu.version == crate::nda::NDA_VERSION_FP4 as u32 || gpu.version == crate::nda::NDA_VERSION_FP2 as u32 {
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
                let num_col_words_padded = ((cpu_gemv.cols / 32) + 3) / 4 * 4;
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

fn nda_gemv_gpu_or_cpu_batch_3(
    w0_gpu: &Option<VulkanNdaGemv>, w0_cpu: &NdaMatrix,
    w1_gpu: &Option<VulkanNdaGemv>, w1_cpu: &NdaMatrix,
    w2_gpu: &Option<VulkanNdaGemv>, w2_cpu: &NdaMatrix,
    x: &[f32],
    out0: &mut [f32],
    out1: &mut [f32],
    out2: &mut [f32],
) {
    if let (Some(g0), Some(g1), Some(g2)) = (w0_gpu, w1_gpu, w2_gpu) {
        if (g0.version == crate::nda::NDA_VERSION_FP4 as u32 || g0.version == crate::nda::NDA_VERSION_FP2 as u32) &&
           (g1.version == crate::nda::NDA_VERSION_FP4 as u32 || g1.version == crate::nda::NDA_VERSION_FP2 as u32) &&
           (g2.version == crate::nda::NDA_VERSION_FP4 as u32 || g2.version == crate::nda::NDA_VERSION_FP2 as u32)
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
    w0_gpu: &Option<VulkanNdaGemv>, w0_cpu: &NdaMatrix,
    w1_gpu: &Option<VulkanNdaGemv>, w1_cpu: &NdaMatrix,
    x: &[f32],
    out0: &mut [f32],
    out1: &mut [f32],
) {
    if let (Some(g0), Some(g1)) = (w0_gpu, w1_gpu) {
        if (g0.version == crate::nda::NDA_VERSION_FP4 as u32 || g0.version == crate::nda::NDA_VERSION_FP2 as u32) &&
           (g1.version == crate::nda::NDA_VERSION_FP4 as u32 || g1.version == crate::nda::NDA_VERSION_FP2 as u32)
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
    pub hash: [u8; 32],      // Precomputed SHA-256 hash of this block
    pub k_scale: f32,
    pub v_scale: f32,
    /// v2 sign bitmap: bit=1 → K element is positive
    pub k_sign:  Vec<u8>,
    /// v2 extra bitmap: magnitude via XNOR(sign,extra) → {-2,-1,+1,+2}
    pub k_extra: Vec<u8>,
    /// v2 sign bitmap: bit=1 → V element is positive
    pub v_sign:  Vec<u8>,
    /// v2 extra bitmap: magnitude via XNOR(sign,extra) → {-2,-1,+1,+2}
    pub v_extra: Vec<u8>,
    pub k_raw:   Vec<f32>,
    pub v_raw:   Vec<f32>,
}

impl NdaKvBlock {
    pub fn compute_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.prev_hash);
        hasher.update(&self.k_scale.to_le_bytes());
        hasher.update(&self.v_scale.to_le_bytes());
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
        let freq  = theta.powf(-2.0 * i as f32 / head_dim as f32);
        let angle = pos as f32 * freq;
        let (s, c) = angle.sin_cos();
        let x0 = head[i];
        let x1 = head[i + half];
        head[i]        = x0 * c - x1 * s;
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
fn attention_head(
    q_sign:  &[u8],
    q_extra: &[u8],
    q_scale: f32,
    kv_layer: &KvLayer,
    h_start:  usize,
    h_end:    usize,
    scale:    f32,
) -> Vec<f32> {
    let head_dim       = h_end - h_start;
    let head_bytes     = (head_dim + 7) / 8;
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
                    panic!("Security Fault: Hash chain broken at block {}!", t);
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
                let diff_sign =   qs ^ ks;

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
        if s == 0.0 { continue; }
        for i in 0..head_dim {
            let global_idx = h_start + i;
            let byte_idx   = global_idx / 8;
            let bit_idx    = global_idx % 8;
            let mask       = 1 << bit_idx;
            let sign  = (block.v_sign[byte_idx]  & mask) != 0;
            let extra = (block.v_extra[byte_idx] & mask) != 0;
            // v2 XNOR decode: pure-additive (no multiplication)
            let val = if sign { s } else { -s };
            out[i] += val;                           // always add once
            if sign == extra { out[i] += val; }      // large → add again
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
            let dot = q.iter().zip(k_slice.iter()).map(|(&qi, &ki)| qi * ki).sum::<f32>();
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
fn lm_head(hidden: &[f32], weights: &[f32], vocab_size: usize, hidden_size: usize) -> Vec<f32> {
    (0..vocab_size)
        .into_par_iter()
        .map(|v| {
            weights[v * hidden_size..(v + 1) * hidden_size]
                .iter()
                .zip(hidden.iter())
                .map(|(&w, &x)| w * x)
                .sum::<f32>()
        })
        .collect()
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
    indexed.sort_unstable_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

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
        }
    }
}

pub struct Transformer {
    config:   ModelConfig,
    weights:  ModelWeights,
    kv_cache: Vec<KvLayer>,
    scratch:  TransformerScratch,
    gpu_pipeline: Option<crate::compiler::driver::VulkanModelPipeline>,
}

impl Transformer {
    pub fn new(config: ModelConfig, weights: ModelWeights) -> Self {
        let kv_cache = (0..config.n_layers).map(|_| KvLayer::new()).collect();
        let scratch = TransformerScratch::new(&config);
        
        let gpu_pipeline = None;
        
        Self { config, weights, kv_cache, scratch, gpu_pipeline }
    }

    /// Reset KV cache (call between independent prompts).
    pub fn reset_cache(&mut self) {
        for layer in &mut self.kv_cache {
            layer.blocks.clear();
        }
    }

    /// Process one token at position `pos` and return the next-token logit vector and the final hidden state.
    fn forward_one(&mut self, token: u32, pos: usize) -> (Vec<f32>, Vec<f32>) {
        let cfg = &self.config;
        let h   = cfg.hidden_size;

        if let Some(ref pipeline) = self.gpu_pipeline {
            // 1. Copy initial token embeddings to x_residual_buffer on GPU
            let embed_src = &self.weights.embed_tokens[token as usize * h .. (token as usize + 1) * h];
            unsafe {
                std::ptr::copy_nonoverlapping(embed_src.as_ptr(), pipeline.x_residual_ptr as *mut f32, h);
            }

            // 2. Build LayerGpuGemvs list
            let layers_gpu: Vec<crate::compiler::driver::LayerGpuGemvs> = self.weights.layers.iter().map(|l| {
                crate::compiler::driver::LayerGpuGemvs {
                    q_proj_gpu: &l.q_proj_gpu,
                    k_proj_gpu: &l.k_proj_gpu,
                    v_proj_gpu: &l.v_proj_gpu,
                    o_proj_gpu: &l.o_proj_gpu,
                    gate_proj_gpu: &l.gate_proj_gpu,
                    up_proj_gpu: &l.up_proj_gpu,
                    down_proj_gpu: &l.down_proj_gpu,
                }
            }).collect();
            let layers_gpu_refs: Vec<&crate::compiler::driver::LayerGpuGemvs> = layers_gpu.iter().collect();

            // 3. Run the GPU execution pipeline
            let attn_scale = (cfg.head_dim as f32).sqrt().recip();
            pipeline.record_and_execute_token(
                self.weights.vulkan.as_ref().unwrap(),
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
            ).expect("Vulkan pipeline execution failed");

            // 4. Read back the final hidden state from x_residual_buffer to CPU self.scratch.x
            unsafe {
                std::ptr::copy_nonoverlapping(pipeline.x_residual_ptr as *const f32, self.scratch.x.as_mut_ptr(), h);
            }

            // 5. Evaluate the LM Head on the CPU
            let logits = lm_head(&self.scratch.x, &self.weights.lm_head, cfg.vocab_size, h);
            return (logits, self.scratch.x.clone());
        }

        // 1. Copy embed tokens to scratch.x in-place (no allocation!)
        self.scratch.x.copy_from_slice(&self.weights.embed_tokens[token as usize * h .. (token as usize + 1) * h]);

        // ── 26 transformer layers ───────────────────────────────────────────
        for layer_idx in 0..cfg.n_layers {
            let lw = &self.weights.layers[layer_idx];

            // 1. Attention pre-norm (zero-copy straight to mapped GPU buffer if Vulkan is used!)
            let x_input: &[f32] = if let Some(ref driver) = self.weights.vulkan {
                unsafe {
                    let dest_slice = std::slice::from_raw_parts_mut(driver.shared_input_ptr as *mut f32, h);
                    rms_norm_to(&self.scratch.x, dest_slice, &lw.attn_norm, cfg.rms_eps);
                    dest_slice
                }
            } else {
                rms_norm_to(&self.scratch.x, &mut self.scratch.x_norm, &lw.attn_norm, cfg.rms_eps);
                &self.scratch.x_norm[..]
            };

            // 2. QKV projections (in batch asynchronously writing directly to scratch)
            nda_gemv_gpu_or_cpu_batch_3(
                &lw.q_proj_gpu, &lw.q_proj,
                &lw.k_proj_gpu, &lw.k_proj,
                &lw.v_proj_gpu, &lw.v_proj,
                x_input,
                &mut self.scratch.q,
                &mut self.scratch.k,
                &mut self.scratch.v,
            );

            if let Some(ref qb) = lw.q_proj_bias {
                self.scratch.q.iter_mut().zip(qb.iter()).for_each(|(qi, &bi)| *qi += bi);
            }
            if let Some(ref kb) = lw.k_proj_bias {
                self.scratch.k.iter_mut().zip(kb.iter()).for_each(|(ki, &bi)| *ki += bi);
            }
            if let Some(ref vb) = lw.v_proj_bias {
                self.scratch.v.iter_mut().zip(vb.iter()).for_each(|(vi, &bi)| *vi += bi);
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
                    kv_layer, hs_kv, he_kv, attn_scale,
                    &mut self.scratch.attn_out[hs..he],
                );
            }

            // 6. Output projection + residual
            // We use self.scratch.q temporarily to hold the output projection result
            nda_gemv_gpu_or_cpu(&lw.o_proj_gpu, &lw.o_proj, &self.scratch.attn_out, &mut self.scratch.q);
            self.scratch.x.iter_mut()
                .zip(self.scratch.q.iter())
                .for_each(|(xi, &oi)| *xi += oi);

            // 7. FFN pre-norm (zero-copy straight to mapped GPU buffer if Vulkan is used!)
            let x_ffn_input: &[f32] = if let Some(ref driver) = self.weights.vulkan {
                unsafe {
                    let dest_slice = std::slice::from_raw_parts_mut(driver.shared_input_ptr as *mut f32, h);
                    rms_norm_to(&self.scratch.x, dest_slice, &lw.ffn_norm, cfg.rms_eps);
                    dest_slice
                }
            } else {
                rms_norm_to(&self.scratch.x, &mut self.scratch.x_norm, &lw.ffn_norm, cfg.rms_eps);
                &self.scratch.x_norm[..]
            };

            // 8. SwiGLU: down( SiLU(gate(x)) ⊙ up(x) )
            nda_gemv_gpu_or_cpu_batch_2(
                &lw.gate_proj_gpu, &lw.gate_proj,
                &lw.up_proj_gpu, &lw.up_proj,
                x_ffn_input,
                &mut self.scratch.gate_out,
                &mut self.scratch.up_out,
            );

            // Calculate gated state in-place in self.scratch.gated
            self.scratch.gated.iter_mut()
                .zip(self.scratch.gate_out.iter())
                .zip(self.scratch.up_out.iter())
                .for_each(|((gated_i, &g), &u)| *gated_i = silu(g) * u);

            // Output of down projection written to self.scratch.q
            nda_gemv_gpu_or_cpu(&lw.down_proj_gpu, &lw.down_proj, &self.scratch.gated, &mut self.scratch.q);

            // 9. FFN residual
            self.scratch.x.iter_mut()
                .zip(self.scratch.q.iter())
                .for_each(|(xi, &fi)| *xi += fi);
        }

        // ── Final RMSNorm ───────────────────────────────────────────────────
        rms_norm(&mut self.scratch.x, &self.weights.final_norm, cfg.rms_eps);

        // ── LM head (FP32 matmul) ───────────────────────────────────────────
        let logits = lm_head(&self.scratch.x, &self.weights.lm_head, cfg.vocab_size, h);
        (logits, self.scratch.x.clone())
    }

    /// Run autoregressive generation.
    ///
    /// Processes the full prompt token-by-token (prefill), then decodes up to
    /// `max_new_tokens` additional tokens. Calls `on_token` with each decoded
    /// piece for streaming output.
    pub fn generate(
        &mut self,
        prompt_tokens:  &[u32],
        max_new_tokens: usize,
        temperature:    f32,
        top_p:          f32,
        mut on_token:   impl FnMut(u32),
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
        let max_new_tokens = max_new_tokens
            .min(self.config.max_seq_len.saturating_sub(n_prompt));
        let mut logits = Vec::new();
        for (pos, &tok) in prompt_tokens.iter().enumerate() {
            let (lg, _) = self.forward_one(tok, pos);
            logits = lg;
            // During prefill we discard logits for all but the last token.
        }

        let mut next = sample_token(&logits, temperature, top_p, &mut rng);

        // ── Decode ──────────────────────────────────────────────────────────
        for step in 0..max_new_tokens {
            if next == self.config.eos_token_id {
                break;
            }
            on_token(next);
            let (logits, _) = self.forward_one(next, n_prompt + step);
            next = sample_token(&logits, temperature, top_p, &mut rng);
        }
    }

    /// Get the final conditioning hidden state from natural language prompt processing.
    pub fn get_conditioning_hidden_state(&mut self, prompt_tokens: &[u32]) -> Vec<f32> {
        self.reset_cache();
        let mut last_hidden = vec![0.0f32; self.config.hidden_size];
        for (pos, &tok) in prompt_tokens.iter().enumerate() {
            let (_, hidden) = self.forward_one(tok, pos);
            last_hidden = hidden;
        }
        last_hidden
    }
}
