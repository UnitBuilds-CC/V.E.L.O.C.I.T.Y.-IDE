//! CLI integration helper for the dual-path engine.

use anyhow::Result;

use super::*;
use crate::pipeline_nda::PipelineMode;

/// Called from main.rs when `--mode nda` or `--mode auto` is requested.
pub fn run_dual_path(
    model_dir: &std::path::Path,
    tokenizer_path: &std::path::Path,
    prompt: &str,
    mode: PipelineMode,
    max_tokens: usize,
    cfg: crate::model::config::ModelConfig,
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
