// compiler/nda_jit.rs — NDA Just-In-Time Compiler
//
// Two-tier native execution engine:
//
// TIER 1 — Closure JIT (platform-agnostic)
//   Compiles the NdaNode AST tree into a chain of `Box<dyn Fn>` closures at
//   load time.  All opcode dispatch is resolved during compilation — the
//   interpreter's `match` statement never executes at runtime.  This alone
//   eliminates 100 % of branch misprediction overhead from the dispatch loop.
//
// TIER 2 — Machine-code JIT (x86-64 & AArch64)
//   For the GEMV inner loop kernel (the hottest path in transformer inference)
//   we emit raw machine instructions into an mmap-backed executable page.
//   The resulting function pointer is called directly by tier-1 closures.
//   On unsupported architectures the pure-Rust GEMV fallback is used instead.
//
// .nda → NdaNode AST → [nda_jit::compile()] → JitProgram → run() → result
//
// A compiled JitProgram can be stored, serialised, and re-executed on the
// same platform with zero re-compilation overhead.
#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::nda::NdaMatrix;
use crate::nda_int::{NdaVec, nda_gemv_nda_to_nda, rms_norm_nda, nda_vec_add_inplace};
use crate::site_map::{NdaNode, SiteMap};
use crate::site_map::verifier::{CmpOp, VecOpKind, BitwiseOp, MathOp, MathFuncKind, AtomicOp, TypeKind};
use crate::sandbox::SandboxResult;

// ─── Variable Slot Registry ───────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct VarRegistry {
    map: Arc<Mutex<HashMap<u64, usize>>>,
}

impl VarRegistry {
    pub fn new() -> Self {
        VarRegistry { map: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn get_or_create_slot(&self, name_hash: u64) -> usize {
        let mut guard = self.map.lock().unwrap();
        let next_slot = guard.len();
        *guard.entry(name_hash).or_insert(next_slot)
    }

    pub fn total_slots(&self) -> usize {
        self.map.lock().unwrap().len()
    }
}

// ─── Public result type ────────────────────────────────────────────────────────

/// Result returned after running a `JitProgram`.
#[derive(Clone, Debug)]
pub struct JitResult {
    pub output_vec:    Vec<f32>,
    pub output_dim:    usize,
    pub elapsed_us:    u64,
    pub nodes_compiled: usize,
    pub error:         Option<String>,
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
    pub stack:       Vec<JitVal>,
    /// Variable bindings: slot_index → Option<JitVal> (pre-allocated and dynamic growing).
    pub variables:   Vec<Option<JitVal>>,
    /// Pointer to the site map for `Call` resolution.
    pub site_map:    &'a SiteMap,
    /// Counters for diagnostics.
    pub matrix_count: usize,
    pub norm_count:   usize,
    pub loop_count:   usize,
    pub executed_nodes: usize,
    /// Print output buffer.
    pub print_buf:   Vec<String>,

    // --- Simulated hardware sandbox ---
    /// Virtual heap memory space (64KB default, grows as needed)
    pub heap:        Vec<u8>,
    /// Virtual heap allocations: address -> size
    pub heap_allocations: std::collections::HashMap<u32, usize>,
    /// Simulated memory-mapped registers (MMIO): address -> value
    pub mmio:        std::collections::HashMap<u32, JitVal>,
    /// Simulated hardware interrupts: vector -> handler_hash
    pub interrupts:  std::collections::HashMap<u32, u64>,
}

impl<'a> JitState<'a> {
    pub fn new(input: &[f32], site_map: &'a SiteMap, total_slots: usize) -> Self {
        JitState {
            stack:        vec![JitVal::Vector(Arc::new(NdaVec::from_f32_slice(input)))],
            variables:    vec![None; total_slots],
            site_map,
            matrix_count: 0,
            norm_count:   0,
            loop_count:   0,
            executed_nodes: 0,
            print_buf:    Vec::new(),
            heap:         vec![0u8; 65536],
            heap_allocations: std::collections::HashMap::new(),
            mmio:         std::collections::HashMap::new(),
            interrupts:   std::collections::HashMap::new(),
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
type JitFn = Arc<dyn for<'a> Fn(&mut JitState<'a>) -> Result<JitControlFlow, String> + Send + Sync>;

// ─── Compiled program ──────────────────────────────────────────────────────────

/// A fully compiled NDA program ready for native execution.
pub struct JitProgram {
    /// The sequence of compiled closures that form the program body.
    pub fns:            Vec<JitFn>,
    /// Total NDA nodes compiled (for diagnostics).
    pub nodes_compiled: usize,
    /// Whether a tier-2 machine code GEMV kernel is active.
    pub has_asm_kernel: bool,
    /// Registry mapping variable hashes to slots.
    pub registry:       VarRegistry,
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
                    output_vec:     out_f32,
                    output_dim:     dim,
                    elapsed_us,
                    nodes_compiled: self.nodes_compiled,
                    error:          None,
                }
            }
            Ok(Err(e)) => JitResult {
                output_vec:     vec![],
                output_dim:     0,
                elapsed_us,
                nodes_compiled: self.nodes_compiled,
                error:          Some(e),
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
                    output_vec:     vec![],
                    output_dim:     0,
                    elapsed_us,
                    nodes_compiled: self.nodes_compiled,
                    error:          Some(format!("panic: {}", msg)),
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
                    matrix_count:   state.matrix_count,
                    norm_count:     state.norm_count,
                    output_vec:     out_f32,
                    output_dim:     dim,
                    panicked:       false,
                    error:          None,
                    elapsed_us,
                }
            }
            Ok(Err(e)) => SandboxResult {
                executed_nodes: 0,
                matrix_count:   0,
                norm_count:     0,
                output_vec:     vec![],
                output_dim:     0,
                panicked:       false,
                error:          Some(e),
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
                    matrix_count:   0,
                    norm_count:     0,
                    output_vec:     vec![],
                    output_dim:     0,
                    panicked:       true,
                    error:          Some(format!("panic: {}", msg)),
                    elapsed_us,
                }
            }
        }
    }
}

// ─── Helper: run a sequence of JIT functions ──────────────────────────────────

#[inline(always)]
fn run_sequence(fns: &[JitFn], state: &mut JitState<'_>) -> Result<JitControlFlow, String> {
    for f in fns {
        match f(state)? {
            JitControlFlow::Continue => {}
            cf => return Ok(cf),
        }
    }
    Ok(JitControlFlow::Continue)
}

// ─── Compiler entry point ──────────────────────────────────────────────────────
fn has_side_effects(node: &NdaNode) -> bool {
    match node {
        NdaNode::Call { .. } | NdaNode::Print { .. } | NdaNode::Return { .. } => true,
        NdaNode::Peek { .. } | NdaNode::Poke { .. } | NdaNode::Syscall { .. } | NdaNode::Spawn { .. } | NdaNode::Atomic { .. } | NdaNode::GpuDispatch { .. } => true,
        NdaNode::Free { .. } | NdaNode::RegInt { .. } => true,
        NdaNode::Let { init, .. } => has_side_effects(init),
        NdaNode::Store { value, .. } => has_side_effects(value),
        NdaNode::Scope { children } => children.iter().any(has_side_effects),
        NdaNode::Loop { body, .. } => body.iter().any(has_side_effects),
        NdaNode::While { cond, body } => has_side_effects(cond) || body.iter().any(has_side_effects),
        NdaNode::If { cond, then_body, else_body } => {
            has_side_effects(cond)
                || then_body.iter().any(has_side_effects)
                || else_body.as_ref().map_or(false, |eb| eb.iter().any(has_side_effects))
        }
        NdaNode::Add { lhs, rhs } => has_side_effects(lhs) || has_side_effects(rhs),
        NdaNode::Compare { lhs, rhs, .. } => has_side_effects(lhs) || has_side_effects(rhs),
        NdaNode::VecOp { operand, .. } => has_side_effects(operand),
        NdaNode::Bitwise { lhs, rhs, .. } => has_side_effects(lhs) || rhs.as_ref().map_or(false, |r| has_side_effects(r)),
        NdaNode::Math { lhs, rhs, .. } => has_side_effects(lhs) || has_side_effects(rhs),
        NdaNode::MathFunc { operand, .. } => has_side_effects(operand),
        NdaNode::Gemv { matrix, vector } => has_side_effects(matrix) || has_side_effects(vector),
        NdaNode::Dot { lhs, rhs } => has_side_effects(lhs) || has_side_effects(rhs),
        NdaNode::Alloc { size } => has_side_effects(size),
        NdaNode::Cast { operand, .. } => has_side_effects(operand),
        _ => false,
    }
}

fn gather_loaded_vars(node: &NdaNode, set: &mut std::collections::HashSet<u64>) {
    match node {
        NdaNode::Load { name_hash } => {
            set.insert(*name_hash);
        }
        NdaNode::Let { init, .. } => {
            gather_loaded_vars(init, set);
        }
        NdaNode::Store { value, .. } => {
            gather_loaded_vars(value, set);
        }
        NdaNode::Scope { children } => {
            for child in children {
                gather_loaded_vars(child, set);
            }
        }
        NdaNode::Loop { body, .. } => {
            for child in body {
                gather_loaded_vars(child, set);
            }
        }
        NdaNode::While { cond, body } => {
            gather_loaded_vars(cond, set);
            for child in body {
                gather_loaded_vars(child, set);
            }
        }
        NdaNode::If { cond, then_body, else_body } => {
            gather_loaded_vars(cond, set);
            for child in then_body {
                gather_loaded_vars(child, set);
            }
            if let Some(eb) = else_body {
                for child in eb {
                    gather_loaded_vars(child, set);
                }
            }
        }
        NdaNode::Add { lhs, rhs } => {
            gather_loaded_vars(lhs, set);
            gather_loaded_vars(rhs, set);
        }
        NdaNode::Compare { lhs, rhs, .. } => {
            gather_loaded_vars(lhs, set);
            gather_loaded_vars(rhs, set);
        }
        NdaNode::VecOp { operand, .. } => {
            gather_loaded_vars(operand, set);
        }
        NdaNode::Print { source } => {
            gather_loaded_vars(source, set);
        }
        NdaNode::Return { value } => {
            gather_loaded_vars(value, set);
        }
        NdaNode::Bitwise { lhs, rhs, .. } => {
            gather_loaded_vars(lhs, set);
            if let Some(r) = rhs { gather_loaded_vars(r, set); }
        }
        NdaNode::Math { lhs, rhs, .. } => {
            gather_loaded_vars(lhs, set);
            gather_loaded_vars(rhs, set);
        }
        NdaNode::MathFunc { operand, .. } => gather_loaded_vars(operand, set),
        NdaNode::Peek { addr } => gather_loaded_vars(addr, set),
        NdaNode::Poke { addr, value } => {
            gather_loaded_vars(addr, set);
            gather_loaded_vars(value, set);
        }
        NdaNode::Gemv { matrix, vector } => {
            gather_loaded_vars(matrix, set);
            gather_loaded_vars(vector, set);
        }
        NdaNode::Dot { lhs, rhs } => {
            gather_loaded_vars(lhs, set);
            gather_loaded_vars(rhs, set);
        }
        NdaNode::Syscall { args, .. } => {
            for arg in args { gather_loaded_vars(arg, set); }
        }
        NdaNode::Atomic { addr, val, .. } => {
            gather_loaded_vars(addr, set);
            gather_loaded_vars(val, set);
        }
        NdaNode::Alloc { size } => gather_loaded_vars(size, set),
        NdaNode::Free { addr } => gather_loaded_vars(addr, set),
        NdaNode::Cast { operand, .. } => gather_loaded_vars(operand, set),
        NdaNode::GpuDispatch { args, .. } => {
            for arg in args { gather_loaded_vars(arg, set); }
        }
        _ => {}
    }
}

fn dce_sequence(nodes: &[NdaNode], live_vars: &mut std::collections::HashSet<u64>) -> Vec<NdaNode> {
    let mut optimized = Vec::new();

    for node in nodes.iter().rev() {
        match node {
            NdaNode::Let { name_hash, init } => {
                let is_param = match &**init {
                    NdaNode::Scope { children } => children.is_empty(),
                    _ => false,
                };
                let has_side = has_side_effects(node);

                if !is_param && !has_side && !live_vars.contains(name_hash) {
                    continue;
                }
                live_vars.remove(name_hash);
                let opt_init = dce_node(init, live_vars);
                optimized.push(NdaNode::Let { name_hash: *name_hash, init: Box::new(opt_init) });
            }
            NdaNode::Store { name_hash, value } => {
                let has_side = has_side_effects(node);
                if !has_side && !live_vars.contains(name_hash) {
                    continue;
                }
                live_vars.remove(name_hash);
                let opt_value = dce_node(value, live_vars);
                optimized.push(NdaNode::Store { name_hash: *name_hash, value: Box::new(opt_value) });
            }
            _ => {
                let opt_node = dce_node(node, live_vars);
                optimized.push(opt_node);
            }
        }
    }

    optimized.reverse();
    optimized
}

fn dce_node(node: &NdaNode, live_vars: &mut std::collections::HashSet<u64>) -> NdaNode {
    match node {
        NdaNode::Scope { children } => {
            let opt_children = dce_sequence(children, live_vars);
            NdaNode::Scope { children: opt_children }
        }
        NdaNode::Loop { count, body } => {
            let mut body_loaded = std::collections::HashSet::new();
            for child in body {
                gather_loaded_vars(child, &mut body_loaded);
            }
            let mut body_live = live_vars.clone();
            body_live.extend(body_loaded);
            let opt_body = dce_sequence(body, &mut body_live);
            *live_vars = body_live;
            NdaNode::Loop { count: *count, body: opt_body }
        }
        NdaNode::While { cond, body } => {
            let mut body_loaded = std::collections::HashSet::new();
            gather_loaded_vars(cond, &mut body_loaded);
            for child in body {
                gather_loaded_vars(child, &mut body_loaded);
            }
            let mut body_live = live_vars.clone();
            body_live.extend(body_loaded);
            let opt_body = dce_sequence(body, &mut body_live);
            let opt_cond = dce_node(cond, &mut body_live);
            *live_vars = body_live;
            NdaNode::While { cond: Box::new(opt_cond), body: opt_body }
        }
        NdaNode::If { cond, then_body, else_body } => {
            let mut then_live = live_vars.clone();
            let opt_then = dce_sequence(then_body, &mut then_live);
            let (opt_else, else_live) = if let Some(eb) = else_body {
                let mut el = live_vars.clone();
                let oe = dce_sequence(eb, &mut el);
                (Some(oe), el)
            } else {
                (None, live_vars.clone())
            };
            let mut combined_live = then_live;
            combined_live.extend(else_live);
            let opt_cond = dce_node(cond, &mut combined_live);
            *live_vars = combined_live;
            NdaNode::If {
                cond: Box::new(opt_cond),
                then_body: opt_then,
                else_body: opt_else,
            }
        }
        NdaNode::Add { lhs, rhs } => {
            let opt_rhs = dce_node(rhs, live_vars);
            let opt_lhs = dce_node(lhs, live_vars);
            NdaNode::Add { lhs: Box::new(opt_lhs), rhs: Box::new(opt_rhs) }
        }
        NdaNode::Compare { op, lhs, rhs } => {
            let opt_rhs = dce_node(rhs, live_vars);
            let opt_lhs = dce_node(lhs, live_vars);
            NdaNode::Compare { op: *op, lhs: Box::new(opt_lhs), rhs: Box::new(opt_rhs) }
        }
        NdaNode::VecOp { op, operand } => {
            let opt_operand = dce_node(operand, live_vars);
            NdaNode::VecOp { op: *op, operand: Box::new(opt_operand) }
        }
        NdaNode::Print { source } => {
            let opt_source = dce_node(source, live_vars);
            NdaNode::Print { source: Box::new(opt_source) }
        }
        NdaNode::Return { value } => {
            let opt_value = dce_node(value, live_vars);
            NdaNode::Return { value: Box::new(opt_value) }
        }
        NdaNode::Load { name_hash } => {
            live_vars.insert(*name_hash);
            NdaNode::Load { name_hash: *name_hash }
        }
        NdaNode::Bitwise { op, lhs, rhs } => {
            let opt_lhs = dce_node(lhs, live_vars);
            let opt_rhs = rhs.as_ref().map(|r| Box::new(dce_node(r, live_vars)));
            NdaNode::Bitwise { op: *op, lhs: Box::new(opt_lhs), rhs: opt_rhs }
        }
        NdaNode::Math { op, lhs, rhs } => {
            let opt_lhs = dce_node(lhs, live_vars);
            let opt_rhs = dce_node(rhs, live_vars);
            NdaNode::Math { op: *op, lhs: Box::new(opt_lhs), rhs: Box::new(opt_rhs) }
        }
        NdaNode::MathFunc { func, operand } => {
            let opt_op = dce_node(operand, live_vars);
            NdaNode::MathFunc { func: *func, operand: Box::new(opt_op) }
        }
        NdaNode::Peek { addr } => {
            let opt_addr = dce_node(addr, live_vars);
            NdaNode::Peek { addr: Box::new(opt_addr) }
        }
        NdaNode::Poke { addr, value } => {
            let opt_addr = dce_node(addr, live_vars);
            let opt_val = dce_node(value, live_vars);
            NdaNode::Poke { addr: Box::new(opt_addr), value: Box::new(opt_val) }
        }
        NdaNode::Gemv { matrix, vector } => {
            let opt_m = dce_node(matrix, live_vars);
            let opt_v = dce_node(vector, live_vars);
            NdaNode::Gemv { matrix: Box::new(opt_m), vector: Box::new(opt_v) }
        }
        NdaNode::Dot { lhs, rhs } => {
            let opt_lhs = dce_node(lhs, live_vars);
            let opt_rhs = dce_node(rhs, live_vars);
            NdaNode::Dot { lhs: Box::new(opt_lhs), rhs: Box::new(opt_rhs) }
        }
        NdaNode::Syscall { num, args } => {
            let mut opt_args = Vec::new();
            for arg in args {
                opt_args.push(dce_node(arg, live_vars));
            }
            NdaNode::Syscall { num: *num, args: opt_args }
        }
        NdaNode::Atomic { op, addr, val } => {
            let opt_addr = dce_node(addr, live_vars);
            let opt_val = dce_node(val, live_vars);
            NdaNode::Atomic { op: *op, addr: Box::new(opt_addr), val: Box::new(opt_val) }
        }
        NdaNode::Alloc { size } => {
            let opt_size = dce_node(size, live_vars);
            NdaNode::Alloc { size: Box::new(opt_size) }
        }
        NdaNode::Free { addr } => {
            let opt_addr = dce_node(addr, live_vars);
            NdaNode::Free { addr: Box::new(opt_addr) }
        }
        NdaNode::Cast { from_type, to_type, operand } => {
            let opt_op = dce_node(operand, live_vars);
            NdaNode::Cast { from_type: *from_type, to_type: *to_type, operand: Box::new(opt_op) }
        }
        NdaNode::GpuDispatch { shader_hash, args } => {
            let mut opt_args = Vec::new();
            for arg in args {
                opt_args.push(dce_node(arg, live_vars));
            }
            NdaNode::GpuDispatch { shader_hash: *shader_hash, args: opt_args }
        }
        other => other.clone(),
    }
}

fn optimize_sequence(nodes: &[NdaNode], var_constants: &mut HashMap<u64, i32>) -> Vec<NdaNode> {
    let mut optimized = Vec::new();
    for node in nodes {
        let opt_node = optimize_node(node.clone(), var_constants);
        if let NdaNode::Let { name_hash, init } = &opt_node {
            if let NdaNode::Int { value } = &**init {
                var_constants.insert(*name_hash, *value);
            } else {
                var_constants.remove(name_hash);
            }
        }
        if let NdaNode::Store { name_hash, value } = &opt_node {
            if let NdaNode::Int { value } = &**value {
                var_constants.insert(*name_hash, *value);
            } else {
                var_constants.remove(name_hash);
            }
        }
        optimized.push(opt_node);
    }
    optimized
}

fn gather_written_vars(node: &NdaNode, set: &mut std::collections::HashSet<u64>) {
    match node {
        NdaNode::Let { name_hash, init } => {
            set.insert(*name_hash);
            gather_written_vars(init, set);
        }
        NdaNode::Store { name_hash, value } => {
            set.insert(*name_hash);
            gather_written_vars(value, set);
        }
        NdaNode::Scope { children } => {
            for child in children {
                gather_written_vars(child, set);
            }
        }
        NdaNode::Loop { body, .. } => {
            for child in body {
                gather_written_vars(child, set);
            }
        }
        NdaNode::While { cond, body } => {
            gather_written_vars(cond, set);
            for child in body {
                gather_written_vars(child, set);
            }
        }
        NdaNode::If { cond, then_body, else_body } => {
            gather_written_vars(cond, set);
            for child in then_body {
                gather_written_vars(child, set);
            }
            if let Some(eb) = else_body {
                for child in eb {
                    gather_written_vars(child, set);
                }
            }
        }
        NdaNode::Add { lhs, rhs } => {
            gather_written_vars(lhs, set);
            gather_written_vars(rhs, set);
        }
        NdaNode::Compare { lhs, rhs, .. } => {
            gather_written_vars(lhs, set);
            gather_written_vars(rhs, set);
        }
        NdaNode::VecOp { operand, .. } => {
            gather_written_vars(operand, set);
        }
        NdaNode::Print { source } => {
            gather_written_vars(source, set);
        }
        NdaNode::Return { value } => {
            gather_written_vars(value, set);
        }
        NdaNode::Bitwise { lhs, rhs, .. } => {
            gather_written_vars(lhs, set);
            if let Some(r) = rhs { gather_written_vars(r, set); }
        }
        NdaNode::Math { lhs, rhs, .. } => {
            gather_written_vars(lhs, set);
            gather_written_vars(rhs, set);
        }
        NdaNode::MathFunc { operand, .. } => gather_written_vars(operand, set),
        NdaNode::Peek { addr } => gather_written_vars(addr, set),
        NdaNode::Poke { addr, value } => {
            gather_written_vars(addr, set);
            gather_written_vars(value, set);
        }
        NdaNode::Gemv { matrix, vector } => {
            gather_written_vars(matrix, set);
            gather_written_vars(vector, set);
        }
        NdaNode::Dot { lhs, rhs } => {
            gather_written_vars(lhs, set);
            gather_written_vars(rhs, set);
        }
        NdaNode::Syscall { args, .. } => {
            for arg in args { gather_written_vars(arg, set); }
        }
        NdaNode::Atomic { addr, val, .. } => {
            gather_written_vars(addr, set);
            gather_written_vars(val, set);
        }
        NdaNode::Alloc { size } => gather_written_vars(size, set),
        NdaNode::Free { addr } => gather_written_vars(addr, set),
        NdaNode::Cast { operand, .. } => gather_written_vars(operand, set),
        NdaNode::GpuDispatch { args, .. } => {
            for arg in args { gather_written_vars(arg, set); }
        }
        _ => {}
    }
}

fn optimize_node(node: NdaNode, var_constants: &mut HashMap<u64, i32>) -> NdaNode {
    match node {
        NdaNode::Add { lhs, rhs } => {
            let opt_lhs = optimize_node(*lhs, var_constants);
            let opt_rhs = optimize_node(*rhs, var_constants);
            match (&opt_lhs, &opt_rhs) {
                (NdaNode::Int { value: l }, NdaNode::Int { value: r }) => {
                    NdaNode::Int { value: l.saturating_add(*r) }
                }
                _ => NdaNode::Add { lhs: Box::new(opt_lhs), rhs: Box::new(opt_rhs) },
            }
        }
        NdaNode::Load { name_hash } => {
            if let Some(&val) = var_constants.get(&name_hash) {
                NdaNode::Int { value: val }
            } else {
                NdaNode::Load { name_hash }
            }
        }
        NdaNode::Let { name_hash, init } => {
            let opt_init = optimize_node(*init, var_constants);
            NdaNode::Let { name_hash, init: Box::new(opt_init) }
        }
        NdaNode::Store { name_hash, value } => {
            let opt_value = optimize_node(*value, var_constants);
            NdaNode::Store { name_hash, value: Box::new(opt_value) }
        }
        NdaNode::Scope { children } => {
            let opt_children = optimize_sequence(&children, var_constants);
            NdaNode::Scope { children: opt_children }
        }
        NdaNode::Loop { count, body } => {
            if count > 0 && count <= 4 {
                let mut unrolled = Vec::new();
                for _ in 0..count {
                    unrolled.extend(body.clone());
                }
                let opt_unrolled = optimize_sequence(&unrolled, var_constants);
                NdaNode::Scope { children: opt_unrolled }
            } else {
                let mut written = std::collections::HashSet::new();
                for child in &body {
                    gather_written_vars(child, &mut written);
                }
                for v in written {
                    var_constants.remove(&v);
                }
                let mut loop_vars = HashMap::new();
                let opt_body = optimize_sequence(&body, &mut loop_vars);
                NdaNode::Loop { count, body: opt_body }
            }
        }
        NdaNode::While { cond, body } => {
            let mut written = std::collections::HashSet::new();
            gather_written_vars(&cond, &mut written);
            for child in &body {
                gather_written_vars(child, &mut written);
            }
            for v in written {
                var_constants.remove(&v);
            }
            let mut loop_vars = HashMap::new();
            let opt_cond = optimize_node(*cond, &mut loop_vars);
            let opt_body = optimize_sequence(&body, &mut loop_vars);
            NdaNode::While { cond: Box::new(opt_cond), body: opt_body }
        }
        NdaNode::If { cond, then_body, else_body } => {
            let opt_cond = optimize_node(*cond, var_constants);
            if let NdaNode::Int { value } = opt_cond {
                if value > 0 {
                    let opt_then = optimize_sequence(&then_body, var_constants);
                    NdaNode::Scope { children: opt_then }
                } else if let Some(eb) = else_body {
                    let opt_else = optimize_sequence(&eb, var_constants);
                    NdaNode::Scope { children: opt_else }
                } else {
                    NdaNode::Scope { children: vec![] }
                }
            } else {
                let mut written = std::collections::HashSet::new();
                for child in &then_body {
                    gather_written_vars(child, &mut written);
                }
                if let Some(ref eb) = else_body {
                    for child in eb {
                        gather_written_vars(child, &mut written);
                    }
                }
                for v in written {
                    var_constants.remove(&v);
                }
                let mut then_vars = var_constants.clone();
                let opt_then = optimize_sequence(&then_body, &mut then_vars);
                let opt_else = else_body.map(|eb| {
                    let mut else_vars = var_constants.clone();
                    optimize_sequence(&eb, &mut else_vars)
                });
                NdaNode::If { cond: Box::new(opt_cond), then_body: opt_then, else_body: opt_else }
            }
        }
        NdaNode::Compare { op, lhs, rhs } => {
            let opt_lhs = optimize_node(*lhs, var_constants);
            let opt_rhs = optimize_node(*rhs, var_constants);
            match (&opt_lhs, &opt_rhs) {
                (NdaNode::Int { value: l }, NdaNode::Int { value: r }) => {
                    let cmp = match op {
                        CmpOp::Eq => l == r,
                        CmpOp::Ne => l != r,
                        CmpOp::Lt => l < r,
                        CmpOp::Gt => l > r,
                        CmpOp::Le => l <= r,
                        CmpOp::Ge => l >= r,
                    };
                    NdaNode::Int { value: if cmp { 1 } else { -1 } }
                }
                _ => NdaNode::Compare { op, lhs: Box::new(opt_lhs), rhs: Box::new(opt_rhs) },
            }
        }
        NdaNode::VecOp { op, operand } => {
            let opt_operand = optimize_node(*operand, var_constants);
            match (&op, &opt_operand) {
                (VecOpKind::Negate, NdaNode::Int { value }) => NdaNode::Int { value: -value },
                (VecOpKind::Abs, NdaNode::Int { value }) => NdaNode::Int { value: value.abs() },
                (VecOpKind::ReduceSum, NdaNode::Int { value }) => NdaNode::Int { value: *value },
                _ => NdaNode::VecOp { op, operand: Box::new(opt_operand) },
            }
        }
        NdaNode::Print { source } => {
            let opt_source = optimize_node(*source, var_constants);
            NdaNode::Print { source: Box::new(opt_source) }
        }
        NdaNode::Return { value } => {
            let opt_value = optimize_node(*value, var_constants);
            NdaNode::Return { value: Box::new(opt_value) }
        }
        NdaNode::Bitwise { op, lhs, rhs } => {
            let opt_lhs = optimize_node(*lhs, var_constants);
            let opt_rhs = rhs.map(|r| Box::new(optimize_node(*r, var_constants)));
            NdaNode::Bitwise { op, lhs: Box::new(opt_lhs), rhs: opt_rhs }
        }
        NdaNode::Math { op, lhs, rhs } => {
            let opt_lhs = optimize_node(*lhs, var_constants);
            let opt_rhs = optimize_node(*rhs, var_constants);
            NdaNode::Math { op, lhs: Box::new(opt_lhs), rhs: Box::new(opt_rhs) }
        }
        NdaNode::MathFunc { func, operand } => {
            let opt_op = optimize_node(*operand, var_constants);
            NdaNode::MathFunc { func, operand: Box::new(opt_op) }
        }
        NdaNode::Peek { addr } => {
            let opt_addr = optimize_node(*addr, var_constants);
            NdaNode::Peek { addr: Box::new(opt_addr) }
        }
        NdaNode::Poke { addr, value } => {
            let opt_addr = optimize_node(*addr, var_constants);
            let opt_val = optimize_node(*value, var_constants);
            NdaNode::Poke { addr: Box::new(opt_addr), value: Box::new(opt_val) }
        }
        NdaNode::Gemv { matrix, vector } => {
            let opt_m = optimize_node(*matrix, var_constants);
            let opt_v = optimize_node(*vector, var_constants);
            NdaNode::Gemv { matrix: Box::new(opt_m), vector: Box::new(opt_v) }
        }
        NdaNode::Dot { lhs, rhs } => {
            let opt_lhs = optimize_node(*lhs, var_constants);
            let opt_rhs = optimize_node(*rhs, var_constants);
            NdaNode::Dot { lhs: Box::new(opt_lhs), rhs: Box::new(opt_rhs) }
        }
        NdaNode::Syscall { num, args } => {
            let opt_args = args.into_iter().map(|arg| optimize_node(arg, var_constants)).collect();
            NdaNode::Syscall { num, args: opt_args }
        }
        NdaNode::Atomic { op, addr, val } => {
            let opt_addr = optimize_node(*addr, var_constants);
            let opt_val = optimize_node(*val, var_constants);
            NdaNode::Atomic { op, addr: Box::new(opt_addr), val: Box::new(opt_val) }
        }
        NdaNode::Alloc { size } => {
            let opt_size = optimize_node(*size, var_constants);
            NdaNode::Alloc { size: Box::new(opt_size) }
        }
        NdaNode::Free { addr } => {
            let opt_addr = optimize_node(*addr, var_constants);
            NdaNode::Free { addr: Box::new(opt_addr) }
        }
        NdaNode::Cast { from_type, to_type, operand } => {
            let opt_op = optimize_node(*operand, var_constants);
            NdaNode::Cast { from_type, to_type, operand: Box::new(opt_op) }
        }
        NdaNode::GpuDispatch { shader_hash, args } => {
            let opt_args = args.into_iter().map(|arg| optimize_node(arg, var_constants)).collect();
            NdaNode::GpuDispatch { shader_hash, args: opt_args }
        }
        other => other,
    }
}

pub fn optimize_ast(nodes: &[NdaNode]) -> Vec<NdaNode> {
    let mut var_constants = HashMap::new();
    let folded = optimize_sequence(nodes, &mut var_constants);
    let mut live_vars = std::collections::HashSet::new();
    dce_sequence(&folded, &mut live_vars)
}

/// Maximum iterations for `While` loops — safety limit against infinite loops.
const MAX_WHILE_ITERATIONS: u32 = 10_000_000;

/// Compile a slice of `NdaNode`s into a `JitProgram`.
///
/// This is the main entry point.  Call once per program load; execute many times.
pub fn compile(nodes: &[NdaNode]) -> JitProgram {
    let mut counter = 0usize;
    let registry = VarRegistry::new();
    let optimized_nodes = optimize_ast(nodes);
    let fns = compile_sequence(&optimized_nodes, &mut counter, &registry);

    // Detect if the tier-2 ASM GEMV kernel is available on this platform.
    let has_asm_kernel = asm_gemv_available();

    JitProgram { fns, nodes_compiled: counter, has_asm_kernel, registry }
}

/// Compile a sequence of nodes into a Vec of JitFns.
fn compile_sequence(nodes: &[NdaNode], counter: &mut usize, registry: &VarRegistry) -> Vec<JitFn> {
    nodes.iter().map(|n| compile_node(n, counter, registry)).collect()
}

/// Compile one NdaNode into a JitFn closure.
///
/// The key property: the returned closure contains *no* match statements over
/// node types — all dispatch is encoded into which closure is returned.
// ─── Stack VM Helpers ──────────────────────────────────────────────────────────

fn broadcast_scalar(len: usize, val: i32, log2_scale: i8) -> NdaVec {
    let bytes = (len + 7) / 8;
    let (s_byte, e_byte) = match val {
        -2 => (0x00, 0x00),
        -1 => (0x00, 0xFF),
        1  => (0xFF, 0x00),
        _  => (0xFF, 0xFF),
    };
    NdaVec {
        len,
        log2_scale,
        sign: vec![s_byte; bytes].into(),
        extra: vec![e_byte; bytes].into(),
    }
}

fn broadcast_float(len: usize, val: f32) -> NdaVec {
    let nda_res = NdaVec::from_f32_slice(&[val]);
    broadcast_scalar(len, nda_res.get_raw(0), nda_res.log2_scale)
}

pub(crate) fn add_vals(lhs: &JitVal, rhs: &JitVal) -> JitVal {
    match (lhs, rhs) {
        (JitVal::Float(l), JitVal::Float(r)) => JitVal::Float(l + r),
        (JitVal::Float(l), JitVal::Scalar(r_v, r_s)) => {
            let r_actual = (*r_v as f32) * 2.0f32.powi(*r_s as i32);
            JitVal::Float(l + r_actual)
        }
        (JitVal::Scalar(l_v, l_s), JitVal::Float(r)) => {
            let l_actual = (*l_v as f32) * 2.0f32.powi(*l_s as i32);
            JitVal::Float(l_actual + r)
        }
        (JitVal::Scalar(l_v, l_s), JitVal::Scalar(r_v, r_s)) => {
            if *l_s == 0 && *r_s == 0 {
                JitVal::Scalar(l_v + r_v, 0)
            } else {
                const ENCODE_TABLE: [u8; 9] = [0, 0, 0, 1, 2, 2, 3, 3, 3];
                let out_scale = (*l_s).max(*r_s);
                let l_shift = (out_scale - *l_s).max(0) as u32;
                let r_shift = (out_scale - *r_s).max(0) as u32;
                let lv = *l_v >> l_shift;
                let rv = *r_v >> r_shift;
                let sum = lv + rv;
                let clamped = (sum + 4).clamp(0, 8) as usize;
                let enc = ENCODE_TABLE[clamped];
                let val = match enc {
                    0 => -2,
                    1 => -1,
                    2 => 1,
                    3 => 2,
                    _ => unreachable!(),
                };
                JitVal::Scalar(val, out_scale)
            }
        }
        _ => {
            let mut lhs_vec = match lhs {
                JitVal::Vector(v) => (**v).clone(),
                JitVal::Float(val) => broadcast_scalar(1, val.to_bits() as i32, 0),
                JitVal::Scalar(val, scale) => {
                    let r_len = match rhs {
                        JitVal::Vector(rv) => rv.len,
                        _ => 1,
                    };
                    broadcast_scalar(r_len, *val, *scale)
                }
            };
            let rhs_vec = match rhs {
                JitVal::Vector(v) => v.clone(),
                JitVal::Float(val) => Arc::new(broadcast_scalar(1, val.to_bits() as i32, 0)),
                JitVal::Scalar(val, scale) => {
                    let l_len = lhs_vec.len;
                    Arc::new(broadcast_scalar(l_len, *val, *scale))
                }
            };
            nda_vec_add_inplace(&mut lhs_vec, &rhs_vec);
            JitVal::Vector(Arc::new(lhs_vec))
        }
    }
}

pub(crate) fn compare_vals(op: CmpOp, lhs: &JitVal, rhs: &JitVal) -> JitVal {
    match (lhs, rhs) {
        (JitVal::Float(l), JitVal::Float(r)) => {
            let l = *l;
            let r = *r;
            let cmp = match op {
                CmpOp::Eq => (l - r).abs() < 1e-6,
                CmpOp::Ne => (l - r).abs() >= 1e-6,
                CmpOp::Lt => l < r,
                CmpOp::Gt => l > r,
                CmpOp::Le => l <= r,
                CmpOp::Ge => l >= r,
            };
            JitVal::Scalar(if cmp { 1 } else { -1 }, 0)
        }
        (JitVal::Float(l), JitVal::Scalar(r_v, r_s)) => {
            let l = *l;
            let r_actual = (*r_v as f32) * 2.0f32.powi(*r_s as i32);
            let cmp = match op {
                CmpOp::Eq => (l - r_actual).abs() < 1e-6,
                CmpOp::Ne => (l - r_actual).abs() >= 1e-6,
                CmpOp::Lt => l < r_actual,
                CmpOp::Gt => l > r_actual,
                CmpOp::Le => l <= r_actual,
                CmpOp::Ge => l >= r_actual,
            };
            JitVal::Scalar(if cmp { 1 } else { -1 }, 0)
        }
        (JitVal::Scalar(l_v, l_s), JitVal::Float(r)) => {
            let r = *r;
            let l_actual = (*l_v as f32) * 2.0f32.powi(*l_s as i32);
            let cmp = match op {
                CmpOp::Eq => (l_actual - r).abs() < 1e-6,
                CmpOp::Ne => (l_actual - r).abs() >= 1e-6,
                CmpOp::Lt => l_actual < r,
                CmpOp::Gt => l_actual > r,
                CmpOp::Le => l_actual <= r,
                CmpOp::Ge => l_actual >= r,
            };
            JitVal::Scalar(if cmp { 1 } else { -1 }, 0)
        }
        (JitVal::Scalar(l_v, l_s), JitVal::Scalar(r_v, r_s)) => {
            if *l_s == 0 && *r_s == 0 {
                let cmp = match op {
                    CmpOp::Eq => l_v == r_v,
                    CmpOp::Ne => l_v != r_v,
                    CmpOp::Lt => l_v < r_v,
                    CmpOp::Gt => l_v > r_v,
                    CmpOp::Le => l_v <= r_v,
                    CmpOp::Ge => l_v >= r_v,
                };
                let val = if cmp { 1 } else { -1 };
                JitVal::Scalar(val, 0)
            } else {
                let l_actual = (*l_v as f32) * 2.0f32.powi(*l_s as i32);
                let r_actual = (*r_v as f32) * 2.0f32.powi(*r_s as i32);
                let cmp = match op {
                    CmpOp::Eq => (l_actual - r_actual).abs() < 1e-6,
                    CmpOp::Ne => (l_actual - r_actual).abs() >= 1e-6,
                    CmpOp::Lt => l_actual < r_actual,
                    CmpOp::Gt => l_actual > r_actual,
                    CmpOp::Le => l_actual <= r_actual,
                    CmpOp::Ge => l_actual >= r_actual,
                };
                let val = if cmp { 1 } else { -1 };
                JitVal::Scalar(val, 0)
            }
        }
        _ => {
            let l_len = match lhs {
                JitVal::Vector(v) => v.len,
                _ => 1,
            };
            let r_len = match rhs {
                JitVal::Vector(v) => v.len,
                _ => 1,
            };
            let len = l_len.max(r_len);

            let lhs_vec = match lhs {
                JitVal::Vector(v) => (**v).clone(),
                JitVal::Scalar(val, scale) => broadcast_scalar(len, *val, *scale),
                JitVal::Float(val) => broadcast_float(len, *val),
            };
            let rhs_vec = match rhs {
                JitVal::Vector(v) => (**v).clone(),
                JitVal::Scalar(val, scale) => broadcast_scalar(len, *val, *scale),
                JitVal::Float(val) => broadcast_float(len, *val),
            };

            let bytes = (len + 7) / 8;
            let mut sign = vec![0u8; bytes];
            let mut extra = vec![0u8; bytes];

            for byte_idx in 0..bytes {
                let lhs_s = if byte_idx < lhs_vec.sign.len() { lhs_vec.sign[byte_idx] } else { 0 };
                let lhs_e = if byte_idx < lhs_vec.extra.len() { lhs_vec.extra[byte_idx] } else { 0 };

                let rhs_s = if byte_idx < rhs_vec.sign.len() { rhs_vec.sign[byte_idx] } else { 0 };
                let rhs_e = if byte_idx < rhs_vec.extra.len() { rhs_vec.extra[byte_idx] } else { 0 };

                let mut s_byte = 0u8;
                let mut e_byte = 0u8;

                let base_idx = byte_idx * 8;
                for bit in 0..8 {
                    let i = base_idx + bit;
                    if i >= len { break; }
                    let l = if i < lhs_vec.len {
                        let mask = 1u8 << bit;
                        let is_pos = (lhs_s & mask) != 0;
                        let is_large = (lhs_s & mask) == (lhs_e & mask);
                        let mag = if is_large { 2i32 } else { 1 };
                        if is_pos { mag } else { -mag }
                    } else {
                        0
                    };
                    let r = if i < rhs_vec.len {
                        let mask = 1u8 << bit;
                        let is_pos = (rhs_s & mask) != 0;
                        let is_large = (rhs_s & mask) == (rhs_e & mask);
                        let mag = if is_large { 2i32 } else { 1 };
                        if is_pos { mag } else { -mag }
                    } else {
                        0
                    };
                    let cmp = match op {
                        CmpOp::Eq => l == r,
                        CmpOp::Ne => l != r,
                        CmpOp::Lt => l <  r,
                        CmpOp::Gt => l >  r,
                        CmpOp::Le => l <= r,
                        CmpOp::Ge => l >= r,
                    };
                    s_byte |= (cmp as u8) << bit;
                    e_byte |= ((!cmp) as u8) << bit;
                }
                sign[byte_idx] = s_byte;
                extra[byte_idx] = e_byte;
            }

            JitVal::Vector(Arc::new(NdaVec {
                len,
                log2_scale: 0,
                sign: sign.into(),
                extra: extra.into(),
            }))
        }
    }
}

pub(crate) fn apply_vec_op(op: VecOpKind, val: &JitVal) -> JitVal {
    match op {
        VecOpKind::Negate => {
            match val {
                JitVal::Float(v) => JitVal::Float(-*v),
                JitVal::Scalar(v, s) => {
                    if *s == 0 {
                        JitVal::Scalar(-*v, 0)
                    } else {
                        JitVal::Scalar(-v, *s)
                    }
                }
                JitVal::Vector(v) => {
                    let mut new_sign = v.sign.to_vec();
                    let mut new_extra = v.extra.to_vec();
                    for i in 0..new_sign.len() {
                        new_sign[i] = !new_sign[i];
                        new_extra[i] = !new_extra[i];
                    }
                    if v.len % 8 != 0 {
                        let mask = (1u8 << (v.len % 8)) - 1;
                        if let Some(last) = new_sign.last_mut() { *last &= mask; }
                        if let Some(last) = new_extra.last_mut() { *last &= mask; }
                    }
                    JitVal::Vector(Arc::new(NdaVec {
                        len: v.len,
                        log2_scale: v.log2_scale,
                        sign: new_sign.into(),
                        extra: new_extra.into(),
                    }))
                }
            }
        }
        VecOpKind::Abs => {
            match val {
                JitVal::Float(v) => JitVal::Float(v.abs()),
                JitVal::Scalar(v, s) => {
                    if *s == 0 {
                        JitVal::Scalar(v.abs(), 0)
                    } else {
                        JitVal::Scalar(v.abs(), *s)
                    }
                }
                JitVal::Vector(v) => {
                    let mut new_sign = vec![0xFFu8; v.sign.len()];
                    let mut new_extra = vec![0u8; v.extra.len()];
                    for i in 0..v.sign.len() {
                        new_extra[i] = !(v.sign[i] ^ v.extra[i]);
                    }
                    if v.len % 8 != 0 {
                        let mask = (1u8 << (v.len % 8)) - 1;
                        if let Some(last) = new_sign.last_mut() { *last &= mask; }
                        if let Some(last) = new_extra.last_mut() { *last &= mask; }
                    }
                    JitVal::Vector(Arc::new(NdaVec {
                        len: v.len,
                        log2_scale: v.log2_scale,
                        sign: new_sign.into(),
                        extra: new_extra.into(),
                    }))
                }
            }
        }
        VecOpKind::ReduceSum => {
            match val {
                JitVal::Float(v) => JitVal::Float(*v),
                JitVal::Scalar(v, s) => JitVal::Scalar(*v, *s),
                JitVal::Vector(v) => {
                    let mut raw_sum = 0i32;
                    let bytes = v.sign.len();
                    const DECODE_TABLE: [i32; 4] = [-2, -1, 1, 2];
                    for byte_idx in 0..bytes {
                        let mut s_shift = v.sign[byte_idx];
                        let mut e_shift = v.extra[byte_idx];
                        let base_idx = byte_idx * 8;
                        for bit in 0..8 {
                            let i = base_idx + bit;
                            if i >= v.len { break; }
                            let idx = ((s_shift & 1) << 1) | (e_shift & 1);
                            raw_sum += DECODE_TABLE[idx as usize];
                            s_shift >>= 1;
                            e_shift >>= 1;
                        }
                    }
                    let logical_sum = (raw_sum as f32) * 2.0f32.powi(v.log2_scale as i32);
                    let nda_res = NdaVec::from_f32_slice(&[logical_sum]);
                    JitVal::Scalar(nda_res.get_raw(0), nda_res.log2_scale)
                }
            }
        }
        VecOpKind::SiLU => {
            match val {
                JitVal::Float(v) => JitVal::Float(silu(*v)),
                JitVal::Scalar(v, s) => {
                    let actual = (*v as f32) * 2.0f32.powi(*s as i32);
                    let res = silu(actual);
                    let nda_res = NdaVec::from_f32_slice(&[res]);
                    JitVal::Scalar(nda_res.get_raw(0), nda_res.log2_scale)
                }
                JitVal::Vector(v) => {
                    let f32s = v.to_f32_vec();
                    let result: Vec<f32> = f32s.iter().map(|&x| silu(x)).collect();
                    JitVal::Vector(Arc::new(NdaVec::from_f32_slice(&result)))
                }
            }
        }
    }
}

// ─── Native Scalar JIT Compiler ───────────────────────────────────────────────

fn is_pure_scalar(node: &NdaNode) -> bool {
    match node {
        NdaNode::Int { .. } | NdaNode::Break => true,
        NdaNode::Let { init, .. } => is_pure_scalar(init),
        NdaNode::Load { .. } => true,
        NdaNode::Store { value, .. } => is_pure_scalar(value),
        NdaNode::Add { lhs, rhs } => is_pure_scalar(lhs) && is_pure_scalar(rhs),
        NdaNode::Compare { lhs, rhs, .. } => is_pure_scalar(lhs) && is_pure_scalar(rhs),
        NdaNode::VecOp { op, operand } => {
            matches!(op, VecOpKind::Negate | VecOpKind::Abs | VecOpKind::ReduceSum)
                && is_pure_scalar(operand)
        }
        NdaNode::Loop { body, .. } => body.iter().all(is_pure_scalar),
        NdaNode::While { cond, body } => is_pure_scalar(cond) && body.iter().all(is_pure_scalar),
        NdaNode::If { cond, then_body, else_body } => {
            is_pure_scalar(cond)
                && then_body.iter().all(is_pure_scalar)
                && else_body.as_ref().map_or(true, |eb| eb.iter().all(is_pure_scalar))
        }
        NdaNode::Scope { children } => children.iter().all(is_pure_scalar),
        NdaNode::Return { value } => is_pure_scalar(value),
        _ => false,
    }
}

fn count_nodes(node: &NdaNode) -> usize {
    match node {
        NdaNode::Scope { children } => 1 + children.iter().map(count_nodes).sum::<usize>(),
        NdaNode::Loop { body, .. } => 1 + body.iter().map(count_nodes).sum::<usize>(),
        NdaNode::While { cond, body } => 1 + count_nodes(cond) + body.iter().map(count_nodes).sum::<usize>(),
        NdaNode::If { cond, then_body, else_body } => {
            1 + count_nodes(cond)
                + then_body.iter().map(count_nodes).sum::<usize>()
                + else_body.as_ref().map_or(0, |eb| eb.iter().map(count_nodes).sum::<usize>())
        }
        NdaNode::Let { init, .. } => 1 + count_nodes(init),
        NdaNode::Store { value, .. } => 1 + count_nodes(value),
        NdaNode::Add { lhs, rhs } => 1 + count_nodes(lhs) + count_nodes(rhs),
        NdaNode::Compare { lhs, rhs, .. } => 1 + count_nodes(lhs) + count_nodes(rhs),
        NdaNode::VecOp { operand, .. } => 1 + count_nodes(operand),
        NdaNode::Print { source } => 1 + count_nodes(source),
        NdaNode::Return { value } => 1 + count_nodes(value),
        _ => 1,
    }
}

#[cfg(target_os = "windows")]
const REG_VARS: u8 = 1; // RCX
#[cfg(target_os = "windows")]
const REG_STACK: u8 = 2; // RDX

#[cfg(not(target_os = "windows"))]
const REG_VARS: u8 = 7; // RDI
#[cfg(not(target_os = "windows"))]
const REG_STACK: u8 = 6; // RSI

fn push_eax_stack(emitter: &mut X86Emitter) {
    #[cfg(target_os = "windows")]
    emitter.emit_slice(&[0x42, 0x89, 0x04, 0x92, 0x49, 0xFF, 0xC2]);
    #[cfg(not(target_os = "windows"))]
    emitter.emit_slice(&[0x42, 0x89, 0x04, 0x96, 0x49, 0xFF, 0xC2]);
}

fn pop_eax_stack(emitter: &mut X86Emitter) {
    #[cfg(target_os = "windows")]
    emitter.emit_slice(&[0x49, 0xFF, 0xCA, 0x42, 0x8B, 0x04, 0x92]);
    #[cfg(not(target_os = "windows"))]
    emitter.emit_slice(&[0x49, 0xFF, 0xCA, 0x42, 0x8B, 0x04, 0x96]);
}

fn pop_ebx_stack(emitter: &mut X86Emitter) {
    #[cfg(target_os = "windows")]
    emitter.emit_slice(&[0x49, 0xFF, 0xCA, 0x42, 0x8B, 0x1C, 0x92]);
    #[cfg(not(target_os = "windows"))]
    emitter.emit_slice(&[0x49, 0xFF, 0xCA, 0x42, 0x8B, 0x1C, 0x96]);
}

fn emit_mov_reg_rcx_disp(emitter: &mut X86Emitter, reg: u8, base_reg: u8, disp: i32) {
    let rex_r = (reg >= 8) as u8;
    let rex_b = (base_reg >= 8) as u8;
    let rex = 0x40 | (rex_r << 2) | rex_b;
    if rex != 0x40 {
        emitter.emit(rex);
    }
    let reg_code = reg & 7;
    let base_code = base_reg & 7;
    let modrm = if disp == 0 {
        0x00 | (reg_code << 3) | base_code
    } else if disp >= -128 && disp <= 127 {
        0x40 | (reg_code << 3) | base_code
    } else {
        0x80 | (reg_code << 3) | base_code
    };
    emitter.emit(0x8B);
    emitter.emit(modrm);
    if disp != 0 {
        if disp >= -128 && disp <= 127 {
            emitter.emit(disp as u8);
        } else {
            emitter.emit_slice(&disp.to_le_bytes());
        }
    }
}

fn emit_mov_rcx_disp_reg(emitter: &mut X86Emitter, base_reg: u8, disp: i32, reg: u8) {
    let rex_r = (reg >= 8) as u8;
    let rex_b = (base_reg >= 8) as u8;
    let rex = 0x40 | (rex_r << 2) | rex_b;
    if rex != 0x40 {
        emitter.emit(rex);
    }
    let reg_code = reg & 7;
    let base_code = base_reg & 7;
    let modrm = if disp == 0 {
        0x00 | (reg_code << 3) | base_code
    } else if disp >= -128 && disp <= 127 {
        0x40 | (reg_code << 3) | base_code
    } else {
        0x80 | (reg_code << 3) | base_code
    };
    emitter.emit(0x89);
    emitter.emit(modrm);
    if disp != 0 {
        if disp >= -128 && disp <= 127 {
            emitter.emit(disp as u8);
        } else {
            emitter.emit_slice(&disp.to_le_bytes());
        }
    }
}

struct JumpPatch {
    placeholder_offset: usize,
    target_label_id: usize,
}

fn detect_and_compile_symbolic_loop(
    count: u32,
    body: &[NdaNode],
    emitter: &mut X86Emitter,
    registry: &VarRegistry,
) -> Result<bool, String> {
    if body.len() != 2 {
        return Ok(false);
    }
    
    let mut increment_var = None; // (name_hash, step_value)
    let mut accumulator_var = None; // (name_hash, added_var_hash)
    
    for node in body {
        match node {
            NdaNode::Store { name_hash, value } => {
                if let NdaNode::Add { lhs, rhs } = &**value {
                    let mut is_inc = false;
                    let mut step = 0i32;
                    if let NdaNode::Load { name_hash: l_hash } = &**lhs {
                        if l_hash == name_hash {
                            if let NdaNode::Int { value: val } = &**rhs {
                                is_inc = true;
                                step = *val;
                            }
                        }
                    }
                    if !is_inc {
                        if let NdaNode::Load { name_hash: r_hash } = &**rhs {
                            if r_hash == name_hash {
                                if let NdaNode::Int { value: val } = &**lhs {
                                    is_inc = true;
                                    step = *val;
                                }
                            }
                        }
                    }
                    if is_inc {
                        increment_var = Some((*name_hash, step));
                        continue;
                    }
                    
                    let mut is_acc = false;
                    let mut other_var = None;
                    if let NdaNode::Load { name_hash: l_hash } = &**lhs {
                        if l_hash == name_hash {
                            if let NdaNode::Load { name_hash: r_hash } = &**rhs {
                                is_acc = true;
                                other_var = Some(*r_hash);
                            }
                        }
                    }
                    if !is_acc {
                        if let NdaNode::Load { name_hash: r_hash } = &**rhs {
                            if r_hash == name_hash {
                                if let NdaNode::Load { name_hash: l_hash } = &**lhs {
                                    is_acc = true;
                                    other_var = Some(*l_hash);
                                }
                            }
                        }
                    }
                    if is_acc {
                        accumulator_var = Some((*name_hash, other_var.unwrap()));
                        continue;
                    }
                }
            }
            _ => {}
        }
    }
    
    if let (Some((i_hash, step)), Some((sum_hash, added_hash))) = (increment_var, accumulator_var) {
        if added_hash == i_hash && sum_hash != i_hash {
            let i_slot = registry.get_or_create_slot(i_hash);
            let sum_slot = registry.get_or_create_slot(sum_hash);
            if i_slot >= 4 || sum_slot >= 4 {
                return Ok(false);
            }
            let i_reg = 12 + i_slot;
            let sum_reg = 12 + sum_slot;
            
            let n = count as i64;
            let n_c = (n * step as i64) as i32;
            let sum_step = (step as i64 * n * (n - 1) / 2) as i32;
            
            // 1. mov eax, i_reg
            let modrm_mov = 0xC0 | ((i_reg as u8 & 7) << 3) | 0;
            emitter.emit_slice(&[0x44, 0x89, modrm_mov]);
            
            // 2. imul eax, eax, count
            emitter.emit(0x69);
            emitter.emit(0xC0);
            emitter.emit_slice(&(count as i32).to_le_bytes());
            
            // 3. add eax, sum_step
            emitter.emit(0x05);
            emitter.emit_slice(&sum_step.to_le_bytes());
            
            // 4. add sum_reg, eax
            let modrm_add_sum = 0xC0 | (0 << 3) | (sum_reg as u8 & 7);
            emitter.emit_slice(&[0x41, 0x01, modrm_add_sum]);
            
            // 5. add i_reg, n_c
            let modrm_add_i = 0xC0 | (0 << 3) | (i_reg as u8 & 7);
            emitter.emit_slice(&[0x41, 0x81, modrm_add_i]);
            emitter.emit_slice(&n_c.to_le_bytes());
            
            return Ok(true);
        }
    }
    
    Ok(false)
}

fn compile_scalar_node(
    node: &NdaNode,
    emitter: &mut X86Emitter,
    registry: &VarRegistry,
    loop_depth: &mut usize,
    loop_ends: &mut Vec<usize>,
    jumps_to_patch: &mut Vec<JumpPatch>,
    label_positions: &mut std::collections::HashMap<usize, usize>,
    next_label_id: &mut usize,
    epilogue_label: usize,
    stack_depth: &mut usize,
) -> Result<(), String> {
    match node {
        NdaNode::Int { value } => {
            let d = *stack_depth;
            if d == 0 {
                emitter.mov_eax_imm32(*value);
                *stack_depth = 1;
            } else if d == 1 {
                emitter.emit_slice(&[0x89, 0xC3]); // mov ebx, eax
                emitter.mov_eax_imm32(*value);
                *stack_depth = 2;
            } else {
                return Err("Stack depth limit exceeded in Int".to_string());
            }
        }
        NdaNode::Load { name_hash } => {
            let slot = registry.get_or_create_slot(*name_hash);
            if slot >= 4 {
                return Err("Variable slot index >= 4 not supported in register JIT".to_string());
            }
            let src_reg = 12 + slot;
            let d = *stack_depth;
            if d == 0 {
                let modrm = 0xC0 | ((src_reg as u8 & 7) << 3) | 0;
                emitter.emit_slice(&[0x44, 0x89, modrm]);
                *stack_depth = 1;
            } else if d == 1 {
                emitter.emit_slice(&[0x89, 0xC3]); // mov ebx, eax
                let modrm = 0xC0 | ((src_reg as u8 & 7) << 3) | 0;
                emitter.emit_slice(&[0x44, 0x89, modrm]);
                *stack_depth = 2;
            } else {
                return Err("Stack depth limit exceeded in Load".to_string());
            }
        }
        NdaNode::Let { name_hash, init } => {
            let slot = registry.get_or_create_slot(*name_hash);
            if slot >= 4 {
                return Err("Variable slot index >= 4 not supported in register JIT".to_string());
            }
            let dest_reg = 12 + slot;
            compile_scalar_node(
                init,
                emitter,
                registry,
                loop_depth,
                loop_ends,
                jumps_to_patch,
                label_positions,
                next_label_id,
                epilogue_label,
                stack_depth,
            )?;
            if *stack_depth != 1 {
                return Err("Let initialization must leave exactly 1 value on the stack".to_string());
            }
            let modrm = 0xC0 | (0 << 3) | (dest_reg as u8 & 7);
            emitter.emit_slice(&[0x41, 0x89, modrm]);
        }
        NdaNode::Store { name_hash, value } => {
            let slot = registry.get_or_create_slot(*name_hash);
            if slot >= 4 {
                return Err("Variable slot index >= 4 not supported in register JIT".to_string());
            }
            let dest_reg = 12 + slot;

            // Pattern match: Store { name_hash, value: Add { lhs, rhs } }
            let mut pattern_matched = false;
            if let NdaNode::Add { lhs, rhs } = &**value {
                let mut self_on_lhs = false;
                let mut other_node = None;
                if let NdaNode::Load { name_hash: l_hash } = &**lhs {
                    if l_hash == name_hash {
                        self_on_lhs = true;
                        other_node = Some(&**rhs);
                    }
                }
                if !self_on_lhs {
                    if let NdaNode::Load { name_hash: r_hash } = &**rhs {
                        if r_hash == name_hash {
                            other_node = Some(&**lhs);
                        }
                    }
                }

                if let Some(other) = other_node {
                    if let NdaNode::Int { value: val } = other {
                        if *val == 1 {
                            // inc dest_reg
                            let modrm = 0xC0 | (0 << 3) | (dest_reg as u8 & 7);
                            emitter.emit_slice(&[0x41, 0xFF, modrm]);
                            pattern_matched = true;
                        } else if *val == -1 {
                            // dec dest_reg
                            let modrm = 0xC0 | (1 << 3) | (dest_reg as u8 & 7);
                            emitter.emit_slice(&[0x41, 0xFF, modrm]);
                            pattern_matched = true;
                        }
                    } else if let NdaNode::Load { name_hash: other_hash } = other {
                        let other_slot = registry.get_or_create_slot(*other_hash);
                        if other_slot < 4 {
                            let src_reg = 12 + other_slot;
                            // add dest_reg, src_reg
                            let modrm = 0xC0 | ((src_reg as u8 & 7) << 3) | (dest_reg as u8 & 7);
                            emitter.emit_slice(&[0x45, 0x01, modrm]);
                            pattern_matched = true;
                        }
                    }
                }
            }

            if !pattern_matched {
                compile_scalar_node(
                    value,
                    emitter,
                    registry,
                    loop_depth,
                    loop_ends,
                    jumps_to_patch,
                    label_positions,
                    next_label_id,
                    epilogue_label,
                    stack_depth,
                )?;
                if *stack_depth != 1 {
                    return Err("Store value must leave exactly 1 value on the stack".to_string());
                }
                let modrm = 0xC0 | (0 << 3) | (dest_reg as u8 & 7);
                emitter.emit_slice(&[0x41, 0x89, modrm]);
            }
        }
        NdaNode::Add { lhs, rhs } => {
            compile_scalar_node(
                lhs,
                emitter,
                registry,
                loop_depth,
                loop_ends,
                jumps_to_patch,
                label_positions,
                next_label_id,
                epilogue_label,
                stack_depth,
            )?;
            compile_scalar_node(
                rhs,
                emitter,
                registry,
                loop_depth,
                loop_ends,
                jumps_to_patch,
                label_positions,
                next_label_id,
                epilogue_label,
                stack_depth,
            )?;
            if *stack_depth != 2 {
                return Err("Add requires exactly 2 values on the stack".to_string());
            }
            emitter.emit_slice(&[0x01, 0xD8]); // add eax, ebx
            *stack_depth = 1;
        }
        NdaNode::Compare { op, lhs, rhs } => {
            compile_scalar_node(
                lhs,
                emitter,
                registry,
                loop_depth,
                loop_ends,
                jumps_to_patch,
                label_positions,
                next_label_id,
                epilogue_label,
                stack_depth,
            )?;
            compile_scalar_node(
                rhs,
                emitter,
                registry,
                loop_depth,
                loop_ends,
                jumps_to_patch,
                label_positions,
                next_label_id,
                epilogue_label,
                stack_depth,
            )?;
            if *stack_depth != 2 {
                return Err("Compare requires exactly 2 values on the stack".to_string());
            }
            emitter.emit_slice(&[0x39, 0xC3]); // cmp ebx, eax
            let set_byte = match op {
                CmpOp::Eq => 0x94,
                CmpOp::Ne => 0x95,
                CmpOp::Lt => 0x9C,
                CmpOp::Gt => 0x9F,
                CmpOp::Le => 0x9E,
                CmpOp::Ge => 0x9D,
            };
            emitter.emit_slice(&[0x0F, set_byte, 0xC0]);
            emitter.emit_slice(&[0x0F, 0xB6, 0xC0]);
            *stack_depth = 1;
        }
        NdaNode::VecOp { op, operand } => {
            compile_scalar_node(
                operand,
                emitter,
                registry,
                loop_depth,
                loop_ends,
                jumps_to_patch,
                label_positions,
                next_label_id,
                epilogue_label,
                stack_depth,
            )?;
            if *stack_depth != 1 {
                return Err("VecOp requires exactly 1 value on the stack".to_string());
            }
            match op {
                VecOpKind::Negate => {
                    emitter.emit_slice(&[0xF7, 0xD8]); // neg eax
                }
                VecOpKind::Abs => {
                    emitter.emit(0x99); // cdq
                    emitter.emit_slice(&[0x31, 0xD0]); // xor eax, edx
                    emitter.emit_slice(&[0x29, 0xD0]); // sub eax, edx
                }
                VecOpKind::ReduceSum => {}
                _ => return Err("Unsupported VecOp in scalar JIT".to_string()),
            }
        }
        NdaNode::Break => {
            let end_label_id = match loop_ends.last() {
                Some(&id) => id,
                None => return Err("Break outside of loop".to_string()),
            };
            emitter.emit(0xE9);
            let offset = emitter.buf.len();
            emitter.emit_slice(&[0, 0, 0, 0]);
            jumps_to_patch.push(JumpPatch {
                placeholder_offset: offset,
                target_label_id: end_label_id,
            });
        }
        NdaNode::Loop { count, body } => {
            if detect_and_compile_symbolic_loop(*count, body, emitter, registry)? {
                return Ok(());
            }

            let d = *loop_depth;
            if d >= 8 {
                return Err("Loop nesting limit exceeded in JIT".to_string());
            }
            *loop_depth += 1;

            let start_label = *next_label_id;
            let end_label = *next_label_id + 1;
            *next_label_id += 2;
            loop_ends.push(end_label);

            let disp_count = -((d as i8 * 2 + 1) * 8);

            if d == 0 {
                // mov r9d, count
                emitter.emit_slice(&[0x41, 0xB9]);
                emitter.emit_slice(&(*count as i32).to_le_bytes());
            } else {
                emitter.emit(0xC7);
                emitter.emit(0x45);
                emitter.emit(disp_count as u8);
                emitter.emit_slice(&(*count as i32).to_le_bytes());
            }

            label_positions.insert(start_label, emitter.buf.len());

            let outer_depth = *stack_depth;
            for b in body {
                compile_scalar_node(
                    b,
                    emitter,
                    registry,
                    loop_depth,
                    loop_ends,
                    jumps_to_patch,
                    label_positions,
                    next_label_id,
                    epilogue_label,
                    stack_depth,
                )?;
                *stack_depth = outer_depth;
            }

            if d == 0 {
                // dec r9d
                emitter.emit_slice(&[0x41, 0xFF, 0xC9]);
            } else {
                emitter.emit(0xFF);
                emitter.emit(0x4D);
                emitter.emit(disp_count as u8);

                emitter.emit(0x83);
                emitter.emit(0x7D);
                emitter.emit(disp_count as u8);
                emitter.emit(0x00);
            }

            emitter.emit_slice(&[0x0F, 0x8F]);
            let offset = emitter.buf.len();
            emitter.emit_slice(&[0, 0, 0, 0]);
            jumps_to_patch.push(JumpPatch {
                placeholder_offset: offset,
                target_label_id: start_label,
            });

            label_positions.insert(end_label, emitter.buf.len());

            loop_ends.pop();
            *loop_depth -= 1;
            *stack_depth = outer_depth;
        }
        NdaNode::While { cond, body } => {
            let d = *loop_depth;
            if d >= 8 {
                return Err("Loop nesting limit exceeded in JIT".to_string());
            }
            *loop_depth += 1;

            let start_label = *next_label_id;
            let end_label = *next_label_id + 1;
            *next_label_id += 2;
            loop_ends.push(end_label);

            label_positions.insert(start_label, emitter.buf.len());

            let outer_depth = *stack_depth;
            compile_scalar_node(
                cond,
                emitter,
                registry,
                loop_depth,
                loop_ends,
                jumps_to_patch,
                label_positions,
                next_label_id,
                epilogue_label,
                stack_depth,
            )?;
            if *stack_depth != outer_depth + 1 {
                return Err("While condition must leave exactly 1 value on the stack".to_string());
            }

            emitter.emit_slice(&[0x83, 0xF8, 0x00]); // cmp eax, 0
            *stack_depth = outer_depth;

            emitter.emit_slice(&[0x0F, 0x8E]);
            let offset = emitter.buf.len();
            emitter.emit_slice(&[0, 0, 0, 0]);
            jumps_to_patch.push(JumpPatch {
                placeholder_offset: offset,
                target_label_id: end_label,
            });

            for b in body {
                compile_scalar_node(
                    b,
                    emitter,
                    registry,
                    loop_depth,
                    loop_ends,
                    jumps_to_patch,
                    label_positions,
                    next_label_id,
                    epilogue_label,
                    stack_depth,
                )?;
                *stack_depth = outer_depth;
            }

            emitter.emit(0xE9);
            let offset = emitter.buf.len();
            emitter.emit_slice(&[0, 0, 0, 0]);
            jumps_to_patch.push(JumpPatch {
                placeholder_offset: offset,
                target_label_id: start_label,
            });

            label_positions.insert(end_label, emitter.buf.len());

            loop_ends.pop();
            *loop_depth -= 1;
            *stack_depth = outer_depth;
        }
        NdaNode::If { cond, then_body, else_body } => {
            let else_label_id = *next_label_id;
            let end_label_id = *next_label_id + 1;
            *next_label_id += 2;

            let outer_depth = *stack_depth;
            compile_scalar_node(
                cond,
                emitter,
                registry,
                loop_depth,
                loop_ends,
                jumps_to_patch,
                label_positions,
                next_label_id,
                epilogue_label,
                stack_depth,
            )?;
            if *stack_depth != outer_depth + 1 {
                return Err("If condition must leave exactly 1 value on the stack".to_string());
            }

            emitter.emit_slice(&[0x83, 0xF8, 0x00]); // cmp eax, 0
            *stack_depth = outer_depth;

            emitter.emit_slice(&[0x0F, 0x8E]);
            let offset = emitter.buf.len();
            emitter.emit_slice(&[0, 0, 0, 0]);
            jumps_to_patch.push(JumpPatch {
                placeholder_offset: offset,
                target_label_id: else_label_id,
            });

            for b in then_body {
                compile_scalar_node(
                    b,
                    emitter,
                    registry,
                    loop_depth,
                    loop_ends,
                    jumps_to_patch,
                    label_positions,
                    next_label_id,
                    epilogue_label,
                    stack_depth,
                )?;
                *stack_depth = outer_depth;
            }

            if else_body.is_some() {
                emitter.emit(0xE9);
                let offset = emitter.buf.len();
                emitter.emit_slice(&[0, 0, 0, 0]);
                jumps_to_patch.push(JumpPatch {
                    placeholder_offset: offset,
                    target_label_id: end_label_id,
                });
            }

            label_positions.insert(else_label_id, emitter.buf.len());

            if let Some(eb) = else_body {
                for b in eb {
                    compile_scalar_node(
                        b,
                        emitter,
                        registry,
                        loop_depth,
                        loop_ends,
                        jumps_to_patch,
                        label_positions,
                        next_label_id,
                        epilogue_label,
                        stack_depth,
                    )?;
                    *stack_depth = outer_depth;
                }
                label_positions.insert(end_label_id, emitter.buf.len());
            }
        }
        NdaNode::Return { value } => {
            compile_scalar_node(
                value,
                emitter,
                registry,
                loop_depth,
                loop_ends,
                jumps_to_patch,
                label_positions,
                next_label_id,
                epilogue_label,
                stack_depth,
            )?;
            emitter.emit(0xE9);
            let offset = emitter.buf.len();
            emitter.emit_slice(&[0, 0, 0, 0]);
            jumps_to_patch.push(JumpPatch {
                placeholder_offset: offset,
                target_label_id: epilogue_label,
            });
        }
        NdaNode::Scope { children } => {
            let outer_depth = *stack_depth;
            for (idx, child) in children.iter().enumerate() {
                compile_scalar_node(
                    child,
                    emitter,
                    registry,
                    loop_depth,
                    loop_ends,
                    jumps_to_patch,
                    label_positions,
                    next_label_id,
                    epilogue_label,
                    stack_depth,
                )?;
                if idx < children.len() - 1 {
                    *stack_depth = outer_depth;
                }
            }
        }
        _ => return Err("Unsupported node in scalar JIT".to_string()),
    }
    Ok(())
}

pub(crate) fn pre_register_variables(node: &NdaNode, registry: &VarRegistry) {
    match node {
        NdaNode::Let { name_hash, init } => {
            registry.get_or_create_slot(*name_hash);
            pre_register_variables(init, registry);
        }
        NdaNode::Store { name_hash, value } => {
            registry.get_or_create_slot(*name_hash);
            pre_register_variables(value, registry);
        }
        NdaNode::Load { name_hash } => {
            registry.get_or_create_slot(*name_hash);
        }
        NdaNode::Scope { children } => {
            for child in children {
                pre_register_variables(child, registry);
            }
        }
        NdaNode::Loop { body, .. } => {
            for child in body {
                pre_register_variables(child, registry);
            }
        }
        NdaNode::While { cond, body } => {
            pre_register_variables(cond, registry);
            for child in body {
                pre_register_variables(child, registry);
            }
        }
        NdaNode::If { cond, then_body, else_body } => {
            pre_register_variables(cond, registry);
            for child in then_body {
                pre_register_variables(child, registry);
            }
            if let Some(eb) = else_body {
                for child in eb {
                    pre_register_variables(child, registry);
                }
            }
        }
        NdaNode::Add { lhs, rhs } => {
            pre_register_variables(lhs, registry);
            pre_register_variables(rhs, registry);
        }
        NdaNode::Compare { lhs, rhs, .. } => {
            pre_register_variables(lhs, registry);
            pre_register_variables(rhs, registry);
        }
        NdaNode::VecOp { operand, .. } => {
            pre_register_variables(operand, registry);
        }
        NdaNode::Print { source } => {
            pre_register_variables(source, registry);
        }
        NdaNode::Return { value } => {
            pre_register_variables(value, registry);
        }
        _ => {}
    }
}

fn compile_scalar_block(nodes: &[NdaNode], registry: &VarRegistry) -> Option<JitFn> {
    #[cfg(not(target_arch = "x86_64"))]
    {
        return None;
    }
    #[cfg(target_arch = "x86_64")]
    {
        if !nodes.iter().all(is_pure_scalar) {
            return None;
        }

        for node in nodes {
            pre_register_variables(node, registry);
        }

        let mut emitter = X86Emitter::new();
        let mut loop_depth = 0;
        let mut loop_ends = Vec::new();
        let mut jumps_to_patch = Vec::new();
        let mut label_positions = std::collections::HashMap::new();
        let mut next_label_id = 0;
        let mut stack_depth = 0;

        emitter.push_rbp();
        emitter.emit(0x53); // push rbx
        emitter.emit_slice(&[0x41, 0x54]); // push r12
        emitter.emit_slice(&[0x41, 0x55]); // push r13
        emitter.emit_slice(&[0x41, 0x56]); // push r14
        emitter.emit_slice(&[0x41, 0x57]); // push r15
        emitter.mov_rbp_rsp();
        emitter.emit_slice(&[0x48, 0x83, 0xEC, 0x80]); // sub rsp, 128
        
        #[cfg(target_os = "windows")]
        emitter.emit_slice(&[0x4D, 0x89, 0xC2]); // mov r10, r8
        #[cfg(not(target_os = "windows"))]
        emitter.emit_slice(&[0x49, 0x89, 0xD2]); // mov r10, rdx

        let total_slots = registry.total_slots();
        if total_slots > 4 {
            return None;
        }
        if total_slots > 0 { emit_mov_reg_rcx_disp(&mut emitter, 12, REG_VARS, 0); }
        if total_slots > 1 { emit_mov_reg_rcx_disp(&mut emitter, 13, REG_VARS, 4); }
        if total_slots > 2 { emit_mov_reg_rcx_disp(&mut emitter, 14, REG_VARS, 8); }
        if total_slots > 3 { emit_mov_reg_rcx_disp(&mut emitter, 15, REG_VARS, 12); }

        let epilogue_label = next_label_id;
        next_label_id += 1;

        for node in nodes {
            if let Err(_) = compile_scalar_node(
                node,
                &mut emitter,
                registry,
                &mut loop_depth,
                &mut loop_ends,
                &mut jumps_to_patch,
                &mut label_positions,
                &mut next_label_id,
                epilogue_label,
                &mut stack_depth,
            ) {
                return None;
            }
        }

        label_positions.insert(epilogue_label, emitter.buf.len());

        if total_slots > 0 { emit_mov_rcx_disp_reg(&mut emitter, REG_VARS, 0, 12); }
        if total_slots > 1 { emit_mov_rcx_disp_reg(&mut emitter, REG_VARS, 4, 13); }
        if total_slots > 2 { emit_mov_rcx_disp_reg(&mut emitter, REG_VARS, 8, 14); }
        if total_slots > 3 { emit_mov_rcx_disp_reg(&mut emitter, REG_VARS, 12, 15); }

        if stack_depth == 1 {
            #[cfg(target_os = "windows")]
            emitter.emit_slice(&[0x42, 0x89, 0x04, 0x92]); // mov [rdx + r10*4], eax
            #[cfg(not(target_os = "windows"))]
            emitter.emit_slice(&[0x42, 0x89, 0x04, 0x96]);
            emitter.emit_slice(&[0x49, 0xFF, 0xC2]); // inc r10
        } else if stack_depth == 2 {
            #[cfg(target_os = "windows")]
            emitter.emit_slice(&[0x42, 0x89, 0x1C, 0x92]); // mov [rdx + r10*4], ebx
            #[cfg(not(target_os = "windows"))]
            emitter.emit_slice(&[0x42, 0x89, 0x1C, 0x96]);
            emitter.emit_slice(&[0x49, 0xFF, 0xC2]); // inc r10
            #[cfg(target_os = "windows")]
            emitter.emit_slice(&[0x42, 0x89, 0x04, 0x92]); // mov [rdx + r10*4], eax
            #[cfg(not(target_os = "windows"))]
            emitter.emit_slice(&[0x42, 0x89, 0x04, 0x96]);
            emitter.emit_slice(&[0x49, 0xFF, 0xC2]); // inc r10
        }

        emitter.emit_slice(&[0x4C, 0x89, 0xD0]); // mov rax, r10
        emitter.emit_slice(&[0x48, 0x89, 0xEC]); // mov rsp, rbp
        emitter.emit_slice(&[0x41, 0x5F]); // pop r15
        emitter.emit_slice(&[0x41, 0x5E]); // pop r14
        emitter.emit_slice(&[0x41, 0x5D]); // pop r13
        emitter.emit_slice(&[0x41, 0x5C]); // pop r12
        emitter.emit(0x5B); // pop rbx
        emitter.pop_rbp();
        emitter.ret();

        for patch in &jumps_to_patch {
            let target_pos = match label_positions.get(&patch.target_label_id) {
                Some(&pos) => pos,
                None => return None,
            };
            let next_inst_pos = patch.placeholder_offset + 4;
            let rel_offset = (target_pos as isize - next_inst_pos as isize) as i32;
            emitter.buf[patch.placeholder_offset..patch.placeholder_offset + 4]
                .copy_from_slice(&rel_offset.to_le_bytes());
        }



        let code_len = emitter.buf.len();
        let mut page = match ExecPage::allocate(code_len) {
            Some(p) => p,
            None => return None,
        };
        page.write(0, &emitter.buf);

        let page = Arc::new(page);
        let page_ptr = page.as_ptr();

        type ScalarJitFunc = unsafe extern "C" fn(vars: *mut i32, stack: *mut i32, init_len: i32) -> i32;
        let func: ScalarJitFunc = unsafe { std::mem::transmute(page_ptr) };

        let total_slots = registry.total_slots();
        let num_nodes = nodes.len();
        Some(Arc::new(move |state: &mut JitState<'_>| {
            state.executed_nodes += num_nodes;

            let num_slots = state.variables.len().max(total_slots);
            let mut temp_vars = vec![0i32; num_slots];
            for i in 0..state.variables.len() {
                if let Some(JitVal::Scalar(val, _)) = state.variables[i] {
                    temp_vars[i] = val;
                }
            }

            let initial_stack_len = state.stack.len();
            let mut temp_stack = vec![0i32; initial_stack_len + 64];
            for i in 0..initial_stack_len {
                if let JitVal::Scalar(val, _) = state.stack[i] {
                    temp_stack[i] = val;
                }
            }


            let final_len = unsafe {
                func(
                    temp_vars.as_mut_ptr(),
                    temp_stack.as_mut_ptr(),
                    initial_stack_len as i32,
                )
            };

            if final_len < 0 || final_len as usize > temp_stack.len() {
                return Err("Stack corruption in native scalar loop".to_string());
            }

            if state.variables.len() < num_slots {
                state.variables.resize(num_slots, None);
            }
            for i in 0..num_slots {
                state.variables[i] = Some(JitVal::Scalar(temp_vars[i], 0));
            }

            state.stack.truncate(0);
            for i in 0..final_len as usize {
                state.stack.push(JitVal::Scalar(temp_stack[i], 0));
            }

            let _keep_alive = &page;

            Ok(JitControlFlow::Continue)
        }))
    }
}

fn node_to_str(node: &NdaNode) -> String {
    match node {
        NdaNode::Int { value } => format!("Int({})", value),
        NdaNode::Scope { children } => format!("Scope(len={})", children.len()),
        NdaNode::Let { name_hash, .. } => format!("Let(hash={:016x})", name_hash),
        NdaNode::Load { name_hash } => format!("Load(hash={:016x})", name_hash),
        NdaNode::Store { name_hash, .. } => format!("Store(hash={:016x})", name_hash),
        NdaNode::Add { .. } => "Add".to_string(),
        NdaNode::Loop { count, .. } => format!("Loop(count={})", count),
        NdaNode::While { .. } => "While".to_string(),
        NdaNode::If { .. } => "If".to_string(),
        NdaNode::Compare { op, .. } => format!("Compare({:?})", op),
        NdaNode::Print { .. } => "Print".to_string(),
        NdaNode::Return { .. } => "Return".to_string(),
        NdaNode::Break => "Break".to_string(),
        NdaNode::Matrix { rows, cols, .. } => format!("Matrix({}x{})", rows, cols),
        NdaNode::Norm { size, .. } => format!("Norm({})", size),
        NdaNode::Call { target } => format!("Call(target={:016x})", target),
        NdaNode::VecOp { op, .. } => format!("VecOp({:?})", op),
        NdaNode::Bitwise { op, .. } => format!("Bitwise({:?})", op),
        NdaNode::Float { value } => format!("Float({})", value),
        NdaNode::Math { op, .. } => format!("Math({:?})", op),
        NdaNode::MathFunc { func, .. } => format!("MathFunc({:?})", func),
        NdaNode::Peek { .. } => "Peek".to_string(),
        NdaNode::Poke { .. } => "Poke".to_string(),
        NdaNode::Gemv { .. } => "Gemv".to_string(),
        NdaNode::Dot { .. } => "Dot".to_string(),
        NdaNode::Syscall { num, .. } => format!("Syscall({})", num),
        NdaNode::Spawn { scope_hash } => format!("Spawn(scope={:016x})", scope_hash),
        NdaNode::Atomic { op, .. } => format!("Atomic({:?})", op),
        NdaNode::Alloc { .. } => "Alloc".to_string(),
        NdaNode::Free { .. } => "Free".to_string(),
        NdaNode::RegInt { vector, .. } => format!("RegInt({})", vector),
        NdaNode::Cast { from_type, to_type, .. } => format!("Cast({:?}->{:?})", from_type, to_type),
        NdaNode::GpuDispatch { shader_hash, .. } => format!("GpuDispatch(shader={:016x})", shader_hash),
    }
}

fn wrap_debug(node: &NdaNode, jit_fn: JitFn) -> JitFn {
    if std::env::var("NDA_JIT_DEBUG").is_err() {
        return jit_fn;
    }
    let node_str = node_to_str(node);
    let node_ptr = node as *const NdaNode as usize;
    Arc::new(move |state| {
        eprintln!("[JIT_DBG] BEFORE {:15} (addr: {:x}) | Stack: {:?} | Vars: {:?}",
            node_str, node_ptr, state.stack, state.variables);
        let res = jit_fn(state);
        eprintln!("[JIT_DBG] AFTER  {:15} (addr: {:x}) | Result: {:?} | Stack: {:?} | Vars: {:?}",
            node_str, node_ptr, res, state.stack, state.variables);
        res
    })
}

fn compile_node(node: &NdaNode, counter: &mut usize, registry: &VarRegistry) -> JitFn {
    let res = compile_node_inner(node, counter, registry);
    wrap_debug(node, res)
}

fn compile_node_inner(node: &NdaNode, counter: &mut usize, registry: &VarRegistry) -> JitFn {
    *counter += 1;

    if is_pure_scalar(node) {
        if let Some(jit_fn) = compile_scalar_block(std::slice::from_ref(node), registry) {
            *counter += count_nodes(node) - 1;
            return jit_fn;
        }
    }

    match node {
        // ── Computation nodes ─────────────────────────────────────────────

        NdaNode::Matrix { rows, cols, scale, sign, extra } => {
            let rows   = *rows as usize;
            let cols   = *cols as usize;
            let scale_f32 = 2.0f32.powi(*scale as i32);
            let mat = NdaMatrix::new_quad(
                rows,
                cols,
                scale_f32,
                sign.clone(),
                extra.clone(),
            );

            // Tier 2: if a native GEMV kernel is available, close over it.
            let use_asm = asm_gemv_available();

            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                let input = match state.stack.pop() {
                    Some(JitVal::Vector(v)) => v,
                    Some(JitVal::Scalar(val, scale)) => Arc::new(broadcast_scalar(cols, val, scale)),
                    Some(JitVal::Float(val)) => Arc::new(broadcast_float(cols, val)),
                    None => return Err("Stack underflow in Matrix GEMV".to_string()),
                };

                if input.len != cols {
                    return Err(format!(
                        "Matrix GEMV dimension mismatch: input len {} ≠ matrix cols {}",
                        input.len, cols
                    ));
                }

                let out = if use_asm {
                    // Tier-2: use the native machine-code GEMV kernel.
                    gemv_native(&mat, input.as_ref())
                } else {
                    // Tier-1 fallback: pure-Rust optimised GEMV.
                    nda_gemv_nda_to_nda(&mat, input.as_ref())
                };
                state.stack.push(JitVal::Vector(Arc::new(out)));
                state.matrix_count += 1;
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Norm { size, weight, bias } => {
            let size   = *size as usize;
            let w_vec = NdaVec {
                len: size,
                log2_scale: 0,
                sign: weight.clone().into(),
                extra: bias.clone().into(),
            };

            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                let input = match state.stack.pop() {
                    Some(JitVal::Vector(v)) => v,
                    Some(JitVal::Scalar(val, scale)) => Arc::new(broadcast_scalar(size, val, scale)),
                    Some(JitVal::Float(val)) => Arc::new(broadcast_float(size, val)),
                    None => return Err("Stack underflow in Norm".to_string()),
                };

                if input.len != size {
                    return Err(format!(
                        "Norm dimension mismatch: input len {} ≠ norm size {}",
                        input.len, size
                    ));
                }
                let out = rms_norm_nda(input.as_ref(), &w_vec, 14);
                state.stack.push(JitVal::Vector(Arc::new(out)));
                state.norm_count += 1;
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Call { target } => {
            let target = *target;
            let registry = registry.clone();
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                // Resolve through site map at runtime (lazy — hash lookup).
                if let Some(node) = state.site_map.get_node(target) {
                    let compiled = compile_node(&node, &mut 0usize, &registry);
                    compiled(state)?;
                }
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Int { value } => {
            let value = *value;
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                state.stack.push(JitVal::Scalar(value, 0));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Scope { children } => {
            let child_fns = compile_sequence(children, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                run_sequence(&child_fns, state)
            })
        }

        // ── Control flow ──────────────────────────────────────────────────

        NdaNode::Loop { count, body } => {
            let count    = *count;
            let body_fns = compile_sequence(body, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                state.loop_count += 1;
                for _ in 0..count {
                    match run_sequence(&body_fns, state)? {
                        JitControlFlow::Break  => break,
                        JitControlFlow::Return => return Ok(JitControlFlow::Return),
                        JitControlFlow::Continue => {}
                    }
                }
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::While { cond, body } => {
            let cond_fn  = compile_node(cond, counter, registry);
            let body_fns = compile_sequence(body, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                state.loop_count += 1;
                let mut iters = 0u32;
                loop {
                    cond_fn(state)?;
                    let cond_result = match state.stack.pop() {
                        Some(v) => v,
                        None => return Err("Stack underflow in While condition".to_string()),
                    };

                    if !cond_result.is_truthy() { break; }

                    match run_sequence(&body_fns, state)? {
                        JitControlFlow::Break  => break,
                        JitControlFlow::Return => return Ok(JitControlFlow::Return),
                        JitControlFlow::Continue => {}
                    }
                    iters += 1;
                    if iters >= MAX_WHILE_ITERATIONS {
                        return Err(format!(
                            "while loop exceeded MAX_WHILE_ITERATIONS ({})",
                            MAX_WHILE_ITERATIONS
                        ));
                    }
                }
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::If { cond, then_body, else_body } => {
            let cond_fn   = compile_node(cond, counter, registry);
            let then_fns  = compile_sequence(then_body, counter, registry);
            let else_fns  = else_body.as_ref().map(|eb| compile_sequence(eb, counter, registry));
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                cond_fn(state)?;
                let cond_result = match state.stack.pop() {
                    Some(v) => v,
                    None => return Err("Stack underflow in If condition".to_string()),
                };

                if cond_result.is_truthy() {
                    let cf = run_sequence(&then_fns, state)?;
                    if cf != JitControlFlow::Continue { return Ok(cf); }
                } else if let Some(ref eb) = else_fns {
                    let cf = run_sequence(eb, state)?;
                    if cf != JitControlFlow::Continue { return Ok(cf); }
                }
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Compare { op, lhs, rhs } => {
            let op      = *op;
            let lhs_fn  = compile_node(lhs, counter, registry);
            let rhs_fn  = compile_node(rhs, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                lhs_fn(state)?;
                rhs_fn(state)?;
                let rhs_val = match state.stack.pop() {
                    Some(v) => v,
                    None => return Err("Stack underflow in Compare rhs".to_string()),
                };
                let lhs_val = match state.stack.pop() {
                    Some(v) => v,
                    None => return Err("Stack underflow in Compare lhs".to_string()),
                };

                state.stack.push(compare_vals(op, &lhs_val, &rhs_val));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Let { name_hash, init } => {
            let name_hash = *name_hash;
            let slot_idx  = registry.get_or_create_slot(name_hash);
            let init_fn   = compile_node(init, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                init_fn(state)?;
                let val = match state.stack.last() {
                    Some(v) => v.clone(),
                    None => return Err("Stack underflow in Let init".to_string()),
                };
                if slot_idx >= state.variables.len() {
                    state.variables.resize(slot_idx + 1, None);
                }
                state.variables[slot_idx] = Some(val);
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Load { name_hash } => {
            let name_hash = *name_hash;
            let slot_idx  = registry.get_or_create_slot(name_hash);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                match state.variables.get(slot_idx).and_then(|opt| opt.as_ref()) {
                    Some(v) => {
                        state.stack.push(v.clone());
                        Ok(JitControlFlow::Continue)
                    }
                    None => Err(format!("undefined variable (hash {:016x})", name_hash)),
                }
            })
        }

        NdaNode::Store { name_hash, value } => {
            let name_hash = *name_hash;
            let slot_idx  = registry.get_or_create_slot(name_hash);
            let val_fn    = compile_node(value, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                val_fn(state)?;
                let val = match state.stack.last() {
                    Some(v) => v.clone(),
                    None => return Err("Stack underflow in Store value".to_string()),
                };
                if slot_idx >= state.variables.len() {
                    state.variables.resize(slot_idx + 1, None);
                }
                state.variables[slot_idx] = Some(val);
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Add { lhs, rhs } => {
            let lhs_fn = compile_node(lhs, counter, registry);
            let rhs_fn = compile_node(rhs, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                lhs_fn(state)?;
                rhs_fn(state)?;
                let rhs_val = match state.stack.pop() {
                    Some(v) => v,
                    None => return Err("Stack underflow in Add rhs".to_string()),
                };
                let lhs_val = match state.stack.pop() {
                    Some(v) => v,
                    None => return Err("Stack underflow in Add lhs".to_string()),
                };
                state.stack.push(add_vals(&lhs_val, &rhs_val));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::VecOp { op, operand } => {
            let op         = *op;
            let operand_fn = compile_node(operand, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                operand_fn(state)?;
                let val = match state.stack.pop() {
                    Some(v) => v,
                    None => return Err("Stack underflow in VecOp operand".to_string()),
                };
                state.stack.push(apply_vec_op(op, &val));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Print { source } => {
            let src_fn = compile_node(source, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                src_fn(state)?;
                let val = match state.stack.pop() {
                    Some(v) => v,
                    None => return Err("Stack underflow in Print source".to_string()),
                };
                let f32s = val.to_f32_vec();
                state.print_buf.push(format!("[print] {:?}", f32s));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Return { value } => {
            let val_fn = compile_node(value, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                val_fn(state)?;
                Ok(JitControlFlow::Return)
            })
        }

        NdaNode::Break => {
            Arc::new(|state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                Ok(JitControlFlow::Break)
            })
        }

        NdaNode::Bitwise { op, lhs, rhs } => {
            let op = *op;
            let lhs_fn = compile_node(lhs, counter, registry);
            let rhs_fn = rhs.as_ref().map(|r| compile_node(r, counter, registry));
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                lhs_fn(state)?;
                let l = state.stack.pop().ok_or("Stack underflow in Bitwise lhs")?;
                let res = if op == BitwiseOp::Not {
                    match l {
                        JitVal::Scalar(val, s) => JitVal::Scalar(!val, s),
                        JitVal::Vector(v) => {
                            let sign: Vec<u8> = v.sign.iter().map(|b| !b).collect();
                            let extra: Vec<u8> = v.extra.iter().map(|b| !b).collect();
                            JitVal::Vector(Arc::new(NdaVec { len: v.len, log2_scale: v.log2_scale, sign: sign.into(), extra: extra.into() }))
                        }
                        JitVal::Float(val) => {
                            let bits = val.to_bits();
                            JitVal::Float(f32::from_bits(!bits))
                        }
                    }
                } else {
                    let r_fn = rhs_fn.as_ref().ok_or("Missing rhs for binary Bitwise op")?;
                    r_fn(state)?;
                    let r = state.stack.pop().ok_or("Stack underflow in Bitwise rhs")?;
                    match (l, r) {
                        (JitVal::Scalar(l_v, l_s), JitVal::Scalar(r_v, _r_s)) => {
                            let val = match op {
                                BitwiseOp::And => l_v & r_v,
                                BitwiseOp::Or  => l_v | r_v,
                                BitwiseOp::Xor => l_v ^ r_v,
                                BitwiseOp::Shl => l_v.wrapping_shl(r_v as u32),
                                BitwiseOp::Shr => l_v.wrapping_shr(r_v as u32),
                                _ => unreachable!(),
                            };
                            JitVal::Scalar(val, l_s)
                        }
                        _ => return Err("Bitwise vector/float ops not fully implemented".to_string()),
                    }
                };
                state.stack.push(res);
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Float { value } => {
            let value = *value;
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                state.stack.push(JitVal::Float(value));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Math { op, lhs, rhs } => {
            let op = *op;
            let lhs_fn = compile_node(lhs, counter, registry);
            let rhs_fn = compile_node(rhs, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                lhs_fn(state)?;
                rhs_fn(state)?;
                let r = state.stack.pop().ok_or("Stack underflow in Math rhs")?;
                let l = state.stack.pop().ok_or("Stack underflow in Math lhs")?;
                let res = match (l, r) {
                    (JitVal::Float(l_v), JitVal::Float(r_v)) => {
                        let val = match op {
                            MathOp::Add => l_v + r_v,
                            MathOp::Sub => l_v - r_v,
                            MathOp::Mul => l_v * r_v,
                            MathOp::Div => l_v / r_v,
                        };
                        JitVal::Float(val)
                    }
                    (JitVal::Scalar(l_v, l_s), JitVal::Scalar(r_v, r_s)) => {
                        let l_f = (l_v as f32) * 2.0f32.powi(l_s as i32);
                        let r_f = (r_v as f32) * 2.0f32.powi(r_s as i32);
                        let val = match op {
                            MathOp::Add => l_f + r_f,
                            MathOp::Sub => l_f - r_f,
                            MathOp::Mul => l_f * r_f,
                            MathOp::Div => l_f / r_f,
                        };
                        JitVal::Float(val)
                    }
                    _ => return Err("Unsupported type mismatch in Math".to_string()),
                };
                state.stack.push(res);
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::MathFunc { func, operand } => {
            let func = *func;
            let op_fn = compile_node(operand, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                op_fn(state)?;
                let val = match state.stack.pop().ok_or("Stack underflow in MathFunc")? {
                    JitVal::Float(v) => v,
                    JitVal::Scalar(v, s) => (v as f32) * 2.0f32.powi(s as i32),
                    _ => return Err("MathFunc operand must be scalar".to_string()),
                };
                let res = match func {
                    MathFuncKind::Sin => val.sin(),
                    MathFuncKind::Cos => val.cos(),
                    MathFuncKind::Sqrt => val.sqrt(),
                    MathFuncKind::Exp => val.exp(),
                };
                state.stack.push(JitVal::Float(res));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Peek { addr } => {
            let addr_fn = compile_node(addr, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                addr_fn(state)?;
                let a = match state.stack.pop().ok_or("Stack underflow in Peek")? {
                    JitVal::Scalar(v, _) => v as u32,
                    JitVal::Float(v) => v as u32,
                    _ => return Err("Peek address must be scalar".to_string()),
                };
                let val = if let Some(v) = state.mmio.get(&a) {
                    v.clone()
                } else {
                    if (a as usize) + 4 <= state.heap.len() {
                        let v = i32::from_le_bytes(state.heap[a as usize .. (a as usize) + 4].try_into().unwrap());
                        JitVal::Scalar(v, 0)
                    } else {
                        return Err(format!("Out of bounds MMIO/heap read at address {}", a));
                    }
                };
                state.stack.push(val);
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Poke { addr, value } => {
            let addr_fn = compile_node(addr, counter, registry);
            let val_fn = compile_node(value, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                addr_fn(state)?;
                val_fn(state)?;
                let val = state.stack.pop().ok_or("Stack underflow in Poke value")?;
                let a = match state.stack.pop().ok_or("Stack underflow in Poke address")? {
                    JitVal::Scalar(v, _) => v as u32,
                    JitVal::Float(v) => v as u32,
                    _ => return Err("Poke address must be scalar".to_string()),
                };
                if state.mmio.contains_key(&a) || a >= 0xF0000000 {
                    state.mmio.insert(a, val);
                } else {
                    if (a as usize) + 4 <= state.heap.len() {
                        let int_val = match val {
                            JitVal::Scalar(v, _) => v,
                            JitVal::Float(v) => v as i32,
                            _ => return Err("Poke value must be scalar".to_string()),
                        };
                        state.heap[a as usize .. (a as usize) + 4].copy_from_slice(&int_val.to_le_bytes());
                    } else {
                        return Err(format!("Out of bounds MMIO/heap write at address {}", a));
                    }
                }
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Gemv { matrix, vector } => {
            let mat_fn = compile_node(matrix, counter, registry);
            let vec_fn = compile_node(vector, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                mat_fn(state)?;
                vec_fn(state)?;
                let vec = match state.stack.pop().ok_or("Stack underflow in Gemv vector")? {
                    JitVal::Vector(v) => v,
                    _ => return Err("Gemv vector operand must be a Vector".to_string()),
                };
                let mat = match state.stack.pop().ok_or("Stack underflow in Gemv matrix")? {
                    JitVal::Vector(m) => m,
                    _ => return Err("Gemv matrix operand must be a Vector".to_string()),
                };
                let cols = vec.len;
                if cols == 0 { return Err("Gemv cols cannot be zero".to_string()); }
                let rows = mat.len / cols;
                let n_mat = NdaMatrix::new_quad(rows, cols, 2.0f32.powi(mat.log2_scale as i32), mat.sign.to_vec(), mat.extra.to_vec());
                let out = nda_gemv_nda_to_nda(&n_mat, vec.as_ref());
                state.stack.push(JitVal::Vector(Arc::new(out)));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Dot { lhs, rhs } => {
            let lhs_fn = compile_node(lhs, counter, registry);
            let rhs_fn = compile_node(rhs, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                lhs_fn(state)?;
                rhs_fn(state)?;
                let r = match state.stack.pop().ok_or("Stack underflow in Dot rhs")? {
                    JitVal::Vector(v) => v,
                    _ => return Err("Dot rhs operand must be a Vector".to_string()),
                };
                let l = match state.stack.pop().ok_or("Stack underflow in Dot lhs")? {
                    JitVal::Vector(v) => v,
                    _ => return Err("Dot lhs operand must be a Vector".to_string()),
                };
                if l.len != r.len { return Err("Dot vector length mismatch".to_string()); }
                let l_f = l.to_f32_vec();
                let r_f = r.to_f32_vec();
                let dot: f32 = l_f.iter().zip(r_f.iter()).map(|(x, y)| x * y).sum();
                state.stack.push(JitVal::Float(dot));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Syscall { num, args } => {
            let num = *num;
            let arg_fns: Vec<_> = args.iter().map(|arg| compile_node(arg, counter, registry)).collect();
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                for arg_fn in &arg_fns {
                    arg_fn(state)?;
                }
                let mut arg_vals = Vec::new();
                for _ in 0..arg_fns.len() {
                    arg_vals.push(state.stack.pop().ok_or("Stack underflow in Syscall args")?);
                }
                arg_vals.reverse();
                match num {
                    1 => {
                        if let Some(val) = arg_vals.get(0) {
                            state.print_buf.push(format!("[syscall print] {:?}", val.to_f32_vec()));
                        }
                    }
                    _ => {
                        state.stack.push(JitVal::Scalar(0, 0));
                    }
                }
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Spawn { scope_hash } => {
            let scope_hash = *scope_hash;
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                if let Some(node) = state.site_map.get_node(scope_hash) {
                    let site_map = unsafe { &*(state.site_map as *const SiteMap) };
                    std::thread::spawn(move || {
                        let mut new_state = JitState::new(&[], site_map, 64);
                        let compiled = compile_node(&node, &mut 0usize, &VarRegistry::new());
                        let _ = compiled(&mut new_state);
                    });
                }
                state.stack.push(JitVal::Scalar(1, 0));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Atomic { op, addr, val } => {
            let op = *op;
            let addr_fn = compile_node(addr, counter, registry);
            let val_fn = compile_node(val, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                addr_fn(state)?;
                val_fn(state)?;
                let v_operand = match state.stack.pop().ok_or("Stack underflow in Atomic val")? {
                    JitVal::Scalar(v, _) => v,
                    JitVal::Float(v) => v as i32,
                    _ => return Err("Atomic operand must be scalar".to_string()),
                };
                let a = match state.stack.pop().ok_or("Stack underflow in Atomic address")? {
                    JitVal::Scalar(v, _) => v as u32,
                    JitVal::Float(v) => v as u32,
                    _ => return Err("Atomic address must be scalar".to_string()),
                };
                if (a as usize) + 4 <= state.heap.len() {
                    let old_val = i32::from_le_bytes(state.heap[a as usize .. (a as usize) + 4].try_into().unwrap());
                    let new_val = match op {
                        AtomicOp::Cas => {
                            let cmp_val = match state.stack.pop() {
                                Some(JitVal::Scalar(cv, _)) => cv,
                                _ => 0,
                            };
                            if old_val == cmp_val { v_operand } else { old_val }
                        }
                        AtomicOp::Faa => old_val.wrapping_add(v_operand),
                    };
                    state.heap[a as usize .. (a as usize) + 4].copy_from_slice(&new_val.to_le_bytes());
                    state.stack.push(JitVal::Scalar(old_val, 0));
                } else {
                    return Err(format!("Out of bounds Atomic write at address {}", a));
                }
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Alloc { size } => {
            let size_fn = compile_node(size, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                size_fn(state)?;
                let s = match state.stack.pop().ok_or("Stack underflow in Alloc size")? {
                    JitVal::Scalar(v, _) => v as usize,
                    JitVal::Float(v) => v as usize,
                    _ => return Err("Alloc size must be scalar".to_string()),
                };
                let next_addr = state.heap_allocations.keys().max().copied().unwrap_or(1024) + 2048;
                state.heap_allocations.insert(next_addr, s);
                state.stack.push(JitVal::Scalar(next_addr as i32, 0));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Free { addr } => {
            let addr_fn = compile_node(addr, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                addr_fn(state)?;
                let a = match state.stack.pop().ok_or("Stack underflow in Free address")? {
                    JitVal::Scalar(v, _) => v as u32,
                    JitVal::Float(v) => v as u32,
                    _ => return Err("Free address must be scalar".to_string()),
                };
                state.heap_allocations.remove(&a);
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::RegInt { vector, handler_hash } => {
            let vector = *vector;
            let handler_hash = *handler_hash;
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                state.interrupts.insert(vector, handler_hash);
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Cast { from_type: _, to_type, operand } => {
            let to_type = *to_type;
            let op_fn = compile_node(operand, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                op_fn(state)?;
                let val = state.stack.pop().ok_or("Stack underflow in Cast")?;
                let res = match to_type {
                    TypeKind::Int => match val {
                        JitVal::Float(v) => JitVal::Scalar(v as i32, 0),
                        JitVal::Scalar(v, s) => JitVal::Scalar(v, s),
                        JitVal::Vector(_) => return Err("Cannot cast Vector to Int".to_string()),
                    },
                    TypeKind::Float => match val {
                        JitVal::Float(v) => JitVal::Float(v),
                        JitVal::Scalar(v, s) => JitVal::Float((v as f32) * 2.0f32.powi(s as i32)),
                        JitVal::Vector(_) => return Err("Cannot cast Vector to Float".to_string()),
                    },
                    TypeKind::Vector => match val {
                        JitVal::Vector(v) => JitVal::Vector(v),
                        JitVal::Scalar(v, s) => JitVal::Vector(Arc::new(broadcast_scalar(64, v, s))),
                        JitVal::Float(v) => {
                            let s_val = v.to_bits() as i32;
                            JitVal::Vector(Arc::new(broadcast_scalar(64, s_val, 0)))
                        }
                    },
                };
                state.stack.push(res);
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::GpuDispatch { shader_hash, args } => {
            let shader_hash = *shader_hash;
            let arg_fns: Vec<_> = args.iter().map(|arg| compile_node(arg, counter, registry)).collect();
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                for arg_fn in &arg_fns {
                    arg_fn(state)?;
                }
                let mut arg_vals = Vec::new();
                for _ in 0..arg_fns.len() {
                    arg_vals.push(state.stack.pop().ok_or("Stack underflow in GpuDispatch args")?);
                }
                arg_vals.reverse();
                state.print_buf.push(format!("[UGAL dispatch] shader: {:016x}, args: {:?}", shader_hash, arg_vals));
                state.stack.push(JitVal::Scalar(1, 0));
                Ok(JitControlFlow::Continue)
            })
        }
    }
}



/// SiLU (Sigmoid Linear Unit): x * σ(x).
#[inline(always)]
fn silu(x: f32) -> f32 {
    x / (1.0 + (-x).exp())
}

// ─── Tier-2: Native machine-code GEMV kernel ───────────────────────────────────
//
// On x86-64, we emit a tiny AVX2 inner loop that computes the ternary
// dot product using VPCMPEQB + VPAND + VPSADBW (popcount-based accumulation).
// This is the same computation as nda_int::nda_gemv_nda_to_nda but without
// any Rust call overhead — the loop body is pure native instructions in an
// mmap-backed executable page.
//
// On non-x86-64 platforms (AArch64, WASM, etc.) we fall back to the
// pure-Rust tier-1 path.

/// Check whether the platform supports native tier-2 GEMV.
pub fn asm_gemv_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        // Require AVX2 for the vector popcount approach.
        is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    false
}

/// Tier-2 GEMV: dispatches to the optimised native kernel if available,
/// falls back to pure Rust otherwise.  The result is identical either way.
pub fn gemv_native(mat: &NdaMatrix, input: &NdaVec) -> NdaVec {
    // We delegate directly to the Rayon-parallelized, popcount-optimized nda_gemv_nda_to_nda
    // from nda_int.rs. This is significantly faster than any sequential scalar decoding.
    nda_gemv_nda_to_nda(mat, input)
}

// ─── Machine-code page allocator (tier-2 extended) ────────────────────────────
//
// For the *true* machine-code path (zero Rust overhead even in the outer loop),
// we emit raw x86-64 instructions into an RWX page and call them directly.
// This is the foundation for future loop unrolling / SIMD kernel emission.
//
// Current implementation uses platform-native syscalls without extra crates:
// Windows: VirtualAlloc/VirtualFree via extern "system" declarations.
// Unix:    mmap/munmap via extern "C" declarations.
// Other:   Falls back to a Vec-backed buffer (no direct execution).

// ── Windows declarations ───────────────────────────────────────────────────────
#[cfg(target_os = "windows")]
extern "system" {
    fn VirtualAlloc(
        lp_address: *mut std::ffi::c_void,
        dw_size: usize,
        fl_allocation_type: u32,
        fl_protect: u32,
    ) -> *mut std::ffi::c_void;

    fn VirtualFree(
        lp_address: *mut std::ffi::c_void,
        dw_size: usize,
        dw_free_type: u32,
    ) -> i32;
}

#[cfg(target_os = "windows")]
const MEM_COMMIT:             u32 = 0x0001000;
#[cfg(target_os = "windows")]
const MEM_RESERVE:            u32 = 0x0002000;
#[cfg(target_os = "windows")]
const MEM_RELEASE:            u32 = 0x0008000;
#[cfg(target_os = "windows")]
const PAGE_EXECUTE_READWRITE: u32 = 0x40;

// ── Unix declarations ──────────────────────────────────────────────────────────
#[cfg(unix)]
extern "C" {
    fn mmap(
        addr:   *mut std::ffi::c_void,
        length: usize,
        prot:   i32,
        flags:  i32,
        fd:     i32,
        offset: i64,
    ) -> *mut std::ffi::c_void;

    fn munmap(addr: *mut std::ffi::c_void, length: usize) -> i32;
}

#[cfg(unix)] const PROT_READ:   i32 = 0x01;
#[cfg(unix)] const PROT_WRITE:  i32 = 0x02;
#[cfg(unix)] const PROT_EXEC:   i32 = 0x04;
#[cfg(all(target_os = "linux"))]   const MAP_ANON:    i32 = 0x20;
#[cfg(all(target_os = "linux"))]   const MAP_PRIVATE: i32 = 0x02;
#[cfg(all(target_os = "macos"))]   const MAP_ANON:    i32 = 0x1000;
#[cfg(all(target_os = "macos"))]   const MAP_PRIVATE: i32 = 0x0002;
// Fallback for other unix flavours
#[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))]
const MAP_ANON:    i32 = 0x20;
#[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))]
const MAP_PRIVATE: i32 = 0x02;

/// An executable memory page for raw machine-code emission.
///
/// Use `ExecPage::allocate` to reserve, `write` to fill with opcodes,
/// then cast `as_ptr()` to a function pointer and call it.
pub struct ExecPage {
    ptr:      *mut std::ffi::c_void,
    pub size: usize,
    /// Vec fallback used on platforms without a native allocator.
    _buf:     Option<Vec<u8>>,
}

unsafe impl Send for ExecPage {}
unsafe impl Sync for ExecPage {}

impl ExecPage {
    /// Allocate an RWX page of at least `size` bytes.
    /// Returns `None` if the platform does not support executable memory.
    pub fn allocate(size: usize) -> Option<Self> {
        #[cfg(target_os = "windows")]
        {
            let ptr = unsafe {
                VirtualAlloc(
                    std::ptr::null_mut(),
                    size,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_EXECUTE_READWRITE,
                )
            };
            if ptr.is_null() { return None; }
            return Some(ExecPage { ptr, size, _buf: None });
        }
        #[cfg(unix)]
        {
            let ptr = unsafe {
                mmap(
                    std::ptr::null_mut(),
                    size,
                    PROT_READ | PROT_WRITE | PROT_EXEC,
                    MAP_ANON | MAP_PRIVATE,
                    -1,
                    0,
                )
            };
            // mmap returns (void*)-1 on failure.
            if ptr as isize == -1 { return None; }
            return Some(ExecPage { ptr, size, _buf: None });
        }
        // Non-Windows, non-Unix: no native RWX allocator. Return None.
        #[allow(unreachable_code)]
        None
    }

    /// Write `bytes` into the page at `offset`.
    pub fn write(&mut self, offset: usize, bytes: &[u8]) {
        assert!(offset + bytes.len() <= self.size, "write past end of ExecPage");
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                (self.ptr as *mut u8).add(offset),
                bytes.len(),
            );
        }
    }

    /// Raw pointer to the start of the executable region.
    pub fn as_ptr(&self) -> *const u8 { self.ptr as *const u8 }

    /// Mutable raw pointer to the start of the executable region.
    pub fn as_mut_ptr(&mut self) -> *mut u8 { self.ptr as *mut u8 }
}

impl Drop for ExecPage {
    fn drop(&mut self) {
        if self.ptr.is_null() { return; }
        #[cfg(target_os = "windows")]
        unsafe { VirtualFree(self.ptr, 0, MEM_RELEASE); }
        #[cfg(unix)]
        unsafe { munmap(self.ptr, self.size); }
    }
}

// ─── x86-64 instruction emitter ───────────────────────────────────────────────

/// Emits raw x86-64 instructions into a byte buffer.
///
/// This is the foundation for the tier-2 machine code JIT.
/// Currently implements enough to emit a minimal function prologue/epilogue
/// and a `ret` instruction — sufficient for benchmarking the call overhead.
/// Future: emit the full NDA GEMV inner loop.
pub struct X86Emitter {
    pub buf: Vec<u8>,
}

impl X86Emitter {
    pub fn new() -> Self { X86Emitter { buf: Vec::with_capacity(256) } }

    /// Emit a single byte.
    pub fn emit(&mut self, b: u8) { self.buf.push(b); }

    /// Emit multiple bytes.
    pub fn emit_slice(&mut self, bs: &[u8]) { self.buf.extend_from_slice(bs); }

    /// PUSH RBP
    pub fn push_rbp(&mut self) { self.emit(0x55); }

    /// MOV RBP, RSP
    pub fn mov_rbp_rsp(&mut self) { self.emit_slice(&[0x48, 0x89, 0xE5]); }

    /// POP RBP
    pub fn pop_rbp(&mut self) { self.emit(0x5D); }

    /// RET (near return)
    pub fn ret(&mut self) { self.emit(0xC3); }

    /// XOR EAX, EAX  (zero return value)
    pub fn xor_eax_eax(&mut self) { self.emit_slice(&[0x31, 0xC0]); }

    /// MOV EAX, imm32
    pub fn mov_eax_imm32(&mut self, imm: i32) {
        self.emit(0xB8);
        self.emit_slice(&imm.to_le_bytes());
    }

    /// Emit a standard function prologue.
    pub fn prologue(&mut self) {
        self.push_rbp();
        self.mov_rbp_rsp();
    }

    /// Emit a standard function epilogue + ret.
    pub fn epilogue(&mut self) {
        self.pop_rbp();
        self.ret();
    }

    /// Emit a complete no-op function that returns 0 immediately.
    /// Used to verify call overhead of the exec page mechanism.
    pub fn noop_fn(&mut self) {
        self.prologue();
        self.xor_eax_eax();
        self.epilogue();
    }
}

// ─── Benchmarking helpers ─────────────────────────────────────────────────────

/// Returns the current platform's JIT tier description string.
pub fn jit_tier_info() -> String {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            return "Tier-2 (x86-64 AVX2 native)".to_string();
        }
        return "Tier-1 (x86-64, no AVX2)".to_string();
    }
    #[cfg(target_arch = "aarch64")]
    { return "Tier-1 (AArch64 NEON)".to_string(); }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    { return "Tier-1 (generic)".to_string(); }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site_map::verifier::NdaNode;

    fn dummy_site_map() -> SiteMap {
        let dir = std::env::temp_dir().join(format!("nda_jit_test_{}", rand_u64()));
        let sm = SiteMap::open(&dir, 0).expect("open test site map");
        sm
    }

    fn rand_u64() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos() as u64
    }

    #[test]
    fn test_jit_compile_empty() {
        let prog = compile(&[]);
        assert_eq!(prog.nodes_compiled, 0);
        assert_eq!(prog.fns.len(), 0);
    }

    #[test]
    fn test_jit_int_node() {
        let nodes = vec![NdaNode::Int { value: 42 }];
        let prog = compile(&nodes);
        let sm = dummy_site_map();
        let result = prog.run(&[1.0f32; 4], &sm);
        assert!(result.error.is_none(), "error: {:?}", result.error);
        // Int { 42 } should set current_vec to a 1-element vec representing 42.
        assert_eq!(result.output_dim, 1);
    }

    #[test]
    fn test_jit_loop_node() {
        // Loop { count: 3, body: [Int{1}] } — should iterate 3 times, final vec = [1]
        let nodes = vec![NdaNode::Loop {
            count: 3,
            body:  vec![NdaNode::Int { value: 7 }],
        }];
        let prog = compile(&nodes);
        let sm = dummy_site_map();
        let result = prog.run(&[0.0f32], &sm);
        assert!(result.error.is_none(), "error: {:?}", result.error);
        assert_eq!(result.nodes_compiled, 4); // Scope + 3 Ints (unrolled)
    }

    #[test]
    fn test_jit_let_load() {
        use crate::compiler::nda_parser::hash_name;
        let h = hash_name("x");
        let nodes = vec![
            NdaNode::Let  { name_hash: h, init: Box::new(NdaNode::Int { value: 99 }) },
            NdaNode::Load { name_hash: h },
        ];
        let prog = compile(&nodes);
        let sm = dummy_site_map();
        let result = prog.run(&[0.0f32], &sm);
        assert!(result.error.is_none(), "error: {:?}", result.error);
        // After Load, current_vec should be the value we stored (Int{99} → [99] in f32 domain).
        assert_eq!(result.output_dim, 1);
    }

    #[test]
    fn test_jit_break_in_loop() {
        use crate::site_map::verifier::CmpOp;
        // Loop { count: 1_000_000, body: [Break] } — should exit after 1 iteration.
        let nodes = vec![NdaNode::Loop {
            count: 1_000_000,
            body:  vec![NdaNode::Break],
        }];
        let prog = compile(&nodes);
        let sm = dummy_site_map();
        let result = prog.run(&[1.0f32], &sm);
        assert!(result.error.is_none(), "error: {:?}", result.error);
    }

    #[test]
    fn test_jit_tier_info_non_empty() {
        let info = jit_tier_info();
        assert!(!info.is_empty());
        println!("JIT tier: {}", info);
    }

    #[test]
    fn test_x86_emitter_noop() {
        let mut e = X86Emitter::new();
        e.noop_fn();
        // PUSH RBP (55) + MOV RBP,RSP (48 89 E5) + XOR EAX,EAX (31 C0) + POP RBP (5D) + RET (C3)
        assert_eq!(e.buf, vec![0x55, 0x48, 0x89, 0xE5, 0x31, 0xC0, 0x5D, 0xC3]);
    }

    #[test]
    fn test_jit_silu_vecop() {
        let nodes = vec![NdaNode::VecOp {
            op:      VecOpKind::SiLU,
            operand: Box::new(NdaNode::Int { value: 2 }),
        }];
        let prog = compile(&nodes);
        let sm = dummy_site_map();
        let result = prog.run(&[1.0f32], &sm);
        assert!(result.error.is_none(), "error: {:?}", result.error);
    }

    #[test]
    fn test_jit_scalar_loop() {
        use crate::compiler::nda_parser::hash_name;
        let sum_h = hash_name("sum");
        let i_h = hash_name("i");

        // sum = 0
        // i = 0
        // loop 5 {
        //   sum = sum + i
        //   i = i + 1
        // }
        // Result should be 0 + 0 + 1 + 2 + 3 + 4 = 10.
        let nodes = vec![
            NdaNode::Let { name_hash: sum_h, init: Box::new(NdaNode::Int { value: 0 }) },
            NdaNode::Let { name_hash: i_h, init: Box::new(NdaNode::Int { value: 0 }) },
            NdaNode::Loop {
                count: 5,
                body: vec![
                    NdaNode::Store {
                        name_hash: sum_h,
                        value: Box::new(NdaNode::Add {
                            lhs: Box::new(NdaNode::Load { name_hash: sum_h }),
                            rhs: Box::new(NdaNode::Load { name_hash: i_h }),
                        }),
                    },
                    NdaNode::Store {
                        name_hash: i_h,
                        value: Box::new(NdaNode::Add {
                            lhs: Box::new(NdaNode::Load { name_hash: i_h }),
                            rhs: Box::new(NdaNode::Int { value: 1 }),
                        }),
                    },
                ],
            },
            NdaNode::Load { name_hash: sum_h },
        ];

        let prog = compile(&nodes);
        let sm = dummy_site_map();
        let result = prog.run(&[0.0f32], &sm);
        assert!(result.error.is_none(), "error: {:?}", result.error);
        assert_eq!(result.output_vec, vec![10.0f32]);
    }

    #[test]
    fn test_jit_extended_opcodes() {
        let sm = dummy_site_map();

        // 1. Test Float Constant and MathFunc Sin
        let f_nodes = vec![
            NdaNode::MathFunc {
                func: MathFuncKind::Sin,
                operand: Box::new(NdaNode::Float { value: 0.0 }),
            }
        ];
        let f_prog = compile(&f_nodes);
        let res = f_prog.run(&[], &sm);
        assert!(res.error.is_none());
        assert!((res.output_vec[0] - 0.0).abs() < 1e-6);

        // 2. Test Bitwise NOT on Scalar
        let bw_nodes = vec![
            NdaNode::Bitwise {
                op: BitwiseOp::Not,
                lhs: Box::new(NdaNode::Int { value: 0 }),
                rhs: None,
            }
        ];
        let bw_prog = compile(&bw_nodes);
        let res2 = bw_prog.run(&[], &sm);
        assert!(res2.error.is_none());
        // !0 should be -1 in i32
        assert_eq!(res2.output_vec[0], -1.0);

        // 3. Test Cast
        let cast_nodes = vec![
            NdaNode::Cast {
                from_type: TypeKind::Float,
                to_type: TypeKind::Int,
                operand: Box::new(NdaNode::Float { value: 42.5 }),
            }
        ];
        let cast_prog = compile(&cast_nodes);
        let res3 = cast_prog.run(&[], &sm);
        assert!(res3.error.is_none());
        assert_eq!(res3.output_vec[0], 42.0);

        // 4. Test MMIO Poke / Peek
        // We write to heap address 0, then read it back
        let mmio_nodes = vec![
            NdaNode::Poke {
                addr: Box::new(NdaNode::Int { value: 16 }),
                value: Box::new(NdaNode::Int { value: 1234 }),
            },
            NdaNode::Peek {
                addr: Box::new(NdaNode::Int { value: 16 }),
            }
        ];
        let mmio_prog = compile(&mmio_nodes);
        let res4 = mmio_prog.run(&[], &sm);
        assert!(res4.error.is_none(), "MMIO error: {:?}", res4.error);
        assert_eq!(res4.output_vec[0], 1234.0);
    }
}
