use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

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
                    kind_counts: std::collections::HashMap::new(),
                    output_log: state.print_buf,
                    loop_iterations: state.loop_count,
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
                kind_counts: std::collections::HashMap::new(),
                output_log: Vec::new(),
                loop_iterations: 0,
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
                    kind_counts: std::collections::HashMap::new(),
                    output_log: Vec::new(),
                    loop_iterations: 0,
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

// ─── Diagnostic: JitState snapshot ─────────────────────────────────────────────

/// Serializable diagnostic snapshot of a `JitState`'s runtime status.
#[derive(Debug, Clone, Serialize)]
pub struct JitStateInfo {
    pub stack_depth: usize,
    pub top_of_stack_type: Option<String>,
    pub total_variable_slots: usize,
    pub bound_variables: usize,
    pub variable_utilization: f64,
    pub matrix_count: usize,
    pub norm_count: usize,
    pub loop_count: usize,
    pub executed_nodes: usize,
    pub print_buffer_lines: usize,
    pub heap_capacity_bytes: usize,
    pub heap_allocations: usize,
    pub heap_allocated_bytes: usize,
    pub mmio_register_count: usize,
    pub interrupt_handler_count: usize,
    pub validation_issues: Vec<String>,
}

/// Produce a diagnostic snapshot of the current JIT state.
pub fn jit_state_info(state: &JitState<'_>) -> JitStateInfo {
    let bound = state.variables.iter().filter(|v| v.is_some()).count();
    let total = state.variables.len();
    let utilization = if total > 0 {
        bound as f64 / total as f64
    } else {
        0.0
    };
    let top_type = state.stack.last().map(|v| match v {
        JitVal::Vector(_) => "Vector".to_string(),
        JitVal::Scalar(_, _) => "Scalar".to_string(),
        JitVal::Float(_) => "Float".to_string(),
    });
    let allocated_bytes: usize = state.heap_allocations.values().sum();

    JitStateInfo {
        stack_depth: state.stack.len(),
        top_of_stack_type: top_type,
        total_variable_slots: total,
        bound_variables: bound,
        variable_utilization: utilization,
        matrix_count: state.matrix_count,
        norm_count: state.norm_count,
        loop_count: state.loop_count,
        executed_nodes: state.executed_nodes,
        print_buffer_lines: state.print_buf.len(),
        heap_capacity_bytes: state.heap.len(),
        heap_allocations: state.heap_allocations.len(),
        heap_allocated_bytes: allocated_bytes,
        mmio_register_count: state.mmio.len(),
        interrupt_handler_count: state.interrupts.len(),
        validation_issues: validate_jit_state(state),
    }
}

/// Validate JitState consistency, returning a list of warnings.
pub fn validate_jit_state(state: &JitState<'_>) -> Vec<String> {
    let mut issues = Vec::new();

    // Check heap allocation bounds
    let heap_len = state.heap.len();
    for (&addr, &size) in &state.heap_allocations {
        let end = (addr as usize).saturating_add(size);
        if end > heap_len {
            issues.push(format!(
                "heap allocation at {} size {} exceeds heap capacity {}",
                addr, size, heap_len
            ));
        }
    }

    // Check MMIO address range — keys are u32 so always valid,
    // but flag addresses in the top page (reserved on most platforms)
    for &addr in state.mmio.keys() {
        if addr >= 0xFFFF_F000 {
            issues.push(format!("MMIO address 0x{:08x} is in reserved high page", addr));
        }
    }

    // Check interrupt vector range
    for &vec in state.interrupts.keys() {
        if vec > 255 {
            issues.push(format!("interrupt vector {} exceeds max 255", vec));
        }
    }

    // Check stack is not unreasonably deep (potential leak)
    if state.stack.len() > 10_000 {
        issues.push(format!(
            "stack depth {} is unusually deep (possible leak)",
            state.stack.len()
        ));
    }

    issues
}

// ─── Diagnostic: JitResult info ────────────────────────────────────────────────

/// Serializable diagnostic from a completed `JitResult`.
#[derive(Debug, Clone, Serialize)]
pub struct JitResultInfo {
    pub output_dim: usize,
    pub output_len: usize,
    pub elapsed_us: u64,
    pub nodes_compiled: usize,
    pub has_error: bool,
    pub error_message: Option<String>,
    pub success: bool,
}

impl JitResultInfo {
    /// Build a diagnostic from a JitResult.
    pub fn from_result(result: &JitResult) -> Self {
        JitResultInfo {
            output_dim: result.output_dim,
            output_len: result.output_vec.len(),
            elapsed_us: result.elapsed_us,
            nodes_compiled: result.nodes_compiled,
            has_error: result.error.is_some(),
            error_message: result.error.clone(),
            success: result.error.is_none(),
        }
    }
}

// ─── Diagnostic: VarRegistry info ──────────────────────────────────────────────

/// Serializable diagnostic snapshot of a `VarRegistry`.
#[derive(Debug, Clone, Serialize)]
pub struct VarRegistryInfo {
    pub total_slots: usize,
}

impl VarRegistry {
    /// Return a diagnostic snapshot of this registry.
    pub fn info(&self) -> VarRegistryInfo {
        VarRegistryInfo {
            total_slots: self.total_slots(),
        }
    }
}

// ─── Diagnostic: JitProgram info ───────────────────────────────────────────────

/// Serializable diagnostic snapshot of a `JitProgram`.
#[derive(Debug, Clone, Serialize)]
pub struct JitProgramInfo {
    pub function_count: usize,
    pub nodes_compiled: usize,
    pub has_asm_kernel: bool,
    pub variable_slots: usize,
    pub validation_issues: Vec<String>,
}

/// Produce a diagnostic snapshot of a JitProgram.
pub fn jit_program_info(prog: &JitProgram) -> JitProgramInfo {
    let mut issues = Vec::new();

    if prog.fns.is_empty() {
        issues.push("program has no compiled functions".to_string());
    }

    if prog.nodes_compiled == 0 && !prog.fns.is_empty() {
        issues.push("nodes_compiled is 0 but functions exist".to_string());
    }

    JitProgramInfo {
        function_count: prog.fns.len(),
        nodes_compiled: prog.nodes_compiled,
        has_asm_kernel: prog.has_asm_kernel,
        variable_slots: prog.registry.total_slots(),
        validation_issues: issues,
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_sitemap() -> (SiteMap, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let sm = SiteMap::open(dir.path(), 0).unwrap();
        (sm, dir)
    }

    // ── JitStateInfo tests ────────────────────────────────────────────────────

    #[test]
    fn jit_state_info_empty() {
        let (sm, _dir) = make_test_sitemap();
        let state = JitState::new(&[1.0, 2.0, 3.0], &sm, 4);
        let info = jit_state_info(&state);
        assert_eq!(info.stack_depth, 1); // input vector on stack
        assert_eq!(info.top_of_stack_type.as_deref(), Some("Vector"));
        assert_eq!(info.total_variable_slots, 4);
        assert_eq!(info.bound_variables, 0);
        assert!((info.variable_utilization - 0.0).abs() < 1e-9);
        assert_eq!(info.matrix_count, 0);
        assert_eq!(info.norm_count, 0);
        assert_eq!(info.loop_count, 0);
        assert_eq!(info.executed_nodes, 0);
        assert_eq!(info.print_buffer_lines, 0);
        assert_eq!(info.heap_capacity_bytes, 65536);
        assert_eq!(info.heap_allocations, 0);
        assert_eq!(info.heap_allocated_bytes, 0);
        assert_eq!(info.mmio_register_count, 0);
        assert_eq!(info.interrupt_handler_count, 0);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn jit_state_info_with_bindings() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 4);
        state.variables[0] = Some(JitVal::Scalar(42, 0));
        state.variables[2] = Some(JitVal::Float(3.14));
        state.matrix_count = 3;
        state.norm_count = 1;
        state.loop_count = 2;
        state.executed_nodes = 10;
        state.print_buf.push("hello".to_string());

        let info = jit_state_info(&state);
        assert_eq!(info.bound_variables, 2);
        assert!((info.variable_utilization - 0.5).abs() < 1e-9);
        assert_eq!(info.matrix_count, 3);
        assert_eq!(info.norm_count, 1);
        assert_eq!(info.loop_count, 2);
        assert_eq!(info.executed_nodes, 10);
        assert_eq!(info.print_buffer_lines, 1);
    }

    #[test]
    fn jit_state_info_top_of_stack_types() {
        let (sm, _dir) = make_test_sitemap();

        // Vector on top
        let state = JitState::new(&[1.0], &sm, 0);
        assert_eq!(jit_state_info(&state).top_of_stack_type.as_deref(), Some("Vector"));

        // Scalar on top
        let mut state2 = JitState::new(&[1.0], &sm, 0);
        state2.stack.push(JitVal::Scalar(5, 1));
        assert_eq!(jit_state_info(&state2).top_of_stack_type.as_deref(), Some("Scalar"));

        // Float on top
        let mut state3 = JitState::new(&[1.0], &sm, 0);
        state3.stack.push(JitVal::Float(2.71));
        assert_eq!(jit_state_info(&state3).top_of_stack_type.as_deref(), Some("Float"));
    }

    #[test]
    fn jit_state_info_heap_allocations() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        state.heap_allocations.insert(0, 1024);
        state.heap_allocations.insert(1024, 2048);
        state.mmio.insert(0x1000, JitVal::Float(1.0));
        state.interrupts.insert(7, 0xDEAD);

        let info = jit_state_info(&state);
        assert_eq!(info.heap_allocations, 2);
        assert_eq!(info.heap_allocated_bytes, 3072);
        assert_eq!(info.mmio_register_count, 1);
        assert_eq!(info.interrupt_handler_count, 1);
    }

    // ── validate_jit_state tests ──────────────────────────────────────────────

    #[test]
    fn validate_jit_state_clean() {
        let (sm, _dir) = make_test_sitemap();
        let state = JitState::new(&[1.0], &sm, 2);
        let issues = validate_jit_state(&state);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_jit_state_heap_overflow() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        // Heap is 65536 bytes; allocate past the end
        state.heap_allocations.insert(65000, 1000);
        let issues = validate_jit_state(&state);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("exceeds heap capacity"));
    }

    #[test]
    fn validate_jit_state_deep_stack() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        for _ in 0..10_001 {
            state.stack.push(JitVal::Scalar(1, 0));
        }
        let issues = validate_jit_state(&state);
        assert!(issues.iter().any(|i| i.contains("unusually deep")));
    }

    #[test]
    fn validate_jit_state_multiple_issues() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        state.heap_allocations.insert(70000, 1000);
        state.interrupts.insert(300, 0x01);
        let issues = validate_jit_state(&state);
        assert!(issues.len() >= 2);
        assert!(issues.iter().any(|i| i.contains("heap allocation")));
        assert!(issues.iter().any(|i| i.contains("interrupt vector")));
    }

    // ── JitResultInfo tests ───────────────────────────────────────────────────

    #[test]
    fn jit_result_info_success() {
        let result = JitResult {
            output_vec: vec![1.0, 2.0, 3.0],
            output_dim: 3,
            elapsed_us: 42,
            nodes_compiled: 10,
            error: None,
        };
        let info = JitResultInfo::from_result(&result);
        assert_eq!(info.output_dim, 3);
        assert_eq!(info.output_len, 3);
        assert_eq!(info.elapsed_us, 42);
        assert_eq!(info.nodes_compiled, 10);
        assert!(!info.has_error);
        assert!(info.error_message.is_none());
        assert!(info.success);
    }

    #[test]
    fn jit_result_info_error() {
        let result = JitResult {
            output_vec: vec![],
            output_dim: 0,
            elapsed_us: 5,
            nodes_compiled: 3,
            error: Some("division by zero".to_string()),
        };
        let info = JitResultInfo::from_result(&result);
        assert_eq!(info.output_dim, 0);
        assert_eq!(info.output_len, 0);
        assert!(info.has_error);
        assert_eq!(info.error_message.as_deref(), Some("division by zero"));
        assert!(!info.success);
    }

    // ── VarRegistry info tests ────────────────────────────────────────────────

    #[test]
    fn var_registry_info_empty() {
        let reg = VarRegistry::new();
        let info = reg.info();
        assert_eq!(info.total_slots, 0);
    }

    #[test]
    fn var_registry_info_with_slots() {
        let reg = VarRegistry::new();
        reg.get_or_create_slot(0xAAAA);
        reg.get_or_create_slot(0xBBBB);
        reg.get_or_create_slot(0xAAAA); // duplicate
        let info = reg.info();
        assert_eq!(info.total_slots, 2);
    }

    // ── JitProgramInfo tests ──────────────────────────────────────────────────

    #[test]
    fn jit_program_info_empty() {
        let reg = VarRegistry::new();
        let prog = JitProgram {
            fns: vec![],
            nodes_compiled: 0,
            has_asm_kernel: false,
            registry: reg,
        };
        let info = jit_program_info(&prog);
        assert_eq!(info.function_count, 0);
        assert_eq!(info.nodes_compiled, 0);
        assert!(!info.has_asm_kernel);
        assert_eq!(info.variable_slots, 0);
        assert!(info.validation_issues.iter().any(|i| i.contains("no compiled functions")));
    }

    #[test]
    fn jit_program_info_with_functions() {
        let reg = VarRegistry::new();
        reg.get_or_create_slot(0x01);
        let dummy_fn: JitFn = Arc::new(|state: &mut JitState<'_>| {
            state.executed_nodes += 1;
            Ok(JitControlFlow::Continue)
        });
        let prog = JitProgram {
            fns: vec![dummy_fn],
            nodes_compiled: 5,
            has_asm_kernel: true,
            registry: reg,
        };
        let info = jit_program_info(&prog);
        assert_eq!(info.function_count, 1);
        assert_eq!(info.nodes_compiled, 5);
        assert!(info.has_asm_kernel);
        assert_eq!(info.variable_slots, 1);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn jit_program_info_mismatch_warning() {
        let reg = VarRegistry::new();
        let dummy_fn: JitFn = Arc::new(|_: &mut JitState<'_>| Ok(JitControlFlow::Continue));
        let prog = JitProgram {
            fns: vec![dummy_fn],
            nodes_compiled: 0, // mismatch: fns exist but count is 0
            has_asm_kernel: false,
            registry: reg,
        };
        let info = jit_program_info(&prog);
        assert!(info.validation_issues.iter().any(|i| i.contains("nodes_compiled is 0")));
    }

    // ── JitControlFlow tests ──────────────────────────────────────────────────

    #[test]
    fn jit_control_flow_equality() {
        assert_eq!(JitControlFlow::Continue, JitControlFlow::Continue);
        assert_eq!(JitControlFlow::Break, JitControlFlow::Break);
        assert_eq!(JitControlFlow::Return, JitControlFlow::Return);
        assert_ne!(JitControlFlow::Continue, JitControlFlow::Break);
        assert_ne!(JitControlFlow::Break, JitControlFlow::Return);
    }

    // ── JitVal tests ──────────────────────────────────────────────────────────

    #[test]
    fn jit_val_is_truthy() {
        assert!(JitVal::Scalar(1, 0).is_truthy());
        assert!(JitVal::Scalar(100, -5).is_truthy());
        assert!(!JitVal::Scalar(0, 0).is_truthy());
        assert!(!JitVal::Scalar(-1, 0).is_truthy());

        assert!(JitVal::Float(1.0).is_truthy());
        assert!(JitVal::Float(0.001).is_truthy());
        assert!(!JitVal::Float(0.0).is_truthy());
        assert!(!JitVal::Float(-1.0).is_truthy());
    }

    #[test]
    fn jit_val_to_f32_vec() {
        let v = JitVal::Float(3.14);
        let f32v = v.to_f32_vec();
        assert_eq!(f32v.len(), 1);
        assert!((f32v[0] - 3.14).abs() < 1e-6);

        let s = JitVal::Scalar(5, 0); // 5 * 2^0 = 5.0
        let f32s = s.to_f32_vec();
        assert_eq!(f32s.len(), 1);
        assert!((f32s[0] - 5.0).abs() < 1e-6);

        let s2 = JitVal::Scalar(3, 2); // 3 * 2^2 = 12.0
        let f32s2 = s2.to_f32_vec();
        assert!((f32s2[0] - 12.0).abs() < 1e-6);
    }

    #[test]
    fn jit_val_vector_to_f32() {
        let nda = NdaVec::from_f32_slice(&[1.0, 2.0, 3.0, 4.0]);
        let v = JitVal::Vector(Arc::new(nda));
        let f32v = v.to_f32_vec();
        assert_eq!(f32v.len(), 4);
        // NDA ternary encoding is lossy — just verify length and non-NaN
        for val in &f32v {
            assert!(!val.is_nan(), "got NaN in output");
        }
    }

    // ── run_sequence tests ────────────────────────────────────────────────────

    #[test]
    fn run_sequence_empty() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        let result = run_sequence(&[], &mut state);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), JitControlFlow::Continue);
    }

    #[test]
    fn run_sequence_continue() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        let fns: Vec<JitFn> = vec![
            Arc::new(|s: &mut JitState<'_>| {
                s.executed_nodes += 1;
                Ok(JitControlFlow::Continue)
            }),
            Arc::new(|s: &mut JitState<'_>| {
                s.executed_nodes += 1;
                Ok(JitControlFlow::Continue)
            }),
        ];
        let result = run_sequence(&fns, &mut state);
        assert!(result.is_ok());
        assert_eq!(state.executed_nodes, 2);
    }

    #[test]
    fn run_sequence_break_stops_early() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        let fns: Vec<JitFn> = vec![
            Arc::new(|_: &mut JitState<'_>| Ok(JitControlFlow::Break)),
            Arc::new(|s: &mut JitState<'_>| {
                s.executed_nodes += 1; // should NOT execute
                Ok(JitControlFlow::Continue)
            }),
        ];
        let result = run_sequence(&fns, &mut state);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), JitControlFlow::Break);
        assert_eq!(state.executed_nodes, 0); // second fn didn't run
    }

    #[test]
    fn run_sequence_return_propagates() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        let fns: Vec<JitFn> = vec![
            Arc::new(|_: &mut JitState<'_>| Ok(JitControlFlow::Return)),
            Arc::new(|_: &mut JitState<'_>| Ok(JitControlFlow::Continue)),
        ];
        let result = run_sequence(&fns, &mut state);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), JitControlFlow::Return);
    }

    #[test]
    fn run_sequence_error_propagates() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        let fns: Vec<JitFn> = vec![
            Arc::new(|_: &mut JitState<'_>| Err("test error".to_string())),
        ];
        let result = run_sequence(&fns, &mut state);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "test error");
    }

    // ── Block 111: expanded tests ────────────────────────────────────────────

    #[test]
    fn var_registry_default() {
        let reg = VarRegistry::default();
        assert_eq!(reg.total_slots(), 0);
    }

    #[test]
    fn var_registry_idempotent() {
        let reg = VarRegistry::new();
        let slot1 = reg.get_or_create_slot(42);
        let slot2 = reg.get_or_create_slot(42);
        assert_eq!(slot1, slot2);
        assert_eq!(reg.total_slots(), 1);
    }

    #[test]
    fn var_registry_sequential_slots() {
        let reg = VarRegistry::new();
        let s0 = reg.get_or_create_slot(0xAA);
        let s1 = reg.get_or_create_slot(0xBB);
        let s2 = reg.get_or_create_slot(0xCC);
        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(reg.total_slots(), 3);
    }

    #[test]
    fn jit_val_is_truthy_vector_positive() {
        let nda = NdaVec::from_f32_slice(&[1.0, 2.0, 3.0]);
        assert!(JitVal::Vector(Arc::new(nda)).is_truthy());
    }

    #[test]
    fn jit_val_clone_preserves_scalar() {
        let v = JitVal::Scalar(42, 3);
        let v2 = v.clone();
        match v2 {
            JitVal::Scalar(val, scale) => {
                assert_eq!(val, 42);
                assert_eq!(scale, 3);
            }
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn jit_val_clone_preserves_float() {
        let v = JitVal::Float(3.14);
        let v2 = v.clone();
        match v2 {
            JitVal::Float(val) => assert!((val - 3.14).abs() < 1e-6),
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn jit_state_new_initial_state() {
        let (sm, _dir) = make_test_sitemap();
        let state = JitState::new(&[1.0, 2.0], &sm, 8);
        assert_eq!(state.stack.len(), 1);
        assert_eq!(state.variables.len(), 8);
        assert_eq!(state.matrix_count, 0);
        assert_eq!(state.executed_nodes, 0);
        assert!(state.print_buf.is_empty());
        assert_eq!(state.heap.len(), 65536);
        assert!(state.heap_allocations.is_empty());
        assert!(state.mmio.is_empty());
        assert!(state.interrupts.is_empty());
    }

    #[test]
    fn jit_state_new_zero_slots() {
        let (sm, _dir) = make_test_sitemap();
        let state = JitState::new(&[1.0], &sm, 0);
        assert_eq!(state.variables.len(), 0);
    }

    #[test]
    fn is_truthy_positive_vector() {
        let nda = NdaVec::from_f32_slice(&[1.0, 2.0, 3.0, 4.0]);
        assert!(JitState::is_truthy(&nda));
    }

    #[test]
    fn is_truthy_negative_vector() {
        let nda = NdaVec::from_f32_slice(&[-5.0, -5.0, -5.0, -5.0]);
        assert!(!JitState::is_truthy(&nda));
    }

    #[test]
    fn validate_mmio_reserved_high_page() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        state.mmio.insert(0xFFFF_F000, JitVal::Float(1.0));
        let issues = validate_jit_state(&state);
        assert!(issues.iter().any(|i| i.contains("reserved high page")));
    }

    #[test]
    fn validate_mmio_normal_address_ok() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        state.mmio.insert(0x1000, JitVal::Float(1.0));
        let issues = validate_jit_state(&state);
        assert!(issues.is_empty());
    }

    #[test]
    fn jit_state_info_serializes() {
        let (sm, _dir) = make_test_sitemap();
        let state = JitState::new(&[1.0], &sm, 4);
        let info = jit_state_info(&state);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"stack_depth\":1"));
        assert!(json.contains("\"heap_capacity_bytes\":65536"));
    }

    #[test]
    fn jit_result_info_serializes() {
        let result = JitResult {
            output_vec: vec![1.0],
            output_dim: 1,
            elapsed_us: 10,
            nodes_compiled: 5,
            error: None,
        };
        let info = JitResultInfo::from_result(&result);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn var_registry_info_serializes() {
        let reg = VarRegistry::new();
        reg.get_or_create_slot(1);
        let info = reg.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"total_slots\":1"));
    }

    #[test]
    fn jit_program_info_serializes() {
        let reg = VarRegistry::new();
        let prog = JitProgram {
            fns: vec![],
            nodes_compiled: 0,
            has_asm_kernel: false,
            registry: reg,
        };
        let info = jit_program_info(&prog);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"function_count\":0"));
    }

    #[test]
    fn jit_program_run_empty() {
        let (sm, _dir) = make_test_sitemap();
        let reg = VarRegistry::new();
        let prog = JitProgram {
            fns: vec![],
            nodes_compiled: 0,
            has_asm_kernel: false,
            registry: reg,
        };
        let result = prog.run(&[1.0, 2.0], &sm);
        assert!(result.error.is_none());
        assert!(result.output_dim > 0);
    }

    #[test]
    fn jit_program_run_with_fn() {
        let (sm, _dir) = make_test_sitemap();
        let reg = VarRegistry::new();
        let dummy_fn: JitFn = Arc::new(|state: &mut JitState<'_>| {
            state.executed_nodes += 1;
            Ok(JitControlFlow::Continue)
        });
        let prog = JitProgram {
            fns: vec![dummy_fn],
            nodes_compiled: 1,
            has_asm_kernel: false,
            registry: reg,
        };
        let result = prog.run(&[1.0], &sm);
        assert!(result.error.is_none());
        assert_eq!(result.nodes_compiled, 1);
    }

    #[test]
    fn jit_program_run_sandboxed_empty() {
        let (sm, _dir) = make_test_sitemap();
        let reg = VarRegistry::new();
        let prog = JitProgram {
            fns: vec![],
            nodes_compiled: 0,
            has_asm_kernel: false,
            registry: reg,
        };
        let result = prog.run_sandboxed(&[1.0], &sm);
        assert!(!result.panicked);
        assert!(result.error.is_none());
    }

    #[test]
    fn jit_program_run_sandboxed_captures_error() {
        let (sm, _dir) = make_test_sitemap();
        let reg = VarRegistry::new();
        let bad_fn: JitFn = Arc::new(|_: &mut JitState<'_>| Err("oops".to_string()));
        let prog = JitProgram {
            fns: vec![bad_fn],
            nodes_compiled: 1,
            has_asm_kernel: false,
            registry: reg,
        };
        let result = prog.run_sandboxed(&[1.0], &sm);
        assert!(!result.panicked);
        assert_eq!(result.error.as_deref(), Some("oops"));
    }

    #[test]
    fn run_sequence_multiple_continues() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        let fns: Vec<JitFn> = (0..5).map(|_| {
            Arc::new(|s: &mut JitState<'_>| {
                s.executed_nodes += 1;
                Ok(JitControlFlow::Continue)
            }) as JitFn
        }).collect();
        let result = run_sequence(&fns, &mut state);
        assert!(result.is_ok());
        assert_eq!(state.executed_nodes, 5);
    }

    #[test]
    fn jit_val_to_f32_vec_scalar_negative_scale() {
        // 5 * 2^(-1) = 2.5
        let s = JitVal::Scalar(5, -1);
        let f32v = s.to_f32_vec();
        assert!((f32v[0] - 2.5).abs() < 1e-6);
    }

    // ── JSON key counts ──────────────────────────────────────────────────

    #[test]
    fn jit_state_info_json_has_16_keys() {
        let (sm, _dir) = make_test_sitemap();
        let state = JitState::new(&[1.0], &sm, 0);
        let info = jit_state_info(&state);
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 16);
    }

    #[test]
    fn jit_result_info_json_has_7_keys() {
        let result = JitResult {
            output_vec: vec![], output_dim: 0, elapsed_us: 0,
            nodes_compiled: 0, error: None,
        };
        let info = JitResultInfo::from_result(&result);
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 7);
    }

    #[test]
    fn jit_program_info_json_has_5_keys() {
        let reg = VarRegistry::new();
        let prog = JitProgram { fns: vec![], nodes_compiled: 0, has_asm_kernel: false, registry: reg };
        let info = jit_program_info(&prog);
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 5);
    }

    #[test]
    fn var_registry_info_json_has_1_key() {
        let reg = VarRegistry::new();
        let info = reg.info();
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 1);
    }

    // ── JSON value verification ──────────────────────────────────────────

    #[test]
    fn jit_state_info_json_values() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 8);
        state.variables[0] = Some(JitVal::Scalar(1, 0));
        state.matrix_count = 5;
        state.executed_nodes = 100;
        let info = jit_state_info(&state);
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["stack_depth"], 1);
        assert_eq!(v["total_variable_slots"], 8);
        assert_eq!(v["bound_variables"], 1);
        assert_eq!(v["matrix_count"], 5);
        assert_eq!(v["executed_nodes"], 100);
        assert_eq!(v["heap_capacity_bytes"], 65536);
    }

    #[test]
    fn jit_result_info_json_values() {
        let result = JitResult {
            output_vec: vec![1.0, 2.0], output_dim: 2, elapsed_us: 50,
            nodes_compiled: 10, error: None,
        };
        let info = JitResultInfo::from_result(&result);
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["output_dim"], 2);
        assert_eq!(v["output_len"], 2);
        assert_eq!(v["elapsed_us"], 50);
        assert_eq!(v["nodes_compiled"], 10);
        assert_eq!(v["has_error"], false);
        assert_eq!(v["success"], true);
    }

    #[test]
    fn jit_result_info_json_error_values() {
        let result = JitResult {
            output_vec: vec![], output_dim: 0, elapsed_us: 5,
            nodes_compiled: 3, error: Some("fail".into()),
        };
        let info = JitResultInfo::from_result(&result);
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["has_error"], true);
        assert_eq!(v["success"], false);
        assert_eq!(v["error_message"], "fail");
    }

    // ── Clone independence ───────────────────────────────────────────────

    #[test]
    fn jit_state_info_clone_independent() {
        let (sm, _dir) = make_test_sitemap();
        let state = JitState::new(&[1.0], &sm, 4);
        let info = jit_state_info(&state);
        let mut cloned = info.clone();
        cloned.validation_issues.push("extra".into());
        assert_ne!(cloned.validation_issues.len(), info.validation_issues.len());
    }

    #[test]
    fn jit_result_info_clone_independent() {
        let result = JitResult {
            output_vec: vec![1.0], output_dim: 1, elapsed_us: 10,
            nodes_compiled: 5, error: Some("err".into()),
        };
        let info = JitResultInfo::from_result(&result);
        let mut cloned = info.clone();
        cloned.error_message = Some("changed".into());
        assert_ne!(cloned.error_message, info.error_message);
    }

    #[test]
    fn jit_program_info_clone_independent() {
        let reg = VarRegistry::new();
        let prog = JitProgram { fns: vec![], nodes_compiled: 0, has_asm_kernel: false, registry: reg };
        let info = jit_program_info(&prog);
        let mut cloned = info.clone();
        cloned.validation_issues.push("test".into());
        assert_ne!(cloned.validation_issues.len(), info.validation_issues.len());
    }

    // ── Debug format ─────────────────────────────────────────────────────

    #[test]
    fn jit_control_flow_debug() {
        assert!(format!("{:?}", JitControlFlow::Continue).contains("Continue"));
        assert!(format!("{:?}", JitControlFlow::Break).contains("Break"));
        assert!(format!("{:?}", JitControlFlow::Return).contains("Return"));
    }

    #[test]
    fn jit_result_debug() {
        let result = JitResult {
            output_vec: vec![1.0], output_dim: 1, elapsed_us: 42,
            nodes_compiled: 5, error: None,
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("output_dim: 1"));
        assert!(debug.contains("elapsed_us: 42"));
        assert!(debug.contains("nodes_compiled: 5"));
    }

    // ── JitVal additional ────────────────────────────────────────────────

    #[test]
    fn jit_val_is_truthy_scalar_zero_scale() {
        assert!(!JitVal::Scalar(0, 5).is_truthy());
        assert!(JitVal::Scalar(1, 5).is_truthy());
    }

    #[test]
    fn jit_val_to_f32_vec_float() {
        let v = JitVal::Float(-2.5);
        let f32v = v.to_f32_vec();
        assert_eq!(f32v.len(), 1);
        assert!((f32v[0] - (-2.5)).abs() < 1e-6);
    }

    #[test]
    fn jit_val_to_f32_vec_scalar_zero() {
        let s = JitVal::Scalar(0, 0);
        let f32v = s.to_f32_vec();
        assert!((f32v[0]).abs() < 1e-6);
    }

    #[test]
    fn jit_val_to_f32_vec_scalar_large_scale() {
        // 1 * 2^10 = 1024
        let s = JitVal::Scalar(1, 10);
        let f32v = s.to_f32_vec();
        assert!((f32v[0] - 1024.0).abs() < 1e-2);
    }

    // ── JitState additional ──────────────────────────────────────────────

    #[test]
    fn jit_state_with_print_buffer() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        state.print_buf.push("line1".into());
        state.print_buf.push("line2".into());
        let info = jit_state_info(&state);
        assert_eq!(info.print_buffer_lines, 2);
    }

    #[test]
    fn jit_state_full_utilization() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 2);
        state.variables[0] = Some(JitVal::Scalar(1, 0));
        state.variables[1] = Some(JitVal::Float(1.0));
        let info = jit_state_info(&state);
        assert!((info.variable_utilization - 1.0).abs() < 1e-9);
        assert_eq!(info.bound_variables, 2);
    }

    // ── VarRegistry additional ───────────────────────────────────────────

    #[test]
    fn var_registry_many_slots() {
        let reg = VarRegistry::new();
        for i in 0..100 {
            reg.get_or_create_slot(i);
        }
        assert_eq!(reg.total_slots(), 100);
    }

    #[test]
    fn var_registry_info_json_value() {
        let reg = VarRegistry::new();
        reg.get_or_create_slot(0xAA);
        reg.get_or_create_slot(0xBB);
        let info = reg.info();
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["total_slots"], 2);
    }

    // ── JitProgramInfo additional ────────────────────────────────────────

    #[test]
    fn jit_program_info_with_asm() {
        let reg = VarRegistry::new();
        let dummy_fn: JitFn = Arc::new(|_: &mut JitState<'_>| Ok(JitControlFlow::Continue));
        let prog = JitProgram {
            fns: vec![dummy_fn], nodes_compiled: 10, has_asm_kernel: true, registry: reg,
        };
        let info = jit_program_info(&prog);
        assert!(info.has_asm_kernel);
        assert_eq!(info.function_count, 1);
        assert_eq!(info.nodes_compiled, 10);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn jit_program_info_json_values() {
        let reg = VarRegistry::new();
        let prog = JitProgram { fns: vec![], nodes_compiled: 0, has_asm_kernel: false, registry: reg };
        let info = jit_program_info(&prog);
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["function_count"], 0);
        assert_eq!(v["nodes_compiled"], 0);
        assert_eq!(v["has_asm_kernel"], false);
        assert_eq!(v["variable_slots"], 0);
    }

    // ── run_sequence additional ──────────────────────────────────────────

    #[test]
    fn run_sequence_single_fn() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        let fns: Vec<JitFn> = vec![
            Arc::new(|s: &mut JitState<'_>| {
                s.executed_nodes += 1;
                Ok(JitControlFlow::Continue)
            }),
        ];
        let result = run_sequence(&fns, &mut state);
        assert!(result.is_ok());
        assert_eq!(state.executed_nodes, 1);
    }

    #[test]
    fn run_sequence_ten_fns() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        let fns: Vec<JitFn> = (0..10).map(|_| {
            Arc::new(|s: &mut JitState<'_>| {
                s.executed_nodes += 1;
                Ok(JitControlFlow::Continue)
            }) as JitFn
        }).collect();
        let result = run_sequence(&fns, &mut state);
        assert!(result.is_ok());
        assert_eq!(state.executed_nodes, 10);
    }

    // ── Validate: edge cases ─────────────────────────────────────────────

    #[test]
    fn validate_jit_state_mmio_boundary() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        // Just below reserved page — should be OK
        state.mmio.insert(0xFFFE_FFFF, JitVal::Float(1.0));
        let issues = validate_jit_state(&state);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_jit_state_interrupt_boundary() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        // Exactly at max — should be OK
        state.interrupts.insert(255, 0x01);
        let issues = validate_jit_state(&state);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_jit_state_interrupt_just_over() {
        let (sm, _dir) = make_test_sitemap();
        let mut state = JitState::new(&[1.0], &sm, 0);
        state.interrupts.insert(256, 0x01);
        let issues = validate_jit_state(&state);
        assert!(issues.iter().any(|i| i.contains("interrupt vector")));
    }

    // ── Pretty JSON ──────────────────────────────────────────────────────

    #[test]
    fn jit_state_info_pretty_json() {
        let (sm, _dir) = make_test_sitemap();
        let state = JitState::new(&[1.0], &sm, 0);
        let info = jit_state_info(&state);
        let pretty = serde_json::to_string_pretty(&info).unwrap();
        assert!(pretty.contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&pretty).unwrap();
        assert_eq!(v["heap_capacity_bytes"], 65536);
    }

    #[test]
    fn jit_result_info_pretty_json() {
        let result = JitResult {
            output_vec: vec![], output_dim: 0, elapsed_us: 0,
            nodes_compiled: 0, error: None,
        };
        let info = JitResultInfo::from_result(&result);
        let pretty = serde_json::to_string_pretty(&info).unwrap();
        assert!(pretty.contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&pretty).unwrap();
        assert_eq!(v["success"], true);
    }
}
