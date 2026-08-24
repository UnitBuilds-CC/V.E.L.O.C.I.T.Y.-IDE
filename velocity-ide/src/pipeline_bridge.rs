// pipeline_bridge.rs — DualPathEngine: routes between Path 1 (text) and Path 2 (NDA)
#![allow(dead_code)]
//
// Path 1 (Text):
//   Natural language in → text tokens out.
//   Can hallucinate. That is acceptable — it handles fuzziness.
//
// Path 2 (NDA):
//   NDA opcodes in → NDA nodes out, Merkle-verified.
//   Cannot hallucinate. Structurally invalid output is rejected at emit-time.
//
// The bridge between them:
//   Natural language intent → Path 1 → hidden_state[896] → conditions Path 2.
//   Path 2 generates NDA programs anchored to that intent vector.
//
// Routing logic (Auto mode):
//   Imperative creation verbs or code keywords → NDA mode.
//   Questions, explanations → Text mode.

use std::{io::Write, path::PathBuf, time::Instant};

use anyhow::Result;
use serde::Serialize;

use crate::{
    model::{config::ModelConfig, transformer::Transformer, weights::ModelWeights},
    pipeline_nda::{NdaPipeline, PipelineMode},
    site_map::verifier::NdaOpcode,
    tokenizer::Tokenizer,
};

// ─── DualPathEngine ───────────────────────────────────────────────────────────

/// Routes user requests between the two pipelines.
///
/// Both pipelines share the same model weights on disk but maintain separate
/// runtime state (KV caches, head weights).  The text path uses the standard
/// floating-point KV cache; the NDA path uses the persistent SiteMap.
pub struct DualPathEngine {
    tokenizer: Tokenizer,
    cfg: ModelConfig,
    model_dir: PathBuf,
    /// Path 2: NDA native pipeline (always present).
    path2: NdaPipeline,
    /// Path 1: text pipeline (lazy-loaded on first text request to save RAM).
    path1: Option<TextPath>,
}

struct TextPath {
    transformer: Transformer,
}

/// The output of one engine invocation.
pub enum EngineOutput {
    /// Path 1 text output.
    Text {
        text: String,
        n_tokens: usize,
        elapsed_ms: u128,
    },
    /// Path 2 NDA output.
    Nda {
        opcodes: Vec<NdaOpcode>,
        root_hash: u64,
        valid: bool,
        /// True when the program was sealed by forced termination (budget exhausted).
        /// Structurally valid but semantically incomplete — not stored in SiteMap.
        force_terminated: bool,
        site_map_key: Option<u64>,
        n_opcodes: usize,
        elapsed_ms: u128,
    },
}

/// Structured execution report from one engine invocation.
///
/// Contains all metrics and diagnostics from a run, suitable for
/// JSON serialization and programmatic consumption.
#[derive(Debug, Clone, Serialize)]
pub struct EngineReport {
    /// Which path was used (Text or Nda).
    pub path: String,
    /// The resolved mode (after Auto detection).
    pub resolved_mode: String,
    /// Prompt token count.
    pub prompt_tokens: usize,
    /// Output token/opcode count.
    pub output_count: usize,
    /// Wall-clock time in microseconds.
    pub elapsed_us: u64,
    /// Throughput (tokens or opcodes per second).
    pub per_second: f64,
    /// Path 1 text output (if text path was used).
    pub text: Option<String>,
    /// Path 2 NDA diagnostics (if NDA path was used).
    pub nda: Option<NdaRunDiagnostics>,
    /// Whether Path 1 was already loaded or had to be lazy-initialized.
    pub path1_lazy_loaded: bool,
    /// Engine status at time of report.
    pub engine_status: EngineStatusSnapshot,
}

/// Diagnostics from an NDA path execution.
#[derive(Debug, Clone, Serialize)]
pub struct NdaRunDiagnostics {
    pub root_hash: u64,
    pub valid: bool,
    pub force_terminated: bool,
    pub site_map_key: Option<u64>,
    pub opcode_count: usize,
    pub sandbox_passed: Option<bool>,
    pub scope_passed: Option<bool>,
    pub scope_similarity: Option<f64>,
    pub site_map_hits: usize,
    pub site_map_misses: usize,
}

/// Snapshot of engine state for diagnostics.
#[derive(Debug, Clone, Serialize)]
pub struct EngineStatusSnapshot {
    pub path1_loaded: bool,
    pub path2_active: bool,
    pub model_dir: String,
    pub vocab_size: usize,
    pub n_layers: usize,
    pub hidden_size: usize,
}

/// Summary of the engine configuration and current state.
#[derive(Debug, Clone, Serialize)]
pub struct EngineInfo {
    pub model_dir: String,
    pub vocab_size: usize,
    pub n_layers: usize,
    pub hidden_size: usize,
    pub ffn_size: usize,
    pub n_heads: usize,
    pub n_kv_heads: usize,
    pub head_dim: usize,
    pub max_seq_len: usize,
    pub path1_loaded: bool,
    pub path2_site_map_stats: String,
    pub tokenizer_merge_count: usize,
}

impl DualPathEngine {
    /// Open the engine.  Path 1 is lazy-loaded; Path 2 is opened immediately.
    pub fn open(
        model_dir: &std::path::Path,
        tokenizer_path: &std::path::Path,
        cfg: ModelConfig,
        _mode: PipelineMode,
    ) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(tokenizer_path)?;

        // Head path defaults to <model_dir>/nda_head.bin
        let head_path = model_dir.join("nda_head.bin");
        let head_path = if head_path.exists() {
            Some(head_path)
        } else {
            None
        };

        let path2 = NdaPipeline::open(
            model_dir,
            None, // site_map_dir: defaults to model_dir/site_map
            head_path.as_deref(),
            cfg.clone(),
        )?;

        eprintln!("[bridge] {}", path2.site_map_stats());

        Ok(Self {
            tokenizer,
            cfg,
            model_dir: model_dir.to_path_buf(),
            path2,
            path1: None,
        })
    }

    /// Run the engine on a prompt.  Routes to Path 1 or Path 2 based on `mode`.
    /// If mode is `Auto`, detection is performed from the prompt text.
    pub fn run(
        &mut self,
        prompt: &str,
        mode: PipelineMode,
        max_tokens: usize,
    ) -> Result<EngineOutput> {
        let resolved_mode = match mode {
            PipelineMode::Auto => PipelineMode::detect(prompt),
            m => m,
        };

        match resolved_mode {
            PipelineMode::Text => self.run_path1(prompt, max_tokens),
            PipelineMode::Nda => self.run_path2(prompt, max_tokens),
            PipelineMode::Auto => unreachable!(),
        }
    }

    /// Return a snapshot of the engine's current status.
    pub fn status_snapshot(&self) -> EngineStatusSnapshot {
        EngineStatusSnapshot {
            path1_loaded: self.path1.is_some(),
            path2_active: true,
            model_dir: self.model_dir.display().to_string(),
            vocab_size: self.cfg.vocab_size,
            n_layers: self.cfg.n_layers,
            hidden_size: self.cfg.hidden_size,
        }
    }

    /// Return detailed engine info for diagnostics.
    pub fn info(&self) -> EngineInfo {
        EngineInfo {
            model_dir: self.model_dir.display().to_string(),
            vocab_size: self.cfg.vocab_size,
            n_layers: self.cfg.n_layers,
            hidden_size: self.cfg.hidden_size,
            ffn_size: self.cfg.ffn_size,
            n_heads: self.cfg.n_heads,
            n_kv_heads: self.cfg.n_kv_heads,
            head_dim: self.cfg.head_dim,
            max_seq_len: self.cfg.max_seq_len,
            path1_loaded: self.path1.is_some(),
            path2_site_map_stats: format!("{}", self.path2.site_map_stats()),
            tokenizer_merge_count: self.tokenizer.merge_count(),
        }
    }

    /// Validate the engine configuration.
    /// Returns a list of warnings (empty = all good).
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.cfg.vocab_size == 0 {
            warnings.push("vocab_size is 0".to_string());
        }
        if self.cfg.n_layers == 0 {
            warnings.push("n_layers is 0".to_string());
        }
        if self.cfg.hidden_size == 0 {
            warnings.push("hidden_size is 0".to_string());
        }
        if self.cfg.n_heads == 0 {
            warnings.push("n_heads is 0".to_string());
        }
        if self.cfg.max_seq_len == 0 {
            warnings.push("max_seq_len is 0".to_string());
        }
        if self.cfg.n_heads > 0 && self.cfg.hidden_size % self.cfg.n_heads != 0 {
            warnings.push(format!(
                "hidden_size ({}) not divisible by n_heads ({})",
                self.cfg.hidden_size, self.cfg.n_heads
            ));
        }
        if !self.model_dir.exists() {
            warnings.push(format!(
                "model_dir does not exist: {}",
                self.model_dir.display()
            ));
        }

        warnings
    }

    /// Run the engine and return a structured report alongside the output.
    pub fn run_with_report(
        &mut self,
        prompt: &str,
        mode: PipelineMode,
        max_tokens: usize,
    ) -> Result<(EngineOutput, EngineReport)> {
        let resolved_mode = match mode {
            PipelineMode::Auto => PipelineMode::detect(prompt),
            m => m,
        };

        let path1_was_loaded = self.path1.is_some();
        let prompt_tokens = self.tokenizer.encode(prompt, true);
        let prompt_token_count = prompt_tokens.len();

        let t_start = Instant::now();
        let output = match resolved_mode {
            PipelineMode::Text => self.run_path1(prompt, max_tokens)?,
            PipelineMode::Nda => self.run_path2(prompt, max_tokens)?,
            PipelineMode::Auto => unreachable!(),
        };
        let elapsed_us = t_start.elapsed().as_micros() as u64;

        let report = match &output {
            EngineOutput::Text { text, n_tokens, .. } => {
                let per_second = if elapsed_us > 0 {
                    (*n_tokens as f64) / (elapsed_us as f64 / 1_000_000.0)
                } else {
                    0.0
                };
                EngineReport {
                    path: "text".to_string(),
                    resolved_mode: format!("{:?}", resolved_mode),
                    prompt_tokens: prompt_token_count,
                    output_count: *n_tokens,
                    elapsed_us,
                    per_second,
                    text: Some(text.clone()),
                    nda: None,
                    path1_lazy_loaded: !path1_was_loaded,
                    engine_status: self.status_snapshot(),
                }
            }
            EngineOutput::Nda {
                opcodes,
                root_hash,
                valid,
                force_terminated,
                site_map_key,
                n_opcodes,
                ..
            } => {
                let per_second = if elapsed_us > 0 {
                    (*n_opcodes as f64) / (elapsed_us as f64 / 1_000_000.0)
                } else {
                    0.0
                };
                EngineReport {
                    path: "nda".to_string(),
                    resolved_mode: format!("{:?}", resolved_mode),
                    prompt_tokens: prompt_token_count,
                    output_count: *n_opcodes,
                    elapsed_us,
                    per_second,
                    text: None,
                    nda: Some(NdaRunDiagnostics {
                        root_hash: *root_hash,
                        valid: *valid,
                        force_terminated: *force_terminated,
                        site_map_key: *site_map_key,
                        opcode_count: opcodes.len(),
                        sandbox_passed: None,
                        scope_passed: None,
                        scope_similarity: None,
                        site_map_hits: 0,
                        site_map_misses: 0,
                    }),
                    path1_lazy_loaded: !path1_was_loaded,
                    engine_status: self.status_snapshot(),
                }
            }
        };

        Ok((output, report))
    }

    // ── Path 1: text generation ───────────────────────────────────────────────

    fn run_path1(&mut self, prompt: &str, max_tokens: usize) -> Result<EngineOutput> {
        // Lazy-load text transformer on first call.
        if self.path1.is_none() {
            eprintln!("[bridge] Loading Path 1 text transformer...");
            let weights = ModelWeights::load(&self.model_dir, &self.cfg)?;
            self.path1 = Some(TextPath {
                transformer: Transformer::new(self.cfg.clone(), weights),
            });
        }
        let path1 = self.path1.as_mut().unwrap();

        let prompt_tokens = self.tokenizer.encode(prompt, true);
        let t_start = Instant::now();
        let mut text = String::new();
        let mut n_tokens = 0usize;

        path1.transformer.generate(
            &prompt_tokens,
            max_tokens,
            0.7, // temperature
            0.9, // top-p
            |tok_id| {
                let piece = self.tokenizer.decode_token(tok_id);
                text.push_str(&piece);
                print!("{piece}");
                std::io::stdout().flush().ok();
                n_tokens += 1;
            },
        );

        Ok(EngineOutput::Text {
            text,
            n_tokens,
            elapsed_ms: t_start.elapsed().as_millis(),
        })
    }

    // ── Path 2: NDA native generation ────────────────────────────────────────

    fn run_path2(&mut self, prompt: &str, max_tokens: usize) -> Result<EngineOutput> {
        eprintln!("[bridge] Path 2 \u{2014} NDA native generation");
        eprintln!(
            "[bridge] Output vocabulary: {} opcodes (zero-hallucination mode)",
            NdaOpcode::VOCAB_SIZE
        );

        // Lazy-load text transformer to compute conditioning hidden state from prompt.
        if self.path1.is_none() {
            eprintln!("[bridge] Loading Path 1 text transformer...");
            let weights = ModelWeights::load(&self.model_dir, &self.cfg)?;
            self.path1 = Some(TextPath {
                transformer: Transformer::new(self.cfg.clone(), weights),
            });
        }
        let path1 = self.path1.as_mut().unwrap();
        let prompt_tokens = self.tokenizer.encode(prompt, true);
        let condition = path1
            .transformer
            .get_conditioning_hidden_state(&prompt_tokens);

        let mut opcodes = Vec::new();
        let t_start = Instant::now();

        let result = self.path2.generate(Some(&condition), max_tokens, |op| {
            print!(" {}", op.name());
            std::io::stdout().flush().ok();
            opcodes.push(op);
        });

        let elapsed_ms = t_start.elapsed().as_millis();
        let n_opcodes = opcodes.len();

        // Print Merkle, Sandbox, and Scope results.
        if result.valid {
            let status = if result.force_terminated {
                "VALID but TRUNCATED"
            } else {
                "VALID (complete)"
            };
            eprintln!("\n[bridge] Merkle     : {}", status);

            if let Some(ref sb) = result.sandbox {
                if sb.panicked || sb.error.is_some() {
                    let err = sb.error.as_deref().unwrap_or("unknown error");
                    eprintln!("[bridge] Sandbox    : FAIL ({})", err);
                } else {
                    eprintln!(
                        "[bridge] Sandbox    : PASS  {} nodes ({} matrices, {} norms, out_dim={}), {}\u{00b5}s",
                        sb.executed_nodes, sb.matrix_count, sb.norm_count, sb.output_dim, sb.elapsed_us
                    );
                }
            }

            if let Some(ref sc) = result.scope {
                let status = if sc.passed { "PASS" } else { "FAIL" };
                let comment = if sc.passed {
                    ""
                } else {
                    " \u{2014} not stored (prompt-program misalignment)"
                };
                eprintln!(
                    "[bridge] Scope      : {}  sim={:.2} (\u{03b8}={:.2}){}",
                    status, sc.similarity, sc.threshold, comment
                );
            }

            if let Some(key) = result.site_map_key {
                eprintln!("[bridge] Stored     : site_map {:016x}", key);
            }
        } else {
            eprintln!("\n[bridge] Merkle     : INVALID (generation failed structurally)");
        }

        eprintln!(
            "[bridge] SiteMap Cache stats: {} hits, {} misses",
            result.stats.site_map_hits, result.stats.site_map_misses
        );
        eprintln!("[bridge] {}", self.path2.site_map_stats());

        Ok(EngineOutput::Nda {
            opcodes,
            root_hash: result.root_hash,
            valid: result.valid,
            force_terminated: result.force_terminated,
            site_map_key: result.site_map_key,
            n_opcodes,
            elapsed_ms,
        })
    }
}

// ─── CLI integration helper ───────────────────────────────────────────────────

/// Called from main.rs when `--mode nda` or `--mode auto` is requested.
pub fn run_dual_path(
    model_dir: &std::path::Path,
    tokenizer_path: &std::path::Path,
    prompt: &str,
    mode: PipelineMode,
    max_tokens: usize,
    cfg: ModelConfig,
) -> Result<()> {
    let mut engine = DualPathEngine::open(model_dir, tokenizer_path, cfg, mode)?;
    let result = engine.run(prompt, mode, max_tokens)?;

    match result {
        EngineOutput::Text {
            n_tokens,
            elapsed_ms,
            ..
        } => {
            let elapsed_s = elapsed_ms as f64 / 1000.0;
            eprintln!(
                "\n\n--- Path 1 (Text) Stats ---\
                 \nTokens : {n_tokens}\
                 \nTime   : {elapsed_s:.2}s\
                 \nTok/s  : {:.2}",
                n_tokens as f64 / elapsed_s.max(1e-6),
            );
        }
        EngineOutput::Nda {
            n_opcodes,
            elapsed_ms,
            valid,
            root_hash,
            force_terminated,
            ..
        } => {
            let elapsed_s = elapsed_ms as f64 / 1000.0;
            let status = match (valid, force_terminated) {
                (true, false) => "VALID (complete)",
                (true, true) => "VALID (truncated \u{2014} increase --max-tokens)",
                _ => "INVALID",
            };
            eprintln!(
                "\n\n--- Path 2 (NDA Native) Stats ---\
                 \nOpcodes    : {n_opcodes}\
                 \nMerkle     : {status} ({root_hash:016x})\
                 \nTime       : {elapsed_s:.2}s\
                 \nOpcodes/s  : {:.2}",
                n_opcodes as f64 / elapsed_s.max(1e-6),
            );
        }
    }

    Ok(())
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_report_serializes() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 10,
            output_count: 42,
            elapsed_us: 500_000,
            per_second: 84.0,
            text: Some("Hello world".to_string()),
            nda: None,
            path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true,
                path2_active: true,
                model_dir: "/tmp/model".to_string(),
                vocab_size: 32000,
                n_layers: 12,
                hidden_size: 768,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"path\":\"text\""));
        assert!(json.contains("\"output_count\":42"));
        assert!(json.contains("\"path1_lazy_loaded\":true"));
    }

    #[test]
    fn nda_run_diagnostics_serializes() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xDEADBEEF,
            valid: true,
            force_terminated: false,
            site_map_key: Some(0x1234),
            opcode_count: 128,
            sandbox_passed: Some(true),
            scope_passed: Some(true),
            scope_similarity: Some(0.95),
            site_map_hits: 5,
            site_map_misses: 3,
        };
        let json = serde_json::to_string(&diag).unwrap();
        assert!(json.contains("\"valid\":true"));
        assert!(json.contains("\"opcode_count\":128"));
        assert!(json.contains("\"scope_similarity\":0.95"));
    }

    #[test]
    fn engine_status_snapshot_serializes() {
        let snap = EngineStatusSnapshot {
            path1_loaded: false,
            path2_active: true,
            model_dir: "/models/test".to_string(),
            vocab_size: 32000,
            n_layers: 24,
            hidden_size: 1024,
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"path1_loaded\":false"));
        assert!(json.contains("\"path2_active\":true"));
    }

    #[test]
    fn engine_info_serializes() {
        let info = EngineInfo {
            model_dir: "/models/test".to_string(),
            vocab_size: 32000,
            n_layers: 12,
            hidden_size: 768,
            ffn_size: 2048,
            n_heads: 12,
            n_kv_heads: 4,
            head_dim: 64,
            max_seq_len: 4096,
            path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(),
            tokenizer_merge_count: 100,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"vocab_size\":32000"));
        assert!(json.contains("\"n_heads\":12"));
    }

    #[test]
    fn engine_output_text_variant() {
        let output = EngineOutput::Text {
            text: "Hello".to_string(),
            n_tokens: 1,
            elapsed_ms: 100,
        };
        match output {
            EngineOutput::Text { text, n_tokens, elapsed_ms } => {
                assert_eq!(text, "Hello");
                assert_eq!(n_tokens, 1);
                assert_eq!(elapsed_ms, 100);
            }
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn engine_output_nda_variant() {
        let output = EngineOutput::Nda {
            opcodes: vec![],
            root_hash: 0xABCD,
            valid: true,
            force_terminated: false,
            site_map_key: Some(42),
            n_opcodes: 0,
            elapsed_ms: 50,
        };
        match output {
            EngineOutput::Nda { valid, site_map_key, .. } => {
                assert!(valid);
                assert_eq!(site_map_key, Some(42));
            }
            _ => panic!("Expected Nda variant"),
        }
    }

    #[test]
    fn nda_diagnostics_none_sandbox() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: false,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 0,
            sandbox_passed: None,
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 0,
            site_map_misses: 0,
        };
        assert!(diag.sandbox_passed.is_none());
        assert!(diag.scope_passed.is_none());
        assert!(!diag.valid);
    }

    #[test]
    fn engine_report_per_second_calculation() {
        // 100 tokens in 0.5 seconds = 200 tok/s
        let elapsed_us = 500_000u64;
        let output_count = 100usize;
        let per_second = if elapsed_us > 0 {
            (output_count as f64) / (elapsed_us as f64 / 1_000_000.0)
        } else {
            0.0
        };
        assert!((per_second - 200.0).abs() < 0.01);
    }

    #[test]
    fn engine_report_zero_elapsed() {
        let elapsed_us = 0u64;
        let output_count = 10usize;
        let per_second = if elapsed_us > 0 {
            (output_count as f64) / (elapsed_us as f64 / 1_000_000.0)
        } else {
            0.0
        };
        assert_eq!(per_second, 0.0);
    }

    // ── Block 99: Comprehensive pipeline_bridge tests ──────────────────────

    #[test]
    fn engine_report_nda_variant_serializes() {
        let report = EngineReport {
            path: "nda".to_string(),
            resolved_mode: "Nda".to_string(),
            prompt_tokens: 20,
            output_count: 64,
            elapsed_us: 200_000,
            per_second: 320.0,
            text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0xCAFEBABE,
                valid: true,
                force_terminated: false,
                site_map_key: Some(0xFF),
                opcode_count: 64,
                sandbox_passed: Some(true),
                scope_passed: Some(false),
                scope_similarity: Some(0.72),
                site_map_hits: 10,
                site_map_misses: 2,
            }),
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false,
                path2_active: true,
                model_dir: "/models/nda".to_string(),
                vocab_size: 151936,
                n_layers: 24,
                hidden_size: 1024,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"path\":\"nda\""));
        assert!(json.contains("\"text\":null"));
        assert!(json.contains("\"root_hash\":3405691582")); // 0xCAFEBABE
        assert!(json.contains("\"force_terminated\":false"));
    }

    #[test]
    fn engine_status_snapshot_clone() {
        let snap = EngineStatusSnapshot {
            path1_loaded: true,
            path2_active: true,
            model_dir: "/test".to_string(),
            vocab_size: 32000,
            n_layers: 12,
            hidden_size: 768,
        };
        let cloned = snap.clone();
        assert_eq!(cloned.path1_loaded, snap.path1_loaded);
        assert_eq!(cloned.model_dir, snap.model_dir);
        assert_eq!(cloned.vocab_size, snap.vocab_size);
    }

    #[test]
    fn engine_report_clone() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 5,
            output_count: 10,
            elapsed_us: 1000,
            per_second: 10000.0,
            text: Some("test".to_string()),
            nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true,
                path2_active: true,
                model_dir: "/m".to_string(),
                vocab_size: 100,
                n_layers: 2,
                hidden_size: 64,
            },
        };
        let cloned = report.clone();
        assert_eq!(cloned.path, report.path);
        assert_eq!(cloned.output_count, report.output_count);
        assert_eq!(cloned.text, report.text);
    }

    #[test]
    fn nda_diagnostics_all_none_fields() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: false,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 0,
            sandbox_passed: None,
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 0,
            site_map_misses: 0,
        };
        let json = serde_json::to_string(&diag).unwrap();
        assert!(json.contains("\"sandbox_passed\":null"));
        assert!(json.contains("\"scope_passed\":null"));
        assert!(json.contains("\"scope_similarity\":null"));
        assert!(json.contains("\"site_map_key\":null"));
    }

    #[test]
    fn nda_diagnostics_force_terminated() {
        let diag = NdaRunDiagnostics {
            root_hash: 0x1234,
            valid: true,
            force_terminated: true,
            site_map_key: None, // not stored when truncated
            opcode_count: 256,
            sandbox_passed: Some(true),
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 0,
            site_map_misses: 0,
        };
        assert!(diag.valid);
        assert!(diag.force_terminated);
        assert!(diag.site_map_key.is_none());
    }

    #[test]
    fn engine_info_all_fields() {
        let info = EngineInfo {
            model_dir: "/models/test".to_string(),
            vocab_size: 151936,
            n_layers: 28,
            hidden_size: 3584,
            ffn_size: 18944,
            n_heads: 28,
            n_kv_heads: 4,
            head_dim: 128,
            max_seq_len: 32768,
            path1_loaded: true,
            path2_site_map_stats: "42 entries".to_string(),
            tokenizer_merge_count: 100000,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"ffn_size\":18944"));
        assert!(json.contains("\"head_dim\":128"));
        assert!(json.contains("\"max_seq_len\":32768"));
        assert!(json.contains("\"tokenizer_merge_count\":100000"));
        assert!(json.contains("\"path1_loaded\":true"));
    }

    #[test]
    fn engine_output_text_fields() {
        let output = EngineOutput::Text {
            text: "The answer is 42".to_string(),
            n_tokens: 5,
            elapsed_ms: 250,
        };
        match &output {
            EngineOutput::Text { text, n_tokens, elapsed_ms } => {
                assert_eq!(text, "The answer is 42");
                assert_eq!(*n_tokens, 5);
                assert_eq!(*elapsed_ms, 250);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn engine_output_nda_with_opcodes() {
        let output = EngineOutput::Nda {
            opcodes: vec![],
            root_hash: 0xFFFF,
            valid: true,
            force_terminated: false,
            site_map_key: Some(0xABCD),
            n_opcodes: 0,
            elapsed_ms: 100,
        };
        match output {
            EngineOutput::Nda {
                root_hash,
                valid,
                force_terminated,
                site_map_key,
                n_opcodes,
                elapsed_ms,
                ..
            } => {
                assert_eq!(root_hash, 0xFFFF);
                assert!(valid);
                assert!(!force_terminated);
                assert_eq!(site_map_key, Some(0xABCD));
                assert_eq!(n_opcodes, 0);
                assert_eq!(elapsed_ms, 100);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn per_second_high_throughput() {
        // 10000 tokens in 1 second
        let elapsed_us = 1_000_000u64;
        let output_count = 10000usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 10000.0).abs() < 0.01);
    }

    #[test]
    fn per_second_subsecond_timing() {
        // 50 tokens in 100ms = 500 tok/s
        let elapsed_us = 100_000u64;
        let output_count = 50usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 500.0).abs() < 0.01);
    }

    #[test]
    fn engine_status_snapshot_path1_not_loaded() {
        let snap = EngineStatusSnapshot {
            path1_loaded: false,
            path2_active: true,
            model_dir: "/models".to_string(),
            vocab_size: 32000,
            n_layers: 12,
            hidden_size: 768,
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"path1_loaded\":false"));
        assert!(json.contains("\"path2_active\":true"));
    }

    #[test]
    fn nda_diagnostics_site_map_stats() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: Some(42),
            opcode_count: 100,
            sandbox_passed: Some(true),
            scope_passed: Some(true),
            scope_similarity: Some(0.99),
            site_map_hits: 50,
            site_map_misses: 10,
        };
        // Cache hit rate = hits / (hits + misses)
        let total = diag.site_map_hits + diag.site_map_misses;
        let hit_rate = diag.site_map_hits as f64 / total as f64;
        assert!(hit_rate > 0.8, "hit rate should be > 80%, got {}", hit_rate);
    }

    #[test]
    fn engine_report_all_json_fields_present() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 1,
            output_count: 1,
            elapsed_us: 1,
            per_second: 1.0,
            text: Some("x".to_string()),
            nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false,
                path2_active: true,
                model_dir: "".to_string(),
                vocab_size: 0,
                n_layers: 0,
                hidden_size: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        // Verify all top-level fields are present
        for field in &[
            "path", "resolved_mode", "prompt_tokens", "output_count",
            "elapsed_us", "per_second", "text", "nda",
            "path1_lazy_loaded", "engine_status",
        ] {
            assert!(json.contains(field), "missing field: {}", field);
        }
    }

    // ── Block 130: comprehensive tests ──────────────────────────────────────

    // ── Debug format ─────────────────────────────────────────────────────

    #[test]
    fn engine_report_debug() {
        let report = EngineReport {
            path: "nda".to_string(),
            resolved_mode: "Nda".to_string(),
            prompt_tokens: 10,
            output_count: 50,
            elapsed_us: 5000,
            per_second: 10000.0,
            text: None,
            nda: None,
            path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true,
                path2_active: true,
                model_dir: "/m".to_string(),
                vocab_size: 100,
                n_layers: 2,
                hidden_size: 64,
            },
        };
        let dbg = format!("{:?}", report);
        assert!(dbg.contains("EngineReport"));
        assert!(dbg.contains("path"));
        assert!(dbg.contains("nda"));
    }

    #[test]
    fn nda_run_diagnostics_debug() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xFF,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 10,
            sandbox_passed: Some(true),
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 3,
            site_map_misses: 1,
        };
        let dbg = format!("{:?}", diag);
        assert!(dbg.contains("NdaRunDiagnostics"));
        assert!(dbg.contains("root_hash"));
    }

    #[test]
    fn engine_status_snapshot_debug() {
        let snap = EngineStatusSnapshot {
            path1_loaded: false,
            path2_active: true,
            model_dir: "/test".to_string(),
            vocab_size: 32000,
            n_layers: 12,
            hidden_size: 768,
        };
        let dbg = format!("{:?}", snap);
        assert!(dbg.contains("EngineStatusSnapshot"));
        assert!(dbg.contains("model_dir"));
    }

    #[test]
    fn engine_info_debug() {
        let info = EngineInfo {
            model_dir: "/m".to_string(),
            vocab_size: 100,
            n_layers: 2,
            hidden_size: 64,
            ffn_size: 256,
            n_heads: 4,
            n_kv_heads: 2,
            head_dim: 16,
            max_seq_len: 512,
            path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(),
            tokenizer_merge_count: 50,
        };
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("EngineInfo"));
        assert!(dbg.contains("vocab_size"));
    }

    // ── Clone independence ───────────────────────────────────────────────

    #[test]
    fn engine_status_snapshot_clone_independence() {
        let snap = EngineStatusSnapshot {
            path1_loaded: true,
            path2_active: true,
            model_dir: "/original".to_string(),
            vocab_size: 32000,
            n_layers: 12,
            hidden_size: 768,
        };
        let mut cloned = snap.clone();
        cloned.model_dir = "/modified".to_string();
        cloned.vocab_size = 999;
        assert_eq!(snap.model_dir, "/original");
        assert_eq!(snap.vocab_size, 32000);
    }

    #[test]
    fn nda_run_diagnostics_clone() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xABCD,
            valid: true,
            force_terminated: false,
            site_map_key: Some(42),
            opcode_count: 100,
            sandbox_passed: Some(true),
            scope_passed: Some(false),
            scope_similarity: Some(0.85),
            site_map_hits: 10,
            site_map_misses: 5,
        };
        let cloned = diag.clone();
        assert_eq!(cloned.root_hash, 0xABCD);
        assert_eq!(cloned.opcode_count, 100);
        assert_eq!(cloned.site_map_hits, 10);
    }

    #[test]
    fn engine_info_clone() {
        let info = EngineInfo {
            model_dir: "/models".to_string(),
            vocab_size: 151936,
            n_layers: 28,
            hidden_size: 3584,
            ffn_size: 18944,
            n_heads: 28,
            n_kv_heads: 4,
            head_dim: 128,
            max_seq_len: 32768,
            path1_loaded: true,
            path2_site_map_stats: "42 entries".to_string(),
            tokenizer_merge_count: 100000,
        };
        let cloned = info.clone();
        assert_eq!(cloned.vocab_size, 151936);
        assert_eq!(cloned.n_heads, 28);
        assert_eq!(cloned.tokenizer_merge_count, 100000);
    }

    // ── JSON all fields ──────────────────────────────────────────────────

    #[test]
    fn engine_status_snapshot_json_all_fields() {
        let snap = EngineStatusSnapshot {
            path1_loaded: true,
            path2_active: true,
            model_dir: "/models/test".to_string(),
            vocab_size: 32000,
            n_layers: 12,
            hidden_size: 768,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["path1_loaded"], true);
        assert_eq!(val["path2_active"], true);
        assert_eq!(val["model_dir"], "/models/test");
        assert_eq!(val["vocab_size"], 32000);
        assert_eq!(val["n_layers"], 12);
        assert_eq!(val["hidden_size"], 768);
    }

    #[test]
    fn nda_run_diagnostics_json_all_fields() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xDEADBEEF,
            valid: true,
            force_terminated: false,
            site_map_key: Some(0x1234),
            opcode_count: 128,
            sandbox_passed: Some(true),
            scope_passed: Some(true),
            scope_similarity: Some(0.95),
            site_map_hits: 5,
            site_map_misses: 3,
        };
        let json = serde_json::to_string(&diag).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["root_hash"].as_u64().unwrap(), 0xDEADBEEF);
        assert_eq!(val["valid"], true);
        assert_eq!(val["force_terminated"], false);
        assert_eq!(val["opcode_count"], 128);
        assert_eq!(val["site_map_hits"], 5);
        assert_eq!(val["site_map_misses"], 3);
    }

    #[test]
    fn engine_info_json_all_fields() {
        let info = EngineInfo {
            model_dir: "/m".to_string(),
            vocab_size: 100,
            n_layers: 2,
            hidden_size: 64,
            ffn_size: 256,
            n_heads: 4,
            n_kv_heads: 2,
            head_dim: 16,
            max_seq_len: 512,
            path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(),
            tokenizer_merge_count: 50,
        };
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["model_dir"], "/m");
        assert_eq!(val["vocab_size"], 100);
        assert_eq!(val["n_layers"], 2);
        assert_eq!(val["hidden_size"], 64);
        assert_eq!(val["ffn_size"], 256);
        assert_eq!(val["n_heads"], 4);
        assert_eq!(val["n_kv_heads"], 2);
        assert_eq!(val["head_dim"], 16);
        assert_eq!(val["max_seq_len"], 512);
        assert_eq!(val["path1_loaded"], false);
        assert_eq!(val["tokenizer_merge_count"], 50);
    }

    // ── per_second calculations ──────────────────────────────────────────

    #[test]
    fn per_second_single_token_one_second() {
        let elapsed_us = 1_000_000u64;
        let output_count = 1usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 1.0).abs() < 0.01);
    }

    #[test]
    fn per_second_millisecond_timing() {
        // 10 tokens in 10ms = 1000 tok/s
        let elapsed_us = 10_000u64;
        let output_count = 10usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 1000.0).abs() < 0.01);
    }

    #[test]
    fn per_second_very_fast() {
        // 1000 opcodes in 1ms
        let elapsed_us = 1_000u64;
        let output_count = 1000usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 1_000_000.0).abs() < 100.0);
    }

    // ── NdaRunDiagnostics field combinations ─────────────────────────────

    #[test]
    fn nda_diagnostics_sandbox_failed() {
        let diag = NdaRunDiagnostics {
            root_hash: 0x1234,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 50,
            sandbox_passed: Some(false),
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 0,
            site_map_misses: 0,
        };
        assert_eq!(diag.sandbox_passed, Some(false));
        assert!(diag.site_map_key.is_none());
    }

    #[test]
    fn nda_diagnostics_scope_failed() {
        let diag = NdaRunDiagnostics {
            root_hash: 0x5678,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 80,
            sandbox_passed: Some(true),
            scope_passed: Some(false),
            scope_similarity: Some(0.3),
            site_map_hits: 5,
            site_map_misses: 5,
        };
        assert_eq!(diag.scope_passed, Some(false));
        assert!(diag.scope_similarity.unwrap() < 0.5);
    }

    #[test]
    fn nda_diagnostics_zero_site_map_access() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: false,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 0,
            sandbox_passed: None,
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 0,
            site_map_misses: 0,
        };
        let total = diag.site_map_hits + diag.site_map_misses;
        assert_eq!(total, 0);
    }

    // ── EngineOutput variant exhaustiveness ──────────────────────────────

    #[test]
    fn engine_output_text_empty_string() {
        let output = EngineOutput::Text {
            text: String::new(),
            n_tokens: 0,
            elapsed_ms: 0,
        };
        match output {
            EngineOutput::Text { text, n_tokens, elapsed_ms } => {
                assert!(text.is_empty());
                assert_eq!(n_tokens, 0);
                assert_eq!(elapsed_ms, 0);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn engine_output_nda_site_map_key_none() {
        let output = EngineOutput::Nda {
            opcodes: vec![],
            root_hash: 0,
            valid: false,
            force_terminated: false,
            site_map_key: None,
            n_opcodes: 0,
            elapsed_ms: 0,
        };
        match output {
            EngineOutput::Nda { site_map_key, valid, .. } => {
                assert!(site_map_key.is_none());
                assert!(!valid);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn engine_report_text_path_lazy_loaded_true() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 5,
            output_count: 10,
            elapsed_us: 1000,
            per_second: 10000.0,
            text: Some("hi".to_string()),
            nda: None,
            path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true,
                path2_active: true,
                model_dir: "/m".to_string(),
                vocab_size: 100,
                n_layers: 2,
                hidden_size: 64,
            },
        };
        assert!(report.path1_lazy_loaded);
        assert!(report.text.is_some());
        assert!(report.nda.is_none());
    }

    // ── Block 146: JSON round-trip tests ──────────────────────────────────

    #[test]
    fn engine_report_json_roundtrip_text() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 42,
            output_count: 128,
            elapsed_us: 500_000,
            per_second: 256.0,
            text: Some("hello world".to_string()),
            nda: None,
            path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true,
                path2_active: true,
                model_dir: "/models/test".to_string(),
                vocab_size: 32000,
                n_layers: 12,
                hidden_size: 768,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["path"], "text");
        assert_eq!(val["prompt_tokens"], 42);
        assert_eq!(val["output_count"], 128);
        assert_eq!(val["text"], "hello world");
        assert!(val["nda"].is_null());
        assert_eq!(val["path1_lazy_loaded"], true);
        assert_eq!(val["engine_status"]["vocab_size"], 32000);
        assert_eq!(val["per_second"], 256.0);
    }

    #[test]
    fn engine_report_json_roundtrip_nda() {
        let report = EngineReport {
            path: "nda".to_string(),
            resolved_mode: "Nda".to_string(),
            prompt_tokens: 20,
            output_count: 64,
            elapsed_us: 200_000,
            per_second: 320.0,
            text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0xCAFEBABE,
                valid: true,
                force_terminated: false,
                site_map_key: Some(0xFF),
                opcode_count: 64,
                sandbox_passed: Some(true),
                scope_passed: Some(false),
                scope_similarity: Some(0.72),
                site_map_hits: 10,
                site_map_misses: 2,
            }),
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false,
                path2_active: true,
                model_dir: "/models/nda".to_string(),
                vocab_size: 151936,
                n_layers: 24,
                hidden_size: 1024,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["path"], "nda");
        assert!(val["text"].is_null());
        assert_eq!(val["nda"]["root_hash"], 0xCAFEBABE as u64);
        assert_eq!(val["nda"]["opcode_count"], 64);
        assert_eq!(val["nda"]["scope_similarity"], 0.72);
        assert_eq!(val["prompt_tokens"], 20);
        assert_eq!(val["path1_lazy_loaded"], false);
    }

    #[test]
    fn nda_run_diagnostics_json_roundtrip() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xDEADBEEFCAFE,
            valid: true,
            force_terminated: true,
            site_map_key: Some(0x123456789ABC),
            opcode_count: 512,
            sandbox_passed: Some(true),
            scope_passed: Some(true),
            scope_similarity: Some(0.99),
            site_map_hits: 100,
            site_map_misses: 5,
        };
        let json = serde_json::to_string(&diag).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["root_hash"], 0xDEADBEEFCAFE_u64);
        assert_eq!(val["valid"], true);
        assert_eq!(val["force_terminated"], true);
        assert_eq!(val["site_map_key"], 0x123456789ABC_u64);
        assert_eq!(val["opcode_count"], 512);
        assert_eq!(val["site_map_hits"], 100);
        assert_eq!(val["scope_similarity"], 0.99);
    }

    #[test]
    fn engine_status_snapshot_json_roundtrip() {
        let snap = EngineStatusSnapshot {
            path1_loaded: true,
            path2_active: true,
            model_dir: "/path/to/models".to_string(),
            vocab_size: 151936,
            n_layers: 28,
            hidden_size: 3584,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["path1_loaded"], true);
        assert_eq!(val["model_dir"], "/path/to/models");
        assert_eq!(val["vocab_size"], 151936);
        assert_eq!(val["n_layers"], 28);
        assert_eq!(val["hidden_size"], 3584);
    }

    #[test]
    fn engine_info_json_roundtrip() {
        let info = EngineInfo {
            model_dir: "/models/qwen".to_string(),
            vocab_size: 151936,
            n_layers: 28,
            hidden_size: 3584,
            ffn_size: 18944,
            n_heads: 28,
            n_kv_heads: 4,
            head_dim: 128,
            max_seq_len: 32768,
            path1_loaded: true,
            path2_site_map_stats: "42 entries".to_string(),
            tokenizer_merge_count: 100000,
        };
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["model_dir"], "/models/qwen");
        assert_eq!(val["vocab_size"], 151936);
        assert_eq!(val["ffn_size"], 18944);
        assert_eq!(val["n_kv_heads"], 4);
        assert_eq!(val["head_dim"], 128);
        assert_eq!(val["max_seq_len"], 32768);
        assert_eq!(val["tokenizer_merge_count"], 100000);
    }

    // ── Pretty JSON ─────────────────────────────────────────────────────

    #[test]
    fn engine_report_pretty_json() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 1,
            output_count: 1,
            elapsed_us: 1,
            per_second: 1.0,
            text: Some("x".to_string()),
            nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false,
                path2_active: true,
                model_dir: "".to_string(),
                vocab_size: 0,
                n_layers: 0,
                hidden_size: 0,
            },
        };
        let pretty = serde_json::to_string_pretty(&report).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));
        // Verify content is present
        assert!(pretty.contains("\"path\": \"text\""));
        assert!(pretty.contains("\"prompt_tokens\": 1"));
    }

    #[test]
    fn nda_run_diagnostics_pretty_json() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xFF,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 10,
            sandbox_passed: Some(true),
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 3,
            site_map_misses: 1,
        };
        let pretty = serde_json::to_string_pretty(&diag).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("\"root_hash\": 255"));
        assert!(pretty.contains("\"valid\": true"));
    }

    // ── Boundary values ─────────────────────────────────────────────────

    #[test]
    fn engine_report_zero_all_numeric_fields() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 0,
            output_count: 0,
            elapsed_us: 0,
            per_second: 0.0,
            text: None,
            nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false,
                path2_active: false,
                model_dir: String::new(),
                vocab_size: 0,
                n_layers: 0,
                hidden_size: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["prompt_tokens"], 0);
        assert_eq!(val["output_count"], 0);
        assert_eq!(val["elapsed_us"], 0);
        assert_eq!(val["per_second"], 0.0);
    }

    #[test]
    fn engine_report_large_values() {
        let report = EngineReport {
            path: "nda".to_string(),
            resolved_mode: "Nda".to_string(),
            prompt_tokens: 1_000_000,
            output_count: 1_000_000,
            elapsed_us: u64::MAX,
            per_second: 1e15,
            text: None,
            nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false,
                path2_active: true,
                model_dir: String::new(),
                vocab_size: 1_000_000,
                n_layers: 1_000_000,
                hidden_size: 1_000_000,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["prompt_tokens"], 1_000_000);
        assert_eq!(val["elapsed_us"], u64::MAX);
    }

    #[test]
    fn nda_diagnostics_u64_max_root_hash() {
        let diag = NdaRunDiagnostics {
            root_hash: u64::MAX,
            valid: true,
            force_terminated: false,
            site_map_key: Some(u64::MAX),
            opcode_count: 100_000,
            sandbox_passed: Some(true),
            scope_passed: Some(true),
            scope_similarity: Some(1.0),
            site_map_hits: 50_000,
            site_map_misses: 50_000,
        };
        let json = serde_json::to_string(&diag).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["root_hash"].as_u64().unwrap(), u64::MAX);
        assert_eq!(val["site_map_key"].as_u64().unwrap(), u64::MAX);
    }

    #[test]
    fn engine_status_snapshot_empty_model_dir() {
        let snap = EngineStatusSnapshot {
            path1_loaded: false,
            path2_active: false,
            model_dir: String::new(),
            vocab_size: 0,
            n_layers: 0,
            hidden_size: 0,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["model_dir"], "");
        assert_eq!(val["path1_loaded"], false);
        assert_eq!(val["path2_active"], false);
    }

    #[test]
    fn engine_info_zero_config() {
        let info = EngineInfo {
            model_dir: String::new(),
            vocab_size: 0,
            n_layers: 0,
            hidden_size: 0,
            ffn_size: 0,
            n_heads: 0,
            n_kv_heads: 0,
            head_dim: 0,
            max_seq_len: 0,
            path1_loaded: false,
            path2_site_map_stats: String::new(),
            tokenizer_merge_count: 0,
        };
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["vocab_size"], 0);
        assert_eq!(val["n_heads"], 0);
        assert_eq!(val["head_dim"], 0);
        assert_eq!(val["max_seq_len"], 0);
        assert_eq!(val["tokenizer_merge_count"], 0);
    }

    // ── Status display formatting ───────────────────────────────────────

    #[test]
    fn status_display_valid_complete() {
        let valid = true;
        let force_terminated = false;
        let status = match (valid, force_terminated) {
            (true, false) => "VALID (complete)",
            (true, true) => "VALID (truncated)",
            _ => "INVALID",
        };
        assert_eq!(status, "VALID (complete)");
    }

    #[test]
    fn status_display_valid_truncated() {
        let valid = true;
        let force_terminated = true;
        let status = match (valid, force_terminated) {
            (true, false) => "VALID (complete)",
            (true, true) => "VALID (truncated)",
            _ => "INVALID",
        };
        assert_eq!(status, "VALID (truncated)");
    }

    #[test]
    fn status_display_invalid() {
        let valid = false;
        let force_terminated = false;
        let status = match (valid, force_terminated) {
            (true, false) => "VALID (complete)",
            (true, true) => "VALID (truncated)",
            _ => "INVALID",
        };
        assert_eq!(status, "INVALID");
    }

    #[test]
    fn status_display_invalid_with_force_terminated() {
        // Edge case: invalid + force_terminated = still INVALID
        let valid = false;
        let force_terminated = true;
        let status = match (valid, force_terminated) {
            (true, false) => "VALID (complete)",
            (true, true) => "VALID (truncated)",
            _ => "INVALID",
        };
        assert_eq!(status, "INVALID");
    }

    // ── Cache hit rate calculations ─────────────────────────────────────

    #[test]
    fn cache_hit_rate_perfect() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 100,
            sandbox_passed: None,
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 100,
            site_map_misses: 0,
        };
        let total = diag.site_map_hits + diag.site_map_misses;
        let hit_rate = diag.site_map_hits as f64 / total as f64;
        assert!((hit_rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_hit_rate_half() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 50,
            sandbox_passed: None,
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 50,
            site_map_misses: 50,
        };
        let total = diag.site_map_hits + diag.site_map_misses;
        let hit_rate = diag.site_map_hits as f64 / total as f64;
        assert!((hit_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn cache_hit_rate_no_hits() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: false,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 10,
            sandbox_passed: None,
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 0,
            site_map_misses: 100,
        };
        let total = diag.site_map_hits + diag.site_map_misses;
        let hit_rate = diag.site_map_hits as f64 / total as f64;
        assert_eq!(hit_rate, 0.0);
    }

    #[test]
    fn cache_hit_rate_high_volume() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 10000,
            sandbox_passed: None,
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 9999,
            site_map_misses: 1,
        };
        let total = diag.site_map_hits + diag.site_map_misses;
        let hit_rate = diag.site_map_hits as f64 / total as f64;
        assert!(hit_rate > 0.999);
    }

    // ── EngineReport mutual exclusivity ─────────────────────────────────

    #[test]
    fn engine_report_text_path_has_no_nda_diagnostics() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 10,
            output_count: 50,
            elapsed_us: 5000,
            per_second: 10000.0,
            text: Some("output".to_string()),
            nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true,
                path2_active: true,
                model_dir: "/m".to_string(),
                vocab_size: 100,
                n_layers: 2,
                hidden_size: 64,
            },
        };
        assert!(report.text.is_some());
        assert!(report.nda.is_none());
        assert_eq!(report.path, "text");
    }

    #[test]
    fn engine_report_nda_path_has_no_text() {
        let report = EngineReport {
            path: "nda".to_string(),
            resolved_mode: "Nda".to_string(),
            prompt_tokens: 10,
            output_count: 50,
            elapsed_us: 5000,
            per_second: 10000.0,
            text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0,
                valid: true,
                force_terminated: false,
                site_map_key: None,
                opcode_count: 50,
                sandbox_passed: None,
                scope_passed: None,
                scope_similarity: None,
                site_map_hits: 0,
                site_map_misses: 0,
            }),
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true,
                path2_active: true,
                model_dir: "/m".to_string(),
                vocab_size: 100,
                n_layers: 2,
                hidden_size: 64,
            },
        };
        assert!(report.text.is_none());
        assert!(report.nda.is_some());
        assert_eq!(report.path, "nda");
    }

    // ── Clone + modify independence ─────────────────────────────────────

    #[test]
    fn nda_diagnostics_clone_independence() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xABCD,
            valid: true,
            force_terminated: false,
            site_map_key: Some(42),
            opcode_count: 100,
            sandbox_passed: Some(true),
            scope_passed: Some(true),
            scope_similarity: Some(0.95),
            site_map_hits: 10,
            site_map_misses: 5,
        };
        let mut cloned = diag.clone();
        cloned.root_hash = 0xFFFF;
        cloned.valid = false;
        cloned.site_map_key = None;
        cloned.opcode_count = 0;
        // Original unchanged
        assert_eq!(diag.root_hash, 0xABCD);
        assert!(diag.valid);
        assert_eq!(diag.site_map_key, Some(42));
        assert_eq!(diag.opcode_count, 100);
    }

    #[test]
    fn engine_info_clone_independence() {
        let info = EngineInfo {
            model_dir: "/original".to_string(),
            vocab_size: 100,
            n_layers: 2,
            hidden_size: 64,
            ffn_size: 256,
            n_heads: 4,
            n_kv_heads: 2,
            head_dim: 16,
            max_seq_len: 512,
            path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(),
            tokenizer_merge_count: 50,
        };
        let mut cloned = info.clone();
        cloned.model_dir = "/modified".to_string();
        cloned.vocab_size = 999;
        cloned.path1_loaded = true;
        // Original unchanged
        assert_eq!(info.model_dir, "/original");
        assert_eq!(info.vocab_size, 100);
        assert_eq!(info.path1_loaded, false);
    }

    #[test]
    fn engine_report_clone_independence() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 10,
            output_count: 50,
            elapsed_us: 5000,
            per_second: 10000.0,
            text: Some("original".to_string()),
            nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true,
                path2_active: true,
                model_dir: "/m".to_string(),
                vocab_size: 100,
                n_layers: 2,
                hidden_size: 64,
            },
        };
        let mut cloned = report.clone();
        cloned.path = "nda".to_string();
        cloned.text = Some("modified".to_string());
        cloned.path1_lazy_loaded = true;
        // Original unchanged
        assert_eq!(report.path, "text");
        assert_eq!(report.text, Some("original".to_string()));
        assert!(!report.path1_lazy_loaded);
    }

    // ── scope_similarity bounds ─────────────────────────────────────────

    #[test]
    fn nda_diagnostics_scope_similarity_zero() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 10,
            sandbox_passed: None,
            scope_passed: Some(false),
            scope_similarity: Some(0.0),
            site_map_hits: 0,
            site_map_misses: 0,
        };
        assert_eq!(diag.scope_similarity, Some(0.0));
        assert_eq!(diag.scope_passed, Some(false));
    }

    #[test]
    fn nda_diagnostics_scope_similarity_one() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: Some(1),
            opcode_count: 10,
            sandbox_passed: Some(true),
            scope_passed: Some(true),
            scope_similarity: Some(1.0),
            site_map_hits: 10,
            site_map_misses: 0,
        };
        assert_eq!(diag.scope_similarity, Some(1.0));
        assert_eq!(diag.scope_passed, Some(true));
    }

    #[test]
    fn nda_diagnostics_scope_similarity_mid_range() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 50,
            sandbox_passed: Some(true),
            scope_passed: Some(true),
            scope_similarity: Some(0.5),
            site_map_hits: 5,
            site_map_misses: 5,
        };
        let sim = diag.scope_similarity.unwrap();
        assert!(sim > 0.0 && sim < 1.0);
    }

    // ── EngineOutput edge cases ─────────────────────────────────────────

    #[test]
    fn engine_output_nda_many_opcodes() {
        let opcodes = vec![]; // NdaOpcode doesn't have a simple constructor
        let output = EngineOutput::Nda {
            opcodes,
            root_hash: u64::MAX,
            valid: true,
            force_terminated: false,
            site_map_key: Some(u64::MAX),
            n_opcodes: 100_000,
            elapsed_ms: 60_000,
        };
        match output {
            EngineOutput::Nda { n_opcodes, elapsed_ms, root_hash, .. } => {
                assert_eq!(n_opcodes, 100_000);
                assert_eq!(elapsed_ms, 60_000);
                assert_eq!(root_hash, u64::MAX);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn engine_output_text_long_text() {
        let long_text = "a".repeat(1_000_000);
        let output = EngineOutput::Text {
            text: long_text.clone(),
            n_tokens: 250_000,
            elapsed_ms: 30_000,
        };
        match output {
            EngineOutput::Text { text, n_tokens, .. } => {
                assert_eq!(text.len(), 1_000_000);
                assert_eq!(n_tokens, 250_000);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn engine_output_text_unicode_content() {
        let output = EngineOutput::Text {
            text: "Hello 世界 🌍 Ñ".to_string(),
            n_tokens: 5,
            elapsed_ms: 100,
        };
        match output {
            EngineOutput::Text { text, .. } => {
                assert!(text.contains("世界"));
                assert!(text.contains("🌍"));
                assert!(text.contains("Ñ"));
            }
            _ => panic!("wrong variant"),
        }
    }

    // ── JSON key count verification ─────────────────────────────────────

    #[test]
    fn engine_report_json_key_count() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 1,
            output_count: 1,
            elapsed_us: 1,
            per_second: 1.0,
            text: None,
            nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false,
                path2_active: true,
                model_dir: "".to_string(),
                vocab_size: 0,
                n_layers: 0,
                hidden_size: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = val.as_object().unwrap();
        // EngineReport has 10 fields
        assert_eq!(obj.len(), 10);
    }

    #[test]
    fn nda_run_diagnostics_json_key_count() {
        let diag = NdaRunDiagnostics {
            root_hash: 0,
            valid: false,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 0,
            sandbox_passed: None,
            scope_passed: None,
            scope_similarity: None,
            site_map_hits: 0,
            site_map_misses: 0,
        };
        let json = serde_json::to_string(&diag).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = val.as_object().unwrap();
        // NdaRunDiagnostics has 10 fields
        assert_eq!(obj.len(), 10);
    }

    #[test]
    fn engine_status_snapshot_json_key_count() {
        let snap = EngineStatusSnapshot {
            path1_loaded: false,
            path2_active: true,
            model_dir: "".to_string(),
            vocab_size: 0,
            n_layers: 0,
            hidden_size: 0,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = val.as_object().unwrap();
        // EngineStatusSnapshot has 6 fields
        assert_eq!(obj.len(), 6);
    }

    #[test]
    fn engine_info_json_key_count() {
        let info = EngineInfo {
            model_dir: "".to_string(),
            vocab_size: 0,
            n_layers: 0,
            hidden_size: 0,
            ffn_size: 0,
            n_heads: 0,
            n_kv_heads: 0,
            head_dim: 0,
            max_seq_len: 0,
            path1_loaded: false,
            path2_site_map_stats: "".to_string(),
            tokenizer_merge_count: 0,
        };
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = val.as_object().unwrap();
        // EngineInfo has 12 fields
        assert_eq!(obj.len(), 12);
    }

    // ── per_second edge cases ───────────────────────────────────────────

    #[test]
    fn per_second_single_microsecond() {
        // 1 token in 1μs = 1M tok/s
        let elapsed_us = 1u64;
        let output_count = 1usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 1_000_000.0).abs() < 1.0);
    }

    #[test]
    fn per_second_large_batch_slow() {
        // 1000 tokens in 60 seconds
        let elapsed_us = 60_000_000u64;
        let output_count = 1000usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 1000.0 / 60.0).abs() < 0.01);
    }

    #[test]
    fn per_second_zero_output_nonzero_time() {
        let elapsed_us = 1_000_000u64;
        let output_count = 0usize;
        let per_second = if elapsed_us > 0 {
            (output_count as f64) / (elapsed_us as f64 / 1_000_000.0)
        } else {
            0.0
        };
        assert_eq!(per_second, 0.0);
    }

    // ── NdaRunDiagnostics sandbox/scope combinations ────────────────────

    #[test]
    fn nda_diagnostics_both_checks_passed() {
        let diag = NdaRunDiagnostics {
            root_hash: 0x1234,
            valid: true,
            force_terminated: false,
            site_map_key: Some(1),
            opcode_count: 100,
            sandbox_passed: Some(true),
            scope_passed: Some(true),
            scope_similarity: Some(0.95),
            site_map_hits: 10,
            site_map_misses: 0,
        };
        assert_eq!(diag.sandbox_passed, Some(true));
        assert_eq!(diag.scope_passed, Some(true));
        assert!(diag.scope_similarity.unwrap() > 0.9);
    }

    #[test]
    fn nda_diagnostics_both_checks_failed() {
        let diag = NdaRunDiagnostics {
            root_hash: 0x5678,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 50,
            sandbox_passed: Some(false),
            scope_passed: Some(false),
            scope_similarity: Some(0.1),
            site_map_hits: 0,
            site_map_misses: 10,
        };
        assert_eq!(diag.sandbox_passed, Some(false));
        assert_eq!(diag.scope_passed, Some(false));
        assert!(diag.scope_similarity.unwrap() < 0.5);
    }

    #[test]
    fn nda_diagnostics_sandbox_pass_scope_fail() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xABCD,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 75,
            sandbox_passed: Some(true),
            scope_passed: Some(false),
            scope_similarity: Some(0.3),
            site_map_hits: 5,
            site_map_misses: 5,
        };
        assert_eq!(diag.sandbox_passed, Some(true));
        assert_eq!(diag.scope_passed, Some(false));
    }

    #[test]
    fn nda_diagnostics_sandbox_fail_scope_pass() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xEF01,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            opcode_count: 60,
            sandbox_passed: Some(false),
            scope_passed: Some(true),
            scope_similarity: Some(0.85),
            site_map_hits: 8,
            site_map_misses: 2,
        };
        assert_eq!(diag.sandbox_passed, Some(false));
        assert_eq!(diag.scope_passed, Some(true));
    }

    // ── EngineReport resolved_mode values ───────────────────────────────

    #[test]
    fn engine_report_resolved_mode_text() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 0,
            output_count: 0,
            elapsed_us: 0,
            per_second: 0.0,
            text: None,
            nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false,
                path2_active: true,
                model_dir: "".to_string(),
                vocab_size: 0,
                n_layers: 0,
                hidden_size: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"resolved_mode\":\"Text\""));
    }

    #[test]
    fn engine_report_resolved_mode_nda() {
        let report = EngineReport {
            path: "nda".to_string(),
            resolved_mode: "Nda".to_string(),
            prompt_tokens: 0,
            output_count: 0,
            elapsed_us: 0,
            per_second: 0.0,
            text: None,
            nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false,
                path2_active: true,
                model_dir: "".to_string(),
                vocab_size: 0,
                n_layers: 0,
                hidden_size: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"resolved_mode\":\"Nda\""));
    }

    // ── EngineStatusSnapshot path combinations ──────────────────────────

    #[test]
    fn engine_status_both_paths_active() {
        let snap = EngineStatusSnapshot {
            path1_loaded: true,
            path2_active: true,
            model_dir: "/models".to_string(),
            vocab_size: 32000,
            n_layers: 12,
            hidden_size: 768,
        };
        assert!(snap.path1_loaded);
        assert!(snap.path2_active);
    }

    #[test]
    fn engine_status_only_path2_active() {
        let snap = EngineStatusSnapshot {
            path1_loaded: false,
            path2_active: true,
            model_dir: "/models".to_string(),
            vocab_size: 32000,
            n_layers: 12,
            hidden_size: 768,
        };
        assert!(!snap.path1_loaded);
        assert!(snap.path2_active);
    }

    // ── EngineInfo head_dim derivation check ────────────────────────────

    #[test]
    fn engine_info_head_dim_consistent() {
        // head_dim should equal hidden_size / n_heads
        let info = EngineInfo {
            model_dir: "/m".to_string(),
            vocab_size: 32000,
            n_layers: 12,
            hidden_size: 768,
            ffn_size: 2048,
            n_heads: 12,
            n_kv_heads: 4,
            head_dim: 64, // 768 / 12 = 64
            max_seq_len: 4096,
            path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(),
            tokenizer_merge_count: 100,
        };
        assert_eq!(info.head_dim, info.hidden_size / info.n_heads);
    }

    #[test]
    fn engine_info_ffn_typical_ratio() {
        // ffn_size is typically 2-4x hidden_size for transformer models
        let info = EngineInfo {
            model_dir: "/m".to_string(),
            vocab_size: 32000,
            n_layers: 12,
            hidden_size: 768,
            ffn_size: 2048,
            n_heads: 12,
            n_kv_heads: 4,
            head_dim: 64,
            max_seq_len: 4096,
            path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(),
            tokenizer_merge_count: 100,
        };
        let ratio = info.ffn_size as f64 / info.hidden_size as f64;
        assert!(ratio >= 2.0 && ratio <= 4.0, "FFN ratio {} out of typical range", ratio);
    }
}
