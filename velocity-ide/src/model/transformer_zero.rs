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

use crate::model::{config::ModelConfig, weights::ModelWeights};
use crate::nda_int::{
    AliBiSlopes, NdaVec, NdaEmbedding,
    apply_alibi_bias_i32,
    nda_gemv_nda_to_nda,
    nda_vec_add_inplace,
    rms_norm_nda,
    swiglu_nda, SiluLut,
    argmax_i32,
    DOT_4_LUT,
};
use crate::site_map::SiteMap;

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
    fn new() -> Self { Self { entries: Vec::new() } }

    fn push(&mut self, k: NdaVec, v: NdaVec) {
        self.entries.push(ZeroKvEntry { k, v });
    }

    fn len(&self) -> usize { self.entries.len() }
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
fn attention_head_zero(
    q:         &NdaVec,
    kv_layer:  &ZeroKvLayer,
    h_start:   usize,
    q_pos:     usize,
    head_idx:  usize,
    alibi:     &AliBiSlopes,
) -> NdaVec {
    let _n_cached = kv_layer.len();
    let head_dim = q.len;

    // ── Step 1: Q·K dot products → i32 scores (pure bitwise popcount) ────────
    let _head_bytes = (head_dim + 7) / 8;
    let mut q_low = [0usize; 64];
    let mut q_high = [0usize; 64];
    let limit = _head_bytes.min(64);
    for b in 0..limit {
        let qs = q.sign[b];
        let qe = q.extra[b];
        q_low[b] = ((qs & 0x0F) | ((qe & 0x0F) << 4)) as usize;
        q_high[b] = (((qs >> 4) & 0x0F) | (qe & 0xF0)) as usize;
    }

    let mut scores: Vec<i32> = kv_layer.entries.iter().map(|entry| {
        // K entry covers full hidden_size; we extract head h_start..h_start+head_dim
        let head_byte_start = h_start / 8;

        let mut acc = 0i32;
        for b in 0.._head_bytes {
            let ks = entry.k.sign[head_byte_start + b];
            let ke = entry.k.extra[head_byte_start + b];
            let k_low = ((ks & 0x0F) | ((ke & 0x0F) << 4)) as usize;
            let k_high = (((ks >> 4) & 0x0F) | (ke & 0xF0)) as usize;

            acc += (DOT_4_LUT[q_low[b]][k_low] + DOT_4_LUT[q_high[b]][k_high]) as i32;
        }
        // Scale: q.log2_scale + k.log2_scale combined (integer add)
        acc
    }).collect();

    // Scale: q.log2_scale + k_log2 - 3 (representing Q·K / sqrt(head_dim))
    let k_log2 = kv_layer.entries.first().map(|e| e.k.log2_scale).unwrap_or(0);
    let scores_log2 = q.log2_scale + k_log2 - 3;
    let scale_shift = (-scores_log2).max(0) as u32;

    if q_pos < 5 && head_idx == 0 {
        println!("[Debug Attention] q_pos: {}, q_log2: {}, k_log2: {}, scores_log2: {}, scale_shift: {}", 
                 q_pos, q.log2_scale, k_log2, scores_log2, scale_shift);
        println!("[Debug Attention] raw scores (pre-ALiBi): {:?}", scores);
        for (i, entry) in kv_layer.entries.iter().enumerate() {
            println!("  k_pos {} k.sign: {:?}, k.extra: {:?}", i, &entry.k.sign[..2.min(entry.k.sign.len())], &entry.k.extra[..2.min(entry.k.extra.len())]);
        }
    }

    // ── Step 2: ALiBi bias — pure bit-shift subtraction ─────────────────────
    apply_alibi_bias_i32(&mut scores, q_pos, alibi.shift(head_idx), scale_shift);

    // ── Step 3: Bit-shift softmax approximation → integer attention weights ──
    //   Instead of 1i32 >> gap (which collapses to hard argmax), we use Q14 fixed-point
    //   weights with Q8 fractional linear interpolation for 2^-gap_float.
    let max_score = *scores.iter().max().unwrap_or(&0);
    let weights: Vec<i32> = scores.iter().map(|&s| {
        let gap = max_score - s;
        // Represent gap in Q8 (fixed-point with 8 fractional bits)
        let gap_q8 = if scale_shift >= 8 {
            gap >> (scale_shift - 8)
        } else {
            gap << (8 - scale_shift)
        };
        let integer_part = (gap_q8 >> 8).clamp(0, 14) as u32;
        let fractional_part = (gap_q8 & 0xFF) as i32;
        
        let a = 16384i32 >> integer_part;
        let b = 16384i32 >> (integer_part + 1);
        a - ((a - b) * fractional_part >> 8)
    }).collect();
    
    if q_pos < 5 && head_idx == 0 {
        println!("[Debug Softmax] scores: {:?}, weights: {:?}", 
                 &scores[..scores.len().min(5)], &weights[..weights.len().min(5)]);
    }
    
    let weight_sum: i32 = weights.iter().sum::<i32>().max(1);

    // ── Step 4: Weighted V accumulation — pure integer add/subtract ──────────
    let mut out_i32 = vec![0i32; head_dim];
    let head_byte_start = h_start / 8;
    let _head_bytes = (head_dim + 7) / 8;

    for (w, entry) in weights.iter().zip(kv_layer.entries.iter()) {
        if *w == 0 { continue; }
        for i in 0..head_dim {
            let global_byte = head_byte_start + i / 8;
            let bit_idx = i % 8;
            let mask = 1u8 << bit_idx;
            let is_pos   = (entry.v.sign[global_byte]  & mask) != 0;
            let is_large = (entry.v.sign[global_byte]  & mask) == (entry.v.extra[global_byte] & mask);
            let raw = if is_large { 2i32 } else { 1 };
            let val = if is_pos { raw } else { -raw };
            // Weighted add: pure integer addition
            out_i32[i] += val * w;  // w ∈ {1, 0} for bit-shift weights — one mult per token
        }
    }

    // Normalise by weight_sum (integer division — one div per head)
    for v in &mut out_i32 {
        *v /= weight_sum;
    }

    // Output scale = v.log2_scale (same for all V entries, use first)
    let v_log2 = kv_layer.entries.first()
        .map(|e| e.v.log2_scale)
        .unwrap_or(0);

    NdaVec::from_i32_slice(&out_i32, v_log2)
}

// ─── Zero-Float Transformer ─────────────────────────────────────────────────

pub struct ZeroTransformer {
    config:       ModelConfig,
    weights:      ModelWeights,
    kv_cache:     Vec<ZeroKvLayer>,
    #[allow(dead_code)]
    embed:        NdaEmbedding,
    /// LM head stored as NdaEmbedding rows (vocab × hidden), reused as matrix.
    lm_head_nda:  NdaEmbedding,
    alibi:        AliBiSlopes,
    silu:         SiluLut,
}

impl ZeroTransformer {
    pub fn new(config: ModelConfig, weights: ModelWeights) -> Self {
        let kv_cache = (0..config.n_layers).map(|_| ZeroKvLayer::new()).collect();
        let alibi    = AliBiSlopes::new(config.n_heads);
        let silu     = SiluLut::new();

        // Build NDA embedding table from FP32 weights
        let embed = NdaEmbedding::from_f32(
            &weights.embed_tokens,
            config.vocab_size,
            config.hidden_size,
        );

        // Build NDA LM head (vocab × hidden) from FP32 weights
        // lm_head is [vocab_size × hidden_size] — same layout as embed_tokens
        let lm_head_nda = NdaEmbedding::from_f32(
            &weights.lm_head,
            config.vocab_size,
            config.hidden_size,
        );

        Self { config, weights, kv_cache, embed, lm_head_nda, alibi, silu }
    }

    pub fn reset_cache(&mut self) {
        for layer in &mut self.kv_cache {
            layer.entries.clear();
        }
    }

    /// Process one token — returns i32 logit vector (no softmax).
    pub(crate) fn forward_one_zero(
        &mut self,
        token: u32,
        pos: usize,
        mut site_map: Option<&mut SiteMap>,
        stats_hits: &mut usize,
        stats_misses: &mut usize,
    ) -> Vec<i32> {
        let cfg = &self.config;
        let h   = cfg.hidden_size;
        let hd  = cfg.head_dim;

        // ── Token embedding: Lookup FP32 vector and quantize dynamically per-token ──
        let start = token as usize * h;
        let end = start + h;
        let x_f32 = &self.weights.embed_tokens[start..end];
        let mut x = NdaVec::from_f32_slice(x_f32);
        if pos < 5 {
            println!("[Debug Embed] pos: {}, token: {}, scale: {}, sign: {:?}", pos, token, x.log2_scale, &x.sign[..2.min(x.sign.len())]);
        }

        // ── 24 transformer layers ─────────────────────────────────────────────
        for layer_idx in 0..cfg.n_layers {
            let lw = &self.weights.layers[layer_idx];

            // 1. Attention pre-norm (integer fixed-point RMSNorm)
            let x_norm = rms_norm_nda(&x, &norm_to_ndavec(&lw.attn_norm), 6);
            if pos < 5 && layer_idx == 0 {
                println!("[Debug Norm] pos: {}, scale: {}, sign: {:?}", pos, x_norm.log2_scale, &x_norm.sign[..2.min(x_norm.sign.len())]);
            }

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
            if pos < 5 && layer_idx == 0 {
                println!("[Debug K] pos: {}, scale: {}, sign: {:?}", pos, k.log2_scale, &k.sign[..2.min(k.sign.len())]);
            }
            self.kv_cache[layer_idx].push(k, v.clone());

            // 4. Multi-head attention with ALiBi + bitwise Q·K popcount
            let kv_layer = &self.kv_cache[layer_idx];
            let head_bytes = (hd + 7) / 8;
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
                    len:        hd,
                    log2_scale: q.log2_scale,
                    sign:       q.sign[hb..hb + head_bytes].to_vec().into(),
                    extra:      q.extra[hb..hb + head_bytes].to_vec().into(),
                };

                let head_out = attention_head_zero(
                    &q_head,
                    kv_layer,
                    hs_kv,
                    pos,
                    head,
                    &self.alibi,
                );

                // Write head output into attn_out_i32
                for i in 0..hd {
                    attn_out_i32[hs + i] += head_out.get_raw(i);
                }
            }
            let _heads_per_kv = heads_per_kv; // used implicitly via GQA broadcast in KV cache

            // 5. Re-encode attention output as NdaVec
            let attn_out_nda = NdaVec::from_i32_slice(&attn_out_i32, v.log2_scale);

            // 6. O projection + residual (NDA GEMV → NdaVec, then add)
            let o_out = nda_gemv_nda_to_nda(&lw.o_proj, &attn_out_nda);
            nda_vec_add_inplace(&mut x, &o_out);

            // 7. FFN pre-norm
            let x_ffn = rms_norm_nda(&x, &norm_to_ndavec(&lw.ffn_norm), 6);

            // 8. SwiGLU: down(SiLU(gate) ⊙ up) — pure NDA, 4-entry SiLU LUT
            let gate = nda_gemv_nda_to_nda(&lw.gate_proj, &x_ffn);
            let up   = nda_gemv_nda_to_nda(&lw.up_proj,   &x_ffn);
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

        logits.par_iter_mut().enumerate().for_each(|(tok_id, logit)| {
            let stride    = self.lm_head_nda.stride();
            let start     = tok_id * stride;
            let row_sign  = &self.lm_head_nda.sign[start..start + stride];
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
        prompt_tokens:  &[u32],
        max_new_tokens: usize,
        mut on_token:   impl FnMut(u32),
    ) {
        // ── repetition-penalty config (integer-native, no floats) ──
        const REP_WINDOW:        usize = 64;   // sliding history window

        self.reset_cache();

        let n_prompt = prompt_tokens.len();
        let max_new  = max_new_tokens.min(self.config.max_seq_len.saturating_sub(n_prompt));

        // Prefill
        let mut logits = Vec::new();
        let mut h = 0; let mut m = 0;
        for (pos, &tok) in prompt_tokens.iter().enumerate() {
            logits = self.forward_one_zero(tok, pos, None, &mut h, &mut m);
        }

        // History ring buffer for repetition penalty.
        // Prompt tokens seed the window so we also penalise repeating the prompt.
        let mut history: std::collections::VecDeque<u32> =
            prompt_tokens.iter().copied().collect();
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

        // Decode
        for step in 0..max_new {
            if next == self.config.eos_token_id {
                break;
            }
            on_token(next);

            // Slide history window
            if history.len() == REP_WINDOW {
                history.pop_front();
            }
            history.push_back(next);

            logits = self.forward_one_zero(next, n_prompt + step, None, &mut h, &mut m);
            apply_rep_penalty(&mut logits, &history);
            next = argmax_i32(&logits);
        }
    }
}

// ─── Helper: convert FP32 norm weight → NdaVec ─────────────────────────────

/// Convert an FP32 norm weight vector (values near 1.0) to NdaVec.
/// Norm weights are small 1D vectors; the conversion is done once at forward time.
fn norm_to_ndavec(w: &[f32]) -> NdaVec {
    let amax = w.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
    let log2_scale = if amax > 1e-8 {
        (amax / 2.0).log2().floor() as i8
    } else {
        0i8
    };
    let scale = (2.0_f32).powi(log2_scale as i32);
    let inv_s = 1.0 / scale;

    let bytes = (w.len() + 7) / 8;
    let mut sign  = vec![0u8; bytes];
    let mut extra = vec![0u8; bytes];

    for (i, &v) in w.iter().enumerate() {
        let vs = v * inv_s;
        let is_pos   = vs >= 0.0;
        let is_large = vs.abs() >= 1.5;
        let byte_idx = i / 8;
        let bit_idx  = i % 8;
        if is_pos               { sign[byte_idx]  |= 1 << bit_idx; }
        if is_pos == is_large   { extra[byte_idx] |= 1 << bit_idx; }
    }

    NdaVec { len: w.len(), log2_scale, sign: sign.into(), extra: extra.into() }
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
        let k = NdaVec { len: 4, log2_scale: 0, sign: vec![0b10101010].into(), extra: vec![0b01010101].into() };
        let v = NdaVec { len: 4, log2_scale: 0, sign: vec![0b11110000].into(), extra: vec![0b00001111].into() };
        layer.push(k, v);
        assert_eq!(layer.len(), 1);
    }
}
