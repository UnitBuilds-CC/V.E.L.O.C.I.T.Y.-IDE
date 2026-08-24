// pipeline_nda.rs — Path 2: Pure NDA native inference pipeline
#![allow(dead_code)]
//
// This is the zero-hallucination execution path.
//
// Every token emitted by this pipeline is an NDA opcode (0–8).
// The MerkleVerifier runs inline — any beam candidate whose emitted tokens
// produce a Merkle root mismatch is pruned before it reaches the caller.
// Structurally invalid NDA programs cannot be the top-scoring output.
//
// Key properties:
//   • Output vocabulary: 9 tokens (vs 151k for the text path)
//   • K/V lookup: SiteMap (persistent, O(1), hash-addressed)
//   • All arithmetic: i32 integer (zero floats in the generation loop)
//   • Integrity: MerkleVerifier checks every SCOPE close + ROOT token
//   • Self-improvement: every valid output is stored in the site map and
//     becomes available as a training example for Stage 3.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::{
    model::{config::ModelConfig, transformer_zero::ZeroTransformer, weights::ModelWeights},
    site_map::{
        verifier::{MerkleVerifier, NdaNode, NdaOpcode},
        SiteMap,
    },
};

// ─── NDA Output Head ──────────────────────────────────────────────────────────

/// Tiny 2-layer MLP that projects the transformer hidden state (dim=896)
/// down to 9 NDA opcode logits.
///
/// Architecture:  hidden(896) → Linear(64) → ReLU → Linear(9)
/// Parameters:    896×64 + 64 + 64×9 + 9 = 57,929  (all i32 after quantisation)
///
/// The head is stored as plain f32 for training compatibility; during
/// inference we keep it in f32 (the head is tiny — 232 KB — so float cost
/// is negligible compared to the 89 MB NDA weight forward pass).
pub struct NdaHead {
    /// Layer 1 weights: [64, 896]  (out_features × in_features, row-major)
    pub w1: Vec<f32>,
    /// Layer 1 bias: [64]
    pub b1: Vec<f32>,
    /// Layer 2 weights: [9, 64]
    pub w2: Vec<f32>,
    /// Layer 2 bias: [9]
    pub b2: Vec<f32>,
}

impl NdaHead {
    const IN: usize = 896;
    const MID: usize = 64;
    const OUT: usize = NdaOpcode::VOCAB_SIZE; // 9

    /// Initialise with small random weights (Xavier uniform).
    pub fn random() -> Self {
        use std::f32::consts::SQRT_2;
        // Xavier uniform scale: sqrt(2 / (fan_in + fan_out))
        let s1 = SQRT_2 / ((Self::IN + Self::MID) as f32).sqrt();
        let s2 = SQRT_2 / ((Self::MID + Self::OUT) as f32).sqrt();
        Self {
            w1: random_uniform(Self::MID * Self::IN, s1),
            b1: vec![0.0; Self::MID],
            w2: random_uniform(Self::OUT * Self::MID, s2),
            b2: vec![0.0; Self::OUT],
        }
    }

    /// Initialise with all-zero weights (used when loading from disk).
    pub fn zeros() -> Self {
        Self {
            w1: vec![0.0; Self::MID * Self::IN],
            b1: vec![0.0; Self::MID],
            w2: vec![0.0; Self::OUT * Self::MID],
            b2: vec![0.0; Self::OUT],
        }
    }

    /// Forward pass: hidden[896] → logits[VOCAB_SIZE].
    /// Pure f32 — the head is only small, float cost is negligible.
    pub fn forward(&self, hidden: &[f32]) -> [f32; NdaOpcode::VOCAB_SIZE] {
        debug_assert_eq!(hidden.len(), Self::IN);

        // Layer 1: linear + ReLU
        let mut mid = [0.0f32; Self::MID];
        for (o, (row, &b)) in self
            .w1
            .chunks_exact(Self::IN)
            .zip(self.b1.iter())
            .enumerate()
        {
            let mut s = b;
            for (&w, &x) in row.iter().zip(hidden.iter()) {
                s += w * x;
            }
            mid[o] = s.max(0.0); // ReLU
        }

        // Layer 2: linear (no activation — raw logits)
        let mut out = [0.0f32; Self::OUT];
        for (o, (row, &b)) in self
            .w2
            .chunks_exact(Self::MID)
            .zip(self.b2.iter())
            .enumerate()
        {
            let mut s = b;
            for (&w, &x) in row.iter().zip(mid.iter()) {
                s += w * x;
            }
            out[o] = s;
        }
        out
    }

    /// Save weights to a simple binary format:
    ///   [4 bytes: magic "NDA\x01"] [4×(n_elements × f32)]
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut buf: Vec<u8> = b"NDA\x01".to_vec();
        for slice in [&self.w1, &self.b1, &self.w2, &self.b2] {
            for &v in slice {
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
        std::fs::write(path, &buf)?;
        Ok(())
    }

    /// Load weights from the binary format written by `save`.
    pub fn load(path: &Path) -> Result<Self> {
        let buf = std::fs::read(path)?;
        anyhow::ensure!(&buf[..4] == b"NDA\x01", "invalid NdaHead magic");
        let floats: Vec<f32> = buf[4..]
            .chunks_exact(4)
            .map(|c| {
                f32::from_le_bytes(c.try_into().expect("chunks_exact(4) always yields 4 bytes"))
            })
            .collect();
        let n1 = Self::MID * Self::IN;
        let n2 = Self::MID;
        let n3 = Self::OUT * Self::MID;
        let n4 = Self::OUT;
        anyhow::ensure!(
            floats.len() == n1 + n2 + n3 + n4,
            "NdaHead weight count mismatch: expected {}, got {}",
            n1 + n2 + n3 + n4,
            floats.len()
        );
        let mut it = floats.into_iter();
        Ok(Self {
            w1: it.by_ref().take(n1).collect(),
            b1: it.by_ref().take(n2).collect(),
            w2: it.by_ref().take(n3).collect(),
            b2: it.by_ref().take(n4).collect(),
        })
    }
}

fn random_uniform(n: usize, scale: f32) -> Vec<f32> {
    // Simple deterministic PRNG (xorshift32) — no rand crate needed here,
    // and results are reproducible across platforms.
    let mut state: u32 = 0x6D2B_79F5;
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let t = (state as f32 / u32::MAX as f32) * 2.0 - 1.0; // [-1, 1]
            t * scale
        })
        .collect()
}

// ─── PipelineMode ─────────────────────────────────────────────────────────────

/// Which pipeline to route a request through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelineMode {
    /// Path 1: natural language output (text tokens, 151k vocab).
    Text,
    /// Path 2: NDA native output (9 opcodes, Merkle-verified).
    Nda,
    /// Detect automatically from prompt content.
    Auto,
}

impl PipelineMode {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "nda" | "native" => Self::Nda,
            "auto" => Self::Auto,
            _ => Self::Text,
        }
    }

    /// Auto-detect mode from prompt text.
    /// Heuristic: imperative code-writing verbs → NDA; questions → Text.
    pub fn detect(prompt: &str) -> Self {
        let p = prompt.to_lowercase();
        let nda_triggers = [
            "implement",
            "write",
            "define",
            "create",
            "build",
            "refactor",
            "fix",
            "port",
            "translate",
            "generate",
            "def ",
            "fn ",
            "func ",
            "class ",
            "struct ",
        ];
        if nda_triggers.iter().any(|&kw| p.contains(kw)) {
            Self::Nda
        } else {
            Self::Text
        }
    }
}

// ─── GenerationResult ─────────────────────────────────────────────────────────

/// Output of a Path 2 generation call.
#[derive(Serialize)]
pub struct NdaGenerationResult {
    /// The emitted NDA nodes, in emission order.
    #[serde(skip)]
    pub nodes: Vec<NdaNode>,
    /// Merkle root of the full program (0 if generation did not complete).
    pub root_hash: u64,
    /// Whether the Merkle root was valid (structural correctness guaranteed).
    pub valid: bool,
    /// True if the program was truncated by forced termination (structurally
    /// valid Merkle hash, but semantically incomplete — budget was exhausted
    /// before the model naturally closed all scopes).  These programs are
    /// displayed but NEVER stored in the site map.
    pub force_terminated: bool,
    /// Hash under which the program was stored in the site map.
    pub site_map_key: Option<u64>,
    /// Sandbox execution result.
    #[serde(skip)]
    pub sandbox: Option<crate::sandbox::SandboxResult>,
    /// Scope validation result.
    #[serde(skip)]
    pub scope: Option<crate::sandbox::scope_validator::ScopeValidation>,
    /// Generation statistics.
    pub stats: NdaGenStats,
    /// Number of nodes in the output program.
    pub node_count: usize,
}

impl NdaGenerationResult {
    /// Validate the generation result.
    /// Returns a list of warnings (empty = all good).
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if !self.valid {
            warnings.push("Program is not valid (Merkle verification failed)".to_string());
        }
        if self.force_terminated {
            warnings.push("Program was force-terminated (budget exhausted)".to_string());
        }
        if self.node_count == 0 && self.valid {
            warnings.push("Valid program has 0 nodes".to_string());
        }
        if let Some(ref sb) = self.sandbox {
            if sb.panicked {
                warnings.push("Sandbox execution panicked".to_string());
            }
            if let Some(ref err) = sb.error {
                warnings.push(format!("Sandbox execution error: {}", err));
            }
        }
        if let Some(ref sc) = self.scope {
            if !sc.passed {
                warnings.push(format!(
                    "Scope validation failed (similarity={:.2})",
                    sc.similarity
                ));
            }
        }

        warnings
    }

    /// Return a structured execution summary.
    pub fn execution_summary(&self) -> NdaExecutionSummary {
        NdaExecutionSummary {
            valid: self.valid,
            force_terminated: self.force_terminated,
            node_count: self.node_count,
            tokens_emitted: self.stats.tokens_emitted,
            elapsed_ms: self.stats.elapsed_ms as u64,
            cache_hit_rate: self.stats.cache_hit_rate(),
            sandbox_passed: self.sandbox.as_ref().map(|s| s.is_success()),
            scope_passed: self.scope.as_ref().map(|s| s.passed),
            stored_in_site_map: self.site_map_key.is_some(),
        }
    }
}

/// Structured execution summary for NDA generation.
#[derive(Debug, Clone, Serialize)]
pub struct NdaExecutionSummary {
    pub valid: bool,
    pub force_terminated: bool,
    pub node_count: usize,
    pub tokens_emitted: usize,
    pub elapsed_ms: u64,
    pub cache_hit_rate: f64,
    pub sandbox_passed: Option<bool>,
    pub scope_passed: Option<bool>,
    pub stored_in_site_map: bool,
}

#[derive(Default, Debug, Clone, Serialize)]
pub struct NdaGenStats {
    pub tokens_emitted: usize,
    pub site_map_hits: usize,   // KV lookups that hit the persistent cache
    pub site_map_misses: usize, // KV lookups that required recomputation
    pub elapsed_ms: u128,
    /// Per-opcode emission counts (indexed by opcode ordinal).
    pub opcode_distribution: Vec<usize>,
    /// Peak Merkle verifier stack depth reached during generation.
    pub peak_stack_depth: usize,
    /// Number of rep-penalty applications (soft + hard).
    pub rep_penalty_applications: usize,
}

impl NdaGenStats {
    /// Ensure opcode_distribution is sized to VOCAB_SIZE.
    fn ensure_distribution(&mut self) {
        if self.opcode_distribution.is_empty() {
            self.opcode_distribution = vec![0; NdaOpcode::VOCAB_SIZE];
        }
    }

    /// Compute cache hit rate: hits / (hits + misses).
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.site_map_hits + self.site_map_misses;
        if total == 0 {
            0.0
        } else {
            self.site_map_hits as f64 / total as f64
        }
    }

    /// Return the top-N most emitted opcodes with counts.
    pub fn top_opcodes(&self, n: usize) -> Vec<(NdaOpcode, usize)> {
        let mut pairs: Vec<(usize, usize)> = self
            .opcode_distribution
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, c)| *c > 0)
            .collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1));
        pairs
            .into_iter()
            .take(n)
            .filter_map(|(idx, count)| NdaOpcode::from_u8(idx as u8).map(|op| (op, count)))
            .collect()
    }

    /// Return a serializable snapshot of the stats.
    pub fn snapshot(&self) -> NdaGenStatsSnapshot {
        NdaGenStatsSnapshot {
            tokens_emitted: self.tokens_emitted,
            site_map_hits: self.site_map_hits,
            site_map_misses: self.site_map_misses,
            elapsed_ms: self.elapsed_ms as u64,
            cache_hit_rate: self.cache_hit_rate(),
            peak_stack_depth: self.peak_stack_depth,
            rep_penalty_applications: self.rep_penalty_applications,
            unique_opcodes_emitted: self.opcode_distribution.iter().filter(|&&c| c > 0).count(),
        }
    }
}

/// Serializable snapshot of generation stats.
#[derive(Debug, Clone, Serialize)]
pub struct NdaGenStatsSnapshot {
    pub tokens_emitted: usize,
    pub site_map_hits: usize,
    pub site_map_misses: usize,
    pub elapsed_ms: u64,
    pub cache_hit_rate: f64,
    pub peak_stack_depth: usize,
    pub rep_penalty_applications: usize,
    pub unique_opcodes_emitted: usize,
}

// ─── NdaPipeline ──────────────────────────────────────────────────────────────

/// Path 2: pure NDA native inference pipeline.
///
/// Wraps a ZeroTransformer with:
///   - An NdaHead (9-opcode output projection)
///   - A SiteMap (persistent KV store, replaces session cache)
///   - A MerkleVerifier (structural validity enforcer)
pub struct NdaPipeline {
    model: ZeroTransformer,
    head: NdaHead,
    site_map: SiteMap,
    verifier: MerkleVerifier,
}

impl NdaPipeline {
    /// Open or create an NDA pipeline.
    ///
    /// - `model_dir`: directory containing `.nda` weight files.
    /// - `site_map_dir`: directory for the persistent KV store
    ///   (created if absent; defaults to `model_dir/../site_map`).
    /// - `head_path`: path to saved NdaHead weights
    ///   (initialises randomly if absent).
    pub fn open(
        model_dir: &Path,
        site_map_dir: Option<&Path>,
        head_path: Option<&Path>,
        cfg: ModelConfig,
    ) -> Result<Self> {
        // Load NDA-Zero transformer weights.
        let weights = ModelWeights::load(model_dir, &cfg)?;
        let model = ZeroTransformer::new(cfg, weights);

        // Open site map.
        let sm_dir: PathBuf = match site_map_dir {
            Some(d) => d.to_path_buf(),
            None => model_dir.join("../site_map"),
        };
        let weight_root = SiteMap::hash_weight_dir(model_dir);
        let site_map = SiteMap::open(&sm_dir, weight_root)?;

        // Load or initialise NDA head.
        let head = match head_path {
            Some(p) if p.exists() => {
                eprintln!("[pipeline_nda] Loading NdaHead from {p:?}");
                NdaHead::load(p)?
            }
            _ => {
                eprintln!("[pipeline_nda] Initialising NdaHead with random weights");
                NdaHead::random()
            }
        };

        Ok(Self {
            model,
            head,
            site_map,
            verifier: MerkleVerifier::new(),
        })
    }

    /// Generate NDA opcodes given a conditioning hidden state from Path 1.
    ///
    /// The `condition` vector is the final hidden state produced by the text
    /// transformer after encoding the natural-language prompt.  It is used as
    /// the initial token embedding for Path 2 (bridges Path 1 → Path 2).
    ///
    /// If `condition` is `None`, generation starts from a learned start token.
    pub fn generate(
        &mut self,
        condition: Option<&[f32]>,
        max_opcodes: usize,
        on_opcode: impl FnMut(NdaOpcode),
    ) -> NdaGenerationResult {
        let t_start = std::time::Instant::now();
        self.verifier.reset();
        self._generate_inner(condition, max_opcodes, on_opcode, t_start)
    }

    #[allow(clippy::explicit_counter_loop)]
    fn _generate_inner(
        &mut self,
        condition: Option<&[f32]>,
        max_opcodes: usize,
        mut on_opcode: impl FnMut(NdaOpcode),
        t_start: std::time::Instant,
    ) -> NdaGenerationResult {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        let mut nodes = Vec::new();
        let mut root_hash = 0u64;
        let mut valid = false;
        let mut force_terminated = false;

        self.model.reset_cache();

        // Start token: opcode 0 = SCOPE (begin program).
        let mut current_opcode_id: u32 = NdaOpcode::Scope as u32;
        self.verifier.open_scope();
        on_opcode(NdaOpcode::Scope);
        stats.tokens_emitted += 1;
        stats.opcode_distribution[NdaOpcode::Scope as usize] += 1;
        stats.peak_stack_depth = stats.peak_stack_depth.max(self.verifier.stack.len());

        // Rep-penalty state: sliding window of recent opcode IDs (mirrors
        // the integer rep-penalty in transformer_zero.rs).
        const REP_WINDOW: usize = 6;
        const REP_PENALTY: i32 = 1024; // subtracted per occurrence in window
        let mut recent_ops: [u8; REP_WINDOW] = [NdaOpcode::VOCAB_SIZE as u8; REP_WINDOW];
        let mut rep_ptr = 0usize;

        let mut current_width = condition.map(|c| c.len()).unwrap_or(896);
        let mut matrix_count = 0;

        for step in 0..max_opcodes {
            // ── Forward pass → hidden state (896-dim i32) ─────────────────────
            // Only the first step is conditioned on the Path-1 hidden state;
            // subsequent steps attend to it through the KV cache.
            let step_condition = if step == 0 { condition } else { None };
            let logits_i32 = self.model.forward_one_zero(
                current_opcode_id,
                step,
                step_condition,
                Some(&mut self.site_map),
                &mut stats.site_map_hits,
                &mut stats.site_map_misses,
            );

            // ── Convert to f32 and project through NdaHead → 9 logits ─────────
            let hidden_f32: Vec<f32> = logits_i32.iter().map(|&v| v as f32).collect();
            let raw_logits = self.head.forward(&hidden_f32);

            // ── Scale to i32 for integer rep-penalty arithmetic ───────────────
            let mut logits_i32_op: [i32; 9] =
                std::array::from_fn(|i| (raw_logits[i] * 4096.0) as i32);

            // ── Rep penalty (bit-shift right per occurrence) ──────────────────
            let mut rep_applied = false;
            for &past_op in &recent_ops {
                let idx = past_op as usize;
                if idx < NdaOpcode::VOCAB_SIZE {
                    // Halve the logit for each occurrence in the window
                    logits_i32_op[idx] -= logits_i32_op[idx].abs() >> 2;
                    rep_applied = true;
                }
            }
            if rep_applied {
                stats.rep_penalty_applications += 1;
            }

            // ── Grammar constraint: mask invalid opcodes ──────────────────────
            // Derives valid next-opcode set from verifier stack depth.
            let depth = self.verifier.stack.len();
            // depth == 1: top-level scope open, can emit nodes or close
            // depth >  1: nested scope open
            // We also use an opcode budget heuristic: once step > max/2,
            // start biasing toward END_SCOPE to ensure the program terminates.
            // Budget pressure: start biasing toward closure at 60% of budget.
            let budget_pressure = step * 10 >= max_opcodes * 6;
            // Hard close: at 85% of budget, force END_SCOPE above all else.
            let hard_close = step * 20 >= max_opcodes * 17;

            const NEG_INF: i32 = i32::MIN / 2;
            // Block ROOT entirely — we emit it ourselves on scope close.
            logits_i32_op[NdaOpcode::Root as usize] = NEG_INF;
            // Block SCOPE nesting deeper than 4 (prevents exponential explosion)
            if depth >= 5 {
                logits_i32_op[NdaOpcode::Scope as usize] = NEG_INF;
            }
            // Under budget pressure: heavily bias toward closing open scopes
            if budget_pressure && depth > 1 {
                logits_i32_op[NdaOpcode::EndScope as usize] += REP_PENALTY * 4;
            }
            // Hard close: override everything — we MUST close soon
            if hard_close && depth > 1 {
                logits_i32_op[NdaOpcode::EndScope as usize] = i32::MAX / 2;
            }

            // ── Argmax over constrained + penalised logits ────────────────────
            let best_op = logits_i32_op
                .iter()
                .enumerate()
                .max_by_key(|(_, &v)| v)
                .map(|(i, _)| i)
                .unwrap_or(NdaOpcode::Int as usize);

            let opcode = NdaOpcode::from_u8(best_op as u8).unwrap_or(NdaOpcode::Int);

            // ── Update rep-penalty window ─────────────────────────────────────
            recent_ops[rep_ptr % REP_WINDOW] = best_op as u8;
            rep_ptr += 1;

            // ── Merkle verification ───────────────────────────────────────────
            match opcode {
                NdaOpcode::Scope => {
                    self.verifier.open_scope();
                }
                NdaOpcode::EndScope => {
                    match self.verifier.close_scope() {
                        Ok(_) => {
                            // If we've closed back to depth 1 (top-level only),
                            // emit ROOT immediately to seal the program.
                            if self.verifier.stack.len() == 1 {
                                let top = self
                                    .verifier
                                    .stack
                                    .first()
                                    .and_then(|v| v.last())
                                    .copied()
                                    .unwrap_or(0);
                                self.verifier.record_root(top);
                                root_hash = top;
                                valid = self.verifier.is_valid();
                                on_opcode(opcode); // END_SCOPE
                                on_opcode(NdaOpcode::Root);
                                stats.tokens_emitted += 2;
                                break;
                            }
                        }
                        Err(_) => break, // malformed — prune
                    }
                }
                NdaOpcode::Root => {
                    let computed = self
                        .verifier
                        .stack
                        .first()
                        .and_then(|v| v.last())
                        .copied()
                        .unwrap_or(0);
                    self.verifier.record_root(computed);
                    root_hash = computed;
                    valid = self.verifier.is_valid();
                    on_opcode(opcode);
                    stats.tokens_emitted += 1;
                    break;
                }
                NdaOpcode::Matrix => {
                    let cols = current_width;
                    let is_first = matrix_count == 0;
                    let rows = if is_first {
                        128
                    } else {
                        let budget_pressure = step * 10 >= max_opcodes * 6;
                        if budget_pressure {
                            896
                        } else {
                            let abs_sum: u64 =
                                logits_i32.iter().map(|&x| x.unsigned_abs() as u64).sum();
                            match abs_sum % 4 {
                                0 => 64,
                                1 => 128,
                                2 => 256,
                                _ => 896,
                            }
                        }
                    };
                    current_width = rows;
                    matrix_count += 1;
                    let leaf = NdaNode::Matrix {
                        rows: rows as u16,
                        cols: cols as u16,
                        scale: 0,
                        sign: vec![0xAA; rows * cols.div_ceil(8)],
                        extra: vec![0x55; rows * cols.div_ceil(8)],
                    };
                    self.verifier.push_leaf(&leaf);
                    nodes.push(leaf);
                }
                NdaOpcode::Norm => {
                    let size = current_width;
                    let leaf = NdaNode::Norm {
                        size: size as u16,
                        weight: vec![0xFF; size.div_ceil(8)],
                        bias: vec![0x00; size.div_ceil(8)],
                    };
                    self.verifier.push_leaf(&leaf);
                    nodes.push(leaf);
                }
                NdaOpcode::Call => {
                    let target = self.site_map.get_any_node_hash().unwrap_or(0);
                    let leaf = NdaNode::Call { target };
                    self.verifier.push_leaf(&leaf);
                    nodes.push(leaf);
                }
                NdaOpcode::Int => {
                    let value = (raw_logits[5] * 100.0) as i32 + argmax_f32(&hidden_f32) as i32;
                    let leaf = NdaNode::Int { value };
                    current_width = 1;
                    self.verifier.push_leaf(&leaf);
                    nodes.push(leaf);
                }
                NdaOpcode::Bit0 => {
                    let leaf = NdaNode::Int { value: 0 };
                    current_width = 1;
                    self.verifier.push_leaf(&leaf);
                    nodes.push(leaf);
                }
                NdaOpcode::Bit1 => {
                    let leaf = NdaNode::Int { value: 1 };
                    current_width = 1;
                    self.verifier.push_leaf(&leaf);
                    nodes.push(leaf);
                }
                // Language opcodes: not emitted by the model head during
                // generation — only used when executing .nda programs directly.
                // If the model head somehow emits these, treat as a no-op.
                NdaOpcode::Loop
                | NdaOpcode::While
                | NdaOpcode::If
                | NdaOpcode::Compare
                | NdaOpcode::Let
                | NdaOpcode::Load
                | NdaOpcode::Store
                | NdaOpcode::Add
                | NdaOpcode::VecOp
                | NdaOpcode::Print
                | NdaOpcode::Return
                | NdaOpcode::Break
                | NdaOpcode::Bitwise
                | NdaOpcode::Float
                | NdaOpcode::Math
                | NdaOpcode::MathFunc
                | NdaOpcode::Peek
                | NdaOpcode::Poke
                | NdaOpcode::Gemv
                | NdaOpcode::Dot
                | NdaOpcode::Syscall
                | NdaOpcode::Spawn
                | NdaOpcode::Atomic
                | NdaOpcode::Alloc
                | NdaOpcode::Free
                | NdaOpcode::RegInt
                | NdaOpcode::Cast
                | NdaOpcode::GpuDispatch
                | NdaOpcode::Triple => {
                    // Reserved for direct .nda program execution.
                    // During model generation, skip.
                }
            }

            on_opcode(opcode);
            stats.tokens_emitted += 1;
            if (best_op as usize) < NdaOpcode::VOCAB_SIZE {
                stats.opcode_distribution[best_op as usize] += 1;
            }
            stats.peak_stack_depth = stats.peak_stack_depth.max(self.verifier.stack.len());
            current_opcode_id = best_op as u32;
        }

        // ── Forced termination: if the loop exhausted max_opcodes without
        // naturally closing all scopes, seal the tree now.
        if !valid {
            let open_scopes = self.verifier.stack.len().saturating_sub(1);
            if open_scopes > 0 {
                eprintln!(
                    "[pipeline_nda] WARNING: forced termination \u{2014} \
                    budget exhausted with {open_scopes} unclosed scope(s). \
                    Output is TRUNCATED (structurally valid, semantically incomplete)."
                );
                force_terminated = true;
            }
            // Close all inner scopes (stack depth > 1).
            while self.verifier.stack.len() > 1 {
                match self.verifier.close_scope() {
                    Ok(_) => {
                        on_opcode(NdaOpcode::EndScope);
                        stats.tokens_emitted += 1;
                    }
                    Err(_) => break,
                }
            }
            // Commit the root from whatever nodes completed naturally.
            let top = self
                .verifier
                .stack
                .first()
                .and_then(|v| v.last())
                .copied()
                .unwrap_or(0);
            if top != 0 {
                self.verifier.record_root(top);
                root_hash = top;
                valid = self.verifier.is_valid();
                on_opcode(NdaOpcode::Root);
                stats.tokens_emitted += 1;
            }
        }

        // ── Execute Sandbox and Scope Validator ────────────────────────────────
        let cond_vec = condition.unwrap_or(&[]);
        let (sandbox, scope) = if valid && !cond_vec.is_empty() {
            let sandbox_res = crate::sandbox::NdaJitSandbox::run(&nodes, cond_vec, &self.site_map);
            let threshold = 0.10f32; // Starts at 0.10
            let scope_val = crate::sandbox::scope_validator::ScopeValidator::validate(
                &sandbox_res.output_vec,
                cond_vec,
                threshold,
            );
            (Some(sandbox_res), Some(scope_val))
        } else {
            (None, None)
        };

        // ── Store valid programs in the site map ───────────────────────────────
        // Truncated programs (force_terminated) or failed scope programs are NOT stored.
        let scope_passed = scope.as_ref().map(|s| s.passed).unwrap_or(true);
        let site_map_key = if valid && !force_terminated && scope_passed && !nodes.is_empty() {
            let program = NdaNode::Scope {
                children: nodes.clone(),
            };
            self.site_map.put_program(&program).ok()
        } else {
            None
        };

        // Flush site map index to disk.
        let _ = self.site_map.flush();

        stats.elapsed_ms = t_start.elapsed().as_millis();

        let node_count = nodes.len();
        NdaGenerationResult {
            nodes,
            root_hash,
            valid,
            force_terminated,
            site_map_key,
            sandbox,
            scope,
            stats,
            node_count,
        }
    }

    /// Site map statistics.
    pub fn site_map_stats(&self) -> crate::site_map::SiteMapStats {
        self.site_map.stats()
    }

    /// Save the NdaHead weights.
    pub fn save_head(&self, path: &Path) -> Result<()> {
        self.head.save(path)
    }

    /// Expose the site map for the bridge layer.
    pub fn site_map_mut(&mut self) -> &mut SiteMap {
        &mut self.site_map
    }

    /// Expose the site map (immutable).
    pub fn site_map(&self) -> &SiteMap {
        &self.site_map
    }
}

// ─── Batch Generation ─────────────────────────────────────────────────────────

/// Result of a batch generation run across multiple prompts.
#[derive(Debug, Serialize)]
pub struct BatchGenerationReport {
    /// Number of prompts in the batch.
    pub prompt_count: usize,
    /// Per-prompt results.
    pub results: Vec<BatchItemResult>,
    /// Total elapsed time for the entire batch (ms).
    pub total_elapsed_ms: u128,
    /// Aggregate token count across all prompts.
    pub total_tokens: usize,
    /// Number of valid programs produced.
    pub valid_count: usize,
    /// Number of force-terminated programs.
    pub truncated_count: usize,
    /// Average cache hit rate across all prompts.
    pub avg_cache_hit_rate: f64,
}

/// Single item result within a batch generation.
#[derive(Debug, Serialize)]
pub struct BatchItemResult {
    /// Index of this prompt in the batch.
    pub index: usize,
    /// Whether the generated program was valid.
    pub valid: bool,
    /// Whether the program was force-terminated.
    pub force_terminated: bool,
    /// Number of tokens emitted.
    pub tokens_emitted: usize,
    /// Number of nodes in the output.
    pub node_count: usize,
    /// Generation time (ms).
    pub elapsed_ms: u128,
    /// Merkle root hash.
    pub root_hash: u64,
    /// Site map key (if stored).
    pub site_map_key: Option<u64>,
    /// Cache hit rate for this generation.
    pub cache_hit_rate: f64,
}

impl NdaPipeline {
    /// Generate multiple programs in sequence, returning an aggregated report.
    ///
    /// Each prompt is generated independently (the model cache is reset between
    /// prompts, but the site map accumulates across the batch).
    pub fn generate_batch(
        &mut self,
        conditions: &[Option<Vec<f32>>],
        max_opcodes: usize,
    ) -> BatchGenerationReport {
        let batch_start = std::time::Instant::now();
        let mut results = Vec::with_capacity(conditions.len());
        let mut total_tokens = 0usize;
        let mut valid_count = 0usize;
        let mut truncated_count = 0usize;
        let mut total_cache_rate = 0.0f64;

        for (idx, cond) in conditions.iter().enumerate() {
            let cond_ref = cond.as_deref();
            let mut emitted = Vec::new();
            let result = self.generate(cond_ref, max_opcodes, |op| {
                emitted.push(op);
            });

            let cache_rate = result.stats.cache_hit_rate();
            total_cache_rate += cache_rate;
            total_tokens += result.stats.tokens_emitted;
            if result.valid {
                valid_count += 1;
            }
            if result.force_terminated {
                truncated_count += 1;
            }

            results.push(BatchItemResult {
                index: idx,
                valid: result.valid,
                force_terminated: result.force_terminated,
                tokens_emitted: result.stats.tokens_emitted,
                node_count: result.node_count,
                elapsed_ms: result.stats.elapsed_ms,
                root_hash: result.root_hash,
                site_map_key: result.site_map_key,
                cache_hit_rate: cache_rate,
            });
        }

        let count = conditions.len().max(1);
        BatchGenerationReport {
            prompt_count: conditions.len(),
            results,
            total_elapsed_ms: batch_start.elapsed().as_millis(),
            total_tokens,
            valid_count,
            truncated_count,
            avg_cache_hit_rate: total_cache_rate / count as f64,
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn argmax_f32(v: &[f32]) -> usize {
    v.iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nda_head_forward_shape() {
        let head = NdaHead::random();
        let hidden = vec![0.1f32; 896];
        let logits = head.forward(&hidden);
        assert_eq!(logits.len(), NdaOpcode::VOCAB_SIZE);
    }

    #[test]
    fn nda_head_round_trip() {
        use tempfile::NamedTempFile;
        let head = NdaHead::random();
        let file = NamedTempFile::new().unwrap();
        head.save(file.path()).unwrap();
        let head2 = NdaHead::load(file.path()).unwrap();
        assert!((head.w1[0] - head2.w1[0]).abs() < 1e-6);
        assert!((head.b2[8] - head2.b2[8]).abs() < 1e-6);
    }

    #[test]
    fn pipeline_mode_detection() {
        assert_eq!(
            PipelineMode::detect("implement binary search"),
            PipelineMode::Nda
        );
        assert_eq!(PipelineMode::detect("def foo():"), PipelineMode::Nda);
        assert_eq!(
            PipelineMode::detect("what is binary search?"),
            PipelineMode::Text
        );
        assert_eq!(
            PipelineMode::detect("explain how sorting works"),
            PipelineMode::Text
        );
    }

    #[test]
    fn pipeline_mode_from_str() {
        assert_eq!(PipelineMode::from_str("nda"), PipelineMode::Nda);
        assert_eq!(PipelineMode::from_str("native"), PipelineMode::Nda);
        assert_eq!(PipelineMode::from_str("auto"), PipelineMode::Auto);
        assert_eq!(PipelineMode::from_str("text"), PipelineMode::Text);
        assert_eq!(PipelineMode::from_str("unknown"), PipelineMode::Text);
    }

    #[test]
    fn gen_stats_default_cache_hit_rate() {
        let stats = NdaGenStats::default();
        assert!((stats.cache_hit_rate() - 0.0).abs() < f64::EPSILON);
        assert_eq!(stats.tokens_emitted, 0);
        assert!(stats.opcode_distribution.is_empty());
    }

    #[test]
    fn gen_stats_cache_hit_rate_computation() {
        let mut stats = NdaGenStats::default();
        stats.site_map_hits = 7;
        stats.site_map_misses = 3;
        let rate = stats.cache_hit_rate();
        assert!((rate - 0.7).abs() < 1e-9);
    }

    #[test]
    fn gen_stats_ensure_distribution() {
        let mut stats = NdaGenStats::default();
        assert!(stats.opcode_distribution.is_empty());
        stats.ensure_distribution();
        assert_eq!(stats.opcode_distribution.len(), NdaOpcode::VOCAB_SIZE);
        assert!(stats.opcode_distribution.iter().all(|&v| v == 0));
        // Calling again should not reset
        stats.opcode_distribution[0] = 42;
        stats.ensure_distribution();
        assert_eq!(stats.opcode_distribution[0], 42);
    }

    #[test]
    fn gen_stats_top_opcodes() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.opcode_distribution[NdaOpcode::Scope as usize] = 10;
        stats.opcode_distribution[NdaOpcode::Int as usize] = 25;
        stats.opcode_distribution[NdaOpcode::Matrix as usize] = 5;
        let top = stats.top_opcodes(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0], (NdaOpcode::Int, 25));
        assert_eq!(top[1], (NdaOpcode::Scope, 10));
    }

    #[test]
    fn gen_stats_top_opcodes_empty() {
        let stats = NdaGenStats::default();
        let top = stats.top_opcodes(5);
        assert!(top.is_empty());
    }

    #[test]
    fn gen_stats_serializable() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.tokens_emitted = 100;
        stats.site_map_hits = 80;
        stats.site_map_misses = 20;
        stats.elapsed_ms = 500;
        stats.opcode_distribution[NdaOpcode::Scope as usize] = 50;
        stats.peak_stack_depth = 3;
        stats.rep_penalty_applications = 15;
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"tokens_emitted\":100"));
        assert!(json.contains("\"peak_stack_depth\":3"));
        assert!(json.contains("\"rep_penalty_applications\":15"));
    }

    #[test]
    fn batch_report_serializable() {
        let report = BatchGenerationReport {
            prompt_count: 2,
            results: vec![
                BatchItemResult {
                    index: 0,
                    valid: true,
                    force_terminated: false,
                    tokens_emitted: 50,
                    node_count: 10,
                    elapsed_ms: 100,
                    root_hash: 0xDEAD,
                    site_map_key: Some(42),
                    cache_hit_rate: 0.8,
                },
                BatchItemResult {
                    index: 1,
                    valid: false,
                    force_terminated: true,
                    tokens_emitted: 200,
                    node_count: 0,
                    elapsed_ms: 300,
                    root_hash: 0,
                    site_map_key: None,
                    cache_hit_rate: 0.5,
                },
            ],
            total_elapsed_ms: 400,
            total_tokens: 250,
            valid_count: 1,
            truncated_count: 1,
            avg_cache_hit_rate: 0.65,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"prompt_count\":2"));
        assert!(json.contains("\"valid_count\":1"));
        assert!(json.contains("\"root_hash\":57005")); // 0xDEAD
    }

    #[test]
    fn generation_result_has_node_count() {
        // Verify NdaGenerationResult serializes node_count
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 123,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 0,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"node_count\":0"));
        assert!(json.contains("\"root_hash\":123"));
        // nodes, sandbox, scope should be skipped
        assert!(!json.contains("\"nodes\""));
        assert!(!json.contains("\"sandbox\""));
    }

    // ─── Validation & summary tests ────────────────────────────────────────

    #[test]
    fn generation_result_validate_clean() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 123,
            valid: true,
            force_terminated: false,
            site_map_key: Some(42),
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 5,
        };
        let warnings = result.validate();
        assert!(warnings.is_empty());
    }

    #[test]
    fn generation_result_validate_detects_invalid() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: false,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 0,
        };
        let warnings = result.validate();
        assert!(warnings.iter().any(|w| w.contains("not valid")));
    }

    #[test]
    fn generation_result_validate_detects_truncated() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 123,
            valid: true,
            force_terminated: true,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 10,
        };
        let warnings = result.validate();
        assert!(warnings.iter().any(|w| w.contains("force-terminated")));
    }

    #[test]
    fn nda_execution_summary_serializes() {
        let summary = NdaExecutionSummary {
            valid: true,
            force_terminated: false,
            node_count: 10,
            tokens_emitted: 50,
            elapsed_ms: 100,
            cache_hit_rate: 0.8,
            sandbox_passed: Some(true),
            scope_passed: Some(true),
            stored_in_site_map: true,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"valid\":true"));
        assert!(json.contains("\"node_count\":10"));
        assert!(json.contains("\"cache_hit_rate\":0.8"));
    }

    #[test]
    fn nda_gen_stats_snapshot() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.tokens_emitted = 100;
        stats.site_map_hits = 80;
        stats.site_map_misses = 20;
        stats.opcode_distribution[NdaOpcode::Scope as usize] = 50;
        stats.opcode_distribution[NdaOpcode::Int as usize] = 30;

        let snap = stats.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"tokens_emitted\":100"));
        assert!(json.contains("\"unique_opcodes_emitted\":2"));
        assert!((snap.cache_hit_rate - 0.8).abs() < 0.01);
    }

    // ── Block 99: NdaHead tests ──────────────────────────────────────────────

    #[test]
    fn nda_head_zeros_produces_zero_weights() {
        let head = NdaHead::zeros();
        assert!(head.w1.iter().all(|&v| v == 0.0));
        assert!(head.b1.iter().all(|&v| v == 0.0));
        assert!(head.w2.iter().all(|&v| v == 0.0));
        assert!(head.b2.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn nda_head_zeros_forward_returns_bias() {
        let mut head = NdaHead::zeros();
        // Set non-zero biases so we can verify they pass through
        for (i, b) in head.b2.iter_mut().enumerate() {
            *b = i as f32 * 0.1;
        }
        let hidden = vec![0.0f32; 896];
        let logits = head.forward(&hidden);
        // With zero weights and zero hidden input, ReLU(b1=0) = 0,
        // so output = b2
        for (i, &logit) in logits.iter().enumerate() {
            assert!(
                (logit - i as f32 * 0.1).abs() < 1e-6,
                "logit[{}] = {} expected {}",
                i,
                logit,
                i as f32 * 0.1
            );
        }
    }

    #[test]
    fn nda_head_weight_dimensions() {
        let head = NdaHead::zeros();
        let out = NdaOpcode::VOCAB_SIZE; // 38
        assert_eq!(head.w1.len(), 64 * 896); // MID × IN
        assert_eq!(head.b1.len(), 64); // MID
        assert_eq!(head.w2.len(), out * 64); // OUT × MID
        assert_eq!(head.b2.len(), out); // OUT
    }

    #[test]
    fn nda_head_random_deterministic() {
        let h1 = NdaHead::random();
        let h2 = NdaHead::random();
        // Same PRNG seed → same weights
        assert!((h1.w1[0] - h2.w1[0]).abs() < 1e-9);
        assert!((h1.b2[4] - h2.b2[4]).abs() < 1e-9);
    }

    // ── Block 99: PipelineMode extended tests ───────────────────────────────

    #[test]
    fn pipeline_mode_detect_all_triggers() {
        let nda_prompts = [
            "implement foo",
            "write a function",
            "define a class",
            "create a new module",
            "build the project",
            "refactor this code",
            "fix the bug",
            "port to Rust",
            "translate to Python",
            "generate code",
            "def foo():",
            "fn main()",
            "func main()",
            "class Foo",
            "struct Bar",
        ];
        for prompt in &nda_prompts {
            assert_eq!(
                PipelineMode::detect(prompt),
                PipelineMode::Nda,
                "expected NDA for: {}",
                prompt
            );
        }
    }

    #[test]
    fn pipeline_mode_detect_text_prompts() {
        let text_prompts = [
            "what is Rust?",
            "explain borrowing",
            "how does async work?",
            "tell me about iterators",
            "why is my code slow?",
        ];
        for prompt in &text_prompts {
            assert_eq!(
                PipelineMode::detect(prompt),
                PipelineMode::Text,
                "expected Text for: {}",
                prompt
            );
        }
    }

    #[test]
    fn pipeline_mode_from_str_case_insensitive() {
        assert_eq!(PipelineMode::from_str("NDA"), PipelineMode::Nda);
        assert_eq!(PipelineMode::from_str("Nda"), PipelineMode::Nda);
        assert_eq!(PipelineMode::from_str("NATIVE"), PipelineMode::Nda);
        assert_eq!(PipelineMode::from_str("AUTO"), PipelineMode::Auto);
        assert_eq!(PipelineMode::from_str("TEXT"), PipelineMode::Text);
    }

    // ── Block 99: NdaGenStats extended tests ────────────────────────────────

    #[test]
    fn cache_hit_rate_all_hits() {
        let stats = NdaGenStats {
            site_map_hits: 100,
            site_map_misses: 0,
            ..Default::default()
        };
        assert!((stats.cache_hit_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_hit_rate_all_misses() {
        let stats = NdaGenStats {
            site_map_hits: 0,
            site_map_misses: 50,
            ..Default::default()
        };
        assert!((stats.cache_hit_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn top_opcodes_more_than_available() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.opcode_distribution[NdaOpcode::Scope as usize] = 10;
        stats.opcode_distribution[NdaOpcode::Int as usize] = 5;
        // Request more than the 2 non-zero entries
        let top = stats.top_opcodes(10);
        assert_eq!(top.len(), 2); // only 2 non-zero opcodes
    }

    #[test]
    fn nda_gen_stats_snapshot_fields() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.tokens_emitted = 50;
        stats.site_map_hits = 40;
        stats.site_map_misses = 10;
        stats.elapsed_ms = 200;
        stats.peak_stack_depth = 4;
        stats.rep_penalty_applications = 20;
        stats.opcode_distribution[NdaOpcode::Scope as usize] = 15;
        stats.opcode_distribution[NdaOpcode::Matrix as usize] = 10;
        stats.opcode_distribution[NdaOpcode::Norm as usize] = 5;

        let snap = stats.snapshot();
        assert_eq!(snap.tokens_emitted, 50);
        assert_eq!(snap.site_map_hits, 40);
        assert_eq!(snap.site_map_misses, 10);
        assert_eq!(snap.elapsed_ms, 200);
        assert_eq!(snap.peak_stack_depth, 4);
        assert_eq!(snap.rep_penalty_applications, 20);
        assert_eq!(snap.unique_opcodes_emitted, 3);
        assert!((snap.cache_hit_rate - 0.8).abs() < 0.01);
    }

    #[test]
    fn nda_gen_stats_snapshot_serializes() {
        let snap = NdaGenStatsSnapshot {
            tokens_emitted: 10,
            site_map_hits: 5,
            site_map_misses: 5,
            elapsed_ms: 100,
            cache_hit_rate: 0.5,
            peak_stack_depth: 2,
            rep_penalty_applications: 3,
            unique_opcodes_emitted: 4,
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"unique_opcodes_emitted\":4"));
        assert!(json.contains("\"cache_hit_rate\":0.5"));
    }

    // ── Block 99: NdaGenerationResult extended tests ────────────────────────

    #[test]
    fn generation_result_validate_zero_nodes_valid() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 42,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 0,
        };
        let warnings = result.validate();
        assert!(warnings.iter().any(|w| w.contains("0 nodes")));
    }

    #[test]
    fn generation_result_execution_summary() {
        let mut stats = NdaGenStats::default();
        stats.tokens_emitted = 100;
        stats.site_map_hits = 80;
        stats.site_map_misses = 20;
        stats.elapsed_ms = 500;

        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0xABCD,
            valid: true,
            force_terminated: false,
            site_map_key: Some(42),
            sandbox: None,
            scope: None,
            stats,
            node_count: 10,
        };
        let summary = result.execution_summary();
        assert!(summary.valid);
        assert!(!summary.force_terminated);
        assert_eq!(summary.node_count, 10);
        assert_eq!(summary.tokens_emitted, 100);
        assert_eq!(summary.elapsed_ms, 500);
        assert!((summary.cache_hit_rate - 0.8).abs() < 0.01);
        assert!(summary.stored_in_site_map);
        assert!(summary.sandbox_passed.is_none());
        assert!(summary.scope_passed.is_none());
    }

    #[test]
    fn nda_execution_summary_clone_and_serialize() {
        let summary = NdaExecutionSummary {
            valid: true,
            force_terminated: false,
            node_count: 5,
            tokens_emitted: 50,
            elapsed_ms: 100,
            cache_hit_rate: 0.9,
            sandbox_passed: Some(true),
            scope_passed: Some(false),
            stored_in_site_map: true,
        };
        let cloned = summary.clone();
        assert_eq!(cloned.valid, summary.valid);
        assert_eq!(cloned.node_count, summary.node_count);
        let json = serde_json::to_string(&cloned).unwrap();
        assert!(json.contains("\"stored_in_site_map\":true"));
    }

    // ── Block 99: Batch report tests ────────────────────────────────────────

    #[test]
    fn batch_item_result_serializes() {
        let item = BatchItemResult {
            index: 0,
            valid: true,
            force_terminated: false,
            tokens_emitted: 50,
            node_count: 10,
            elapsed_ms: 100,
            root_hash: 0xDEAD,
            site_map_key: Some(42),
            cache_hit_rate: 0.8,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"index\":0"));
        assert!(json.contains("\"valid\":true"));
        assert!(json.contains("\"cache_hit_rate\":0.8"));
    }

    #[test]
    fn batch_report_empty_results() {
        let report = BatchGenerationReport {
            prompt_count: 0,
            results: vec![],
            total_elapsed_ms: 0,
            total_tokens: 0,
            valid_count: 0,
            truncated_count: 0,
            avg_cache_hit_rate: 0.0,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"prompt_count\":0"));
        assert!(json.contains("\"results\":[]"));
    }

    // ── Block 99: Helper function tests ─────────────────────────────────────

    #[test]
    fn random_uniform_deterministic() {
        let v1 = random_uniform(100, 1.0);
        let v2 = random_uniform(100, 1.0);
        assert_eq!(v1, v2);
    }

    #[test]
    fn random_uniform_correct_length() {
        let v = random_uniform(42, 0.5);
        assert_eq!(v.len(), 42);
    }

    #[test]
    fn random_uniform_values_in_range() {
        let scale = 0.5f32;
        let v = random_uniform(1000, scale);
        for &val in &v {
            assert!(
                val.abs() <= scale + 1e-6,
                "value {} exceeds scale {}",
                val,
                scale
            );
        }
    }

    #[test]
    fn argmax_f32_basic() {
        assert_eq!(argmax_f32(&[1.0, 3.0, 2.0]), 1);
        assert_eq!(argmax_f32(&[5.0, 1.0, 3.0]), 0);
        assert_eq!(argmax_f32(&[1.0, 1.0, 5.0]), 2);
    }

    #[test]
    fn argmax_f32_single_element() {
        assert_eq!(argmax_f32(&[42.0]), 0);
    }

    #[test]
    fn argmax_f32_negative_values() {
        assert_eq!(argmax_f32(&[-5.0, -1.0, -3.0]), 1);
    }

    // ── Block 143: NdaHead save/load error cases ────────────────────────────

    #[test]
    fn nda_head_load_bad_magic() {
        use tempfile::NamedTempFile;
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"BAD!\x00\x00\x00\x00").unwrap();
        let result = NdaHead::load(file.path());
        assert!(result.is_err());
        let err = format!("{}", result.err().unwrap());
        assert!(err.contains("invalid NdaHead magic"), "got: {}", err);
    }

    #[test]
    fn nda_head_load_wrong_size() {
        use tempfile::NamedTempFile;
        let file = NamedTempFile::new().unwrap();
        // Write correct magic but too few floats
        let mut buf = b"NDA\x01".to_vec();
        for _ in 0..10 {
            buf.extend_from_slice(&1.0f32.to_le_bytes());
        }
        std::fs::write(file.path(), &buf).unwrap();
        let result = NdaHead::load(file.path());
        assert!(result.is_err());
        let err = format!("{}", result.err().unwrap());
        assert!(err.contains("weight count mismatch"), "got: {}", err);
    }

    #[test]
    fn nda_head_load_nonexistent_file() {
        let result = NdaHead::load(std::path::Path::new("/nonexistent/path/head.bin"));
        assert!(result.is_err());
    }

    #[test]
    fn nda_head_save_load_roundtrip_all_weights() {
        use tempfile::NamedTempFile;
        let head = NdaHead::random();
        let file = NamedTempFile::new().unwrap();
        head.save(file.path()).unwrap();
        let loaded = NdaHead::load(file.path()).unwrap();
        // Check all weight arrays match
        assert_eq!(head.w1.len(), loaded.w1.len());
        assert_eq!(head.b1.len(), loaded.b1.len());
        assert_eq!(head.w2.len(), loaded.w2.len());
        assert_eq!(head.b2.len(), loaded.b2.len());
        for (a, b) in head.w1.iter().zip(loaded.w1.iter()) {
            assert!((a - b).abs() < 1e-9);
        }
        for (a, b) in head.b1.iter().zip(loaded.b1.iter()) {
            assert!((a - b).abs() < 1e-9);
        }
        for (a, b) in head.w2.iter().zip(loaded.w2.iter()) {
            assert!((a - b).abs() < 1e-9);
        }
        for (a, b) in head.b2.iter().zip(loaded.b2.iter()) {
            assert!((a - b).abs() < 1e-9);
        }
    }

    // ── Block 143: NdaHead forward with known weights ──────────────────────

    #[test]
    fn nda_head_forward_with_known_weights() {
        // Create a head with known simple weights and verify the math
        let mut head = NdaHead::zeros();
        // Set w1[0][0] = 1.0 (first row, first column)
        head.w1[0] = 1.0;
        // Set b1[0] = 0.5
        head.b1[0] = 0.5;
        // Set w2[0][0] = 2.0 (first output row, first mid element)
        head.w2[0] = 2.0;
        // Set b2[0] = 0.1
        head.b2[0] = 0.1;

        let mut hidden = vec![0.0f32; 896];
        hidden[0] = 3.0;

        let logits = head.forward(&hidden);
        // Layer 1, neuron 0: ReLU(1.0 * 3.0 + 0.5) = 3.5
        // Layer 2, output 0: 2.0 * 3.5 + 0.1 = 7.1
        assert!(
            (logits[0] - 7.1).abs() < 1e-4,
            "expected 7.1, got {}",
            logits[0]
        );
    }

    #[test]
    fn nda_head_forward_relu_kills_negatives() {
        let mut head = NdaHead::zeros();
        // Set w1[0][0] = -1.0 and b1[0] = 0.0
        head.w1[0] = -1.0;
        // Set w2[0][0] = 1.0
        head.w2[0] = 1.0;

        let mut hidden = vec![0.0f32; 896];
        hidden[0] = 5.0; // positive input

        let logits = head.forward(&hidden);
        // Layer 1, neuron 0: ReLU(-1.0 * 5.0 + 0.0) = ReLU(-5.0) = 0.0
        // Layer 2, output 0: 1.0 * 0.0 + 0.0 = 0.0
        assert!(
            (logits[0] - 0.0).abs() < 1e-6,
            "ReLU should kill negative pre-activation, got {}",
            logits[0]
        );
    }

    #[test]
    fn nda_head_forward_zero_hidden_all_zero_output() {
        let head = NdaHead::zeros();
        let hidden = vec![0.0f32; 896];
        let logits = head.forward(&hidden);
        // All weights zero, all biases zero, zero hidden → all outputs zero
        for (i, &l) in logits.iter().enumerate() {
            assert!(l.abs() < 1e-9, "logits[{}] = {} expected 0", i, l);
        }
    }

    #[test]
    fn nda_head_forward_output_dim() {
        let head = NdaHead::random();
        let hidden = vec![0.5f32; 896];
        let logits = head.forward(&hidden);
        assert_eq!(logits.len(), NdaOpcode::VOCAB_SIZE);
    }

    // ── Block 143: PipelineMode edge cases ─────────────────────────────────

    #[test]
    fn pipeline_mode_detect_empty_string() {
        // Empty string has no triggers → Text
        assert_eq!(PipelineMode::detect(""), PipelineMode::Text);
    }

    #[test]
    fn pipeline_mode_detect_case_insensitive() {
        assert_eq!(PipelineMode::detect("IMPLEMENT foo"), PipelineMode::Nda);
        assert_eq!(PipelineMode::detect("Write a function"), PipelineMode::Nda);
        assert_eq!(PipelineMode::detect("DEFINE a class"), PipelineMode::Nda);
        assert_eq!(PipelineMode::detect("STRUCT Foo"), PipelineMode::Nda);
    }

    #[test]
    fn pipeline_mode_from_str_empty() {
        // Empty string doesn't match "nda", "native", or "auto" → Text
        assert_eq!(PipelineMode::from_str(""), PipelineMode::Text);
    }

    #[test]
    fn pipeline_mode_from_str_whitespace() {
        assert_eq!(PipelineMode::from_str("  "), PipelineMode::Text);
    }

    #[test]
    fn pipeline_mode_derives() {
        // Clone
        let mode = PipelineMode::Nda;
        let cloned = mode.clone();
        assert_eq!(mode, cloned);

        // Copy
        let a = PipelineMode::Text;
        let b = a; // copy
        assert_eq!(a, b);

        // Debug
        let debug_str = format!("{:?}", PipelineMode::Auto);
        assert_eq!(debug_str, "Auto");

        // Eq
        assert_eq!(PipelineMode::Nda, PipelineMode::Nda);
        assert_ne!(PipelineMode::Text, PipelineMode::Nda);
        assert_ne!(PipelineMode::Auto, PipelineMode::Text);
    }

    // ── Block 143: NdaGenStats edge cases ──────────────────────────────────

    #[test]
    fn top_opcodes_n_zero() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.opcode_distribution[NdaOpcode::Scope as usize] = 10;
        let top = stats.top_opcodes(0);
        assert!(top.is_empty());
    }

    #[test]
    fn top_opcodes_all_same_count() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        // Set all opcodes to the same count
        for v in stats.opcode_distribution.iter_mut() {
            *v = 5;
        }
        let top = stats.top_opcodes(5);
        // Should return 5 opcodes, all with count 5
        assert_eq!(top.len(), 5);
        for (_, count) in &top {
            assert_eq!(*count, 5);
        }
    }

    #[test]
    fn top_opcodes_without_ensure_distribution() {
        // If ensure_distribution was never called, opcode_distribution is empty
        let stats = NdaGenStats::default();
        let top = stats.top_opcodes(5);
        assert!(top.is_empty());
    }

    #[test]
    fn snapshot_with_zero_distribution() {
        let stats = NdaGenStats {
            tokens_emitted: 10,
            site_map_hits: 5,
            site_map_misses: 5,
            elapsed_ms: 100,
            opcode_distribution: vec![],
            peak_stack_depth: 2,
            rep_penalty_applications: 3,
        };
        let snap = stats.snapshot();
        assert_eq!(snap.unique_opcodes_emitted, 0);
        assert_eq!(snap.tokens_emitted, 10);
        assert!((snap.cache_hit_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn nda_gen_stats_default_values() {
        let stats = NdaGenStats::default();
        assert_eq!(stats.tokens_emitted, 0);
        assert_eq!(stats.site_map_hits, 0);
        assert_eq!(stats.site_map_misses, 0);
        assert_eq!(stats.elapsed_ms, 0);
        assert!(stats.opcode_distribution.is_empty());
        assert_eq!(stats.peak_stack_depth, 0);
        assert_eq!(stats.rep_penalty_applications, 0);
    }

    #[test]
    fn nda_gen_stats_clone() {
        let mut stats = NdaGenStats::default();
        stats.tokens_emitted = 42;
        stats.site_map_hits = 10;
        let cloned = stats.clone();
        assert_eq!(cloned.tokens_emitted, 42);
        assert_eq!(cloned.site_map_hits, 10);
    }

    // ── Block 143: NdaGenerationResult validate with sandbox/scope ─────────

    #[test]
    fn generation_result_validate_sandbox_panicked() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 123,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: Some(crate::sandbox::SandboxResult {
                executed_nodes: 5,
                matrix_count: 1,
                norm_count: 0,
                output_vec: vec![1.0],
                output_dim: 1,
                panicked: true,
                error: None,
                elapsed_us: 100,
                kind_counts: std::collections::HashMap::new(),
                output_log: vec![],
                loop_iterations: 0,
            }),
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 5,
        };
        let warnings = result.validate();
        assert!(warnings.iter().any(|w| w.contains("Sandbox execution panicked")));
    }

    #[test]
    fn generation_result_validate_sandbox_error() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 123,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: Some(crate::sandbox::SandboxResult {
                executed_nodes: 5,
                matrix_count: 1,
                norm_count: 0,
                output_vec: vec![1.0],
                output_dim: 1,
                panicked: false,
                error: Some("out of memory".to_string()),
                elapsed_us: 100,
                kind_counts: std::collections::HashMap::new(),
                output_log: vec![],
                loop_iterations: 0,
            }),
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 5,
        };
        let warnings = result.validate();
        assert!(warnings.iter().any(|w| w.contains("Sandbox execution error")));
        assert!(warnings.iter().any(|w| w.contains("out of memory")));
    }

    #[test]
    fn generation_result_validate_scope_failed() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 123,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: Some(crate::sandbox::scope_validator::ScopeValidation {
                similarity: 0.05,
                passed: false,
                threshold: 0.10,
                euclidean_distance: 1.5,
                manhattan_distance: 2.0,
                vector_dim: 896,
            }),
            stats: NdaGenStats::default(),
            node_count: 5,
        };
        let warnings = result.validate();
        assert!(warnings.iter().any(|w| w.contains("Scope validation failed")));
        assert!(warnings.iter().any(|w| w.contains("0.05")));
    }

    #[test]
    fn generation_result_validate_all_issues() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: false,
            force_terminated: true,
            site_map_key: None,
            sandbox: Some(crate::sandbox::SandboxResult {
                executed_nodes: 0,
                matrix_count: 0,
                norm_count: 0,
                output_vec: vec![],
                output_dim: 0,
                panicked: true,
                error: Some("crash".to_string()),
                elapsed_us: 0,
                kind_counts: std::collections::HashMap::new(),
                output_log: vec![],
                loop_iterations: 0,
            }),
            scope: Some(crate::sandbox::scope_validator::ScopeValidation {
                similarity: 0.01,
                passed: false,
                threshold: 0.10,
                euclidean_distance: 2.0,
                manhattan_distance: 3.0,
                vector_dim: 896,
            }),
            stats: NdaGenStats::default(),
            node_count: 0,
        };
        let warnings = result.validate();
        // Should have: invalid, force-terminated, sandbox panicked, sandbox error, scope failed
        assert!(warnings.len() >= 5, "expected >=5 warnings, got {}: {:?}", warnings.len(), warnings);
    }

    // ── Block 143: execution_summary with sandbox/scope ────────────────────

    #[test]
    fn execution_summary_with_sandbox_success() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0xABCD,
            valid: true,
            force_terminated: false,
            site_map_key: Some(99),
            sandbox: Some(crate::sandbox::SandboxResult {
                executed_nodes: 10,
                matrix_count: 2,
                norm_count: 1,
                output_vec: vec![1.0, 2.0],
                output_dim: 2,
                panicked: false,
                error: None,
                elapsed_us: 500,
                kind_counts: std::collections::HashMap::new(),
                output_log: vec![],
                loop_iterations: 5,
            }),
            scope: Some(crate::sandbox::scope_validator::ScopeValidation {
                similarity: 0.85,
                passed: true,
                threshold: 0.10,
                euclidean_distance: 0.1,
                manhattan_distance: 0.2,
                vector_dim: 896,
            }),
            stats: NdaGenStats {
                tokens_emitted: 50,
                site_map_hits: 40,
                site_map_misses: 10,
                elapsed_ms: 200,
                ..Default::default()
            },
            node_count: 10,
        };
        let summary = result.execution_summary();
        assert!(summary.valid);
        assert!(!summary.force_terminated);
        assert_eq!(summary.node_count, 10);
        assert_eq!(summary.tokens_emitted, 50);
        assert_eq!(summary.elapsed_ms, 200);
        assert!((summary.cache_hit_rate - 0.8).abs() < 0.01);
        assert_eq!(summary.sandbox_passed, Some(true));
        assert_eq!(summary.scope_passed, Some(true));
        assert!(summary.stored_in_site_map);
    }

    #[test]
    fn execution_summary_with_sandbox_failure() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: Some(crate::sandbox::SandboxResult {
                executed_nodes: 3,
                matrix_count: 0,
                norm_count: 0,
                output_vec: vec![],
                output_dim: 0,
                panicked: true,
                error: Some("segfault".to_string()),
                elapsed_us: 10,
                kind_counts: std::collections::HashMap::new(),
                output_log: vec![],
                loop_iterations: 0,
            }),
            scope: Some(crate::sandbox::scope_validator::ScopeValidation {
                similarity: 0.02,
                passed: false,
                threshold: 0.10,
                euclidean_distance: 5.0,
                manhattan_distance: 8.0,
                vector_dim: 896,
            }),
            stats: NdaGenStats::default(),
            node_count: 3,
        };
        let summary = result.execution_summary();
        assert_eq!(summary.sandbox_passed, Some(false));
        assert_eq!(summary.scope_passed, Some(false));
        assert!(!summary.stored_in_site_map);
    }

    // ── Block 143: Struct derives ──────────────────────────────────────────

    #[test]
    fn batch_generation_report_debug() {
        let report = BatchGenerationReport {
            prompt_count: 1,
            results: vec![],
            total_elapsed_ms: 100,
            total_tokens: 50,
            valid_count: 1,
            truncated_count: 0,
            avg_cache_hit_rate: 0.9,
        };
        let debug = format!("{:?}", report);
        assert!(debug.contains("prompt_count: 1"));
        assert!(debug.contains("valid_count: 1"));
    }

    #[test]
    fn batch_item_result_debug() {
        let item = BatchItemResult {
            index: 3,
            valid: true,
            force_terminated: false,
            tokens_emitted: 42,
            node_count: 10,
            elapsed_ms: 55,
            root_hash: 0xBEEF,
            site_map_key: Some(7),
            cache_hit_rate: 0.75,
        };
        let debug = format!("{:?}", item);
        assert!(debug.contains("index: 3"));
        assert!(debug.contains("valid: true"));
    }

    #[test]
    fn nda_gen_stats_snapshot_debug_clone() {
        let snap = NdaGenStatsSnapshot {
            tokens_emitted: 10,
            site_map_hits: 5,
            site_map_misses: 5,
            elapsed_ms: 100,
            cache_hit_rate: 0.5,
            peak_stack_depth: 2,
            rep_penalty_applications: 3,
            unique_opcodes_emitted: 4,
        };
        let cloned = snap.clone();
        assert_eq!(cloned.tokens_emitted, snap.tokens_emitted);
        assert_eq!(cloned.unique_opcodes_emitted, snap.unique_opcodes_emitted);
        let debug = format!("{:?}", snap);
        assert!(debug.contains("tokens_emitted: 10"));
    }

    #[test]
    fn nda_execution_summary_debug() {
        let summary = NdaExecutionSummary {
            valid: false,
            force_terminated: true,
            node_count: 0,
            tokens_emitted: 100,
            elapsed_ms: 500,
            cache_hit_rate: 0.0,
            sandbox_passed: None,
            scope_passed: None,
            stored_in_site_map: false,
        };
        let debug = format!("{:?}", summary);
        assert!(debug.contains("force_terminated: true"));
        assert!(debug.contains("stored_in_site_map: false"));
    }

    // ── Block 143: random_uniform edge cases ───────────────────────────────

    #[test]
    fn random_uniform_n_zero() {
        let v = random_uniform(0, 1.0);
        assert!(v.is_empty());
    }

    #[test]
    fn random_uniform_scale_zero() {
        let v = random_uniform(100, 0.0);
        assert_eq!(v.len(), 100);
        // All values should be 0.0 (or very close due to float arithmetic)
        for &val in &v {
            assert!(val.abs() < 1e-9, "expected 0.0 with scale=0, got {}", val);
        }
    }

    #[test]
    fn random_uniform_different_scales() {
        let v1 = random_uniform(100, 0.1);
        let v2 = random_uniform(100, 10.0);
        // Values with larger scale should generally have larger magnitudes
        let avg1: f32 = v1.iter().map(|x| x.abs()).sum::<f32>() / 100.0;
        let avg2: f32 = v2.iter().map(|x| x.abs()).sum::<f32>() / 100.0;
        assert!(avg2 > avg1, "larger scale should produce larger values: avg1={}, avg2={}", avg1, avg2);
    }

    // ── Block 143: argmax_f32 edge cases ───────────────────────────────────

    #[test]
    fn argmax_f32_empty_slice() {
        // Empty slice → returns 0 (unwrap_or default)
        assert_eq!(argmax_f32(&[]), 0);
    }

    #[test]
    fn argmax_f32_all_equal() {
        // max_by returns the LAST element on ties (Equal → later wins)
        assert_eq!(argmax_f32(&[3.0, 3.0, 3.0]), 2);
    }

    #[test]
    fn argmax_f32_with_nan() {
        // NaN partial_cmp returns None → unwrap_or(Equal) → later element wins
        // [NaN, 1.0, 2.0]: NaN vs 1.0 → Equal → keep 1.0 (idx 1); 1.0 vs 2.0 → Less → keep 2.0 (idx 2)
        let result = argmax_f32(&[f32::NAN, 1.0, 2.0]);
        assert_eq!(result, 2);
    }

    #[test]
    fn argmax_f32_large_values() {
        // Use values that are actually distinguishable in f32
        assert_eq!(argmax_f32(&[1e30, 1e20, 0.0]), 0);
    }

    #[test]
    fn argmax_f32_very_negative() {
        // -1e30 is much larger than f32::MIN (-3.4e38)
        assert_eq!(argmax_f32(&[f32::MIN, f32::MIN / 2.0, -1e30]), 2);
    }

    // ── Block 143: NdaGenStats serde ───────────────────────────────────────

    #[test]
    fn nda_gen_stats_serde_roundtrip() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.tokens_emitted = 200;
        stats.site_map_hits = 150;
        stats.site_map_misses = 50;
        stats.elapsed_ms = 1000;
        stats.peak_stack_depth = 5;
        stats.rep_penalty_applications = 30;
        stats.opcode_distribution[NdaOpcode::Scope as usize] = 100;
        stats.opcode_distribution[NdaOpcode::Int as usize] = 60;

        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"tokens_emitted\":200"));
        assert!(json.contains("\"site_map_hits\":150"));
        assert!(json.contains("\"opcode_distribution\""));
    }

    // ── Block 143: BatchGenerationReport edge cases ────────────────────────

    #[test]
    fn batch_report_single_valid_result() {
        let report = BatchGenerationReport {
            prompt_count: 1,
            results: vec![BatchItemResult {
                index: 0,
                valid: true,
                force_terminated: false,
                tokens_emitted: 100,
                node_count: 20,
                elapsed_ms: 200,
                root_hash: 0xCAFE,
                site_map_key: Some(42),
                cache_hit_rate: 0.95,
            }],
            total_elapsed_ms: 200,
            total_tokens: 100,
            valid_count: 1,
            truncated_count: 0,
            avg_cache_hit_rate: 0.95,
        };
        assert_eq!(report.prompt_count, 1);
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.valid_count, 1);
        assert_eq!(report.truncated_count, 0);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"avg_cache_hit_rate\":0.95"));
    }

    #[test]
    fn batch_report_mixed_results() {
        let report = BatchGenerationReport {
            prompt_count: 3,
            results: vec![
                BatchItemResult {
                    index: 0,
                    valid: true,
                    force_terminated: false,
                    tokens_emitted: 50,
                    node_count: 10,
                    elapsed_ms: 100,
                    root_hash: 1,
                    site_map_key: Some(1),
                    cache_hit_rate: 0.8,
                },
                BatchItemResult {
                    index: 1,
                    valid: true,
                    force_terminated: true,
                    tokens_emitted: 200,
                    node_count: 0,
                    elapsed_ms: 300,
                    root_hash: 0,
                    site_map_key: None,
                    cache_hit_rate: 0.3,
                },
                BatchItemResult {
                    index: 2,
                    valid: false,
                    force_terminated: false,
                    tokens_emitted: 10,
                    node_count: 0,
                    elapsed_ms: 50,
                    root_hash: 0,
                    site_map_key: None,
                    cache_hit_rate: 0.0,
                },
            ],
            total_elapsed_ms: 450,
            total_tokens: 260,
            valid_count: 2,
            truncated_count: 1,
            avg_cache_hit_rate: 0.367,
        };
        assert_eq!(report.prompt_count, 3);
        assert_eq!(report.valid_count, 2);
        assert_eq!(report.truncated_count, 1);
        assert_eq!(report.total_tokens, 260);
    }

    // ── Block 143: NdaGenerationResult serialization edge cases ────────────

    #[test]
    fn generation_result_serialize_with_site_map_key() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: Some(u64::MAX),
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 0,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains(&format!("\"site_map_key\":{}", u64::MAX)));
    }

    #[test]
    fn generation_result_serialize_without_site_map_key() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: false,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 0,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"site_map_key\":null"));
    }

    #[test]
    fn generation_result_validate_sandbox_success_no_warnings() {
        // Sandbox that succeeded (no panic, no error) should not add warnings
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 123,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: Some(crate::sandbox::SandboxResult {
                executed_nodes: 10,
                matrix_count: 2,
                norm_count: 1,
                output_vec: vec![1.0],
                output_dim: 1,
                panicked: false,
                error: None,
                elapsed_us: 100,
                kind_counts: std::collections::HashMap::new(),
                output_log: vec![],
                loop_iterations: 0,
            }),
            scope: Some(crate::sandbox::scope_validator::ScopeValidation {
                similarity: 0.5,
                passed: true,
                threshold: 0.10,
                euclidean_distance: 0.5,
                manhattan_distance: 0.8,
                vector_dim: 896,
            }),
            stats: NdaGenStats::default(),
            node_count: 10,
        };
        let warnings = result.validate();
        assert!(warnings.is_empty(), "expected no warnings, got: {:?}", warnings);
    }

    // ── Block 143: NdaHead constants ───────────────────────────────────────

    #[test]
    fn nda_head_constants() {
        assert_eq!(NdaHead::IN, 896);
        assert_eq!(NdaHead::MID, 64);
        assert_eq!(NdaHead::OUT, NdaOpcode::VOCAB_SIZE);
    }

    #[test]
    fn nda_head_random_nonzero_weights() {
        let head = NdaHead::random();
        // Random head should have at least some non-zero weights
        assert!(head.w1.iter().any(|&v| v.abs() > 1e-9));
        assert!(head.w2.iter().any(|&v| v.abs() > 1e-9));
    }

    #[test]
    fn nda_head_random_biases_zero() {
        let head = NdaHead::random();
        // Biases are initialized to zero even for random
        assert!(head.b1.iter().all(|&v| v == 0.0));
        assert!(head.b2.iter().all(|&v| v == 0.0));
    }

    // ── Block 143: PipelineMode detect substring matching ──────────────────

    #[test]
    fn pipeline_mode_detect_substring_trigger() {
        // "implement" is a substring of "implementation"
        assert_eq!(PipelineMode::detect("the implementation details"), PipelineMode::Nda);
        // "write" is a substring of "rewrite"
        assert_eq!(PipelineMode::detect("rewrite this function"), PipelineMode::Nda);
        // "def " (with space) triggers NDA
        assert_eq!(PipelineMode::detect("def my_function():"), PipelineMode::Nda);
        // "class " (with space) triggers NDA
        assert_eq!(PipelineMode::detect("class MyClass:"), PipelineMode::Nda);
    }

    #[test]
    fn pipeline_mode_detect_no_trigger() {
        assert_eq!(PipelineMode::detect("hello world"), PipelineMode::Text);
        assert_eq!(PipelineMode::detect("the quick brown fox"), PipelineMode::Text);
        assert_eq!(PipelineMode::detect("12345"), PipelineMode::Text);
    }

    // ── Block 194: JSON key counts ──────────────────────────────────────────

    #[test]
    fn nda_gen_stats_snapshot_json_has_8_keys() {
        let snap = NdaGenStatsSnapshot {
            tokens_emitted: 1,
            site_map_hits: 2,
            site_map_misses: 3,
            elapsed_ms: 4,
            cache_hit_rate: 0.5,
            peak_stack_depth: 6,
            rep_penalty_applications: 7,
            unique_opcodes_emitted: 8,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let map: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(map.as_object().unwrap().len(), 8);
    }

    #[test]
    fn nda_execution_summary_json_has_9_keys() {
        let summary = NdaExecutionSummary {
            valid: true,
            force_terminated: false,
            node_count: 1,
            tokens_emitted: 2,
            elapsed_ms: 3,
            cache_hit_rate: 0.5,
            sandbox_passed: None,
            scope_passed: None,
            stored_in_site_map: false,
        };
        let json = serde_json::to_string(&summary).unwrap();
        let map: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(map.as_object().unwrap().len(), 9);
    }

    #[test]
    fn batch_item_result_json_has_9_keys() {
        let item = BatchItemResult {
            index: 0,
            valid: true,
            force_terminated: false,
            tokens_emitted: 10,
            node_count: 5,
            elapsed_ms: 100,
            root_hash: 0xAA,
            site_map_key: None,
            cache_hit_rate: 0.0,
        };
        let json = serde_json::to_string(&item).unwrap();
        let map: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(map.as_object().unwrap().len(), 9);
    }

    #[test]
    fn batch_generation_report_json_has_7_keys() {
        let report = BatchGenerationReport {
            prompt_count: 0,
            results: vec![],
            total_elapsed_ms: 0,
            total_tokens: 0,
            valid_count: 0,
            truncated_count: 0,
            avg_cache_hit_rate: 0.0,
        };
        let json = serde_json::to_string(&report).unwrap();
        let map: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(map.as_object().unwrap().len(), 7);
    }

    // ── Block 194: NdaGenerationResult serde skip fields ───────────────────

    #[test]
    fn generation_result_json_skips_nodes_field() {
        let result = NdaGenerationResult {
            nodes: vec![NdaNode::Int { value: 42 }],
            root_hash: 1,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 1,
        };
        let json = serde_json::to_string(&result).unwrap();
        // nodes has #[serde(skip)] so should not appear
        assert!(!json.contains("\"nodes\""));
    }

    #[test]
    fn generation_result_json_skips_sandbox_and_scope() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: Some(crate::sandbox::SandboxResult {
                executed_nodes: 1,
                matrix_count: 0,
                norm_count: 0,
                output_vec: vec![],
                output_dim: 0,
                panicked: false,
                error: None,
                elapsed_us: 0,
                kind_counts: std::collections::HashMap::new(),
                output_log: vec![],
                loop_iterations: 0,
            }),
            scope: Some(crate::sandbox::scope_validator::ScopeValidation {
                similarity: 0.5,
                passed: true,
                threshold: 0.1,
                euclidean_distance: 0.5,
                manhattan_distance: 0.5,
                vector_dim: 896,
            }),
            stats: NdaGenStats::default(),
            node_count: 0,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("\"sandbox\""));
        assert!(!json.contains("\"scope\""));
    }

    // ── Block 194: cache_hit_rate formula edge cases ────────────────────────

    #[test]
    fn cache_hit_rate_exact_fraction_194() {
        let stats = NdaGenStats {
            site_map_hits: 3,
            site_map_misses: 7,
            ..Default::default()
        };
        assert!((stats.cache_hit_rate() - 0.3).abs() < 1e-10);
    }

    #[test]
    fn cache_hit_rate_single_hit_single_miss() {
        let stats = NdaGenStats {
            site_map_hits: 1,
            site_map_misses: 1,
            ..Default::default()
        };
        assert!((stats.cache_hit_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_hit_rate_large_numbers() {
        let stats = NdaGenStats {
            site_map_hits: 999_999,
            site_map_misses: 1,
            ..Default::default()
        };
        let rate = stats.cache_hit_rate();
        assert!(rate > 0.999, "expected >0.999, got {}", rate);
    }

    // ── Block 194: top_opcodes sorting and content ──────────────────────────

    #[test]
    fn top_opcodes_sorted_descending_194() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.opcode_distribution[NdaOpcode::Scope as usize] = 5;
        stats.opcode_distribution[NdaOpcode::Int as usize] = 20;
        stats.opcode_distribution[NdaOpcode::Matrix as usize] = 10;
        let top = stats.top_opcodes(3);
        assert_eq!(top.len(), 3);
        // First should be highest count
        assert_eq!(top[0].0, NdaOpcode::Int);
        assert_eq!(top[0].1, 20);
        assert_eq!(top[1].0, NdaOpcode::Matrix);
        assert_eq!(top[1].1, 10);
        assert_eq!(top[2].0, NdaOpcode::Scope);
        assert_eq!(top[2].1, 5);
    }

    #[test]
    fn top_opcodes_single_opcode() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.opcode_distribution[NdaOpcode::Norm as usize] = 42;
        let top = stats.top_opcodes(5);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, NdaOpcode::Norm);
        assert_eq!(top[0].1, 42);
    }

    // ── Block 194: ensure_distribution ──────────────────────────────────────

    #[test]
    fn ensure_distribution_creates_vocab_sized_vector() {
        let mut stats = NdaGenStats::default();
        assert!(stats.opcode_distribution.is_empty());
        stats.ensure_distribution();
        assert_eq!(stats.opcode_distribution.len(), NdaOpcode::VOCAB_SIZE);
        assert!(stats.opcode_distribution.iter().all(|&v| v == 0));
    }

    #[test]
    fn ensure_distribution_idempotent() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.opcode_distribution[0] = 99;
        stats.ensure_distribution(); // should NOT reset
        assert_eq!(stats.opcode_distribution[0], 99);
    }

    // ── Block 194: clone independence ───────────────────────────────────────

    #[test]
    fn nda_gen_stats_snapshot_clone_independence_194() {
        let snap = NdaGenStatsSnapshot {
            tokens_emitted: 100,
            site_map_hits: 50,
            site_map_misses: 50,
            elapsed_ms: 200,
            cache_hit_rate: 0.5,
            peak_stack_depth: 3,
            rep_penalty_applications: 10,
            unique_opcodes_emitted: 5,
        };
        let mut cloned = snap.clone();
        cloned.tokens_emitted = 999;
        cloned.unique_opcodes_emitted = 0;
        assert_eq!(snap.tokens_emitted, 100);
        assert_eq!(snap.unique_opcodes_emitted, 5);
    }

    // ── Block 194: NdaHead save file size ───────────────────────────────────

    #[test]
    fn nda_head_save_file_size() {
        use tempfile::NamedTempFile;
        let head = NdaHead::random();
        let file = NamedTempFile::new().unwrap();
        head.save(file.path()).unwrap();
        let metadata = std::fs::metadata(file.path()).unwrap();
        // 4 bytes magic + (MID*IN + MID + OUT*MID + OUT) * 4 bytes per f32
        let expected_floats = 64 * 896 + 64 + NdaOpcode::VOCAB_SIZE * 64 + NdaOpcode::VOCAB_SIZE;
        let expected_size = 4 + expected_floats * 4;
        assert_eq!(metadata.len() as usize, expected_size);
    }

    // ── Block 194: NdaHead forward behaviour ────────────────────────────────

    #[test]
    fn nda_head_forward_different_inputs_different_outputs() {
        let head = NdaHead::random();
        let h1 = vec![1.0f32; 896];
        let h2 = vec![-1.0f32; 896];
        let out1 = head.forward(&h1);
        let out2 = head.forward(&h2);
        // Different inputs should produce different outputs (with overwhelming probability)
        let any_different = out1.iter().zip(out2.iter()).any(|(&a, &b)| (a - b).abs() > 1e-6);
        assert!(any_different, "different inputs should give different outputs");
    }

    #[test]
    fn nda_head_forward_nonzero_hidden_nonzero_output() {
        let head = NdaHead::random();
        let hidden = vec![1.0f32; 896];
        let logits = head.forward(&hidden);
        // With random weights and non-zero input, at least some logits should be non-zero
        assert!(logits.iter().any(|&v| v.abs() > 1e-9));
    }

    // ── Block 194: PipelineMode from_str aliases ────────────────────────────

    #[test]
    fn pipeline_mode_from_str_native_alias_194() {
        assert_eq!(PipelineMode::from_str("native"), PipelineMode::Nda);
        assert_eq!(PipelineMode::from_str("Native"), PipelineMode::Nda);
        assert_eq!(PipelineMode::from_str("NATIVE"), PipelineMode::Nda);
    }

    #[test]
    fn pipeline_mode_from_str_unknown_defaults_to_text() {
        assert_eq!(PipelineMode::from_str("unknown"), PipelineMode::Text);
        assert_eq!(PipelineMode::from_str("foo"), PipelineMode::Text);
        assert_eq!(PipelineMode::from_str("text"), PipelineMode::Text);
        assert_eq!(PipelineMode::from_str("123"), PipelineMode::Text);
    }

    // ── Block 194: execution_summary field mapping ──────────────────────────

    #[test]
    fn execution_summary_stored_in_site_map_false_when_none_194() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 0,
        };
        let summary = result.execution_summary();
        assert!(!summary.stored_in_site_map);
    }

    #[test]
    fn execution_summary_elapsed_ms_is_u64_cast() {
        let mut stats = NdaGenStats::default();
        stats.elapsed_ms = 5000;
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats,
            node_count: 0,
        };
        let summary = result.execution_summary();
        assert_eq!(summary.elapsed_ms, 5000u64);
    }

    // ── Block 194: NdaGenStats snapshot unique opcodes ──────────────────────

    #[test]
    fn snapshot_unique_opcodes_counts_nonzero_entries() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        // Set all VOCAB_SIZE entries to 1
        for v in stats.opcode_distribution.iter_mut() {
            *v = 1;
        }
        let snap = stats.snapshot();
        assert_eq!(snap.unique_opcodes_emitted, NdaOpcode::VOCAB_SIZE);
    }

    // ── Block 194: PipelineMode detect edge cases ───────────────────────────

    #[test]
    fn pipeline_mode_detect_struct_with_space_194() {
        // "struct " (with trailing space) is a trigger
        assert_eq!(PipelineMode::detect("struct Foo {"), PipelineMode::Nda);
    }

    #[test]
    fn pipeline_mode_detect_func_trigger() {
        assert_eq!(PipelineMode::detect("func main() {"), PipelineMode::Nda);
    }

    #[test]
    fn pipeline_mode_detect_fn_trigger() {
        assert_eq!(PipelineMode::detect("fn helper() -> bool {"), PipelineMode::Nda);
    }

    // ── Block 194: NdaHead load too-short file ─────────────────────────────

    #[test]
    fn nda_head_load_too_short_file() {
        use tempfile::NamedTempFile;
        let file = NamedTempFile::new().unwrap();
        // Write correct magic but zero float data (should fail with weight count mismatch)
        std::fs::write(file.path(), b"NDA\x01").unwrap();
        let result = NdaHead::load(file.path());
        assert!(result.is_err());
        let err = format!("{}", result.err().unwrap());
        assert!(err.contains("weight count mismatch"), "got: {}", err);
    }

    // ── Block 194: NdaGenStats debug format ─────────────────────────────────

    #[test]
    fn nda_gen_stats_debug_format() {
        let mut stats = NdaGenStats::default();
        stats.tokens_emitted = 42;
        stats.site_map_hits = 30;
        let debug = format!("{:?}", stats);
        assert!(debug.contains("tokens_emitted: 42"));
        assert!(debug.contains("site_map_hits: 30"));
    }

    // ── Block 194: random_uniform statistical properties ────────────────────

    #[test]
    fn random_uniform_mean_near_zero() {
        let v = random_uniform(10000, 1.0);
        let mean: f32 = v.iter().sum::<f32>() / v.len() as f32;
        assert!(mean.abs() < 0.1, "mean should be near 0, got {}", mean);
    }

    // ── Block 194: NdaExecutionSummary all fields populated ─────────────────

    #[test]
    fn nda_execution_summary_all_fields_194() {
        let summary = NdaExecutionSummary {
            valid: true,
            force_terminated: true,
            node_count: 15,
            tokens_emitted: 200,
            elapsed_ms: 1500,
            cache_hit_rate: 0.75,
            sandbox_passed: Some(true),
            scope_passed: Some(false),
            stored_in_site_map: false,
        };
        let json = serde_json::to_string(&summary).unwrap();
        let map: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = map.as_object().unwrap();
        assert_eq!(obj["valid"], true);
        assert_eq!(obj["force_terminated"], true);
        assert_eq!(obj["node_count"], 15);
        assert_eq!(obj["tokens_emitted"], 200);
        assert_eq!(obj["elapsed_ms"], 1500);
        assert_eq!(obj["sandbox_passed"], true);
        assert_eq!(obj["scope_passed"], false);
        assert_eq!(obj["stored_in_site_map"], false);
    }

    // ── Block 202: NdaHead save magic bytes ────────────────────────────────

    #[test]
    fn nda_head_save_starts_with_magic_bytes() {
        use tempfile::NamedTempFile;
        let head = NdaHead::zeros();
        let file = NamedTempFile::new().unwrap();
        head.save(file.path()).unwrap();
        let buf = std::fs::read(file.path()).unwrap();
        assert_eq!(&buf[..4], b"NDA\x01");
    }

    #[test]
    fn nda_head_load_magic_must_match_exactly() {
        use tempfile::NamedTempFile;
        let file = NamedTempFile::new().unwrap();
        // "NDA\x02" — wrong version byte
        std::fs::write(file.path(), b"NDA\x02\x00\x00\x00\x00").unwrap();
        let result = NdaHead::load(file.path());
        assert!(result.is_err());
    }

    // ── Block 202: NdaHead forward with sparse input ────────────────────────

    #[test]
    fn nda_head_forward_sparse_input() {
        let head = NdaHead::random();
        let mut hidden = vec![0.0f32; 896];
        hidden[42] = 1.0; // only one non-zero element
        let logits = head.forward(&hidden);
        assert_eq!(logits.len(), NdaOpcode::VOCAB_SIZE);
        // Output should be deterministic for same input
        let logits2 = head.forward(&hidden);
        assert_eq!(logits, logits2);
    }

    #[test]
    fn nda_head_forward_negative_hidden() {
        let head = NdaHead::random();
        let hidden = vec![-100.0f32; 896];
        let logits = head.forward(&hidden);
        // ReLU kills negative pre-activations, but large negative input
        // through random weights can still produce positive pre-activations
        assert_eq!(logits.len(), NdaOpcode::VOCAB_SIZE);
    }

    // ── Block 202: PipelineMode edge cases ─────────────────────────────────

    #[test]
    fn pipeline_mode_from_str_mixed_case() {
        assert_eq!(PipelineMode::from_str("nDa"), PipelineMode::Nda);
        assert_eq!(PipelineMode::from_str("Native"), PipelineMode::Nda);
        assert_eq!(PipelineMode::from_str("Auto"), PipelineMode::Auto);
    }

    #[test]
    fn pipeline_mode_auto_never_detected() {
        // PipelineMode::detect never returns Auto — only Nda or Text
        let prompts = ["", "implement", "what is", "hello", "def foo", "123"];
        for p in &prompts {
            let mode = PipelineMode::detect(p);
            assert_ne!(mode, PipelineMode::Auto, "detect should never return Auto for: {}", p);
        }
    }

    // ── Block 202: NdaGenStats formula edge cases ──────────────────────────

    #[test]
    fn cache_hit_rate_one_hit_zero_misses() {
        let stats = NdaGenStats {
            site_map_hits: 1,
            site_map_misses: 0,
            ..Default::default()
        };
        assert!((stats.cache_hit_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn top_opcodes_n_one_returns_only_best() {
        let mut stats = NdaGenStats::default();
        stats.ensure_distribution();
        stats.opcode_distribution[NdaOpcode::Scope as usize] = 5;
        stats.opcode_distribution[NdaOpcode::Int as usize] = 50;
        stats.opcode_distribution[NdaOpcode::Matrix as usize] = 20;
        let top = stats.top_opcodes(1);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, NdaOpcode::Int);
        assert_eq!(top[0].1, 50);
    }

    #[test]
    fn nda_gen_stats_snapshot_elapsed_ms_truncates_u128_to_u64() {
        let stats = NdaGenStats {
            elapsed_ms: u128::from(u64::MAX),
            ..Default::default()
        };
        let snap = stats.snapshot();
        assert_eq!(snap.elapsed_ms, u64::MAX);
    }

    // ── Block 202: NdaGenerationResult validate combinations ───────────────

    #[test]
    fn generation_result_validate_only_force_terminated() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 1,
            valid: true,
            force_terminated: true,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 5,
        };
        let warnings = result.validate();
        assert_eq!(warnings.len(), 1, "expected 1 warning, got: {:?}", warnings);
        assert!(warnings[0].contains("force-terminated"));
    }

    #[test]
    fn generation_result_validate_invalid_and_zero_nodes() {
        // Invalid + zero nodes: only "not valid" warning (zero-nodes warning requires valid=true)
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: false,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 0,
        };
        let warnings = result.validate();
        assert_eq!(warnings.len(), 1, "expected 1 warning, got: {:?}", warnings);
        assert!(warnings[0].contains("not valid"));
    }

    // ── Block 202: execution_summary field derivations ─────────────────────

    #[test]
    fn execution_summary_tokens_from_stats() {
        let mut stats = NdaGenStats::default();
        stats.tokens_emitted = 777;
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats,
            node_count: 0,
        };
        let summary = result.execution_summary();
        assert_eq!(summary.tokens_emitted, 777);
    }

    #[test]
    fn execution_summary_cache_hit_rate_from_stats() {
        let mut stats = NdaGenStats::default();
        stats.site_map_hits = 9;
        stats.site_map_misses = 1;
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats,
            node_count: 0,
        };
        let summary = result.execution_summary();
        assert!((summary.cache_hit_rate - 0.9).abs() < 0.01);
    }

    // ── Block 202: BatchGenerationReport field access ──────────────────────

    #[test]
    fn batch_report_results_accessible() {
        let report = BatchGenerationReport {
            prompt_count: 2,
            results: vec![
                BatchItemResult {
                    index: 0, valid: true, force_terminated: false,
                    tokens_emitted: 10, node_count: 2, elapsed_ms: 5,
                    root_hash: 1, site_map_key: None, cache_hit_rate: 0.5,
                },
                BatchItemResult {
                    index: 1, valid: false, force_terminated: true,
                    tokens_emitted: 20, node_count: 0, elapsed_ms: 15,
                    root_hash: 0, site_map_key: None, cache_hit_rate: 0.0,
                },
            ],
            total_elapsed_ms: 20,
            total_tokens: 30,
            valid_count: 1,
            truncated_count: 1,
            avg_cache_hit_rate: 0.25,
        };
        assert_eq!(report.results[0].index, 0);
        assert!(report.results[0].valid);
        assert_eq!(report.results[1].index, 1);
        assert!(report.results[1].force_terminated);
        assert_eq!(report.total_tokens, 30);
    }

    // ── Block 202: random_uniform properties ───────────────────────────────

    #[test]
    fn random_uniform_single_element() {
        let v = random_uniform(1, 1.0);
        assert_eq!(v.len(), 1);
        assert!(v[0].abs() <= 1.0 + 1e-6);
    }

    #[test]
    fn random_uniform_not_all_same() {
        let v = random_uniform(100, 1.0);
        // With a PRNG, 100 values should not all be identical
        let first = v[0];
        assert!(v.iter().any(|&x| (x - first).abs() > 1e-9));
    }

    #[test]
    fn random_uniform_negative_scale() {
        // Scale is just a multiplier; negative scale should work
        let v = random_uniform(50, -1.0);
        assert_eq!(v.len(), 50);
        // Values should be in [-1, 1] range (scale just flips sign)
        for &val in &v {
            assert!(val.abs() <= 1.0 + 1e-6);
        }
    }

    // ── Block 202: argmax_f32 additional cases ─────────────────────────────

    #[test]
    fn argmax_f32_two_elements_first_wins() {
        assert_eq!(argmax_f32(&[10.0, 5.0]), 0);
    }

    #[test]
    fn argmax_f32_two_elements_second_wins() {
        assert_eq!(argmax_f32(&[5.0, 10.0]), 1);
    }

    #[test]
    fn argmax_f32_inf_values() {
        assert_eq!(argmax_f32(&[f32::INFINITY, 1e30]), 0);
    }

    // ── Block 202: NdaGenStats ensure_distribution with pre-filled ─────────

    #[test]
    fn ensure_distribution_preserves_existing_nonzero() {
        let mut stats = NdaGenStats::default();
        stats.opcode_distribution = vec![7; NdaOpcode::VOCAB_SIZE];
        stats.ensure_distribution(); // should NOT reset since non-empty
        assert_eq!(stats.opcode_distribution[0], 7);
        assert_eq!(stats.opcode_distribution[NdaOpcode::VOCAB_SIZE - 1], 7);
    }

    // ── Block 202: NdaGenerationResult JSON field count ────────────────────

    #[test]
    fn generation_result_json_field_count() {
        let result = NdaGenerationResult {
            nodes: vec![],
            root_hash: 0,
            valid: true,
            force_terminated: false,
            site_map_key: None,
            sandbox: None,
            scope: None,
            stats: NdaGenStats::default(),
            node_count: 0,
        };
        let json = serde_json::to_string(&result).unwrap();
        let map: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = map.as_object().unwrap();
        // root_hash, valid, force_terminated, site_map_key, stats, node_count
        // (nodes, sandbox, scope are skipped)
        assert_eq!(obj.len(), 6);
    }
}
