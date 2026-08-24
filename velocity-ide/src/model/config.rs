// model/config.rs — V.E.L.O.C.I.T.Y.-IDE
//
// Static configuration for BitNet b1.58-3B and Qwen2.5-Coder-0.5B (NDA-Zero).

use serde::Serialize;

/// Architecture configuration for a single model variant.
#[derive(Debug, Clone, Serialize)]
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
            eos_token_id: 151_643,
            bos_token_id: 151_643,
        }
    }

    /// Total NDA parameter count (excludes embeddings and norms).
    #[allow(dead_code)]
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

    /// Validate config invariants. Returns a list of violations (empty = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.hidden_size == 0 {
            issues.push("hidden_size must be > 0".into());
        }
        if self.n_heads == 0 {
            issues.push("n_heads must be > 0".into());
        }
        if self.n_layers == 0 {
            issues.push("n_layers must be > 0".into());
        }
        if self.vocab_size == 0 {
            issues.push("vocab_size must be > 0".into());
        }
        if self.n_heads > 0 && self.hidden_size % self.n_heads != 0 {
            issues.push(format!(
                "hidden_size ({}) must be divisible by n_heads ({})",
                self.hidden_size, self.n_heads
            ));
        }
        if self.n_kv_heads > 0 && self.n_heads % self.n_kv_heads != 0 {
            issues.push(format!(
                "n_heads ({}) must be divisible by n_kv_heads ({})",
                self.n_heads, self.n_kv_heads
            ));
        }
        if self.head_dim != self.hidden_size / self.n_heads.max(1) {
            issues.push(format!(
                "head_dim ({}) should equal hidden_size / n_heads ({})",
                self.head_dim,
                self.hidden_size / self.n_heads.max(1)
            ));
        }
        if self.n_kv_heads > 0 && self.head_dim > 0 {
            let kv_dim = self.n_kv_heads * self.head_dim;
            if kv_dim > self.hidden_size {
                issues.push(format!(
                    "KV dim ({} × {} = {}) exceeds hidden_size ({})",
                    self.n_kv_heads, self.head_dim, kv_dim, self.hidden_size
                ));
            }
        }
        issues
    }

    /// Return a human-readable summary of the config.
    pub fn summary(&self) -> String {
        format!(
            "ModelConfig: {} layers, hidden={}, ffn={}, heads={} (kv_heads={}), \
             head_dim={}, vocab={}, max_seq={}, alibi={}",
            self.n_layers,
            self.hidden_size,
            self.ffn_size,
            self.n_heads,
            self.n_kv_heads,
            self.head_dim,
            self.vocab_size,
            self.max_seq_len,
            self.uses_alibi(),
        )
    }

    /// Total number of parameters (ternary weights counted as 1 each).
    #[allow(dead_code)]
    pub fn total_param_count(&self) -> usize {
        self.ternary_param_count()
            + self.vocab_size * self.hidden_size // embed_tokens
            + self.hidden_size // final_norm
            + self.vocab_size * self.hidden_size // lm_head
    }

    /// Estimate memory footprint in bytes for FP32 weights.
    pub fn fp32_memory_bytes(&self) -> usize {
        self.total_param_count() * 4
    }

    /// Estimate memory footprint in bytes for NDA ternary weights.
    /// Ternary weights use 2 bits per parameter (sign + extra).
    pub fn nda_memory_bytes(&self) -> usize {
        let ternary_bits = self.ternary_param_count() * 2;
        let embed_bits = self.vocab_size * self.hidden_size * 16; // FP16 embeddings
        (ternary_bits + embed_bits) / 8
    }

    /// Return a serializable snapshot of the config.
    pub fn snapshot(&self) -> ConfigSnapshot {
        ConfigSnapshot {
            n_layers: self.n_layers,
            hidden_size: self.hidden_size,
            ffn_size: self.ffn_size,
            n_heads: self.n_heads,
            n_kv_heads: self.n_kv_heads,
            head_dim: self.head_dim,
            vocab_size: self.vocab_size,
            max_seq_len: self.max_seq_len,
            uses_alibi: self.uses_alibi(),
            fp32_memory_bytes: self.fp32_memory_bytes(),
            nda_memory_bytes: self.nda_memory_bytes(),
            total_params: self.total_param_count(),
            validation_issues: self.validate(),
        }
    }

    /// Estimate KV cache memory for a given batch size and sequence length.
    pub fn kv_cache_bytes(&self, batch_size: usize, seq_len: usize) -> usize {
        let kv_dim = self.n_kv_heads * self.head_dim;
        // 2 (K + V) * batch * seq * layers * kv_dim * 4 bytes (FP32)
        2 * batch_size * seq_len * self.n_layers * kv_dim * 4
    }

    /// Return the attention type as a string.
    pub fn attention_type(&self) -> &'static str {
        if self.n_kv_heads == self.n_heads {
            "MHA"
        } else if self.n_kv_heads == 1 {
            "MQA"
        } else {
            "GQA"
        }
    }
}

/// Serializable snapshot of model configuration.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSnapshot {
    pub n_layers: usize,
    pub hidden_size: usize,
    pub ffn_size: usize,
    pub n_heads: usize,
    pub n_kv_heads: usize,
    pub head_dim: usize,
    pub vocab_size: usize,
    pub max_seq_len: usize,
    pub uses_alibi: bool,
    pub fp32_memory_bytes: usize,
    pub nda_memory_bytes: usize,
    pub total_params: usize,
    pub validation_issues: Vec<String>,
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

    #[test]
    fn test_config_validate_valid() {
        let bitnet = ModelConfig::bitnet_3b();
        assert!(bitnet.validate().is_empty());

        let qwen = ModelConfig::qwen_coder_05b();
        assert!(qwen.validate().is_empty());
    }

    #[test]
    fn test_config_validate_invalid() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.hidden_size = 0;
        let issues = cfg.validate();
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| i.contains("hidden_size")));
    }

    #[test]
    fn test_config_validate_bad_divisibility() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.n_heads = 7; // 3200 / 7 != integer
        let issues = cfg.validate();
        assert!(issues.iter().any(|i| i.contains("divisible")));
    }

    #[test]
    fn test_config_summary() {
        let cfg = ModelConfig::qwen_coder_05b();
        let summary = cfg.summary();
        assert!(summary.contains("24 layers"));
        assert!(summary.contains("hidden=896"));
        assert!(summary.contains("alibi=true"));
    }

    #[test]
    fn test_config_serializable() {
        let cfg = ModelConfig::bitnet_3b();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"n_layers\":26"));
        assert!(json.contains("\"hidden_size\":3200"));
    }

    #[test]
    fn test_total_param_count() {
        let cfg = ModelConfig::bitnet_3b();
        let total = cfg.total_param_count();
        let ternary = cfg.ternary_param_count();
        assert!(total > ternary); // includes embeddings + norms
    }

    // ─── Memory estimation tests ─────────────────────────────────────────

    #[test]
    fn test_fp32_memory_bytes() {
        let cfg = ModelConfig::bitnet_3b();
        let fp32_bytes = cfg.fp32_memory_bytes();
        assert!(fp32_bytes > 0);
        assert_eq!(fp32_bytes, cfg.total_param_count() * 4);
    }

    #[test]
    fn test_nda_memory_bytes() {
        let cfg = ModelConfig::bitnet_3b();
        let nda_bytes = cfg.nda_memory_bytes();
        assert!(nda_bytes > 0);
        // NDA should be much smaller than FP32
        assert!(nda_bytes < cfg.fp32_memory_bytes());
    }

    #[test]
    fn test_kv_cache_bytes() {
        let cfg = ModelConfig::qwen_coder_05b();
        let cache_bytes = cfg.kv_cache_bytes(1, 512);
        assert!(cache_bytes > 0);
        // 2 * 1 * 512 * 24 * (2 * 64) * 4 = 3,145,728 bytes
        let expected = 2 * 1 * 512 * 24 * (2 * 64) * 4;
        assert_eq!(cache_bytes, expected);
    }

    #[test]
    fn test_attention_type() {
        let bitnet = ModelConfig::bitnet_3b();
        assert_eq!(bitnet.attention_type(), "MHA");

        let qwen = ModelConfig::qwen_coder_05b();
        assert_eq!(qwen.attention_type(), "GQA");
    }

    #[test]
    fn test_config_snapshot_serializes() {
        let cfg = ModelConfig::qwen_coder_05b();
        let snap = cfg.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"n_layers\":24"));
        assert!(json.contains("\"hidden_size\":896"));
        assert!(json.contains("\"uses_alibi\":true"));
        assert!(json.contains("\"fp32_memory_bytes\":"));
    }

    #[test]
    fn test_config_snapshot_validation() {
        let cfg = ModelConfig::bitnet_3b();
        let snap = cfg.snapshot();
        assert!(snap.validation_issues.is_empty());

        let mut bad_cfg = ModelConfig::bitnet_3b();
        bad_cfg.hidden_size = 0;
        let bad_snap = bad_cfg.snapshot();
        assert!(!bad_snap.validation_issues.is_empty());
    }

    // ─── Validation edge cases ────────────────────────────────────────────

    #[test]
    fn test_validate_zero_n_heads() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.n_heads = 0;
        let issues = cfg.validate();
        assert!(issues.iter().any(|i| i.contains("n_heads")));
    }

    #[test]
    fn test_validate_zero_n_layers() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.n_layers = 0;
        let issues = cfg.validate();
        assert!(issues.iter().any(|i| i.contains("n_layers")));
    }

    #[test]
    fn test_validate_zero_vocab_size() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.vocab_size = 0;
        let issues = cfg.validate();
        assert!(issues.iter().any(|i| i.contains("vocab_size")));
    }

    #[test]
    fn test_validate_head_dim_mismatch() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.head_dim = 42; // should be 100
        let issues = cfg.validate();
        assert!(issues.iter().any(|i| i.contains("head_dim")));
    }

    #[test]
    fn test_validate_kv_dim_exceeds_hidden() {
        let mut cfg = ModelConfig::bitnet_3b();
        // n_kv_heads=32, head_dim=200 → kv_dim=6400 > hidden_size=3200
        cfg.head_dim = 200;
        cfg.n_heads = 16; // 3200/16=200 so head_dim check passes
        cfg.n_kv_heads = 32;
        let issues = cfg.validate();
        assert!(issues.iter().any(|i| i.contains("KV dim")));
    }

    #[test]
    fn test_validate_bad_gqa_ratio() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.n_kv_heads = 3; // 32 % 3 != 0
        let issues = cfg.validate();
        assert!(issues.iter().any(|i| i.contains("n_heads") && i.contains("n_kv_heads")));
    }

    #[test]
    fn test_validate_multiple_issues() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.hidden_size = 0;
        cfg.n_heads = 0;
        cfg.n_layers = 0;
        cfg.vocab_size = 0;
        let issues = cfg.validate();
        assert!(issues.len() >= 4);
    }

    // ─── Summary tests ────────────────────────────────────────────────────

    #[test]
    fn test_summary_bitnet() {
        let cfg = ModelConfig::bitnet_3b();
        let summary = cfg.summary();
        assert!(summary.contains("26 layers"));
        assert!(summary.contains("hidden=3200"));
        assert!(summary.contains("ffn=8640"));
        assert!(summary.contains("heads=32"));
        assert!(summary.contains("kv_heads=32"));
        assert!(summary.contains("head_dim=100"));
        assert!(summary.contains("vocab=32000"));
        assert!(summary.contains("alibi=false"));
    }

    #[test]
    fn test_summary_contains_all_fields() {
        let cfg = ModelConfig::qwen_coder_05b();
        let summary = cfg.summary();
        assert!(summary.contains("max_seq=2048"));
        assert!(summary.contains("kv_heads=2"));
        assert!(summary.contains("head_dim=64"));
        assert!(summary.contains("vocab=151936"));
    }

    // ─── Snapshot field accuracy ──────────────────────────────────────────

    #[test]
    fn test_snapshot_field_values_bitnet() {
        let cfg = ModelConfig::bitnet_3b();
        let snap = cfg.snapshot();
        assert_eq!(snap.n_layers, 26);
        assert_eq!(snap.hidden_size, 3200);
        assert_eq!(snap.ffn_size, 8640);
        assert_eq!(snap.n_heads, 32);
        assert_eq!(snap.n_kv_heads, 32);
        assert_eq!(snap.head_dim, 100);
        assert_eq!(snap.vocab_size, 32_000);
        assert_eq!(snap.max_seq_len, 2048);
        assert!(!snap.uses_alibi);
        assert!(snap.fp32_memory_bytes > 0);
        assert!(snap.nda_memory_bytes > 0);
        assert!(snap.total_params > 0);
        assert!(snap.validation_issues.is_empty());
    }

    #[test]
    fn test_snapshot_field_values_qwen() {
        let cfg = ModelConfig::qwen_coder_05b();
        let snap = cfg.snapshot();
        assert_eq!(snap.n_layers, 24);
        assert_eq!(snap.hidden_size, 896);
        assert_eq!(snap.n_heads, 14);
        assert_eq!(snap.n_kv_heads, 2);
        assert!(snap.uses_alibi);
    }

    #[test]
    fn test_snapshot_memory_consistency() {
        let cfg = ModelConfig::bitnet_3b();
        let snap = cfg.snapshot();
        assert_eq!(snap.fp32_memory_bytes, cfg.fp32_memory_bytes());
        assert_eq!(snap.nda_memory_bytes, cfg.nda_memory_bytes());
        assert_eq!(snap.total_params, cfg.total_param_count());
    }

    // ─── KV cache scaling ─────────────────────────────────────────────────

    #[test]
    fn test_kv_cache_zero_batch() {
        let cfg = ModelConfig::bitnet_3b();
        assert_eq!(cfg.kv_cache_bytes(0, 512), 0);
    }

    #[test]
    fn test_kv_cache_zero_seq() {
        let cfg = ModelConfig::bitnet_3b();
        assert_eq!(cfg.kv_cache_bytes(4, 0), 0);
    }

    #[test]
    fn test_kv_cache_scales_linearly_with_batch() {
        let cfg = ModelConfig::bitnet_3b();
        let b1 = cfg.kv_cache_bytes(1, 256);
        let b4 = cfg.kv_cache_bytes(4, 256);
        assert_eq!(b4, b1 * 4);
    }

    #[test]
    fn test_kv_cache_scales_linearly_with_seq() {
        let cfg = ModelConfig::bitnet_3b();
        let s128 = cfg.kv_cache_bytes(2, 128);
        let s512 = cfg.kv_cache_bytes(2, 512);
        assert_eq!(s512, s128 * 4);
    }

    #[test]
    fn test_kv_cache_bitnet_vs_qwen() {
        let bitnet = ModelConfig::bitnet_3b();
        let qwen = ModelConfig::qwen_coder_05b();
        // bitnet: kv_dim = 32*100 = 3200, qwen: kv_dim = 2*64 = 128
        let b = bitnet.kv_cache_bytes(1, 1);
        let q = qwen.kv_cache_bytes(1, 1);
        // bitnet per-layer kv = 2*3200*4 = 25600, qwen = 2*128*4 = 1024
        // total: bitnet = 26*25600 = 665600, qwen = 24*1024 = 24576
        assert_eq!(b, 26 * 2 * 3200 * 4);
        assert_eq!(q, 24 * 2 * 128 * 4);
    }

    // ─── Attention type variants ──────────────────────────────────────────

    #[test]
    fn test_attention_type_mqa() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.n_kv_heads = 1;
        assert_eq!(cfg.attention_type(), "MQA");
    }

    #[test]
    fn test_attention_type_gqa_intermediate() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.n_kv_heads = 8; // 32/8 = 4 → GQA
        assert_eq!(cfg.attention_type(), "GQA");
    }

    // ─── Memory estimation ────────────────────────────────────────────────

    #[test]
    fn test_fp32_memory_qwen() {
        let cfg = ModelConfig::qwen_coder_05b();
        let fp32 = cfg.fp32_memory_bytes();
        assert_eq!(fp32, cfg.total_param_count() * 4);
        assert!(fp32 > 0);
    }

    #[test]
    fn test_nda_memory_qwen() {
        let cfg = ModelConfig::qwen_coder_05b();
        let nda = cfg.nda_memory_bytes();
        let fp32 = cfg.fp32_memory_bytes();
        assert!(nda > 0);
        assert!(nda < fp32);
    }

    #[test]
    fn test_nda_fp32_ratio_significant() {
        // NDA should be substantially smaller than FP32 (at least 50% reduction)
        let cfg = ModelConfig::bitnet_3b();
        let ratio = cfg.nda_memory_bytes() as f64 / cfg.fp32_memory_bytes() as f64;
        assert!(ratio < 0.5, "NDA/FP32 ratio should be < 0.5, got {}", ratio);
    }

    // ─── Ternary param count ──────────────────────────────────────────────

    #[test]
    fn test_ternary_param_count_bitnet_formula() {
        let cfg = ModelConfig::bitnet_3b();
        let kv_dim = cfg.n_kv_heads * cfg.head_dim; // 32*100=3200
        let attn = cfg.hidden_size * cfg.hidden_size * 2 // Q + O
            + cfg.hidden_size * kv_dim * 2; // K + V
        let ffn = 2 * cfg.ffn_size * cfg.hidden_size + cfg.hidden_size * cfg.ffn_size;
        let expected = cfg.n_layers * (attn + ffn);
        assert_eq!(cfg.ternary_param_count(), expected);
    }

    #[test]
    fn test_total_param_includes_embeddings() {
        let cfg = ModelConfig::bitnet_3b();
        let total = cfg.total_param_count();
        let ternary = cfg.ternary_param_count();
        let embed_overhead = total - ternary;
        // embed_tokens + lm_head + final_norm
        let expected_overhead = cfg.vocab_size * cfg.hidden_size * 2 + cfg.hidden_size;
        assert_eq!(embed_overhead, expected_overhead);
    }

    // ─── Clone independence ───────────────────────────────────────────────

    #[test]
    fn test_config_clone_independent() {
        let mut cfg = ModelConfig::bitnet_3b();
        let original = cfg.clone();
        cfg.hidden_size = 999;
        assert_eq!(original.hidden_size, 3200);
    }

    // ─── ALiBi shift generation ───────────────────────────────────────────

    #[test]
    fn test_alibi_shifts_monotonically_increasing() {
        let cfg = ModelConfig::qwen_coder_05b();
        for w in cfg.alibi_shifts.windows(2) {
            assert!(w[1] >= w[0], "ALiBi shifts should be non-decreasing");
        }
    }

    #[test]
    fn test_alibi_shifts_first_and_last() {
        let cfg = ModelConfig::qwen_coder_05b();
        // head 1: round(8*1/14) = round(0.571) = 1
        assert_eq!(cfg.alibi_shifts[0], 1);
        // head 14: round(8*14/14) = round(8.0) = 8
        assert_eq!(cfg.alibi_shifts[13], 8);
    }

    // ─── Uses ALiBi ───────────────────────────────────────────────────────

    #[test]
    fn test_uses_alibi_with_shifts() {
        let cfg = ModelConfig::qwen_coder_05b();
        assert!(cfg.uses_alibi());
        assert!(!cfg.alibi_shifts.is_empty());
    }

    #[test]
    fn test_uses_alibi_without_shifts() {
        let cfg = ModelConfig::bitnet_3b();
        assert!(!cfg.uses_alibi());
        assert!(cfg.alibi_shifts.is_empty());
    }

    // ─── Snapshot JSON roundtrip ──────────────────────────────────────────

    #[test]
    fn test_snapshot_json_contains_all_fields() {
        let cfg = ModelConfig::qwen_coder_05b();
        let snap = cfg.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"n_layers\":24"));
        assert!(json.contains("\"hidden_size\":896"));
        assert!(json.contains("\"ffn_size\":4864"));
        assert!(json.contains("\"n_heads\":14"));
        assert!(json.contains("\"n_kv_heads\":2"));
        assert!(json.contains("\"head_dim\":64"));
        assert!(json.contains("\"vocab_size\":151936"));
        assert!(json.contains("\"max_seq_len\":2048"));
        assert!(json.contains("\"uses_alibi\":true"));
        assert!(json.contains("\"fp32_memory_bytes\":"));
        assert!(json.contains("\"nda_memory_bytes\":"));
        assert!(json.contains("\"total_params\":"));
        assert!(json.contains("\"validation_issues\":[]"));
    }

    // ─── Config Debug format ──────────────────────────────────────────────

    #[test]
    fn test_config_debug_format() {
        let cfg = ModelConfig::bitnet_3b();
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("ModelConfig"));
        assert!(debug.contains("3200"));
    }

    // ── Block 168: Additional tests ────────────────────────────────────────

    #[test]
    fn config_json_has_exactly_13_keys() {
        let cfg = ModelConfig::bitnet_3b();
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_object().unwrap().len(), 13);
    }

    #[test]
    fn snapshot_json_has_exactly_13_keys() {
        let cfg = ModelConfig::qwen_coder_05b();
        let snap = cfg.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_object().unwrap().len(), 13);
    }

    #[test]
    fn snapshot_clone_is_independent() {
        let cfg = ModelConfig::bitnet_3b();
        let snap1 = cfg.snapshot();
        let mut snap2 = snap1.clone();
        snap2.n_layers = 999;
        assert_eq!(snap1.n_layers, 26);
    }

    #[test]
    fn kv_cache_scales_linearly_with_layers() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.n_layers = 1;
        let one_layer = cfg.kv_cache_bytes(1, 1);
        cfg.n_layers = 10;
        let ten_layers = cfg.kv_cache_bytes(1, 1);
        assert_eq!(ten_layers, one_layer * 10);
    }

    #[test]
    fn nda_memory_formula() {
        let cfg = ModelConfig::bitnet_3b();
        let ternary_bits = cfg.ternary_param_count() * 2;
        let embed_bits = cfg.vocab_size * cfg.hidden_size * 16;
        let expected = (ternary_bits + embed_bits) / 8;
        assert_eq!(cfg.nda_memory_bytes(), expected);
    }

    #[test]
    fn total_param_count_formula() {
        let cfg = ModelConfig::bitnet_3b();
        let expected = cfg.ternary_param_count()
            + cfg.vocab_size * cfg.hidden_size  // embed_tokens
            + cfg.hidden_size                     // final_norm
            + cfg.vocab_size * cfg.hidden_size;  // lm_head
        assert_eq!(cfg.total_param_count(), expected);
    }

    #[test]
    fn qwen_total_param_count_formula() {
        let cfg = ModelConfig::qwen_coder_05b();
        let expected = cfg.ternary_param_count()
            + cfg.vocab_size * cfg.hidden_size * 2
            + cfg.hidden_size;
        assert_eq!(cfg.total_param_count(), expected);
    }

    #[test]
    fn summary_bitnet_contains_mha() {
        let cfg = ModelConfig::bitnet_3b();
        // summary doesn't include attention_type, but we verify it doesn't panic
        let s = cfg.summary();
        assert!(s.contains("ModelConfig"));
    }

    #[test]
    fn config_debug_contains_all_field_names() {
        let cfg = ModelConfig::bitnet_3b();
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("n_layers"));
        assert!(debug.contains("hidden_size"));
        assert!(debug.contains("ffn_size"));
        assert!(debug.contains("n_heads"));
        assert!(debug.contains("vocab_size"));
    }

    #[test]
    fn snapshot_debug_format() {
        let cfg = ModelConfig::bitnet_3b();
        let snap = cfg.snapshot();
        let debug = format!("{:?}", snap);
        assert!(debug.contains("ConfigSnapshot"));
        assert!(debug.contains("n_layers"));
    }

    #[test]
    fn alibi_shifts_count_matches_n_heads() {
        let cfg = ModelConfig::qwen_coder_05b();
        assert_eq!(cfg.alibi_shifts.len(), cfg.n_heads);
    }

    #[test]
    fn kv_cache_bytes_large_batch() {
        let cfg = ModelConfig::bitnet_3b();
        let b1 = cfg.kv_cache_bytes(1, 100);
        let b32 = cfg.kv_cache_bytes(32, 100);
        assert_eq!(b32, b1 * 32);
    }

    #[test]
    fn nda_memory_qwen_less_than_fp32() {
        let cfg = ModelConfig::qwen_coder_05b();
        let ratio = cfg.nda_memory_bytes() as f64 / cfg.fp32_memory_bytes() as f64;
        assert!(ratio < 0.5, "NDA/FP32 ratio for qwen should be < 0.5, got {}", ratio);
    }

    #[test]
    fn bitnet_rope_theta() {
        let cfg = ModelConfig::bitnet_3b();
        assert_eq!(cfg.rope_theta, 10_000.0);
    }

    #[test]
    fn qwen_rope_theta() {
        let cfg = ModelConfig::qwen_coder_05b();
        assert_eq!(cfg.rope_theta, 1_000_000.0);
    }

    #[test]
    fn eos_bos_token_ids() {
        let bitnet = ModelConfig::bitnet_3b();
        assert_eq!(bitnet.eos_token_id, 2);
        assert_eq!(bitnet.bos_token_id, 1);

        let qwen = ModelConfig::qwen_coder_05b();
        assert_eq!(qwen.eos_token_id, 151_643);
        assert_eq!(qwen.bos_token_id, 151_643);
    }

    #[test]
    fn rms_eps_values() {
        let bitnet = ModelConfig::bitnet_3b();
        assert_eq!(bitnet.rms_eps, 1e-5);

        let qwen = ModelConfig::qwen_coder_05b();
        assert_eq!(qwen.rms_eps, 1e-6);
    }

    #[test]
    fn validate_with_n_heads_zero_skips_derived_checks() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.n_heads = 0;
        let issues = cfg.validate();
        // Should have at least "n_heads must be > 0"
        assert!(issues.iter().any(|i| i.contains("n_heads must be > 0")));
    }

    #[test]
    fn snapshot_validation_issues_match_validate() {
        let mut cfg = ModelConfig::bitnet_3b();
        cfg.n_kv_heads = 3; // 32 % 3 != 0
        let snap = cfg.snapshot();
        let direct = cfg.validate();
        assert_eq!(snap.validation_issues.len(), direct.len());
    }

    #[test]
    fn config_clone_preserves_alibi() {
        let cfg = ModelConfig::qwen_coder_05b();
        let cloned = cfg.clone();
        assert_eq!(cloned.alibi_shifts, cfg.alibi_shifts);
        assert_eq!(cloned.uses_alibi(), cfg.uses_alibi());
    }

    #[test]
    fn ternary_param_count_qwen_formula() {
        let cfg = ModelConfig::qwen_coder_05b();
        let kv_dim = cfg.n_kv_heads * cfg.head_dim; // 2*64=128
        let attn = cfg.hidden_size * cfg.hidden_size * 2
            + cfg.hidden_size * kv_dim * 2;
        let ffn = 2 * cfg.ffn_size * cfg.hidden_size + cfg.hidden_size * cfg.ffn_size;
        let expected = cfg.n_layers * (attn + ffn);
        assert_eq!(cfg.ternary_param_count(), expected);
    }

    #[test]
    fn fp32_memory_is_four_bytes_per_param() {
        let cfg = ModelConfig::qwen_coder_05b();
        assert_eq!(cfg.fp32_memory_bytes(), cfg.total_param_count() * 4);
    }

    #[test]
    fn bitnet_head_dim_is_100() {
        let cfg = ModelConfig::bitnet_3b();
        assert_eq!(cfg.head_dim, 100);
        assert_eq!(cfg.hidden_size / cfg.n_heads, 100);
    }

    #[test]
    fn qwen_head_dim_is_64() {
        let cfg = ModelConfig::qwen_coder_05b();
        assert_eq!(cfg.head_dim, 64);
        assert_eq!(cfg.hidden_size / cfg.n_heads, 64);
    }
}
