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
                (true, true) => "VALID (truncated — increase --max-tokens)",
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
