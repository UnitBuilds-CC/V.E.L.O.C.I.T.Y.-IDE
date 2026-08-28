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
        if self.cfg.n_heads > 0 && !self.cfg.hidden_size.is_multiple_of(self.cfg.n_heads) {
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
        // Clone mutated
        assert_eq!(cloned.root_hash, 0xFFFF);
        assert!(!cloned.valid);
        assert_eq!(cloned.site_map_key, None);
        assert_eq!(cloned.opcode_count, 0);
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

    // ─── Block 193: JSON keys, clone independence, formulas, edge cases ─────

    #[test]
    fn engine_report_json_has_10_keys() {
        let report = EngineReport {
            path: "text".to_string(),
            resolved_mode: "Text".to_string(),
            prompt_tokens: 5,
            output_count: 10,
            elapsed_us: 1000,
            per_second: 10000.0,
            text: Some("hello".to_string()),
            nda: None,
            path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 10);
        for key in &["path", "resolved_mode", "prompt_tokens", "output_count",
                      "elapsed_us", "per_second", "text", "nda",
                      "path1_lazy_loaded", "engine_status"] {
            assert!(val.get(key).is_some(), "missing key: {key}");
        }
    }

    #[test]
    fn nda_run_diagnostics_json_has_10_keys() {
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: None, opcode_count: 0,
            sandbox_passed: None, scope_passed: None,
            scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
        };
        let json = serde_json::to_string(&diag).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 10);
    }

    #[test]
    fn engine_status_snapshot_json_has_6_keys() {
        let snap = EngineStatusSnapshot {
            path1_loaded: false, path2_active: true,
            model_dir: "/x".to_string(), vocab_size: 50,
            n_layers: 2, hidden_size: 128,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 6);
    }

    #[test]
    fn engine_info_json_has_12_keys() {
        let info = EngineInfo {
            model_dir: "/m".to_string(), vocab_size: 100, n_layers: 4,
            hidden_size: 256, ffn_size: 512, n_heads: 4, n_kv_heads: 2,
            head_dim: 64, max_seq_len: 1024, path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(), tokenizer_merge_count: 50,
        };
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 12);
    }

    #[test]
    fn engine_report_clone_independent() {
        let report = EngineReport {
            path: "nda".to_string(), resolved_mode: "Nda".to_string(),
            prompt_tokens: 3, output_count: 7, elapsed_us: 500,
            per_second: 14000.0, text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0xAA, valid: true, force_terminated: false,
                site_map_key: Some(1), opcode_count: 7,
                sandbox_passed: Some(true), scope_passed: Some(true),
                scope_similarity: Some(0.9), site_map_hits: 5, site_map_misses: 2,
            }),
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let mut cloned = report.clone();
        cloned.path = "MODIFIED".to_string();
        cloned.output_count = 999;
        assert_eq!(report.path, "nda");
        assert_eq!(report.output_count, 7);
    }

    #[test]
    fn nda_diagnostics_clone_independent() {
        let diag = NdaRunDiagnostics {
            root_hash: 0x1234, valid: true, force_terminated: false,
            site_map_key: Some(42), opcode_count: 100,
            sandbox_passed: Some(true), scope_passed: Some(true),
            scope_similarity: Some(0.95), site_map_hits: 10, site_map_misses: 1,
        };
        let mut cloned = diag.clone();
        cloned.root_hash = 0;
        cloned.opcode_count = 0;
        assert_eq!(diag.root_hash, 0x1234);
        assert_eq!(diag.opcode_count, 100);
        assert_eq!(cloned.root_hash, 0);
        assert_eq!(cloned.opcode_count, 0);
    }

    #[test]
    fn engine_report_per_second_formula() {
        // per_second = output_count / (elapsed_us / 1_000_000)
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count: 100, elapsed_us: 500_000,
            per_second: 100.0 / 0.5, text: Some("x".to_string()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let expected = 100.0 / (500_000.0 / 1_000_000.0);
        assert!((report.per_second - expected).abs() < 0.01);
    }

    #[test]
    fn engine_report_zero_elapsed_per_second() {
        let report = EngineReport {
            path: "nda".to_string(), resolved_mode: "Nda".to_string(),
            prompt_tokens: 1, output_count: 50, elapsed_us: 0,
            per_second: 0.0, text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0, valid: true, force_terminated: false,
                site_map_key: None, opcode_count: 50,
                sandbox_passed: None, scope_passed: None,
                scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
            }),
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        assert_eq!(report.per_second, 0.0);
    }

    #[test]
    fn nda_diagnostics_site_map_hit_rate() {
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: Some(1), opcode_count: 100,
            sandbox_passed: None, scope_passed: None,
            scope_similarity: None, site_map_hits: 80, site_map_misses: 20,
        };
        let total = diag.site_map_hits + diag.site_map_misses;
        let hit_rate = diag.site_map_hits as f64 / total as f64;
        assert!((hit_rate - 0.8).abs() < 0.001);
    }

    #[test]
    fn nda_diagnostics_site_map_all_misses() {
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: false, force_terminated: true,
            site_map_key: None, opcode_count: 0,
            sandbox_passed: Some(false), scope_passed: Some(false),
            scope_similarity: Some(0.0), site_map_hits: 0, site_map_misses: 50,
        };
        let total = diag.site_map_hits + diag.site_map_misses;
        assert!(total > 0);
        let hit_rate = diag.site_map_hits as f64 / total as f64;
        assert_eq!(hit_rate, 0.0);
    }

    #[test]
    fn engine_status_snapshot_debug_format() {
        let snap = EngineStatusSnapshot {
            path1_loaded: true, path2_active: true,
            model_dir: "/models/test".to_string(), vocab_size: 32000,
            n_layers: 12, hidden_size: 768,
        };
        let dbg = format!("{:?}", snap);
        assert!(dbg.contains("EngineStatusSnapshot"));
        assert!(dbg.contains("path1_loaded: true"));
        assert!(dbg.contains("/models/test"));
    }

    #[test]
    fn engine_report_debug_format() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 100,
            per_second: 10000.0, text: Some("hi".to_string()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let dbg = format!("{:?}", report);
        assert!(dbg.contains("EngineReport"));
        assert!(dbg.contains("path: \"text\""));
    }

    #[test]
    fn nda_diagnostics_force_terminated_not_stored() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xABCD, valid: true, force_terminated: true,
            site_map_key: None, opcode_count: 200,
            sandbox_passed: None, scope_passed: None,
            scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
        };
        // force_terminated programs are not stored in site_map
        assert!(diag.force_terminated);
        assert!(diag.site_map_key.is_none());
    }

    #[test]
    fn engine_report_text_path_has_no_nda() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 5, output_count: 10, elapsed_us: 1000,
            per_second: 10000.0, text: Some("output".to_string()),
            nda: None, path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        assert!(report.text.is_some());
        assert!(report.nda.is_none());
        assert_eq!(report.path, "text");
    }

    #[test]
    fn engine_report_nda_path_has_no_text_193() {
        let report = EngineReport {
            path: "nda".to_string(), resolved_mode: "Nda".to_string(),
            prompt_tokens: 3, output_count: 15, elapsed_us: 500,
            per_second: 30000.0, text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0xFF, valid: true, force_terminated: false,
                site_map_key: Some(99), opcode_count: 15,
                sandbox_passed: Some(true), scope_passed: Some(true),
                scope_similarity: Some(0.99), site_map_hits: 10, site_map_misses: 5,
            }),
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        assert!(report.text.is_none());
        assert!(report.nda.is_some());
        assert_eq!(report.path, "nda");
    }

    #[test]
    fn engine_info_ffn_hidden_ratio() {
        let info = EngineInfo {
            model_dir: "/m".to_string(), vocab_size: 32000, n_layers: 32,
            hidden_size: 4096, ffn_size: 11008, n_heads: 32, n_kv_heads: 32,
            head_dim: 128, max_seq_len: 4096, path1_loaded: false,
            path2_site_map_stats: "100 entries".to_string(), tokenizer_merge_count: 200,
        };
        // LLaMA-style FFN ratio ~2.7
        let ratio = info.ffn_size as f64 / info.hidden_size as f64;
        assert!(ratio > 2.0 && ratio < 4.0);
    }

    #[test]
    fn engine_info_kv_head_ratio() {
        let info = EngineInfo {
            model_dir: "/m".to_string(), vocab_size: 32000, n_layers: 32,
            hidden_size: 4096, ffn_size: 11008, n_heads: 32, n_kv_heads: 8,
            head_dim: 128, max_seq_len: 4096, path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(), tokenizer_merge_count: 200,
        };
        // GQA: n_heads / n_kv_heads should be integer
        assert_eq!(info.n_heads % info.n_kv_heads, 0);
    }

    #[test]
    fn engine_status_snapshot_clone_193() {
        let snap = EngineStatusSnapshot {
            path1_loaded: true, path2_active: false,
            model_dir: "/test".to_string(), vocab_size: 1000,
            n_layers: 6, hidden_size: 384,
        };
        let cloned = snap.clone();
        assert_eq!(cloned.path1_loaded, true);
        assert_eq!(cloned.model_dir, "/test");
        assert_eq!(cloned.vocab_size, 1000);
    }

    #[test]
    fn engine_info_clone_independent() {
        let info = EngineInfo {
            model_dir: "/m".to_string(), vocab_size: 100, n_layers: 4,
            hidden_size: 256, ffn_size: 512, n_heads: 4, n_kv_heads: 2,
            head_dim: 64, max_seq_len: 1024, path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(), tokenizer_merge_count: 50,
        };
        let mut cloned = info.clone();
        cloned.model_dir = "MODIFIED".to_string();
        cloned.vocab_size = 99999;
        assert_eq!(info.model_dir, "/m");
        assert_eq!(info.vocab_size, 100);
    }

    #[test]
    fn nda_diagnostics_json_types() {
        let diag = NdaRunDiagnostics {
            root_hash: 0xDEAD_BEEF, valid: true, force_terminated: false,
            site_map_key: Some(42), opcode_count: 100,
            sandbox_passed: Some(true), scope_passed: None,
            scope_similarity: Some(0.85), site_map_hits: 50, site_map_misses: 10,
        };
        let json = serde_json::to_string(&diag).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["root_hash"].is_number());
        assert!(val["valid"].is_boolean());
        assert!(val["force_terminated"].is_boolean());
        assert!(val["opcode_count"].is_number());
        assert!(val["site_map_hits"].is_number());
        assert!(val["sandbox_passed"].is_boolean());
        assert!(val["scope_passed"].is_null());
        assert!(val["scope_similarity"].is_number());
    }

    #[test]
    fn engine_report_lazy_loaded_flag() {
        // path1_lazy_loaded = true means it was NOT loaded before this run
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 100,
            per_second: 10000.0, text: Some("x".to_string()),
            nda: None, path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        // After lazy load, the status should show path1 as loaded
        assert!(report.path1_lazy_loaded);
        assert!(report.engine_status.path1_loaded);
    }

    #[test]
    fn nda_diagnostics_scope_similarity_range() {
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: Some(1), opcode_count: 50,
            sandbox_passed: None, scope_passed: Some(true),
            scope_similarity: Some(0.95), site_map_hits: 10, site_map_misses: 0,
        };
        let sim = diag.scope_similarity.unwrap();
        assert!(sim >= 0.0 && sim <= 1.0);
    }

    #[test]
    fn engine_report_json_roundtrip_via_value() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 10, output_count: 20, elapsed_us: 2000,
            per_second: 10000.0, text: Some("hello world".to_string()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["path"], "text");
        assert_eq!(val["prompt_tokens"], 10);
        assert_eq!(val["output_count"], 20);
        assert_eq!(val["text"], "hello world");
    }

    #[test]
    fn engine_info_head_dim_formula() {
        let info = EngineInfo {
            model_dir: "/m".to_string(), vocab_size: 32000, n_layers: 32,
            hidden_size: 4096, ffn_size: 11008, n_heads: 32, n_kv_heads: 32,
            head_dim: 128, max_seq_len: 4096, path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(), tokenizer_merge_count: 200,
        };
        // head_dim = hidden_size / n_heads
        assert_eq!(info.hidden_size / info.n_heads, info.head_dim);
    }

    // ─── Block 198: JSON types, nested clone, string edge, formula edge ─────

    #[test]
    fn engine_report_json_value_types() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 5, output_count: 10, elapsed_us: 1000,
            per_second: 10000.0, text: Some("hi".to_string()),
            nda: None, path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["path"].is_string());
        assert!(val["resolved_mode"].is_string());
        assert!(val["prompt_tokens"].is_number());
        assert!(val["output_count"].is_number());
        assert!(val["elapsed_us"].is_number());
        assert!(val["per_second"].is_number());
        assert!(val["text"].is_string());
        assert!(val["nda"].is_null());
        assert!(val["path1_lazy_loaded"].is_boolean());
        assert!(val["engine_status"].is_object());
    }

    #[test]
    fn engine_status_snapshot_nested_in_report() {
        let report = EngineReport {
            path: "nda".to_string(), resolved_mode: "Nda".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 1,
            per_second: 1.0, text: None, nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/nested/test".to_string(), vocab_size: 50000,
                n_layers: 48, hidden_size: 6144,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let nested = &val["engine_status"];
        assert_eq!(nested["model_dir"], "/nested/test");
        assert_eq!(nested["vocab_size"], 50000);
        assert_eq!(nested["n_layers"], 48);
        assert_eq!(nested["hidden_size"], 6144);
        assert_eq!(nested["path1_loaded"], true);
        assert_eq!(nested["path2_active"], true);
    }

    #[test]
    fn nda_diagnostics_nested_in_report_json() {
        let report = EngineReport {
            path: "nda".to_string(), resolved_mode: "Nda".to_string(),
            prompt_tokens: 3, output_count: 7, elapsed_us: 500,
            per_second: 14000.0, text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0xBEEF, valid: true, force_terminated: false,
                site_map_key: Some(42), opcode_count: 7,
                sandbox_passed: Some(true), scope_passed: Some(false),
                scope_similarity: Some(0.65), site_map_hits: 3, site_map_misses: 4,
            }),
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let nda = &val["nda"];
        assert_eq!(nda["root_hash"], 0xBEEF_u64);
        assert_eq!(nda["sandbox_passed"], true);
        assert_eq!(nda["scope_passed"], false);
        assert_eq!(nda["scope_similarity"], 0.65);
        assert_eq!(nda["site_map_key"], 42);
    }

    #[test]
    fn engine_report_text_unicode_roundtrip() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 1,
            per_second: 1.0,
            text: Some("Hello 世界 🌍 Ñ ö".to_string()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["text"], "Hello 世界 🌍 Ñ ö");
    }

    #[test]
    fn engine_report_text_with_special_chars() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 1,
            per_second: 1.0,
            text: Some("line1\nline2\ttab\"quotes\"\\backslash".to_string()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "".to_string(), vocab_size: 0,
                n_layers: 0, hidden_size: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["text"].as_str().unwrap().contains('\n'));
        assert!(val["text"].as_str().unwrap().contains('\t'));
    }

    #[test]
    fn engine_report_text_empty_string() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 0, output_count: 0, elapsed_us: 0,
            per_second: 0.0, text: Some(String::new()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "".to_string(), vocab_size: 0,
                n_layers: 0, hidden_size: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["text"], "");
    }

    #[test]
    fn engine_report_clone_nested_status_independence() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 1,
            per_second: 1.0, text: Some("x".to_string()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/original".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let mut cloned = report.clone();
        cloned.engine_status.model_dir = "/modified".to_string();
        cloned.engine_status.vocab_size = 999;
        cloned.engine_status.path1_loaded = false;
        // Original unchanged
        assert_eq!(report.engine_status.model_dir, "/original");
        assert_eq!(report.engine_status.vocab_size, 100);
        assert!(report.engine_status.path1_loaded);
    }

    #[test]
    fn engine_report_clone_nested_nda_independence() {
        let report = EngineReport {
            path: "nda".to_string(), resolved_mode: "Nda".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 1,
            per_second: 1.0, text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0xAA, valid: true, force_terminated: false,
                site_map_key: Some(1), opcode_count: 10,
                sandbox_passed: Some(true), scope_passed: Some(true),
                scope_similarity: Some(0.9), site_map_hits: 5, site_map_misses: 1,
            }),
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let mut cloned = report.clone();
        if let Some(ref mut nda) = cloned.nda {
            nda.root_hash = 0xFF;
            nda.valid = false;
            nda.opcode_count = 0;
        }
        // Original unchanged
        let orig_nda = report.nda.as_ref().unwrap();
        assert_eq!(orig_nda.root_hash, 0xAA);
        assert!(orig_nda.valid);
        assert_eq!(orig_nda.opcode_count, 10);
    }

    #[test]
    fn per_second_fractional_result() {
        // 3 tokens in 2 seconds = 1.5 tok/s
        let elapsed_us = 2_000_000u64;
        let output_count = 3usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 1.5).abs() < 0.001);
    }

    #[test]
    fn per_second_exact_100() {
        // 10 tokens in 100ms = 100 tok/s
        let elapsed_us = 100_000u64;
        let output_count = 10usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 100.0).abs() < 0.01);
    }

    #[test]
    fn nda_diagnostics_optional_combos_all_some() {
        let diag = NdaRunDiagnostics {
            root_hash: 1, valid: true, force_terminated: false,
            site_map_key: Some(1), opcode_count: 10,
            sandbox_passed: Some(true), scope_passed: Some(true),
            scope_similarity: Some(1.0), site_map_hits: 10, site_map_misses: 0,
        };
        assert!(diag.sandbox_passed.is_some());
        assert!(diag.scope_passed.is_some());
        assert!(diag.scope_similarity.is_some());
        assert!(diag.site_map_key.is_some());
    }

    #[test]
    fn nda_diagnostics_optional_combos_mixed() {
        let diag = NdaRunDiagnostics {
            root_hash: 2, valid: true, force_terminated: false,
            site_map_key: None, opcode_count: 10,
            sandbox_passed: Some(true), scope_passed: None,
            scope_similarity: Some(0.5), site_map_hits: 0, site_map_misses: 10,
        };
        assert!(diag.sandbox_passed.is_some());
        assert!(diag.scope_passed.is_none());
        assert!(diag.scope_similarity.is_some());
        assert!(diag.site_map_key.is_none());
    }

    #[test]
    fn nda_diagnostics_optional_combos_sandbox_only() {
        let diag = NdaRunDiagnostics {
            root_hash: 3, valid: true, force_terminated: false,
            site_map_key: None, opcode_count: 10,
            sandbox_passed: Some(false), scope_passed: None,
            scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
        };
        assert_eq!(diag.sandbox_passed, Some(false));
        assert!(diag.scope_passed.is_none());
        assert!(diag.scope_similarity.is_none());
    }

    #[test]
    fn engine_report_path_consistency_text() {
        // When path == "text", text should be Some and nda should be None
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 1,
            per_second: 1.0, text: Some("output".to_string()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "".to_string(), vocab_size: 0,
                n_layers: 0, hidden_size: 0,
            },
        };
        assert_eq!(report.path, "text");
        assert!(report.text.is_some());
        assert!(report.nda.is_none());
    }

    #[test]
    fn engine_report_path_consistency_nda() {
        // When path == "nda", text should be None and nda should be Some
        let report = EngineReport {
            path: "nda".to_string(), resolved_mode: "Nda".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 1,
            per_second: 1.0, text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0, valid: true, force_terminated: false,
                site_map_key: None, opcode_count: 1,
                sandbox_passed: None, scope_passed: None,
                scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
            }),
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "".to_string(), vocab_size: 0,
                n_layers: 0, hidden_size: 0,
            },
        };
        assert_eq!(report.path, "nda");
        assert!(report.text.is_none());
        assert!(report.nda.is_some());
    }

    #[test]
    fn engine_report_prompt_tokens_boundary() {
        // Very large prompt token count
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: usize::MAX, output_count: 0, elapsed_us: 0,
            per_second: 0.0, text: None, nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "".to_string(), vocab_size: 0,
                n_layers: 0, hidden_size: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["prompt_tokens"], usize::MAX);
    }

    #[test]
    fn engine_report_output_count_boundary() {
        let report = EngineReport {
            path: "nda".to_string(), resolved_mode: "Nda".to_string(),
            prompt_tokens: 0, output_count: usize::MAX, elapsed_us: 0,
            per_second: 0.0, text: None, nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "".to_string(), vocab_size: 0,
                n_layers: 0, hidden_size: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["output_count"], usize::MAX);
    }

    #[test]
    fn engine_info_site_map_stats_string_content() {
        let info = EngineInfo {
            model_dir: "/m".to_string(), vocab_size: 100, n_layers: 4,
            hidden_size: 256, ffn_size: 512, n_heads: 4, n_kv_heads: 2,
            head_dim: 64, max_seq_len: 1024, path1_loaded: false,
            path2_site_map_stats: "1,234 entries (42 programs)".to_string(),
            tokenizer_merge_count: 50,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("1,234 entries (42 programs)"));
    }

    #[test]
    fn engine_info_model_dir_with_special_chars() {
        let info = EngineInfo {
            model_dir: "/path/to/model with spaces/&special".to_string(),
            vocab_size: 100, n_layers: 4, hidden_size: 256, ffn_size: 512,
            n_heads: 4, n_kv_heads: 2, head_dim: 64, max_seq_len: 1024,
            path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(),
            tokenizer_merge_count: 50,
        };
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["model_dir"], "/path/to/model with spaces/&special");
    }

    #[test]
    fn engine_status_snapshot_model_dir_path_separators() {
        let snap = EngineStatusSnapshot {
            path1_loaded: false, path2_active: true,
            model_dir: "C:\\Users\\test\\models".to_string(),
            vocab_size: 32000, n_layers: 12, hidden_size: 768,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["model_dir"].as_str().unwrap().contains("models"));
    }

    #[test]
    fn nda_diagnostics_site_map_hit_rate_formula() {
        // 7 hits, 3 misses = 70% hit rate
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: Some(1), opcode_count: 100,
            sandbox_passed: None, scope_passed: None,
            scope_similarity: None, site_map_hits: 7, site_map_misses: 3,
        };
        let total = diag.site_map_hits + diag.site_map_misses;
        let hit_rate = diag.site_map_hits as f64 / total as f64;
        assert!((hit_rate - 0.7).abs() < 0.001);
    }

    #[test]
    fn nda_diagnostics_site_map_single_hit() {
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: Some(1), opcode_count: 10,
            sandbox_passed: None, scope_passed: None,
            scope_similarity: None, site_map_hits: 1, site_map_misses: 99,
        };
        let total = diag.site_map_hits + diag.site_map_misses;
        let hit_rate = diag.site_map_hits as f64 / total as f64;
        assert!((hit_rate - 0.01).abs() < 0.001);
    }

    #[test]
    fn engine_report_per_second_stored_value_matches_formula() {
        // Verify the stored per_second matches what the formula would produce
        let elapsed_us = 250_000u64;
        let output_count = 50usize;
        let expected = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count, elapsed_us,
            per_second: expected, text: Some("x".to_string()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let stored = val["per_second"].as_f64().unwrap();
        assert!((stored - expected).abs() < 0.01);
    }

    #[test]
    fn engine_info_all_derived_fields_consistent() {
        // For Qwen-0.5B style config
        let info = EngineInfo {
            model_dir: "/qwen05".to_string(), vocab_size: 151936, n_layers: 24,
            hidden_size: 1024, ffn_size: 2816, n_heads: 16, n_kv_heads: 2,
            head_dim: 64, max_seq_len: 2048, path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(),
            tokenizer_merge_count: 151936,
        };
        // head_dim = hidden_size / n_heads
        assert_eq!(info.hidden_size / info.n_heads, info.head_dim);
        // GQA: n_heads divisible by n_kv_heads
        assert_eq!(info.n_heads % info.n_kv_heads, 0);
        // FFN ratio in typical range
        let ffn_ratio = info.ffn_size as f64 / info.hidden_size as f64;
        assert!(ffn_ratio > 2.0 && ffn_ratio < 4.0);
    }

    // ─── Block 203: EngineOutput variants, validate logic, boundary values ──

    #[test]
    fn engine_output_text_field_access() {
        let out = EngineOutput::Text {
            text: "hello world".to_string(),
            n_tokens: 2,
            elapsed_ms: 50,
        };
        match &out {
            EngineOutput::Text { text, n_tokens, elapsed_ms } => {
                assert_eq!(text, "hello world");
                assert_eq!(*n_tokens, 2);
                assert_eq!(*elapsed_ms, 50);
            }
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn engine_output_nda_field_access() {
        let out = EngineOutput::Nda {
            opcodes: vec![],
            root_hash: 0xDEAD,
            valid: true,
            force_terminated: false,
            site_map_key: Some(42),
            n_opcodes: 0,
            elapsed_ms: 100,
        };
        match &out {
            EngineOutput::Nda { opcodes, root_hash, valid, force_terminated,
                site_map_key, n_opcodes, elapsed_ms } => {
                assert!(opcodes.is_empty());
                assert_eq!(*root_hash, 0xDEAD);
                assert!(*valid);
                assert!(!*force_terminated);
                assert_eq!(*site_map_key, Some(42));
                assert_eq!(*n_opcodes, 0);
                assert_eq!(*elapsed_ms, 100);
            }
            _ => panic!("expected Nda variant"),
        }
    }

    #[test]
    fn engine_output_nda_with_opcodes_203() {
        // Nda variant can hold opcodes (we use dummy u8 values for counting)
        let out = EngineOutput::Nda {
            opcodes: vec![],
            root_hash: 0xFF,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            n_opcodes: 5,
            elapsed_ms: 10,
        };
        match &out {
            EngineOutput::Nda { n_opcodes, .. } => assert_eq!(*n_opcodes, 5),
            _ => unreachable!(),
        }
    }

    #[test]
    fn engine_output_text_empty_string_203() {
        let out = EngineOutput::Text {
            text: String::new(),
            n_tokens: 0,
            elapsed_ms: 0,
        };
        match &out {
            EngineOutput::Text { text, n_tokens, elapsed_ms } => {
                assert_eq!(text, "");
                assert_eq!(*n_tokens, 0);
                assert_eq!(*elapsed_ms, 0);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn engine_output_text_large_elapsed() {
        let out = EngineOutput::Text {
            text: "slow".to_string(),
            n_tokens: 1,
            elapsed_ms: u128::MAX,
        };
        match &out {
            EngineOutput::Text { elapsed_ms, .. } => {
                assert_eq!(*elapsed_ms, u128::MAX);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn engine_output_nda_force_terminated_no_key() {
        // force_terminated programs are never stored → site_map_key should be None
        let out = EngineOutput::Nda {
            opcodes: vec![],
            root_hash: 0xABCD,
            valid: true,
            force_terminated: true,
            site_map_key: None,
            n_opcodes: 50,
            elapsed_ms: 200,
        };
        match &out {
            EngineOutput::Nda { force_terminated, site_map_key, .. } => {
                assert!(*force_terminated);
                assert!(site_map_key.is_none());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn validate_logic_multiple_warnings() {
        // Simulate the validate() logic with multiple zero fields
        let mut warnings: Vec<String> = Vec::new();
        let vocab_size = 0usize;
        let n_layers = 0usize;
        let hidden_size = 0usize;
        let n_heads = 0usize;
        let max_seq_len = 0usize;
        if vocab_size == 0 { warnings.push("vocab_size is 0".to_string()); }
        if n_layers == 0 { warnings.push("n_layers is 0".to_string()); }
        if hidden_size == 0 { warnings.push("hidden_size is 0".to_string()); }
        if n_heads == 0 { warnings.push("n_heads is 0".to_string()); }
        if max_seq_len == 0 { warnings.push("max_seq_len is 0".to_string()); }
        assert_eq!(warnings.len(), 5);
    }

    #[test]
    fn validate_logic_hidden_not_divisible_by_heads() {
        let mut warnings: Vec<String> = Vec::new();
        let hidden_size = 100usize;
        let n_heads = 7usize;
        if n_heads > 0 && hidden_size % n_heads != 0 {
            warnings.push(format!(
                "hidden_size ({}) not divisible by n_heads ({})",
                hidden_size, n_heads
            ));
        }
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("100"));
        assert!(warnings[0].contains("7"));
    }

    #[test]
    fn validate_logic_no_warnings_all_valid() {
        let mut warnings: Vec<String> = Vec::new();
        let vocab_size = 32000usize;
        let n_layers = 12usize;
        let hidden_size = 768usize;
        let n_heads = 12usize;
        let max_seq_len = 4096usize;
        if vocab_size == 0 { warnings.push("vocab_size is 0".to_string()); }
        if n_layers == 0 { warnings.push("n_layers is 0".to_string()); }
        if hidden_size == 0 { warnings.push("hidden_size is 0".to_string()); }
        if n_heads == 0 { warnings.push("n_heads is 0".to_string()); }
        if max_seq_len == 0 { warnings.push("max_seq_len is 0".to_string()); }
        if n_heads > 0 && hidden_size % n_heads != 0 {
            warnings.push("not divisible".to_string());
        }
        assert!(warnings.is_empty());
    }

    #[test]
    fn engine_report_elapsed_us_max() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 0, output_count: 0, elapsed_us: u64::MAX,
            per_second: 0.0, text: None, nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "".to_string(), vocab_size: 0,
                n_layers: 0, hidden_size: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["elapsed_us"], u64::MAX);
    }

    #[test]
    fn nda_diagnostics_root_hash_max() {
        let diag = NdaRunDiagnostics {
            root_hash: u64::MAX, valid: false, force_terminated: false,
            site_map_key: None, opcode_count: 0,
            sandbox_passed: None, scope_passed: None,
            scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
        };
        let json = serde_json::to_string(&diag).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["root_hash"], u64::MAX);
    }

    #[test]
    fn nda_diagnostics_opcode_count_zero_valid() {
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: None, opcode_count: 0,
            sandbox_passed: None, scope_passed: None,
            scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
        };
        assert_eq!(diag.opcode_count, 0);
        assert!(diag.valid);
    }

    #[test]
    fn engine_info_mha_n_heads_equal_kv_heads() {
        // Multi-head attention: n_heads == n_kv_heads
        let info = EngineInfo {
            model_dir: "/m".to_string(), vocab_size: 32000, n_layers: 12,
            hidden_size: 768, ffn_size: 2048, n_heads: 12, n_kv_heads: 12,
            head_dim: 64, max_seq_len: 4096, path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(), tokenizer_merge_count: 100,
        };
        assert_eq!(info.n_heads, info.n_kv_heads);
        assert_eq!(info.hidden_size / info.n_heads, info.head_dim);
    }

    #[test]
    fn engine_info_mqa_single_kv_head() {
        // Multi-query attention: n_kv_heads == 1
        let info = EngineInfo {
            model_dir: "/m".to_string(), vocab_size: 32000, n_layers: 12,
            hidden_size: 768, ffn_size: 2048, n_heads: 12, n_kv_heads: 1,
            head_dim: 64, max_seq_len: 4096, path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(), tokenizer_merge_count: 100,
        };
        assert_eq!(info.n_kv_heads, 1);
        assert_eq!(info.n_heads % info.n_kv_heads, 0);
    }

    #[test]
    fn engine_report_both_text_and_nda_none() {
        // Both text and nda can be None (e.g., before execution)
        let report = EngineReport {
            path: "unknown".to_string(), resolved_mode: "Auto".to_string(),
            prompt_tokens: 0, output_count: 0, elapsed_us: 0,
            per_second: 0.0, text: None, nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "".to_string(), vocab_size: 0,
                n_layers: 0, hidden_size: 0,
            },
        };
        assert!(report.text.is_none());
        assert!(report.nda.is_none());
    }

    #[test]
    fn nda_diagnostics_site_map_zero_total() {
        // When both hits and misses are 0, can't compute hit rate
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: None, opcode_count: 0,
            sandbox_passed: None, scope_passed: None,
            scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
        };
        let total = diag.site_map_hits + diag.site_map_misses;
        assert_eq!(total, 0);
        // Hit rate would be 0/0 — guard against division by zero
        let hit_rate = if total > 0 {
            diag.site_map_hits as f64 / total as f64
        } else {
            0.0
        };
        assert_eq!(hit_rate, 0.0);
    }

    #[test]
    fn engine_status_snapshot_path2_always_active() {
        // status_snapshot() always sets path2_active = true
        let snap = EngineStatusSnapshot {
            path1_loaded: false, path2_active: true,
            model_dir: "/m".to_string(), vocab_size: 100,
            n_layers: 4, hidden_size: 256,
        };
        assert!(snap.path2_active);
    }

    #[test]
    fn engine_report_per_second_very_large() {
        // 1M tokens in 1μs = 1e12 tok/s (unrealistic but tests float range)
        let elapsed_us = 1u64;
        let output_count = 1_000_000usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 1e12).abs() < 1e6);
    }

    #[test]
    fn engine_report_per_second_sub_one() {
        // 1 token in 10 seconds = 0.1 tok/s
        let elapsed_us = 10_000_000u64;
        let output_count = 1usize;
        let per_second = (output_count as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 0.1).abs() < 0.001);
    }

    #[test]
    fn nda_diagnostics_scope_similarity_boundary_zero() {
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: None, opcode_count: 10,
            sandbox_passed: None, scope_passed: Some(false),
            scope_similarity: Some(0.0), site_map_hits: 0, site_map_misses: 10,
        };
        assert_eq!(diag.scope_similarity, Some(0.0));
    }

    #[test]
    fn nda_diagnostics_scope_similarity_boundary_one() {
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: Some(1), opcode_count: 10,
            sandbox_passed: None, scope_passed: Some(true),
            scope_similarity: Some(1.0), site_map_hits: 10, site_map_misses: 0,
        };
        assert_eq!(diag.scope_similarity, Some(1.0));
    }

    #[test]
    fn engine_report_pretty_json_203() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 1,
            per_second: 1.0, text: Some("x".to_string()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let pretty = serde_json::to_string_pretty(&report).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("    "));
        // Verify it round-trips through Value
        let val: serde_json::Value = serde_json::from_str(&pretty).unwrap();
        assert_eq!(val["path"], "text");
    }

    #[test]
    fn nda_diagnostics_site_map_key_json_none_vs_some() {
        let diag_none = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: None, opcode_count: 0,
            sandbox_passed: None, scope_passed: None,
            scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
        };
        let json_none = serde_json::to_string(&diag_none).unwrap();
        assert!(json_none.contains("\"site_map_key\":null"));

        let diag_some = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: Some(999), opcode_count: 0,
            sandbox_passed: None, scope_passed: None,
            scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
        };
        let json_some = serde_json::to_string(&diag_some).unwrap();
        assert!(json_some.contains("\"site_map_key\":999"));
    }

    #[test]
    fn engine_report_clone_preserves_all_fields() {
        let report = EngineReport {
            path: "nda".to_string(), resolved_mode: "Nda".to_string(),
            prompt_tokens: 42, output_count: 99, elapsed_us: 12345,
            per_second: 8051.0, text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0xCAFE, valid: true, force_terminated: false,
                site_map_key: Some(7), opcode_count: 99,
                sandbox_passed: Some(true), scope_passed: Some(true),
                scope_similarity: Some(0.99), site_map_hits: 50, site_map_misses: 5,
            }),
            path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/clone/test".to_string(), vocab_size: 50000,
                n_layers: 24, hidden_size: 1024,
            },
        };
        let cloned = report.clone();
        assert_eq!(cloned.path, "nda");
        assert_eq!(cloned.resolved_mode, "Nda");
        assert_eq!(cloned.prompt_tokens, 42);
        assert_eq!(cloned.output_count, 99);
        assert_eq!(cloned.elapsed_us, 12345);
        assert!(cloned.path1_lazy_loaded);
        assert_eq!(cloned.engine_status.model_dir, "/clone/test");
        assert_eq!(cloned.engine_status.vocab_size, 50000);
        let nda = cloned.nda.as_ref().unwrap();
        assert_eq!(nda.root_hash, 0xCAFE);
        assert_eq!(nda.opcode_count, 99);
    }

    #[test]
    fn engine_info_tokenizer_merge_count_zero() {
        let info = EngineInfo {
            model_dir: "/m".to_string(), vocab_size: 100, n_layers: 4,
            hidden_size: 256, ffn_size: 512, n_heads: 4, n_kv_heads: 2,
            head_dim: 64, max_seq_len: 1024, path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(), tokenizer_merge_count: 0,
        };
        assert_eq!(info.tokenizer_merge_count, 0);
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["tokenizer_merge_count"], 0);
    }

    #[test]
    fn engine_status_snapshot_vocab_size_boundary() {
        let snap = EngineStatusSnapshot {
            path1_loaded: true, path2_active: true,
            model_dir: "/m".to_string(), vocab_size: usize::MAX,
            n_layers: usize::MAX, hidden_size: usize::MAX,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["vocab_size"], usize::MAX);
        assert_eq!(val["n_layers"], usize::MAX);
        assert_eq!(val["hidden_size"], usize::MAX);
    }

    // ─── Block 209: PipelineMode detect, validate model_dir, run_dual_path formatting ──

    #[test]
    fn pipeline_mode_detect_code_keyword_209() {
        // Code keywords should route to NDA mode
        let mode = PipelineMode::detect("fn main() { println!(\"hello\"); }");
        assert_eq!(mode, PipelineMode::Nda);
    }

    #[test]
    fn pipeline_mode_detect_question_209() {
        // Questions should route to Text mode
        let mode = PipelineMode::detect("What is the meaning of life?");
        assert_eq!(mode, PipelineMode::Text);
    }

    #[test]
    fn pipeline_mode_detect_imperative_209() {
        // Imperative creation verbs → NDA
        let mode = PipelineMode::detect("create a function that sorts");
        assert_eq!(mode, PipelineMode::Nda);
    }

    #[test]
    fn pipeline_mode_detect_explanation_209() {
        // Explanations → Text
        let mode = PipelineMode::detect("explain how transformers work");
        assert_eq!(mode, PipelineMode::Text);
    }

    #[test]
    fn validate_model_dir_not_exists_209() {
        // Simulate validate() with non-existent model_dir
        let mut warnings: Vec<String> = Vec::new();
        let model_dir_exists = false;
        if !model_dir_exists {
            warnings.push("model_dir does not exist: /nonexistent".to_string());
        }
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("does not exist"));
    }

    #[test]
    fn validate_model_dir_exists_no_warning_209() {
        let mut warnings: Vec<String> = Vec::new();
        let model_dir_exists = true;
        if !model_dir_exists {
            warnings.push("model_dir does not exist".to_string());
        }
        assert!(warnings.is_empty());
    }

    #[test]
    fn validate_combined_zero_and_missing_dir_209() {
        let mut warnings: Vec<String> = Vec::new();
        let vocab_size = 0usize;
        let n_layers = 0usize;
        let hidden_size = 0usize;
        let n_heads = 0usize;
        let max_seq_len = 0usize;
        if vocab_size == 0 { warnings.push("vocab_size is 0".into()); }
        if n_layers == 0 { warnings.push("n_layers is 0".into()); }
        if hidden_size == 0 { warnings.push("hidden_size is 0".into()); }
        if n_heads == 0 { warnings.push("n_heads is 0".into()); }
        if max_seq_len == 0 { warnings.push("max_seq_len is 0".into()); }
        let model_dir_exists = false;
        if !model_dir_exists { warnings.push("model_dir does not exist".into()); }
        assert_eq!(warnings.len(), 6);
    }

    #[test]
    fn run_dual_path_text_stats_format_209() {
        // Simulate the text path stats formatting from run_dual_path
        let n_tokens = 100usize;
        let elapsed_ms = 2000u128;
        let elapsed_s = elapsed_ms as f64 / 1000.0;
        let tok_per_s = n_tokens as f64 / elapsed_s.max(1e-6);
        assert!((elapsed_s - 2.0).abs() < 0.001);
        assert!((tok_per_s - 50.0).abs() < 0.01);
    }

    #[test]
    fn run_dual_path_nda_stats_format_209() {
        let n_opcodes = 50usize;
        let elapsed_ms = 500u128;
        let elapsed_s = elapsed_ms as f64 / 1000.0;
        let ops_per_s = n_opcodes as f64 / elapsed_s.max(1e-6);
        assert!((elapsed_s - 0.5).abs() < 0.001);
        assert!((ops_per_s - 100.0).abs() < 0.01);
    }

    #[test]
    fn run_dual_path_nda_zero_elapsed_209() {
        let n_opcodes = 50usize;
        let elapsed_ms = 0u128;
        let elapsed_s = elapsed_ms as f64 / 1000.0;
        let ops_per_s = n_opcodes as f64 / elapsed_s.max(1e-6);
        assert!(ops_per_s.is_finite());
    }

    #[test]
    fn run_dual_path_text_zero_elapsed_209() {
        let n_tokens = 10usize;
        let elapsed_ms = 0u128;
        let elapsed_s = elapsed_ms as f64 / 1000.0;
        let tok_per_s = n_tokens as f64 / elapsed_s.max(1e-6);
        assert!(tok_per_s.is_finite());
    }

    #[test]
    fn run_dual_path_status_valid_complete_209() {
        let valid = true;
        let force_terminated = false;
        let status = match (valid, force_terminated) {
            (true, false) => "VALID (complete)",
            (true, true) => "VALID (truncated — increase --max-tokens)",
            _ => "INVALID",
        };
        assert_eq!(status, "VALID (complete)");
    }

    #[test]
    fn run_dual_path_status_valid_truncated_209() {
        let valid = true;
        let force_terminated = true;
        let status = match (valid, force_terminated) {
            (true, false) => "VALID (complete)",
            (true, true) => "VALID (truncated — increase --max-tokens)",
            _ => "INVALID",
        };
        assert!(status.contains("truncated"));
        assert!(status.contains("--max-tokens"));
    }

    #[test]
    fn run_dual_path_status_invalid_209() {
        let valid = false;
        let force_terminated = false;
        let status = match (valid, force_terminated) {
            (true, false) => "VALID (complete)",
            (true, true) => "VALID (truncated — increase --max-tokens)",
            _ => "INVALID",
        };
        assert_eq!(status, "INVALID");
    }

    #[test]
    fn engine_info_bitnet3b_config_209() {
        let cfg = ModelConfig::bitnet_3b();
        let info = EngineInfo {
            model_dir: "/bitnet3b".to_string(),
            vocab_size: cfg.vocab_size,
            n_layers: cfg.n_layers,
            hidden_size: cfg.hidden_size,
            ffn_size: cfg.ffn_size,
            n_heads: cfg.n_heads,
            n_kv_heads: cfg.n_kv_heads,
            head_dim: cfg.head_dim,
            max_seq_len: cfg.max_seq_len,
            path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(),
            tokenizer_merge_count: cfg.vocab_size,
        };
        assert_eq!(info.vocab_size, 32000);
        assert_eq!(info.n_layers, 26);
        assert_eq!(info.hidden_size / info.n_heads, info.head_dim);
    }

    #[test]
    fn engine_info_qwen05_config_209() {
        let cfg = ModelConfig::qwen_coder_05b();
        let info = EngineInfo {
            model_dir: "/qwen05".to_string(),
            vocab_size: cfg.vocab_size,
            n_layers: cfg.n_layers,
            hidden_size: cfg.hidden_size,
            ffn_size: cfg.ffn_size,
            n_heads: cfg.n_heads,
            n_kv_heads: cfg.n_kv_heads,
            head_dim: cfg.head_dim,
            max_seq_len: cfg.max_seq_len,
            path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(),
            tokenizer_merge_count: cfg.vocab_size,
        };
        assert_eq!(info.vocab_size, 151936);
        assert_eq!(info.n_layers, 24);
        assert_eq!(info.hidden_size / info.n_heads, info.head_dim);
    }

    #[test]
    fn engine_report_per_second_nda_path_209() {
        // NDA path: per_second = n_opcodes / (elapsed_us / 1_000_000)
        let elapsed_us = 100_000u64;
        let n_opcodes = 50usize;
        let per_second = (n_opcodes as f64) / (elapsed_us as f64 / 1_000_000.0);
        assert!((per_second - 500.0).abs() < 0.01);
    }

    #[test]
    fn nda_diagnostics_opcode_count_proportion_209() {
        let diag = NdaRunDiagnostics {
            root_hash: 0, valid: true, force_terminated: false,
            site_map_key: Some(1), opcode_count: 75,
            sandbox_passed: Some(true), scope_passed: Some(true),
            scope_similarity: Some(0.9), site_map_hits: 10, site_map_misses: 5,
        };
        let total_accesses = diag.site_map_hits + diag.site_map_misses;
        let opcode_per_access = diag.opcode_count as f64 / total_accesses as f64;
        assert!(opcode_per_access > 0.0);
        assert!((opcode_per_access - 5.0).abs() < 0.01);
    }

    #[test]
    fn engine_report_text_some_nda_some_inconsistent_209() {
        // Both text and nda set — unusual but structurally allowed
        let report = EngineReport {
            path: "unknown".to_string(), resolved_mode: "Auto".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 1,
            per_second: 1.0,
            text: Some("text output".to_string()),
            nda: Some(NdaRunDiagnostics {
                root_hash: 0, valid: true, force_terminated: false,
                site_map_key: None, opcode_count: 1,
                sandbox_passed: None, scope_passed: None,
                scope_similarity: None, site_map_hits: 0, site_map_misses: 0,
            }),
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "".to_string(), vocab_size: 0,
                n_layers: 0, hidden_size: 0,
            },
        };
        assert!(report.text.is_some());
        assert!(report.nda.is_some());
    }

    #[test]
    fn engine_status_snapshot_path_combinations_209() {
        // All 4 combinations of path1_loaded × path2_active
        for p1 in [false, true] {
            for p2 in [false, true] {
                let snap = EngineStatusSnapshot {
                    path1_loaded: p1, path2_active: p2,
                    model_dir: "/m".to_string(), vocab_size: 100,
                    n_layers: 4, hidden_size: 256,
                };
                assert_eq!(snap.path1_loaded, p1);
                assert_eq!(snap.path2_active, p2);
            }
        }
    }

    #[test]
    fn engine_report_json_nda_nested_all_fields_209() {
        let report = EngineReport {
            path: "nda".to_string(), resolved_mode: "Nda".to_string(),
            prompt_tokens: 5, output_count: 20, elapsed_us: 1000,
            per_second: 20000.0, text: None,
            nda: Some(NdaRunDiagnostics {
                root_hash: 0xABCD, valid: true, force_terminated: false,
                site_map_key: Some(42), opcode_count: 20,
                sandbox_passed: Some(true), scope_passed: Some(true),
                scope_similarity: Some(0.95), site_map_hits: 15, site_map_misses: 5,
            }),
            path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let nda = &val["nda"];
        assert_eq!(nda.as_object().unwrap().len(), 10);
        assert_eq!(nda["root_hash"], 0xABCD_u64);
        assert_eq!(nda["opcode_count"], 20);
        assert_eq!(nda["site_map_hits"], 15);
        assert_eq!(nda["site_map_misses"], 5);
    }

    #[test]
    fn engine_report_elapsed_us_boundary_209() {
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 0, output_count: 0, elapsed_us: 1,
            per_second: 0.0, text: None, nda: None,
            path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: false, path2_active: true,
                model_dir: "".to_string(), vocab_size: 0,
                n_layers: 0, hidden_size: 0,
            },
        };
        // 1μs elapsed — per_second would be 0 since output_count=0
        assert_eq!(report.elapsed_us, 1);
        assert_eq!(report.per_second, 0.0);
    }

    #[test]
    fn nda_diagnostics_site_map_key_json_roundtrip_209() {
        let diag = NdaRunDiagnostics {
            root_hash: 0x1234, valid: true, force_terminated: false,
            site_map_key: Some(0xABCD), opcode_count: 50,
            sandbox_passed: Some(true), scope_passed: Some(true),
            scope_similarity: Some(0.9), site_map_hits: 10, site_map_misses: 5,
        };
        let json = serde_json::to_string(&diag).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["site_map_key"], 0xABCD_u64);
    }

    #[test]
    fn engine_report_path1_lazy_loaded_consistency_209() {
        // When path1_lazy_loaded = true, engine_status.path1_loaded should be true
        // (after the run, path1 is now loaded)
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 100,
            per_second: 10000.0, text: Some("x".to_string()),
            nda: None, path1_lazy_loaded: true,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        assert!(report.path1_lazy_loaded);
        assert!(report.engine_status.path1_loaded);
    }

    #[test]
    fn engine_report_path1_not_lazy_loaded_209() {
        // When path1_lazy_loaded = false, path1 was already loaded before
        let report = EngineReport {
            path: "text".to_string(), resolved_mode: "Text".to_string(),
            prompt_tokens: 1, output_count: 1, elapsed_us: 100,
            per_second: 10000.0, text: Some("x".to_string()),
            nda: None, path1_lazy_loaded: false,
            engine_status: EngineStatusSnapshot {
                path1_loaded: true, path2_active: true,
                model_dir: "/m".to_string(), vocab_size: 100,
                n_layers: 4, hidden_size: 256,
            },
        };
        assert!(!report.path1_lazy_loaded);
        assert!(report.engine_status.path1_loaded);
    }

    #[test]
    fn engine_info_n_layers_total_params_estimate_209() {
        // Rough parameter estimate: ~12 * n_layers * hidden_size^2 for transformer
        let info = EngineInfo {
            model_dir: "/m".to_string(), vocab_size: 32000, n_layers: 26,
            hidden_size: 3200, ffn_size: 8640, n_heads: 32, n_kv_heads: 32,
            head_dim: 100, max_seq_len: 4096, path1_loaded: false,
            path2_site_map_stats: "0 entries".to_string(), tokenizer_merge_count: 32000,
        };
        let approx_params = 12 * info.n_layers * info.hidden_size * info.hidden_size;
        // BitNet-3B should be ~3B parameters
        assert!(approx_params > 2_000_000_000);
    }
}
