//! `impl DualPathEngine` — core engine logic.

use std::io::Write;
use std::time::Instant;

use anyhow::Result;

use super::*;
use crate::model::{transformer::Transformer, weights::ModelWeights};
use crate::pipeline_nda::PipelineMode;
use crate::site_map::verifier::NdaOpcode;

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
        eprintln!("[bridge] Path 2 — NDA native generation");
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
                        "[bridge] Sandbox    : PASS  {} nodes ({} matrices, {} norms, out_dim={}), {}µs",
                        sb.executed_nodes, sb.matrix_count, sb.norm_count, sb.output_dim, sb.elapsed_us
                    );
                }
            }

            if let Some(ref sc) = result.scope {
                let status = if sc.passed { "PASS" } else { "FAIL" };
                let comment = if sc.passed {
                    ""
                } else {
                    " — not stored (prompt-program misalignment)"
                };
                eprintln!(
                    "[bridge] Scope      : {}  sim={:.2} (θ={:.2}){}",
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
