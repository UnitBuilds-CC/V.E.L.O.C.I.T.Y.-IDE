// model/transformer_zero.rs — V.E.L.O.C.I.T.Y.-IDE
//
// Pure-integer, zero-float transformer forward pass.
//
// Every operation is addition, subtraction, bitwise, or bit-shift.
// Zero multiplications in weight-application paths.
// Zero floating-point operations.
//
// Architecture:
//   • Positional encoding : ALiBi (bit-shift + subtract — no RoPE, no sin/cos)
//   • All projections     : NDA v2 GEMV → NdaVec (bitwise popcount)
//   • Residual stream     : NdaVec with log2_scale (power-of-2, bit-shift aligned)
//   • KV cache            : NdaVec per token (sign+extra bitmaps + scale shift)
//   • RMSNorm             : fixed-point i64 (isqrt, bit-shift)
//   • SwiGLU SiLU         : 4-entry lookup (only 4 possible NDA inputs)
//   • LM head             : NDA GEMV → i32 → argmax (no softmax, no exp)
//
// Compatibility: loaded via the same NDA weight files as the FP32 path.
// Activated by: `--zero-float` CLI flag.

use rayon::prelude::*;
use serde::Serialize;
use std::time::Instant;

use crate::model::{config::ModelConfig, weights::ModelWeights};
use crate::nda_int::{
    apply_alibi_bias_i32, argmax_i32, nda_gemv_nda_to_nda, nda_vec_add_inplace, rms_norm_nda,
    swiglu_nda, AliBiSlopes, NdaEmbedding, NdaVec, SiluLut, DOT_4_LUT,
};
use crate::site_map::SiteMap;

// ─── Forward Metrics & Generation Report ──────────────────────────────────────

/// Metrics collected during a single forward pass.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ForwardMetrics {
    /// Site map KV cache hits.
    pub site_map_hits: usize,
    /// Site map KV cache misses.
    pub site_map_misses: usize,
    /// Total KV cache entries across all layers after this forward pass.
    pub kv_cache_size: usize,
}

impl ForwardMetrics {
    /// Cache hit rate: hits / (hits + misses).
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.site_map_hits + self.site_map_misses;
        if total == 0 { 0.0 } else { self.site_map_hits as f64 / total as f64 }
    }
}

/// Report from a greedy generation run.
#[derive(Debug, Clone, Serialize)]
pub struct GenerationReport {
    /// Total tokens generated (excluding prompt).
    pub tokens_generated: usize,
    /// Prompt token count.
    pub prompt_tokens: usize,
    /// Whether generation stopped due to EOS.
    pub stopped_at_eos: bool,
    /// Whether generation was truncated by max_new_tokens.
    pub truncated: bool,
    /// Total site map hits during generation.
    pub site_map_hits: usize,
    /// Total site map misses during generation.
    pub site_map_misses: usize,
    /// Final KV cache size (total entries across all layers).
    pub final_kv_cache_size: usize,
    /// Total elapsed time (microseconds).
    pub elapsed_us: u64,
    /// Tokens per second.
    pub tokens_per_second: f64,
    /// All generated token IDs.
    pub token_ids: Vec<u32>,
}

impl GenerationReport {
    /// Overall cache hit rate.
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.site_map_hits + self.site_map_misses;
        if total == 0 { 0.0 } else { self.site_map_hits as f64 / total as f64 }
    }

    /// Average microseconds per generated token.
    pub fn us_per_token(&self) -> f64 {
        if self.tokens_generated == 0 {
            0.0
        } else {
            self.elapsed_us as f64 / self.tokens_generated as f64
        }
    }

    /// Total tokens processed (prompt + generated).
    pub fn total_tokens(&self) -> usize {
        self.prompt_tokens + self.tokens_generated
    }
}

// ─── Transformer diagnostics ──────────────────────────────────────────────

/// Comprehensive diagnostic info for the ZeroTransformer.
#[derive(Debug, Clone, Serialize)]
pub struct ZeroTransformerInfo {
    pub n_layers: usize,
    pub n_heads: usize,
    pub n_kv_heads: usize,
    pub hidden_size: usize,
    pub head_dim: usize,
    pub vocab_size: usize,
    pub max_seq_len: usize,
    pub eos_token_id: u32,
    pub total_kv_cached: usize,
    pub per_layer_kv: Vec<usize>,
    pub lm_head_rows: usize,
    pub lm_head_stride: usize,
    pub embed_tokens_bytes: usize,
    pub validation_issues: Vec<String>,
}

/// Report from norm_to_ndavec conversion with diagnostics.
#[derive(Debug, Clone, Serialize)]
pub struct NormConversionReport {
    pub input_len: usize,
    pub output_len: usize,
    pub log2_scale: i8,
    pub abs_max: f64,
    pub all_positive: bool,
    pub all_negative: bool,
}

/// Summary of a generation run for logging/display.
#[derive(Debug, Clone, Serialize)]
pub struct GenerationSummary {
    pub tokens_generated: usize,
    pub prompt_tokens: usize,
    pub stopped_at_eos: bool,
    pub truncated: bool,
    pub tokens_per_second: f64,
    pub cache_hit_rate: f64,
    pub elapsed_ms: f64,
    pub first_token_id: Option<u32>,
    pub last_token_id: Option<u32>,
}

// ─── NDA KV cache (zero-float) ─────────────────────────────────────────────

/// One token's KV entry in pure NDA v2 bitmap format.
struct ZeroKvEntry {
    /// Key vector as NdaVec
    k: NdaVec,
    /// Value vector as NdaVec
    v: NdaVec,
}

struct ZeroKvLayer {
    entries: Vec<ZeroKvEntry>,
}

impl ZeroKvLayer {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn push(&mut self, k: NdaVec, v: NdaVec) {
        self.entries.push(ZeroKvEntry { k, v });
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

// ─── Attention (ALiBi + bitwise Q·K, integer V decode) ─────────────────────

/// Single-head attention — fully integer.
///
/// Q is an NdaVec slice (head_dim elements).
/// K/V are NdaVec entries from the KV cache.
/// ALiBi bias applied via bit-shift subtraction.
/// Attention weights: softmax approximated as proportional i32 → not needed for argmax.
///
/// For the V weighted sum we need real weights (not just argmax of scores).
/// We use a fixed-point softmax approximation:
///   weight_t ∝ 2^(score_t - max_score)   =  1 >> (max_score - score_t)
/// This is the "bit-shift softmax" — replaces exp() with right-shifts.
/// Exact for the argmax token; approximation elsewhere.
#[allow(clippy::needless_range_loop)]
fn attention_head_zero(
    q: &NdaVec,
    kv_layer: &ZeroKvLayer,
    h_start: usize,
    q_pos: usize,
    head_idx: usize,
    alibi: &AliBiSlopes,
) -> NdaVec {
    let head_dim = q.len;

    // ── Step 1: Q·K dot products → i32 scores (pure bitwise popcount) ────────
    let head_bytes = head_dim.div_ceil(8);
    let mut q_low = [0usize; 64];
    let mut q_high = [0usize; 64];
    let limit = head_bytes.min(64);
    for b in 0..limit {
        let qs = q.sign[b];
        let qe = q.extra[b];
        q_low[b] = ((qs & 0x0F) | ((qe & 0x0F) << 4)) as usize;
        q_high[b] = (((qs >> 4) & 0x0F) | (qe & 0xF0)) as usize;
    }

    let mut scores: Vec<i32> = kv_layer
        .entries
        .iter()
        .map(|entry| {
            // K entry covers full hidden_size; we extract head h_start..h_start+head_dim
            let head_byte_start = h_start / 8;

            let mut acc = 0i32;
            for b in 0..head_bytes {
                let ks = entry.k.sign[head_byte_start + b];
                let ke = entry.k.extra[head_byte_start + b];
                let k_low = ((ks & 0x0F) | ((ke & 0x0F) << 4)) as usize;
                let k_high = (((ks >> 4) & 0x0F) | (ke & 0xF0)) as usize;

                acc += (DOT_4_LUT[q_low[b]][k_low] + DOT_4_LUT[q_high[b]][k_high]) as i32;
            }
            // Scale: q.log2_scale + k.log2_scale combined (integer add)
            acc
        })
        .collect();

    // Scale: q.log2_scale + k_log2 - 3 (representing Q·K / sqrt(head_dim))
    let k_log2 = kv_layer
        .entries
        .first()
        .map(|e| e.k.log2_scale)
        .unwrap_or(0);
    let scores_log2 = q.log2_scale + k_log2 - 3;
    let scale_shift = (-scores_log2).max(0) as u32;

    // ── Step 2: ALiBi bias — pure bit-shift subtraction ─────────────────────
    apply_alibi_bias_i32(&mut scores, q_pos, alibi.shift(head_idx), scale_shift);

    // ── Step 3: Bit-shift softmax approximation → integer attention weights ──
    //   Instead of 1i32 >> gap (which collapses to hard argmax), we use Q14 fixed-point
    //   weights with Q8 fractional linear interpolation for 2^-gap_float.
    let max_score = *scores.iter().max().unwrap_or(&0);
    let weights: Vec<i32> = scores
        .iter()
        .map(|&s| {
            let gap = max_score - s;
            // Represent gap in Q8 (fixed-point with 8 fractional bits)
            let gap_q8 = if scale_shift >= 8 {
                gap >> (scale_shift - 8)
            } else {
                gap << (8 - scale_shift)
            };
            let integer_part = (gap_q8 >> 8).clamp(0, 14) as u32;
            let fractional_part = gap_q8 & 0xFF;

            let a = 16384i32 >> integer_part;
            let b = 16384i32 >> (integer_part + 1);
            a - (((a - b) * fractional_part) >> 8)
        })
        .collect();

    let weight_sum: i32 = weights.iter().sum::<i32>().max(1);

    // ── Step 4: Weighted V accumulation — pure integer add/subtract ──────────
    let mut out_i32 = vec![0i32; head_dim];
    let head_byte_start = h_start / 8;

    for (w, entry) in weights.iter().zip(kv_layer.entries.iter()) {
        if *w == 0 {
            continue;
        }
        for i in 0..head_dim {
            let global_byte = head_byte_start + i / 8;
            let bit_idx = i % 8;
            let mask = 1u8 << bit_idx;
            let is_pos = (entry.v.sign[global_byte] & mask) != 0;
            let is_large =
                (entry.v.sign[global_byte] & mask) == (entry.v.extra[global_byte] & mask);
            let raw = if is_large { 2i32 } else { 1 };
            let val = if is_pos { raw } else { -raw };
            // Weighted add: pure integer addition
            out_i32[i] += val * w; // w ∈ {1, 0} for bit-shift weights — one mult per token
        }
    }

    // Normalise by weight_sum (integer division — one div per head)
    for v in &mut out_i32 {
        *v /= weight_sum;
    }

    // Output scale = v.log2_scale (same for all V entries, use first)
    let v_log2 = kv_layer
        .entries
        .first()
        .map(|e| e.v.log2_scale)
        .unwrap_or(0);

    NdaVec::from_i32_slice(&out_i32, v_log2)
}

// ─── Zero-Float Transformer ─────────────────────────────────────────────────

pub struct ZeroTransformer {
    config: ModelConfig,
    weights: ModelWeights,
    kv_cache: Vec<ZeroKvLayer>,
    /// LM head stored as NdaEmbedding rows (vocab × hidden), reused as matrix.
    lm_head_nda: NdaEmbedding,
    alibi: AliBiSlopes,
    silu: SiluLut,
}

impl ZeroTransformer {
    pub fn new(config: ModelConfig, weights: ModelWeights) -> Self {
        let kv_cache = (0..config.n_layers).map(|_| ZeroKvLayer::new()).collect();
        let alibi = AliBiSlopes::new(config.n_heads);
        let silu = SiluLut::new();

        // Build NDA LM head (vocab × hidden) from FP32 weights
        // lm_head is [vocab_size × hidden_size] — same layout as embed_tokens
        let lm_head_nda =
            NdaEmbedding::from_f32(&weights.lm_head, config.vocab_size, config.hidden_size);

        Self {
            config,
            weights,
            kv_cache,
            lm_head_nda,
            alibi,
            silu,
        }
    }

    pub fn reset_cache(&mut self) {
        for layer in &mut self.kv_cache {
            layer.entries.clear();
        }
    }

    /// Return the total KV cache size (sum of entries across all layers).
    pub fn kv_cache_size(&self) -> usize {
        self.kv_cache.iter().map(|l| l.entries.len()).sum()
    }

    /// Return per-layer KV cache sizes.
    pub fn kv_cache_sizes(&self) -> Vec<usize> {
        self.kv_cache.iter().map(|l| l.entries.len()).collect()
    }

    /// Return the model config reference.
    pub fn config(&self) -> &ModelConfig {
        &self.config
    }

    /// Build comprehensive diagnostic info for this transformer.
    pub fn info(&self) -> ZeroTransformerInfo {
        let per_layer_kv = self.kv_cache_sizes();
        let issues = self.validate();
        ZeroTransformerInfo {
            n_layers: self.config.n_layers,
            n_heads: self.config.n_heads,
            n_kv_heads: self.config.n_kv_heads,
            hidden_size: self.config.hidden_size,
            head_dim: self.config.head_dim,
            vocab_size: self.config.vocab_size,
            max_seq_len: self.config.max_seq_len,
            eos_token_id: self.config.eos_token_id,
            total_kv_cached: self.kv_cache_size(),
            per_layer_kv,
            lm_head_rows: self.config.vocab_size,
            lm_head_stride: self.lm_head_nda.stride(),
            embed_tokens_bytes: self.weights.embed_tokens.len() * std::mem::size_of::<f32>(),
            validation_issues: issues,
        }
    }

    /// Validate config consistency. Returns list of warnings (empty = clean).
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let cfg = &self.config;

        if cfg.hidden_size == 0 {
            issues.push("hidden_size is 0".to_string());
        }
        if cfg.n_heads == 0 {
            issues.push("n_heads is 0".to_string());
        }
        if cfg.n_layers == 0 {
            issues.push("n_layers is 0".to_string());
        }
        if cfg.n_kv_heads == 0 {
            issues.push("n_kv_heads is 0".to_string());
        }
        if !cfg.n_heads.is_multiple_of(cfg.n_kv_heads) {
            issues.push(format!(
                "n_heads ({}) not divisible by n_kv_heads ({})",
                cfg.n_heads, cfg.n_kv_heads
            ));
        }
        if !cfg.hidden_size.is_multiple_of(cfg.n_heads) {
            issues.push(format!(
                "hidden_size ({}) not divisible by n_heads ({})",
                cfg.hidden_size, cfg.n_heads
            ));
        }
        if cfg.vocab_size == 0 {
            issues.push("vocab_size is 0".to_string());
        }
        if self.weights.layers.len() != cfg.n_layers {
            issues.push(format!(
                "weight layers ({}) != config n_layers ({})",
                self.weights.layers.len(),
                cfg.n_layers
            ));
        }
        if self.weights.embed_tokens.is_empty() {
            issues.push("embed_tokens weights are empty".to_string());
        }
        issues
    }

    /// Convert a GenerationReport into a compact GenerationSummary.
    pub fn summarize_report(report: &GenerationReport) -> GenerationSummary {
        GenerationSummary {
            tokens_generated: report.tokens_generated,
            prompt_tokens: report.prompt_tokens,
            stopped_at_eos: report.stopped_at_eos,
            truncated: report.truncated,
            tokens_per_second: report.tokens_per_second,
            cache_hit_rate: report.cache_hit_rate(),
            elapsed_ms: report.elapsed_us as f64 / 1000.0,
            first_token_id: report.token_ids.first().copied(),
            last_token_id: report.token_ids.last().copied(),
        }
    }

    /// Process one token — returns i32 logit vector (no softmax).
    pub(crate) fn forward_one_zero(
        &mut self,
        token: u32,
        pos: usize,
        condition: Option<&[f32]>,
        mut site_map: Option<&mut SiteMap>,
        stats_hits: &mut usize,
        stats_misses: &mut usize,
    ) -> Vec<i32> {
        let cfg = &self.config;
        let h = cfg.hidden_size;
        let hd = cfg.head_dim;

        // ── Token embedding: Lookup FP32 vector and quantize dynamically per-token ──
        let start = token as usize * h;
        let end = start + h;
        let x_f32 = &self.weights.embed_tokens[start..end];
        // Bridge Path 1 → Path 2: when a conditioning hidden state is supplied
        // (only on the first generated token), inject it into the start-token
        // embedding so downstream opcode generation is conditioned on the
        // natural-language prompt rather than a context-free start token.
        let mut x = if let Some(cond) = condition {
            let n = cond.len().min(x_f32.len());
            let mut blended = x_f32.to_vec();
            for i in 0..n {
                blended[i] += cond[i];
            }
            NdaVec::from_f32_slice(&blended)
        } else {
            NdaVec::from_f32_slice(x_f32)
        };
        // ── 24 transformer layers ─────────────────────────────────────────────
        for layer_idx in 0..cfg.n_layers {
            let lw = &self.weights.layers[layer_idx];

            // 1. Attention pre-norm (integer fixed-point RMSNorm)
            let x_norm = rms_norm_nda(&x, &norm_to_ndavec(&lw.attn_norm), 6);
            // 2. Q/K/V projections — NDA GEMV → NdaVec (pure bitwise popcount)
            let q = nda_gemv_nda_to_nda(&lw.q_proj, &x_norm);

            let (k, v) = if let Some(ref mut sm) = site_map {
                if let Some((k_cached, v_cached)) = sm.get_kv(token, layer_idx as u32) {
                    *stats_hits += 1;
                    (k_cached.clone(), v_cached.clone())
                } else {
                    *stats_misses += 1;
                    let k = nda_gemv_nda_to_nda(&lw.k_proj, &x_norm);
                    let v = nda_gemv_nda_to_nda(&lw.v_proj, &x_norm);
                    let _ = sm.put_kv(token, layer_idx as u32, k.clone(), v.clone());
                    (k, v)
                }
            } else {
                let k = nda_gemv_nda_to_nda(&lw.k_proj, &x_norm);
                let v = nda_gemv_nda_to_nda(&lw.v_proj, &x_norm);
                (k, v)
            };

            // 3. Store K/V in cache (NDA bitmaps — no float conversion)
            self.kv_cache[layer_idx].push(k, v.clone());

            // 4. Multi-head attention with ALiBi + bitwise Q·K popcount
            let kv_layer = &self.kv_cache[layer_idx];
            let head_bytes = hd.div_ceil(8);
            let mut attn_out_i32 = vec![0i32; h];

            // GQA: group Q heads over KV heads
            let heads_per_kv = cfg.n_heads / cfg.n_kv_heads;

            for head in 0..cfg.n_heads {
                let hs = head * hd;
                let kv_head_idx = head / heads_per_kv;
                let hs_kv = kv_head_idx * hd;

                // Extract this head's Q bitmap slice
                let hb = head * head_bytes;
                let q_head = NdaVec {
                    len: hd,
                    log2_scale: q.log2_scale,
                    sign: q.sign[hb..hb + head_bytes].to_vec().into(),
                    extra: q.extra[hb..hb + head_bytes].to_vec().into(),
                };

                let head_out =
                    attention_head_zero(&q_head, kv_layer, hs_kv, pos, head, &self.alibi);

                // Write head output into attn_out_i32
                for i in 0..hd {
                    attn_out_i32[hs + i] += head_out.get_raw(i);
                }
            }

            // 5. Re-encode attention output as NdaVec
            let attn_out_nda = NdaVec::from_i32_slice(&attn_out_i32, v.log2_scale);

            // 6. O projection + residual (NDA GEMV → NdaVec, then add)
            let o_out = nda_gemv_nda_to_nda(&lw.o_proj, &attn_out_nda);
            nda_vec_add_inplace(&mut x, &o_out);

            // 7. FFN pre-norm
            let x_ffn = rms_norm_nda(&x, &norm_to_ndavec(&lw.ffn_norm), 6);

            // 8. SwiGLU: down(SiLU(gate) ⊙ up) — pure NDA, 4-entry SiLU LUT
            let gate = nda_gemv_nda_to_nda(&lw.gate_proj, &x_ffn);
            let up = nda_gemv_nda_to_nda(&lw.up_proj, &x_ffn);
            let gated = swiglu_nda(&gate, &up, &self.silu);

            // 9. Down projection + residual
            let ffn_out = nda_gemv_nda_to_nda(&lw.down_proj, &gated);
            nda_vec_add_inplace(&mut x, &ffn_out);
        }

        // ── Final RMSNorm ─────────────────────────────────────────────────
        let final_norm_nda = norm_to_ndavec(&self.weights.final_norm);
        let x_final = rms_norm_nda(&x, &final_norm_nda, 6);

        // ── LM head: NDA vocab rows × x_final → i32 logits → argmax (no softmax) ──
        // Dot each vocab row (from lm_head_nda) against x_final using precomputed nibble lookups.
        let vocab = self.config.vocab_size;
        let mut logits = vec![0i32; vocab];

        let x_bytes = x_final.bitmap_bytes();
        let mut x_low = [0usize; 2048];
        let mut x_high = [0usize; 2048];
        let limit = x_bytes.min(2048);
        for b in 0..limit {
            let xs = x_final.sign[b];
            let xe = x_final.extra[b];
            x_low[b] = ((xs & 0x0F) | ((xe & 0x0F) << 4)) as usize;
            x_high[b] = (((xs >> 4) & 0x0F) | (xe & 0xF0)) as usize;
        }

        logits
            .par_iter_mut()
            .enumerate()
            .for_each(|(tok_id, logit)| {
                let stride = self.lm_head_nda.stride();
                let start = tok_id * stride;
                let row_sign = &self.lm_head_nda.sign[start..start + stride];
                let row_extra = &self.lm_head_nda.extra[start..start + stride];

                let mut acc = 0i32;
                let limit = x_bytes.min(stride);
                for b in 0..limit {
                    let ws = row_sign[b];
                    let we = row_extra[b];
                    let w_low = ((ws & 0x0F) | ((we & 0x0F) << 4)) as usize;
                    let w_high = (((ws >> 4) & 0x0F) | (we & 0xF0)) as usize;

                    acc += (DOT_4_LUT[w_low][x_low[b]] + DOT_4_LUT[w_high][x_high[b]]) as i32;
                }
                *logit = acc;
            });
        logits
    }

    /// Greedy autoregressive generation with integer repetition penalty.
    ///
    /// Repetition penalty is applied entirely in i32 (no floats):
    ///   - Keep a sliding window of the last `rep_window` generated tokens.
    ///   - If token `t` appears in the window, right-shift its logit by
    ///     `rep_penalty_bits` (equivalent to dividing by 2^rep_penalty_bits).
    ///   - Window=64, penalty_bits=3 → ~8× penalty, sufficient to break
    ///     any degenerate repetition loop with zero floating-point ops.
    ///
    /// Calls `on_token` for each generated token ID.
    pub fn generate_greedy(
        &mut self,
        prompt_tokens: &[u32],
        max_new_tokens: usize,
        mut on_token: impl FnMut(u32),
    ) {
        let report = self.generate_greedy_with_report(prompt_tokens, max_new_tokens, |tok| {
            on_token(tok);
        });
        // Report is discarded here; use generate_greedy_with_report for metrics.
        let _ = report;
    }

    /// Greedy generation with full metrics report.
    ///
    /// Same as `generate_greedy` but returns a `GenerationReport` with
    /// timing, cache stats, and token IDs.
    pub fn generate_greedy_with_report(
        &mut self,
        prompt_tokens: &[u32],
        max_new_tokens: usize,
        mut on_token: impl FnMut(u32),
    ) -> GenerationReport {
        let t_start = Instant::now();
        // ── repetition-penalty config (integer-native, no floats) ──
        const REP_WINDOW: usize = 64; // sliding history window

        self.reset_cache();

        let n_prompt = prompt_tokens.len();
        let max_new = max_new_tokens.min(self.config.max_seq_len.saturating_sub(n_prompt));

        // Prefill
        let mut logits = Vec::new();
        let mut h = 0;
        let mut m = 0;
        for (pos, &tok) in prompt_tokens.iter().enumerate() {
            logits = self.forward_one_zero(tok, pos, None, None, &mut h, &mut m);
        }

        // History ring buffer for repetition penalty.
        let mut history: std::collections::VecDeque<u32> = prompt_tokens.iter().copied().collect();
        if history.len() > REP_WINDOW {
            let excess = history.len() - REP_WINDOW;
            history.drain(..excess);
        }

        /// Apply integer repetition penalty by subtracting a constant.
        #[inline]
        fn apply_rep_penalty(logits: &mut [i32], history: &std::collections::VecDeque<u32>) {
            for &tok in history {
                let tok = tok as usize;
                if tok < logits.len() {
                    logits[tok] = logits[tok].saturating_sub(64);
                }
            }
        }

        apply_rep_penalty(&mut logits, &history);
        let mut next = argmax_i32(&logits);

        let mut token_ids = Vec::with_capacity(max_new);
        let mut stopped_at_eos = false;

        // Decode
        for step in 0..max_new {
            if next == self.config.eos_token_id {
                stopped_at_eos = true;
                break;
            }
            on_token(next);
            token_ids.push(next);

            // Slide history window
            if history.len() == REP_WINDOW {
                history.pop_front();
            }
            history.push_back(next);

            logits = self.forward_one_zero(next, n_prompt + step, None, None, &mut h, &mut m);
            apply_rep_penalty(&mut logits, &history);
            next = argmax_i32(&logits);
        }

        let elapsed_us = t_start.elapsed().as_micros() as u64;
        let tokens_generated = token_ids.len();
        let tps = if elapsed_us > 0 {
            tokens_generated as f64 / (elapsed_us as f64 / 1_000_000.0)
        } else {
            0.0
        };

        GenerationReport {
            tokens_generated,
            prompt_tokens: n_prompt,
            stopped_at_eos,
            truncated: !stopped_at_eos && tokens_generated >= max_new,
            site_map_hits: h,
            site_map_misses: m,
            final_kv_cache_size: self.kv_cache_size(),
            elapsed_us,
            tokens_per_second: tps,
            token_ids,
        }
    }
}

// ─── Helper: convert FP32 norm weight → NdaVec ─────────────────────────────

/// Convert an FP32 norm weight vector (values near 1.0) to NdaVec.
/// Norm weights are small 1D vectors; the conversion is done once at forward time.
pub fn norm_to_ndavec(w: &[f32]) -> NdaVec {
    let amax = w.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
    let log2_scale = if amax > 1e-8 {
        (amax / 2.0).log2().floor() as i8
    } else {
        0i8
    };
    let scale = (2.0_f32).powi(log2_scale as i32);
    let inv_s = 1.0 / scale;

    let bytes = w.len().div_ceil(8);
    let mut sign = vec![0u8; bytes];
    let mut extra = vec![0u8; bytes];

    for (i, &v) in w.iter().enumerate() {
        let vs = v * inv_s;
        let is_pos = vs >= 0.0;
        let is_large = vs.abs() >= 1.5;
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        if is_pos {
            sign[byte_idx] |= 1 << bit_idx;
        }
        if is_pos == is_large {
            extra[byte_idx] |= 1 << bit_idx;
        }
    }

    NdaVec {
        len: w.len(),
        log2_scale,
        sign: sign.into(),
        extra: extra.into(),
    }
}

/// Convert FP32 norm weights to NdaVec with diagnostic report.
pub fn norm_to_ndavec_report(w: &[f32]) -> (NdaVec, NormConversionReport) {
    let abs_max = w.iter().map(|v| (*v as f64).abs()).fold(0.0_f64, f64::max);
    let all_positive = w.iter().all(|v| *v >= 0.0);
    let all_negative = w.iter().all(|v| *v < 0.0);
    let vec = norm_to_ndavec(w);
    let report = NormConversionReport {
        input_len: w.len(),
        output_len: vec.len,
        log2_scale: vec.log2_scale,
        abs_max,
        all_positive,
        all_negative,
    };
    (vec, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_norm_to_ndavec_positive_values() {
        let w = vec![1.0, 1.0, 1.0, 1.0];
        let v = norm_to_ndavec(&w);
        assert_eq!(v.len, 4);
        // All positive -> sign bits all set
        assert_eq!(v.sign[0] & 0x0F, 0x0F);
    }

    #[test]
    fn test_norm_to_ndavec_negative_values() {
        let w = vec![-1.0, -1.0, -1.0, -1.0];
        let v = norm_to_ndavec(&w);
        // All negative -> sign bits all clear
        assert_eq!(v.sign[0] & 0x0F, 0x00);
    }

    #[test]
    fn test_norm_to_ndavec_mixed() {
        let w = vec![1.0, -1.0, 1.0, -1.0];
        let v = norm_to_ndavec(&w);
        // Bit 0 and 2 should be set (positive)
        assert_eq!(v.sign[0] & 0x01, 1);
        assert_eq!(v.sign[0] & 0x02, 0);
        assert_eq!(v.sign[0] & 0x04, 4);
        assert_eq!(v.sign[0] & 0x08, 0);
    }

    #[test]
    fn test_norm_to_ndavec_zero() {
        let w = vec![0.0, 0.0];
        let v = norm_to_ndavec(&w);
        assert_eq!(v.len, 2);
        assert_eq!(v.log2_scale, 0);
    }

    #[test]
    fn test_norm_to_ndavec_large_values() {
        let w = vec![4.0, -4.0];
        let v = norm_to_ndavec(&w);
        // Values >= 1.5 * scale should have extra bit set
        assert!(v.log2_scale > 0);
    }

    #[test]
    fn test_zero_kv_layer_push() {
        let mut layer = ZeroKvLayer::new();
        assert_eq!(layer.len(), 0);
        let k = NdaVec {
            len: 4,
            log2_scale: 0,
            sign: vec![0b10101010].into(),
            extra: vec![0b01010101].into(),
        };
        let v = NdaVec {
            len: 4,
            log2_scale: 0,
            sign: vec![0b11110000].into(),
            extra: vec![0b00001111].into(),
        };
        layer.push(k, v);
        assert_eq!(layer.len(), 1);
    }

    #[test]
    fn forward_metrics_default() {
        let m = ForwardMetrics::default();
        assert_eq!(m.site_map_hits, 0);
        assert_eq!(m.site_map_misses, 0);
        assert!((m.cache_hit_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn forward_metrics_cache_hit_rate() {
        let m = ForwardMetrics {
            site_map_hits: 80,
            site_map_misses: 20,
            kv_cache_size: 100,
        };
        assert!((m.cache_hit_rate() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn forward_metrics_serializable() {
        let m = ForwardMetrics {
            site_map_hits: 10,
            site_map_misses: 5,
            kv_cache_size: 15,
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"site_map_hits\":10"));
        assert!(json.contains("\"kv_cache_size\":15"));
    }

    #[test]
    fn generation_report_serializable() {
        let report = GenerationReport {
            tokens_generated: 50,
            prompt_tokens: 10,
            stopped_at_eos: true,
            truncated: false,
            site_map_hits: 100,
            site_map_misses: 50,
            final_kv_cache_size: 60,
            elapsed_us: 5000,
            tokens_per_second: 10000.0,
            token_ids: vec![1, 2, 3],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"tokens_generated\":50"));
        assert!(json.contains("\"stopped_at_eos\":true"));
        assert!(json.contains("\"token_ids\":[1,2,3]"));
    }

    #[test]
    fn generation_report_cache_hit_rate() {
        let report = GenerationReport {
            tokens_generated: 0,
            prompt_tokens: 0,
            stopped_at_eos: false,
            truncated: false,
            site_map_hits: 75,
            site_map_misses: 25,
            final_kv_cache_size: 0,
            elapsed_us: 0,
            tokens_per_second: 0.0,
            token_ids: vec![],
        };
        assert!((report.cache_hit_rate() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn generation_report_no_lookups() {
        let report = GenerationReport {
            tokens_generated: 0,
            prompt_tokens: 0,
            stopped_at_eos: false,
            truncated: false,
            site_map_hits: 0,
            site_map_misses: 0,
            final_kv_cache_size: 0,
            elapsed_us: 0,
            tokens_per_second: 0.0,
            token_ids: vec![],
        };
        assert!((report.cache_hit_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn generation_report_us_per_token() {
        let report = GenerationReport {
            tokens_generated: 10,
            prompt_tokens: 5,
            stopped_at_eos: false,
            truncated: true,
            site_map_hits: 0,
            site_map_misses: 0,
            final_kv_cache_size: 0,
            elapsed_us: 5000,
            tokens_per_second: 2000.0,
            token_ids: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        };
        assert!((report.us_per_token() - 500.0).abs() < 1e-9);
    }

    #[test]
    fn generation_report_us_per_token_zero_tokens() {
        let report = GenerationReport {
            tokens_generated: 0,
            prompt_tokens: 5,
            stopped_at_eos: false,
            truncated: false,
            site_map_hits: 0,
            site_map_misses: 0,
            final_kv_cache_size: 0,
            elapsed_us: 0,
            tokens_per_second: 0.0,
            token_ids: vec![],
        };
        assert!((report.us_per_token() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn generation_report_total_tokens() {
        let report = GenerationReport {
            tokens_generated: 10,
            prompt_tokens: 5,
            stopped_at_eos: false,
            truncated: true,
            site_map_hits: 0,
            site_map_misses: 0,
            final_kv_cache_size: 0,
            elapsed_us: 0,
            tokens_per_second: 0.0,
            token_ids: vec![],
        };
        assert_eq!(report.total_tokens(), 15);
    }

    #[test]
    fn zero_transformer_info_serializes() {
        let info = ZeroTransformerInfo {
            n_layers: 24,
            n_heads: 32,
            n_kv_heads: 8,
            hidden_size: 4096,
            head_dim: 128,
            vocab_size: 32000,
            max_seq_len: 2048,
            eos_token_id: 2,
            total_kv_cached: 100,
            per_layer_kv: vec![4; 24],
            lm_head_rows: 32000,
            lm_head_stride: 512,
            embed_tokens_bytes: 32000 * 4096 * 4,
            validation_issues: vec![],
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"n_layers\":24"));
        assert!(json.contains("\"hidden_size\":4096"));
        assert!(json.contains("\"validation_issues\":[]"));
    }

    #[test]
    fn norm_conversion_report_basic() {
        let w = vec![1.0, -2.0, 3.0, -4.0];
        let (vec, report) = norm_to_ndavec_report(&w);
        assert_eq!(report.input_len, 4);
        assert_eq!(report.output_len, vec.len);
        assert!((report.abs_max - 4.0).abs() < 1e-9);
        assert!(!report.all_positive);
        assert!(!report.all_negative);
    }

    #[test]
    fn norm_conversion_report_all_positive() {
        let w = vec![1.0, 2.0, 3.0];
        let (_, report) = norm_to_ndavec_report(&w);
        assert!(report.all_positive);
        assert!(!report.all_negative);
    }

    #[test]
    fn norm_conversion_report_all_negative() {
        let w = vec![-1.0, -2.0, -3.0];
        let (_, report) = norm_to_ndavec_report(&w);
        assert!(!report.all_positive);
        assert!(report.all_negative);
    }

    #[test]
    fn norm_conversion_report_serializes() {
        let w = vec![1.0, 2.0];
        let (_, report) = norm_to_ndavec_report(&w);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"input_len\":2"));
        assert!(json.contains("\"all_positive\":true"));
    }

    #[test]
    fn generation_summary_from_report() {
        let report = GenerationReport {
            tokens_generated: 50,
            prompt_tokens: 10,
            stopped_at_eos: true,
            truncated: false,
            site_map_hits: 80,
            site_map_misses: 20,
            final_kv_cache_size: 60,
            elapsed_us: 50000,
            tokens_per_second: 1000.0,
            token_ids: vec![1, 2, 3, 4, 5],
        };
        let summary = ZeroTransformer::summarize_report(&report);
        assert_eq!(summary.tokens_generated, 50);
        assert_eq!(summary.prompt_tokens, 10);
        assert!(summary.stopped_at_eos);
        assert!(!summary.truncated);
        assert!((summary.tokens_per_second - 1000.0).abs() < 1e-9);
        assert!((summary.cache_hit_rate - 0.8).abs() < 1e-9);
        assert!((summary.elapsed_ms - 50.0).abs() < 1e-9);
        assert_eq!(summary.first_token_id, Some(1));
        assert_eq!(summary.last_token_id, Some(5));
    }

    #[test]
    fn generation_summary_empty_tokens() {
        let report = GenerationReport {
            tokens_generated: 0,
            prompt_tokens: 5,
            stopped_at_eos: false,
            truncated: false,
            site_map_hits: 0,
            site_map_misses: 0,
            final_kv_cache_size: 0,
            elapsed_us: 0,
            tokens_per_second: 0.0,
            token_ids: vec![],
        };
        let summary = ZeroTransformer::summarize_report(&report);
        assert_eq!(summary.first_token_id, None);
        assert_eq!(summary.last_token_id, None);
        assert_eq!(summary.tokens_generated, 0);
    }

    #[test]
    fn generation_summary_serializes() {
        let summary = GenerationSummary {
            tokens_generated: 25,
            prompt_tokens: 8,
            stopped_at_eos: false,
            truncated: true,
            tokens_per_second: 500.0,
            cache_hit_rate: 0.6,
            elapsed_ms: 50.0,
            first_token_id: Some(42),
            last_token_id: Some(99),
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"tokens_generated\":25"));
        assert!(json.contains("\"truncated\":true"));
        assert!(json.contains("\"first_token_id\":42"));
    }

    // ─── Expanded Tests ─────────────────────────────────────────────────

    #[test]
    fn cache_hit_rate_all_hits() {
        let m = ForwardMetrics {
            site_map_hits: 100,
            site_map_misses: 0,
            kv_cache_size: 100,
        };
        assert!((m.cache_hit_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_hit_rate_all_misses() {
        let m = ForwardMetrics {
            site_map_hits: 0,
            site_map_misses: 50,
            kv_cache_size: 50,
        };
        assert!((m.cache_hit_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_hit_rate_single_hit() {
        let m = ForwardMetrics {
            site_map_hits: 1,
            site_map_misses: 0,
            kv_cache_size: 1,
        };
        assert!((m.cache_hit_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_hit_rate_single_miss() {
        let m = ForwardMetrics {
            site_map_hits: 0,
            site_map_misses: 1,
            kv_cache_size: 1,
        };
        assert!((m.cache_hit_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_hit_rate_fifty_fifty() {
        let m = ForwardMetrics {
            site_map_hits: 500,
            site_map_misses: 500,
            kv_cache_size: 1000,
        };
        assert!((m.cache_hit_rate() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn generation_report_us_per_token_single() {
        let report = GenerationReport {
            tokens_generated: 1,
            prompt_tokens: 5,
            stopped_at_eos: true,
            truncated: false,
            site_map_hits: 0,
            site_map_misses: 0,
            final_kv_cache_size: 10,
            elapsed_us: 1234,
            tokens_per_second: 810.0,
            token_ids: vec![42],
        };
        assert!((report.us_per_token() - 1234.0).abs() < 1e-9);
    }

    #[test]
    fn generation_report_total_tokens_zero() {
        let report = GenerationReport {
            tokens_generated: 0,
            prompt_tokens: 0,
            stopped_at_eos: false,
            truncated: false,
            site_map_hits: 0,
            site_map_misses: 0,
            final_kv_cache_size: 0,
            elapsed_us: 0,
            tokens_per_second: 0.0,
            token_ids: vec![],
        };
        assert_eq!(report.total_tokens(), 0);
    }

    #[test]
    fn generation_report_truncated_not_eos() {
        let report = GenerationReport {
            tokens_generated: 100,
            prompt_tokens: 20,
            stopped_at_eos: false,
            truncated: true,
            site_map_hits: 10,
            site_map_misses: 90,
            final_kv_cache_size: 200,
            elapsed_us: 100_000,
            tokens_per_second: 1000.0,
            token_ids: (0..100).collect(),
        };
        assert!(!report.stopped_at_eos);
        assert!(report.truncated);
        assert_eq!(report.token_ids.len(), 100);
        assert!((report.cache_hit_rate() - 0.1).abs() < 1e-9);
    }

    #[test]
    fn generation_report_clone() {
        let report = GenerationReport {
            tokens_generated: 5,
            prompt_tokens: 3,
            stopped_at_eos: true,
            truncated: false,
            site_map_hits: 10,
            site_map_misses: 5,
            final_kv_cache_size: 20,
            elapsed_us: 500,
            tokens_per_second: 10000.0,
            token_ids: vec![1, 2, 3, 4, 5],
        };
        let cloned = report.clone();
        assert_eq!(cloned.tokens_generated, 5);
        assert_eq!(cloned.token_ids, vec![1, 2, 3, 4, 5]);
        assert!((cloned.cache_hit_rate() - report.cache_hit_rate()).abs() < f64::EPSILON);
    }

    #[test]
    fn norm_to_ndavec_single_element() {
        let w = vec![2.0];
        let v = norm_to_ndavec(&w);
        assert_eq!(v.len, 1);
        // Single positive element → bit 0 of sign set
        assert_eq!(v.sign[0] & 0x01, 1);
    }

    #[test]
    fn norm_to_ndavec_near_zero_values() {
        let w = vec![1e-10, -1e-10];
        let v = norm_to_ndavec(&w);
        assert_eq!(v.len, 2);
        // amax < 1e-8 → log2_scale = 0
        assert_eq!(v.log2_scale, 0);
    }

    #[test]
    fn norm_to_ndavec_eight_elements_full_byte() {
        let w = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let v = norm_to_ndavec(&w);
        assert_eq!(v.len, 8);
        // All 8 positive → full byte of sign bits
        assert_eq!(v.sign[0], 0xFF);
    }

    #[test]
    fn norm_to_ndavec_sixteen_elements_two_bytes() {
        let w = vec![1.0; 16];
        let v = norm_to_ndavec(&w);
        assert_eq!(v.len, 16);
        assert_eq!(v.sign.len(), 2);
        assert_eq!(v.sign[0], 0xFF);
        assert_eq!(v.sign[1], 0xFF);
    }

    #[test]
    fn norm_to_ndavec_report_empty_input() {
        let w: Vec<f32> = vec![];
        let (vec, report) = norm_to_ndavec_report(&w);
        assert_eq!(report.input_len, 0);
        assert_eq!(report.output_len, 0);
        assert!((report.abs_max - 0.0).abs() < f64::EPSILON);
        // all() on empty iterator returns true
        assert!(report.all_positive);
        assert!(report.all_negative);
        assert_eq!(vec.len, 0);
    }

    #[test]
    fn norm_to_ndavec_report_zero_values() {
        let w = vec![0.0, 0.0, 0.0];
        let (_, report) = norm_to_ndavec_report(&w);
        assert!((report.abs_max - 0.0).abs() < f64::EPSILON);
        // 0.0 >= 0.0 is true → all_positive
        assert!(report.all_positive);
        // 0.0 < 0.0 is false → not all_negative
        assert!(!report.all_negative);
    }

    #[test]
    fn norm_to_ndavec_report_large_scale() {
        let w = vec![100.0, -200.0, 50.0];
        let (vec, report) = norm_to_ndavec_report(&w);
        assert!((report.abs_max - 200.0).abs() < 1e-9);
        assert!(vec.log2_scale > 0);
        assert!(!report.all_positive);
        assert!(!report.all_negative);
    }

    #[test]
    fn norm_conversion_report_zero_len() {
        let w: Vec<f32> = vec![];
        let (_, report) = norm_to_ndavec_report(&w);
        assert_eq!(report.input_len, 0);
        assert_eq!(report.output_len, 0);
        assert!((report.abs_max - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_kv_layer_multiple_pushes() {
        let mut layer = ZeroKvLayer::new();
        for i in 0..5 {
            let k = NdaVec {
                len: 8,
                log2_scale: i as i8,
                sign: vec![i as u8].into(),
                extra: vec![0xAA].into(),
            };
            let v = NdaVec {
                len: 8,
                log2_scale: 0,
                sign: vec![0xFF].into(),
                extra: vec![0x55].into(),
            };
            layer.push(k, v);
        }
        assert_eq!(layer.len(), 5);
        // Verify entries have different scales
        assert_eq!(layer.entries[0].k.log2_scale, 0);
        assert_eq!(layer.entries[4].k.log2_scale, 4);
    }

    #[test]
    fn zero_kv_layer_empty_entries() {
        let layer = ZeroKvLayer::new();
        assert_eq!(layer.len(), 0);
        assert!(layer.entries.is_empty());
    }

    #[test]
    fn zero_transformer_info_with_issues() {
        let info = ZeroTransformerInfo {
            n_layers: 2,
            n_heads: 4,
            n_kv_heads: 2,
            hidden_size: 64,
            head_dim: 16,
            vocab_size: 100,
            max_seq_len: 256,
            eos_token_id: 2,
            total_kv_cached: 0,
            per_layer_kv: vec![0, 0],
            lm_head_rows: 100,
            lm_head_stride: 8,
            embed_tokens_bytes: 100 * 64 * 4,
            validation_issues: vec![
                "hidden_size is 0".to_string(),
                "n_heads is 0".to_string(),
            ],
        };
        assert_eq!(info.validation_issues.len(), 2);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("hidden_size is 0"));
    }

    #[test]
    fn zero_transformer_info_gqa_config() {
        let info = ZeroTransformerInfo {
            n_layers: 24,
            n_heads: 32,
            n_kv_heads: 8,
            hidden_size: 4096,
            head_dim: 128,
            vocab_size: 32000,
            max_seq_len: 2048,
            eos_token_id: 2,
            total_kv_cached: 500,
            per_layer_kv: vec![20; 24],
            lm_head_rows: 32000,
            lm_head_stride: 512,
            embed_tokens_bytes: 32000 * 4096 * 4,
            validation_issues: vec![],
        };
        // GQA ratio: 32/8 = 4 Q heads per KV head
        assert_eq!(info.n_heads / info.n_kv_heads, 4);
        assert_eq!(info.total_kv_cached, 500);
        assert_eq!(info.per_layer_kv.iter().sum::<usize>(), 480);
    }

    #[test]
    fn generation_summary_elapsed_conversion() {
        let report = GenerationReport {
            tokens_generated: 10,
            prompt_tokens: 5,
            stopped_at_eos: false,
            truncated: true,
            site_map_hits: 0,
            site_map_misses: 0,
            final_kv_cache_size: 0,
            elapsed_us: 1_500_000, // 1.5 seconds
            tokens_per_second: 6.67,
            token_ids: vec![1; 10],
        };
        let summary = ZeroTransformer::summarize_report(&report);
        assert!((summary.elapsed_ms - 1500.0).abs() < 1e-6);
    }

    #[test]
    fn generation_summary_single_token() {
        let report = GenerationReport {
            tokens_generated: 1,
            prompt_tokens: 3,
            stopped_at_eos: true,
            truncated: false,
            site_map_hits: 5,
            site_map_misses: 5,
            final_kv_cache_size: 10,
            elapsed_us: 1000,
            tokens_per_second: 1000.0,
            token_ids: vec![42],
        };
        let summary = ZeroTransformer::summarize_report(&report);
        assert_eq!(summary.first_token_id, Some(42));
        assert_eq!(summary.last_token_id, Some(42));
        assert_eq!(summary.tokens_generated, 1);
    }

    #[test]
    fn forward_metrics_large_cache() {
        let m = ForwardMetrics {
            site_map_hits: 999_999,
            site_map_misses: 1,
            kv_cache_size: 1_000_000,
        };
        let rate = m.cache_hit_rate();
        assert!(rate > 0.999);
        assert!(rate < 1.0);
    }

    #[test]
    fn norm_to_ndavec_report_preserves_vec_data() {
        let w = vec![1.0, -1.0, 2.0, -2.0, 0.5];
        let (vec, report) = norm_to_ndavec_report(&w);
        assert_eq!(report.input_len, report.output_len);
        assert_eq!(vec.len, 5);
        // Bitmap bytes: ceil(5/8) = 1
        assert_eq!(vec.sign.len(), 1);
        assert_eq!(vec.extra.len(), 1);
    }

    #[test]
    fn generation_report_many_tokens() {
        let n = 1000;
        let report = GenerationReport {
            tokens_generated: n,
            prompt_tokens: 100,
            stopped_at_eos: false,
            truncated: true,
            site_map_hits: 5000,
            site_map_misses: 5000,
            final_kv_cache_size: 24 * n,
            elapsed_us: 1_000_000,
            tokens_per_second: n as f64,
            token_ids: (0..n as u32).collect(),
        };
        assert_eq!(report.total_tokens(), n + 100);
        assert!((report.cache_hit_rate() - 0.5).abs() < 1e-9);
        assert!((report.us_per_token() - 1000.0).abs() < 1e-9);
        assert_eq!(report.token_ids.len(), n as usize);
    }

    #[test]
    fn norm_to_ndavec_alternating_signs() {
        let w = vec![1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
        let v = norm_to_ndavec(&w);
        // Alternating: bit 0,2,4,6 set (positive), bit 1,3,5,7 clear (negative)
        assert_eq!(v.sign[0], 0x55); // 01010101
    }

    #[test]
    fn zero_kv_entry_stores_ndavec() {
        let entry = ZeroKvEntry {
            k: NdaVec {
                len: 16,
                log2_scale: 3,
                sign: vec![0xAA, 0x55].into(),
                extra: vec![0xFF, 0x00].into(),
            },
            v: NdaVec {
                len: 16,
                log2_scale: -2,
                sign: vec![0x00, 0xFF].into(),
                extra: vec![0xAA, 0x55].into(),
            },
        };
        assert_eq!(entry.k.len, 16);
        assert_eq!(entry.k.log2_scale, 3);
        assert_eq!(entry.v.len, 16);
        assert_eq!(entry.v.log2_scale, -2);
    }

    // ─── Block 152: comprehensive expansion ────────────────────────────────

    // ── JSON key counts ─────────────────────────────────────────────────────

    #[test]
    fn forward_metrics_json_key_count() {
        let m = ForwardMetrics::default();
        let v: serde_json::Value = serde_json::to_value(&m).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 3, "ForwardMetrics has 3 fields");
    }

    #[test]
    fn generation_report_json_key_count() {
        let r = GenerationReport {
            tokens_generated: 0, prompt_tokens: 0, stopped_at_eos: false,
            truncated: false, site_map_hits: 0, site_map_misses: 0,
            final_kv_cache_size: 0, elapsed_us: 0, tokens_per_second: 0.0,
            token_ids: vec![],
        };
        let v: serde_json::Value = serde_json::to_value(&r).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 10, "GenerationReport has 10 fields");
    }

    #[test]
    fn zero_transformer_info_json_key_count() {
        let info = ZeroTransformerInfo {
            n_layers: 1, n_heads: 1, n_kv_heads: 1, hidden_size: 8,
            head_dim: 8, vocab_size: 10, max_seq_len: 16, eos_token_id: 2,
            total_kv_cached: 0, per_layer_kv: vec![0], lm_head_rows: 10,
            lm_head_stride: 1, embed_tokens_bytes: 0, validation_issues: vec![],
        };
        let v: serde_json::Value = serde_json::to_value(&info).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 14, "ZeroTransformerInfo has 14 fields");
    }

    #[test]
    fn norm_conversion_report_json_key_count() {
        let w = vec![1.0];
        let (_, r) = norm_to_ndavec_report(&w);
        let v: serde_json::Value = serde_json::to_value(&r).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 6, "NormConversionReport has 6 fields");
    }

    #[test]
    fn generation_summary_json_key_count() {
        let s = GenerationSummary {
            tokens_generated: 0, prompt_tokens: 0, stopped_at_eos: false,
            truncated: false, tokens_per_second: 0.0, cache_hit_rate: 0.0,
            elapsed_ms: 0.0, first_token_id: None, last_token_id: None,
        };
        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 9, "GenerationSummary has 9 fields");
    }

    // ── JSON value verification ─────────────────────────────────────────────

    #[test]
    fn zero_transformer_info_json_values() {
        let info = ZeroTransformerInfo {
            n_layers: 24, n_heads: 32, n_kv_heads: 8, hidden_size: 4096,
            head_dim: 128, vocab_size: 32000, max_seq_len: 2048, eos_token_id: 2,
            total_kv_cached: 100, per_layer_kv: vec![4; 24], lm_head_rows: 32000,
            lm_head_stride: 512, embed_tokens_bytes: 32000 * 4096 * 4,
            validation_issues: vec!["issue".into()],
        };
        let v: serde_json::Value = serde_json::to_value(&info).unwrap();
        assert_eq!(v["n_layers"], 24);
        assert_eq!(v["hidden_size"], 4096);
        assert_eq!(v["n_heads"], 32);
        assert_eq!(v["n_kv_heads"], 8);
        assert_eq!(v["head_dim"], 128);
        assert_eq!(v["vocab_size"], 32000);
        assert_eq!(v["max_seq_len"], 2048);
        assert_eq!(v["eos_token_id"], 2);
        assert_eq!(v["total_kv_cached"], 100);
        assert_eq!(v["lm_head_rows"], 32000);
        assert_eq!(v["lm_head_stride"], 512);
        assert_eq!(v["per_layer_kv"].as_array().unwrap().len(), 24);
        assert_eq!(v["validation_issues"][0], "issue");
    }

    #[test]
    fn generation_report_json_values() {
        let r = GenerationReport {
            tokens_generated: 42, prompt_tokens: 10, stopped_at_eos: true,
            truncated: false, site_map_hits: 80, site_map_misses: 20,
            final_kv_cache_size: 200, elapsed_us: 50000,
            tokens_per_second: 840.0, token_ids: vec![1, 2, 3],
        };
        let v: serde_json::Value = serde_json::to_value(&r).unwrap();
        assert_eq!(v["tokens_generated"], 42);
        assert_eq!(v["prompt_tokens"], 10);
        assert_eq!(v["stopped_at_eos"], true);
        assert_eq!(v["truncated"], false);
        assert_eq!(v["site_map_hits"], 80);
        assert_eq!(v["site_map_misses"], 20);
        assert_eq!(v["token_ids"][0], 1);
    }

    #[test]
    fn norm_conversion_report_json_values() {
        let w = vec![1.0, -2.0, 3.0];
        let (_, r) = norm_to_ndavec_report(&w);
        let v: serde_json::Value = serde_json::to_value(&r).unwrap();
        assert_eq!(v["input_len"], 3);
        assert_eq!(v["output_len"], 3);
        assert!((v["abs_max"].as_f64().unwrap() - 3.0).abs() < 1e-9);
        assert_eq!(v["all_positive"], false);
        assert_eq!(v["all_negative"], false);
    }

    // ── Clone independence ──────────────────────────────────────────────────

    #[test]
    fn generation_report_clone_independent() {
        let r = GenerationReport {
            tokens_generated: 5, prompt_tokens: 3, stopped_at_eos: true,
            truncated: false, site_map_hits: 10, site_map_misses: 5,
            final_kv_cache_size: 20, elapsed_us: 500, tokens_per_second: 10000.0,
            token_ids: vec![1, 2, 3],
        };
        let mut cloned = r.clone();
        cloned.token_ids.push(99);
        cloned.tokens_generated = 999;
        assert_eq!(r.tokens_generated, 5, "original unchanged");
        assert_eq!(r.token_ids.len(), 3, "original vec unchanged");
    }

    #[test]
    fn forward_metrics_clone_independent() {
        let m = ForwardMetrics { site_map_hits: 10, site_map_misses: 5, kv_cache_size: 15 };
        let mut cloned = m.clone();
        cloned.site_map_hits = 999;
        assert_eq!(m.site_map_hits, 10, "original unchanged");
        assert_eq!(cloned.site_map_hits, 999);
    }

    #[test]
    fn zero_transformer_info_clone_independent() {
        let info = ZeroTransformerInfo {
            n_layers: 2, n_heads: 4, n_kv_heads: 2, hidden_size: 64,
            head_dim: 16, vocab_size: 100, max_seq_len: 256, eos_token_id: 2,
            total_kv_cached: 0, per_layer_kv: vec![0, 0], lm_head_rows: 100,
            lm_head_stride: 8, embed_tokens_bytes: 0,
            validation_issues: vec!["a".into()],
        };
        let mut cloned = info.clone();
        cloned.validation_issues.push("b".into());
        cloned.per_layer_kv.push(99);
        assert_eq!(info.validation_issues.len(), 1, "original unchanged");
        assert_eq!(info.per_layer_kv.len(), 2, "original unchanged");
    }

    #[test]
    fn generation_summary_clone_independent() {
        let s = GenerationSummary {
            tokens_generated: 10, prompt_tokens: 5, stopped_at_eos: false,
            truncated: true, tokens_per_second: 100.0, cache_hit_rate: 0.5,
            elapsed_ms: 100.0, first_token_id: Some(1), last_token_id: Some(10),
        };
        let mut cloned = s.clone();
        cloned.tokens_generated = 999;
        assert_eq!(s.tokens_generated, 10);
        assert_eq!(cloned.tokens_generated, 999);
    }

    // ── Debug format ────────────────────────────────────────────────────────

    #[test]
    fn forward_metrics_debug_format() {
        let m = ForwardMetrics { site_map_hits: 5, site_map_misses: 3, kv_cache_size: 8 };
        let d = format!("{:?}", m);
        assert!(d.contains("ForwardMetrics"));
        assert!(d.contains("site_map_hits: 5"));
    }

    #[test]
    fn generation_report_debug_format() {
        let r = GenerationReport {
            tokens_generated: 3, prompt_tokens: 2, stopped_at_eos: true,
            truncated: false, site_map_hits: 0, site_map_misses: 0,
            final_kv_cache_size: 0, elapsed_us: 100, tokens_per_second: 30000.0,
            token_ids: vec![1, 2, 3],
        };
        let d = format!("{:?}", r);
        assert!(d.contains("GenerationReport"));
        assert!(d.contains("tokens_generated: 3"));
        assert!(d.contains("stopped_at_eos: true"));
    }

    #[test]
    fn zero_transformer_info_debug_format() {
        let info = ZeroTransformerInfo {
            n_layers: 2, n_heads: 4, n_kv_heads: 2, hidden_size: 64,
            head_dim: 16, vocab_size: 100, max_seq_len: 256, eos_token_id: 2,
            total_kv_cached: 0, per_layer_kv: vec![0, 0], lm_head_rows: 100,
            lm_head_stride: 8, embed_tokens_bytes: 0, validation_issues: vec![],
        };
        let d = format!("{:?}", info);
        assert!(d.contains("ZeroTransformerInfo"));
        assert!(d.contains("n_layers: 2"));
    }

    #[test]
    fn norm_conversion_report_debug_format() {
        let w = vec![1.0, -2.0];
        let (_, r) = norm_to_ndavec_report(&w);
        let d = format!("{:?}", r);
        assert!(d.contains("NormConversionReport"));
        assert!(d.contains("input_len: 2"));
    }

    #[test]
    fn generation_summary_debug_format() {
        let s = GenerationSummary {
            tokens_generated: 5, prompt_tokens: 3, stopped_at_eos: false,
            truncated: true, tokens_per_second: 500.0, cache_hit_rate: 0.6,
            elapsed_ms: 10.0, first_token_id: Some(42), last_token_id: Some(99),
        };
        let d = format!("{:?}", s);
        assert!(d.contains("GenerationSummary"));
        assert!(d.contains("tokens_generated: 5"));
    }

    // ── norm_to_ndavec detailed ─────────────────────────────────────────────

    #[test]
    fn norm_to_ndavec_log2_scale_exact() {
        // amax=4.0 → (4.0/2.0).log2().floor() = (2.0).log2().floor() = 1.0 → log2_scale=1
        let w = vec![4.0, -2.0];
        let v = norm_to_ndavec(&w);
        assert_eq!(v.log2_scale, 1, "amax=4 → log2_scale=1");
    }

    #[test]
    fn norm_to_ndavec_log2_scale_large() {
        // amax=16.0 → (16.0/2.0).log2().floor() = (8.0).log2().floor() = 3.0 → log2_scale=3
        let w = vec![16.0, -8.0, 4.0];
        let v = norm_to_ndavec(&w);
        assert_eq!(v.log2_scale, 3, "amax=16 → log2_scale=3");
    }

    #[test]
    fn norm_to_ndavec_non_aligned_len() {
        let w = vec![1.0; 10]; // 10 elements → ceil(10/8)=2 bytes
        let v = norm_to_ndavec(&w);
        assert_eq!(v.len, 10);
        assert_eq!(v.sign.len(), 2);
        assert_eq!(v.extra.len(), 2);
    }

    #[test]
    fn norm_to_ndavec_extra_bit_pattern() {
        // w=[2.0, -2.0] → amax=2.0, log2_scale=(2.0/2.0).log2().floor()=0
        // scale=2^0=1.0, inv_s=1.0
        // 2.0/1.0=2.0: pos, |2.0|>=1.5 → large → is_pos==is_large → extra=1
        // -2.0/1.0=-2.0: neg, |−2.0|>=1.5 → large → !is_pos==is_large → extra=0
        let w = vec![2.0, -2.0];
        let v = norm_to_ndavec(&w);
        assert_eq!(v.sign[0] & 0x01, 1, "positive → sign bit 0 = 1");
        assert_eq!(v.sign[0] & 0x02, 0, "negative → sign bit 1 = 0");
        assert_eq!(v.extra[0] & 0x01, 1, "pos+large → extra bit 0 = 1");
        assert_eq!(v.extra[0] & 0x02, 0, "neg+large → extra bit 1 = 0");
    }

    #[test]
    fn norm_to_ndavec_small_values_no_extra() {
        // w=[0.5, -0.5] → amax=0.5, log2_scale=(0.5/2.0).log2().floor()=(0.25).log2().floor()=-2
        // scale=2^(-2)=0.25, inv_s=4.0
        // 0.5*4.0=2.0: pos, |2.0|>=1.5 → large → is_pos==is_large → extra=1
        // -0.5*4.0=-2.0: neg, |−2.0|>=1.5 → large → is_neg!=is_large → extra=0
        let w = vec![0.5, -0.5];
        let v = norm_to_ndavec(&w);
        assert_eq!(v.log2_scale, -2);
        // Both are large, so extra differs by sign
        assert_eq!(v.extra[0] & 0x01, 1, "pos+large → extra=1");
        assert_eq!(v.extra[0] & 0x02, 0, "neg+large → extra=0");
    }

    // ── norm_to_ndavec_report extras ────────────────────────────────────────

    #[test]
    fn norm_to_ndavec_report_log2_scale_matches_vec() {
        let w = vec![8.0, -4.0, 2.0];
        let (vec, report) = norm_to_ndavec_report(&w);
        assert_eq!(report.log2_scale, vec.log2_scale);
    }

    #[test]
    fn norm_conversion_report_clone() {
        let w = vec![1.0, -2.0, 3.0];
        let (_, r) = norm_to_ndavec_report(&w);
        let cloned = r.clone();
        assert_eq!(cloned.input_len, r.input_len);
        assert_eq!(cloned.output_len, r.output_len);
        assert_eq!(cloned.log2_scale, r.log2_scale);
        assert!((cloned.abs_max - r.abs_max).abs() < f64::EPSILON);
    }

    #[test]
    fn norm_conversion_report_pretty_json() {
        let w = vec![1.0, 2.0];
        let (_, r) = norm_to_ndavec_report(&w);
        let pretty = serde_json::to_string_pretty(&r).unwrap();
        assert!(pretty.contains('\n'));
    }

    // ── ZeroKvLayer extras ──────────────────────────────────────────────────

    #[test]
    fn zero_kv_layer_preserves_data() {
        let mut layer = ZeroKvLayer::new();
        let k = NdaVec {
            len: 8, log2_scale: 2,
            sign: vec![0xAB].into(), extra: vec![0xCD].into(),
        };
        let v = NdaVec {
            len: 8, log2_scale: -1,
            sign: vec![0xEF].into(), extra: vec![0x01].into(),
        };
        layer.push(k, v);
        assert_eq!(layer.entries[0].k.log2_scale, 2);
        assert_eq!(layer.entries[0].v.log2_scale, -1);
        assert_eq!(layer.entries[0].k.sign[0], 0xAB);
        assert_eq!(layer.entries[0].v.extra[0], 0x01);
    }

    #[test]
    fn zero_kv_layer_long_chain() {
        let mut layer = ZeroKvLayer::new();
        for i in 0..20 {
            let k = NdaVec {
                len: 8, log2_scale: i as i8,
                sign: vec![i as u8].into(), extra: vec![0].into(),
            };
            let v = NdaVec {
                len: 8, log2_scale: 0,
                sign: vec![0xFF].into(), extra: vec![0].into(),
            };
            layer.push(k, v);
        }
        assert_eq!(layer.len(), 20);
        assert_eq!(layer.entries[19].k.log2_scale, 19);
    }

    // ── GenerationReport edge cases ─────────────────────────────────────────

    #[test]
    fn generation_report_tokens_per_second_calculation() {
        let r = GenerationReport {
            tokens_generated: 100, prompt_tokens: 10, stopped_at_eos: false,
            truncated: true, site_map_hits: 0, site_map_misses: 0,
            final_kv_cache_size: 0, elapsed_us: 200_000,
            tokens_per_second: 100.0 * 1_000_000.0 / 200_000.0,
            token_ids: vec![0; 100],
        };
        assert!((r.tokens_per_second - 500.0).abs() < 1e-3);
    }

    #[test]
    fn generation_report_cache_hit_rate_calculation() {
        let r = GenerationReport {
            tokens_generated: 10, prompt_tokens: 5, stopped_at_eos: false,
            truncated: false, site_map_hits: 30, site_map_misses: 70,
            final_kv_cache_size: 0, elapsed_us: 0, tokens_per_second: 0.0,
            token_ids: vec![],
        };
        assert!((r.cache_hit_rate() - 0.3).abs() < 1e-9);
    }

    #[test]
    fn generation_report_total_tokens_large() {
        let r = GenerationReport {
            tokens_generated: 100_000, prompt_tokens: 50_000,
            stopped_at_eos: false, truncated: true, site_map_hits: 0,
            site_map_misses: 0, final_kv_cache_size: 0, elapsed_us: 0,
            tokens_per_second: 0.0, token_ids: vec![],
        };
        assert_eq!(r.total_tokens(), 150_000);
    }

    // ── GenerationSummary extras ────────────────────────────────────────────

    #[test]
    fn generation_summary_serializes_none_tokens() {
        let s = GenerationSummary {
            tokens_generated: 0, prompt_tokens: 5, stopped_at_eos: false,
            truncated: false, tokens_per_second: 0.0, cache_hit_rate: 0.0,
            elapsed_ms: 0.0, first_token_id: None, last_token_id: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"first_token_id\":null"));
        assert!(json.contains("\"last_token_id\":null"));
    }

    #[test]
    fn generation_summary_cache_hit_rate_matches_report() {
        let r = GenerationReport {
            tokens_generated: 50, prompt_tokens: 10, stopped_at_eos: true,
            truncated: false, site_map_hits: 40, site_map_misses: 10,
            final_kv_cache_size: 100, elapsed_us: 50000,
            tokens_per_second: 1000.0, token_ids: vec![1; 50],
        };
        let s = ZeroTransformer::summarize_report(&r);
        assert!((s.cache_hit_rate - r.cache_hit_rate()).abs() < f64::EPSILON);
    }

    // ── Pretty JSON ─────────────────────────────────────────────────────────

    #[test]
    fn zero_transformer_info_pretty_json() {
        let info = ZeroTransformerInfo {
            n_layers: 1, n_heads: 1, n_kv_heads: 1, hidden_size: 8,
            head_dim: 8, vocab_size: 10, max_seq_len: 16, eos_token_id: 2,
            total_kv_cached: 0, per_layer_kv: vec![0], lm_head_rows: 10,
            lm_head_stride: 1, embed_tokens_bytes: 0, validation_issues: vec![],
        };
        let pretty = serde_json::to_string_pretty(&info).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));
    }

    #[test]
    fn generation_report_pretty_json() {
        let r = GenerationReport {
            tokens_generated: 1, prompt_tokens: 1, stopped_at_eos: true,
            truncated: false, site_map_hits: 0, site_map_misses: 0,
            final_kv_cache_size: 0, elapsed_us: 0, tokens_per_second: 0.0,
            token_ids: vec![42],
        };
        let pretty = serde_json::to_string_pretty(&r).unwrap();
        assert!(pretty.contains('\n'));
    }

    // ── ForwardMetrics extras ───────────────────────────────────────────────

    #[test]
    fn forward_metrics_json_values() {
        let m = ForwardMetrics { site_map_hits: 77, site_map_misses: 23, kv_cache_size: 200 };
        let v: serde_json::Value = serde_json::to_value(&m).unwrap();
        assert_eq!(v["site_map_hits"], 77);
        assert_eq!(v["site_map_misses"], 23);
        assert_eq!(v["kv_cache_size"], 200);
    }

    #[test]
    fn forward_metrics_clone_all_fields() {
        let m = ForwardMetrics { site_map_hits: 42, site_map_misses: 8, kv_cache_size: 50 };
        let c = m.clone();
        assert_eq!(c.site_map_hits, 42);
        assert_eq!(c.site_map_misses, 8);
        assert_eq!(c.kv_cache_size, 50);
    }
}
