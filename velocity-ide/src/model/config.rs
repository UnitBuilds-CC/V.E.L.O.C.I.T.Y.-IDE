// model/config.rs — V.E.L.O.C.I.T.Y.-IDE
//
// Static configuration for BitNet b1.58-3B and Qwen2.5-Coder-0.5B (NDA-Zero).

/// Architecture configuration for a single model variant.
#[derive(Debug, Clone)]
pub struct ModelConfig {
    /// Number of transformer layers
    pub n_layers: usize,
    /// Token embedding / hidden dimension
    pub hidden_size: usize,
    /// FFN intermediate dimension (SwiGLU gate and up projections)
    pub ffn_size: usize,
    /// Number of attention heads
    pub n_heads: usize,
    /// Number of KV heads (= n_heads for MHA, < n_heads for GQA)
    pub n_kv_heads: usize,
    /// Per-head dimension = hidden_size / n_heads
    pub head_dim: usize,
    /// Vocabulary size
    pub vocab_size: usize,
    /// Maximum sequence length supported by the KV cache
    pub max_seq_len: usize,
    /// RoPE base frequency θ (used by FP32 path; ignored by zero-float ALiBi path)
    pub rope_theta: f32,
    /// ALiBi right-shift amounts per head (zero-float path only).
    /// bias = (q_pos − k_pos) >> alibi_shifts[head]  (pure bit-shift, no multiply)
    /// Empty = use RoPE instead.
    #[allow(dead_code)]
    pub alibi_shifts: Vec<u8>,
    /// RMSNorm epsilon
    pub rms_eps: f32,
    /// Token ID representing end-of-sequence
    pub eos_token_id: u32,
    /// Token ID representing beginning-of-sequence
    pub bos_token_id: u32,
}

impl ModelConfig {
    /// `1bitLLM/bitnet_b1_58-3B` — 26 layers, hidden=3200, ffn=8640, heads=32.
    pub fn bitnet_3b() -> Self {
        Self {
            n_layers: 26,
            hidden_size: 3200,
            ffn_size: 8640,
            n_heads: 32,
            n_kv_heads: 32,
            head_dim: 3200 / 32,
            vocab_size: 32_000,
            max_seq_len: 2048,
            rope_theta: 10_000.0,
            alibi_shifts: vec![],
            rms_eps: 1e-5,
            eos_token_id: 2,
            bos_token_id: 1,
        }
    }

    /// `Qwen2.5-Coder-0.5B` — NDA-Zero target.
    ///
    /// 24 layers, hidden=896, ffn=4864, GQA (14 Q heads / 2 KV heads).
    /// ALiBi positional encoding: bias = (q_pos − k_pos) >> shift_h
    /// Pure bit-shift — zero multiplication for positional encoding.
    pub fn qwen_coder_05b() -> Self {
        let n_heads = 14usize;
        let alibi_shifts = (1..=n_heads)
            .map(|h| {
                let exact = 8.0 * h as f32 / n_heads as f32;
                exact.round().clamp(1.0, 30.0) as u8
            })
            .collect();
        Self {
            n_layers: 24,
            hidden_size: 896,
            ffn_size: 4864,
            n_heads,
            n_kv_heads: 2,
            head_dim: 896 / 14,
            vocab_size: 151_936,
            max_seq_len: 2048,
            rope_theta: 1_000_000.0,
            alibi_shifts,
            rms_eps: 1e-6,
            eos_token_id: 151_645,
            bos_token_id: 151_643,
        }
    }

    /// Total NDA parameter count (excludes embeddings and norms).
    pub fn ternary_param_count(&self) -> usize {
        let kv_dim = self.n_kv_heads * self.head_dim;
        let attn_per_layer = self.hidden_size * self.hidden_size
            + self.hidden_size * kv_dim
            + self.hidden_size * kv_dim
            + self.hidden_size * self.hidden_size;
        let ffn_per_layer = 2 * self.ffn_size * self.hidden_size + self.hidden_size * self.ffn_size;
        self.n_layers * (attn_per_layer + ffn_per_layer)
    }

    /// True if this config uses ALiBi (zero-float path).
    #[inline]
    #[allow(dead_code)]
    pub fn uses_alibi(&self) -> bool {
        !self.alibi_shifts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitnet_3b_config() {
        let cfg = ModelConfig::bitnet_3b();
        assert_eq!(cfg.n_layers, 26);
        assert_eq!(cfg.hidden_size, 3200);
        assert_eq!(cfg.ffn_size, 8640);
        assert_eq!(cfg.n_heads, 32);
        assert_eq!(cfg.n_kv_heads, 32);
        assert_eq!(cfg.head_dim, 100);
        assert_eq!(cfg.vocab_size, 32_000);
        assert_eq!(cfg.max_seq_len, 2048);
        assert_eq!(cfg.eos_token_id, 2);
        assert_eq!(cfg.bos_token_id, 1);
        assert!(!cfg.uses_alibi());
    }

    #[test]
    fn test_qwen_coder_05b_config() {
        let cfg = ModelConfig::qwen_coder_05b();
        assert_eq!(cfg.n_layers, 24);
        assert_eq!(cfg.hidden_size, 896);
        assert_eq!(cfg.ffn_size, 4864);
        assert_eq!(cfg.n_heads, 14);
        assert_eq!(cfg.n_kv_heads, 2);
        assert_eq!(cfg.head_dim, 64);
        assert_eq!(cfg.vocab_size, 151_936);
        assert!(cfg.uses_alibi());
        assert_eq!(cfg.alibi_shifts.len(), 14);
    }

    #[test]
    fn test_head_dim_invariant() {
        let bitnet = ModelConfig::bitnet_3b();
        assert_eq!(bitnet.head_dim, bitnet.hidden_size / bitnet.n_heads);

        let qwen = ModelConfig::qwen_coder_05b();
        assert_eq!(qwen.head_dim, qwen.hidden_size / qwen.n_heads);
    }

    #[test]
    fn test_ternary_param_count_positive() {
        let cfg = ModelConfig::bitnet_3b();
        let count = cfg.ternary_param_count();
        assert!(count > 0);
        assert!(count > 1_000_000_000);
    }

    #[test]
    fn test_ternary_param_count_qwen() {
        let cfg = ModelConfig::qwen_coder_05b();
        let count = cfg.ternary_param_count();
        assert!(count > 0);
    }

    #[test]
    fn test_alibi_shifts_bounded() {
        let cfg = ModelConfig::qwen_coder_05b();
        for &shift in &cfg.alibi_shifts {
            assert!(shift >= 1);
            assert!(shift <= 30);
        }
    }

    #[test]
    fn test_bitnet_no_alibi() {
        let cfg = ModelConfig::bitnet_3b();
        assert!(cfg.alibi_shifts.is_empty());
        assert!(!cfg.uses_alibi());
    }
}
