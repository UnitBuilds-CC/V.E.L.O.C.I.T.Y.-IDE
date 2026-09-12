// pipeline_bridge — DualPathEngine: routes between Path 1 (text) and Path 2 (NDA)
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

mod cli;
mod engine;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use serde::Serialize;

use crate::{
    model::{config::ModelConfig, transformer::Transformer},
    pipeline_nda::NdaPipeline,
    site_map::verifier::NdaOpcode,
    tokenizer::Tokenizer,
};

// Re-export public API so external callers see the same paths.
pub use cli::run_dual_path;

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

pub(crate) struct TextPath {
    pub(crate) transformer: Transformer,
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
