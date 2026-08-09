use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::nda_int::NdaVec;
use crate::safety::SafeMutex;
use crate::sandbox::SandboxResult;
use crate::site_map::SiteMap;

// ─── Variable Slot Registry ───────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct VarRegistry {
    map: Arc<Mutex<HashMap<u64, usize>>>,
}

impl Default for VarRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl VarRegistry {
    pub fn new() -> Self {
        VarRegistry {
            map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get_or_create_slot(&self, name_hash: u64) -> usize {
        let mut guard = self.map.lock_safe();
        let next_slot = guard.len();
        *guard.entry(name_hash).or_insert(next_slot)
    }

    pub fn total_slots(&self) -> usize {
        self.map.lock_safe().len()
    }
}

// ─── Public result type ────────────────────────────────────────────────────────

/// Result returned after running a `JitProgram`.
#[derive(Clone, Debug)]
pub struct JitResult {
    pub output_vec: Vec<f32>,
    pub output_dim: usize,
    pub elapsed_us: u64,
    pub nodes_compiled: usize,
    pub error: Option<String>,
}

// ─── Internal JIT value type ───────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum JitVal {
    Vector(Arc<NdaVec>),
    Scalar(i32, i8), // (value, log2_scale)
    Float(f32),
}

impl JitVal {
    pub fn is_truthy(&self) -> bool {
        match self {
            JitVal::Scalar(val, _) => *val > 0,
            JitVal::Float(val) => *val > 0.0,
            JitVal::Vector(v) => JitState::is_truthy(v),
        }
    }

    pub fn to_f32_vec(&self) -> Vec<f32> {
        match self {
            JitVal::Vector(v) => v.to_f32_vec(),
            JitVal::Float(val) => vec![*val],
            JitVal::Scalar(val, scale) => {
                let actual = (*val as f32) * 2.0f32.powi(*scale as i32);
                vec![actual]
            }
        }
    }
}

// ─── Internal runtime state ────────────────────────────────────────────────────

/// Mutable state threaded through JIT-compiled closures at runtime.
pub struct JitState<'a> {
    /// The stack of data flowing through the network.
    pub stack: Vec<JitVal>,
    /// Variable bindings: slot_index → Option<JitVal> (pre-allocated and dynamic growing).
    pub variables: Vec<Option<JitVal>>,
    /// Pointer to the site map for `Call` resolution.
    pub site_map: &'a SiteMap,
    /// Counters for diagnostics.
    pub matrix_count: usize,
    pub norm_count: usize,
    pub loop_count: usize,
    pub executed_nodes: usize,
    /// Print output buffer.
    pub print_buf: Vec<String>,

    // --- Simulated hardware sandbox ---
    /// Virtual heap memory space (64KB default, grows as needed)
    pub heap: Vec<u8>,
    /// Virtual heap allocations: address -> size
    pub heap_allocations: std::collections::HashMap<u32, usize>,
    /// Simulated memory-mapped registers (MMIO): address -> value
    pub mmio: std::collections::HashMap<u32, JitVal>,
    /// Simulated hardware interrupts: vector -> handler_hash
    pub interrupts: std::collections::HashMap<u32, u64>,
}

impl<'a> JitState<'a> {
    pub fn new(input: &[f32], site_map: &'a SiteMap, total_slots: usize) -> Self {
        JitState {
            stack: vec![JitVal::Vector(Arc::new(NdaVec::from_f32_slice(input)))],
            variables: vec![None; total_slots],
            site_map,
            matrix_count: 0,
            norm_count: 0,
            loop_count: 0,
            executed_nodes: 0,
            print_buf: Vec::new(),
            heap: vec![0u8; 65536],
            heap_allocations: std::collections::HashMap::new(),
            mmio: std::collections::HashMap::new(),
            interrupts: std::collections::HashMap::new(),
        }
    }

    /// Check if the current vector is "truthy": sum of raw values > 0, optimized to avoid division/modulo.
    pub fn is_truthy(v: &NdaVec) -> bool {
        let mut sum: i64 = 0;
        let bytes = v.sign.len();
        const DECODE_TABLE: [i32; 4] = [-2, -1, 1, 2];
        for byte_idx in 0..bytes {
            let mut s_shift = v.sign[byte_idx];
            let mut e_shift = v.extra[byte_idx];
            let base_idx = byte_idx * 8;
            for bit in 0..8 {
                let i = base_idx + bit;
                if i >= v.len {
                    break;
                }
                let idx = ((s_shift & 1) << 1) | (e_shift & 1);
                sum += DECODE_TABLE[idx as usize] as i64;
                s_shift >>= 1;
                e_shift >>= 1;
            }
        }
        sum > 0
    }
}

/// Control flow signals propagated through the closure tree at runtime.
#[derive(Debug, PartialEq)]
pub enum JitControlFlow {
    Continue,
    Break,
    Return,
}

// ─── JIT function type ─────────────────────────────────────────────────────────

/// A compiled JIT function: takes mutable JitState and returns control flow.
pub type JitFn =
    Arc<dyn for<'a> Fn(&mut JitState<'a>) -> Result<JitControlFlow, String> + Send + Sync>;

// ─── Compiled program ──────────────────────────────────────────────────────────

/// A fully compiled NDA program ready for native execution.
pub struct JitProgram {
    /// The sequence of compiled closures that form the program body.
    pub fns: Vec<JitFn>,
    /// Total NDA nodes compiled (for diagnostics).
    pub nodes_compiled: usize,
    /// Whether a tier-2 machine code GEMV kernel is active.
    pub has_asm_kernel: bool,
    /// Registry mapping variable hashes to slots.
    pub registry: VarRegistry,
}

impl JitProgram {
    /// Execute the compiled program with the given input vector.
    ///
    /// # Panics
    /// Never panics — all errors are caught and returned as `JitResult::error`.
    pub fn run(&self, input: &[f32], site_map: &SiteMap) -> JitResult {
        let t = Instant::now();
        let total_slots = self.registry.total_slots();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut state = JitState::new(input, site_map, total_slots);
            let _cf = run_sequence(&self.fns, &mut state)?;
            Ok::<JitState<'_>, String>(state)
        }));

        let elapsed_us = t.elapsed().as_micros() as u64;

        match result {
            Ok(Ok(state)) => {
                let out_f32 = if let Some(v) = state.stack.last() {
                    v.to_f32_vec()
                } else {
                    vec![]
                };
                let dim = out_f32.len();
                // Flush print buffer to stdout
                for line in &state.print_buf {
                    println!("{}", line);
                }
                JitResult {
                    output_vec: out_f32,
                    output_dim: dim,
                    elapsed_us,
                    nodes_compiled: self.nodes_compiled,
                    error: None,
                }
            }
            Ok(Err(e)) => JitResult {
                output_vec: vec![],
                output_dim: 0,
                elapsed_us,
                nodes_compiled: self.nodes_compiled,
                error: Some(e),
            },
            Err(panic_val) => {
                let msg = if let Some(s) = panic_val.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_val.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic during JIT execution".to_string()
                };
                JitResult {
                    output_vec: vec![],
                    output_dim: 0,
                    elapsed_us,
                    nodes_compiled: self.nodes_compiled,
                    error: Some(format!("panic: {}", msg)),
                }
            }
        }
    }

    /// Execute the JIT program in a sandboxed way, capturing output logs
    /// and returning a `SandboxResult` compatibly with the interpreter sandbox.
    pub fn run_sandboxed(&self, input: &[f32], site_map: &SiteMap) -> SandboxResult {
        let t = Instant::now();
        let total_slots = self.registry.total_slots();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut state = JitState::new(input, site_map, total_slots);
            let _cf = run_sequence(&self.fns, &mut state)?;
            Ok::<JitState<'_>, String>(state)
        }));

        let elapsed_us = t.elapsed().as_micros() as u64;

        match result {
            Ok(Ok(state)) => {
                let out_f32 = if let Some(v) = state.stack.last() {
                    v.to_f32_vec()
                } else {
                    vec![]
                };
                let dim = out_f32.len();
                SandboxResult {
                    executed_nodes: state.executed_nodes,
                    matrix_count: state.matrix_count,
                    norm_count: state.norm_count,
                    output_vec: out_f32,
                    output_dim: dim,
                    panicked: false,
                    error: None,
                    elapsed_us,
                }
            }
            Ok(Err(e)) => SandboxResult {
                executed_nodes: 0,
                matrix_count: 0,
                norm_count: 0,
                output_vec: vec![],
                output_dim: 0,
                panicked: false,
                error: Some(e),
                elapsed_us,
            },
            Err(panic_val) => {
                let msg = if let Some(s) = panic_val.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_val.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic during JIT execution".to_string()
                };
                SandboxResult {
                    executed_nodes: 0,
                    matrix_count: 0,
                    norm_count: 0,
                    output_vec: vec![],
                    output_dim: 0,
                    panicked: true,
                    error: Some(format!("panic: {}", msg)),
                    elapsed_us,
                }
            }
        }
    }
}

// ─── Helper: run a sequence of JIT functions ──────────────────────────────────

#[inline(always)]
pub fn run_sequence(fns: &[JitFn], state: &mut JitState<'_>) -> Result<JitControlFlow, String> {
    for f in fns {
        match f(state)? {
            JitControlFlow::Continue => {}
            cf => return Ok(cf),
        }
    }
    Ok(JitControlFlow::Continue)
}
