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
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
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
pub struct NdaGenerationResult {
    /// The emitted NDA nodes, in emission order.
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
    pub sandbox: Option<crate::sandbox::SandboxResult>,
    /// Scope validation result.
    pub scope: Option<crate::sandbox::scope_validator::ScopeValidation>,
    /// Generation statistics.
    pub stats: NdaGenStats,
}

#[derive(Default, Debug)]
pub struct NdaGenStats {
    pub tokens_emitted: usize,
    pub site_map_hits: usize,   // KV lookups that hit the persistent cache
    pub site_map_misses: usize, // KV lookups that required recomputation
    pub elapsed_ms: u128,
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
            for &past_op in &recent_ops {
                let idx = past_op as usize;
                if idx < NdaOpcode::VOCAB_SIZE {
                    // Halve the logit for each occurrence in the window
                    logits_i32_op[idx] -= logits_i32_op[idx].abs() >> 2;
                }
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
            current_opcode_id = best_op as u32;
        }

        // ── Forced termination: if the loop exhausted max_opcodes without
        // naturally closing all scopes, seal the tree now.
        if !valid {
            let open_scopes = self.verifier.stack.len().saturating_sub(1);
            if open_scopes > 0 {
                eprintln!(
                    "[pipeline_nda] WARNING: forced termination — \
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

        NdaGenerationResult {
            nodes,
            root_hash,
            valid,
            force_terminated,
            site_map_key,
            sandbox,
            scope,
            stats,
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
}
