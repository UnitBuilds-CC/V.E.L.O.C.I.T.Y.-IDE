// sandbox/mod.rs — Executing NDA opcode trees with nda_int kernels
#![allow(dead_code, unused)]
pub mod jit_sandbox;
pub mod scope_validator;

pub use jit_sandbox::NdaJitSandbox;

use serde::Serialize;

use crate::nda::NdaMatrix;
use crate::nda_int::NdaVec;
use crate::site_map::verifier::{BitwiseOp, MathFuncKind, MathOp};
use crate::site_map::{NdaNode, SiteMap};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
pub struct SandboxResult {
    pub executed_nodes: usize,
    pub matrix_count: usize,
    pub norm_count: usize,
    pub output_vec: Vec<f32>, // final output of the execution chain
    pub output_dim: usize,
    pub panicked: bool,
    pub error: Option<String>,
    pub elapsed_us: u64,
    /// Per-node-kind execution counts (e.g. "Matrix" => 5, "Norm" => 3).
    pub kind_counts: HashMap<String, usize>,
    /// Captured print output from Print nodes.
    pub output_log: Vec<String>,
    /// Number of loop iterations executed.
    pub loop_iterations: usize,
}

impl SandboxResult {
    /// Whether execution completed without panic or error.
    pub fn is_success(&self) -> bool {
        !self.panicked && self.error.is_none()
    }

    /// Return the top-N most executed node kinds.
    pub fn top_kinds(&self, n: usize) -> Vec<(&str, usize)> {
        let mut pairs: Vec<(&str, usize)> = self
            .kind_counts
            .iter()
            .map(|(k, &v)| (k.as_str(), v))
            .collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1));
        pairs.into_iter().take(n).collect()
    }

    /// Validate the execution result.
    /// Returns a list of warnings (empty = all good).
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.panicked {
            warnings.push("Execution panicked".to_string());
        }
        if let Some(ref err) = self.error {
            warnings.push(format!("Execution error: {}", err));
        }
        if self.executed_nodes == 0 && !self.panicked {
            warnings.push("No nodes were executed".to_string());
        }
        if self.output_dim == 0 && self.is_success() {
            warnings.push("Output dimension is 0".to_string());
        }
        if self.loop_iterations > 1_000_000 {
            warnings.push(format!(
                "High loop iteration count: {} (potential infinite loop)",
                self.loop_iterations
            ));
        }

        warnings
    }

    /// Return a structured execution summary.
    pub fn execution_summary(&self) -> SandboxExecutionSummary {
        SandboxExecutionSummary {
            success: self.is_success(),
            executed_nodes: self.executed_nodes,
            matrix_count: self.matrix_count,
            norm_count: self.norm_count,
            output_dim: self.output_dim,
            elapsed_us: self.elapsed_us,
            loop_iterations: self.loop_iterations,
            unique_kinds: self.kind_counts.len(),
            output_log_lines: self.output_log.len(),
            has_error: self.error.is_some(),
        }
    }

    /// Compute throughput in operations per second.
    pub fn throughput_ops_per_sec(&self) -> f64 {
        if self.elapsed_us == 0 {
            return 0.0;
        }
        (self.executed_nodes as f64) / (self.elapsed_us as f64 / 1_000_000.0)
    }

    /// Return the ratio of successful computation nodes to total executed.
    /// Returns 0.0 if no nodes were executed.
    pub fn computation_ratio(&self) -> f64 {
        if self.executed_nodes == 0 {
            return 0.0;
        }
        (self.matrix_count + self.norm_count) as f64 / self.executed_nodes as f64
    }

    /// Return a detailed execution profile with per-kind breakdown.
    pub fn execution_profile(&self) -> SandboxExecutionProfile {
        let mut sorted_kinds: Vec<(String, usize)> = self
            .kind_counts
            .iter()
            .map(|(k, &v)| (k.clone(), v))
            .collect();
        sorted_kinds.sort_by(|a, b| b.1.cmp(&a.1));
        SandboxExecutionProfile {
            total_nodes: self.executed_nodes,
            unique_kinds: self.kind_counts.len(),
            top_kinds: sorted_kinds,
            output_dim: self.output_dim,
            output_log_lines: self.output_log.len(),
            loop_iterations: self.loop_iterations,
            elapsed_us: self.elapsed_us,
            throughput_ops: self.throughput_ops_per_sec(),
            computation_ratio: self.computation_ratio(),
        }
    }
}

/// Structured execution summary (safe to log).
#[derive(Debug, Clone, Serialize)]
pub struct SandboxExecutionSummary {
    pub success: bool,
    pub executed_nodes: usize,
    pub matrix_count: usize,
    pub norm_count: usize,
    pub output_dim: usize,
    pub elapsed_us: u64,
    pub loop_iterations: usize,
    pub unique_kinds: usize,
    pub output_log_lines: usize,
    pub has_error: bool,
}

/// Detailed execution profile with per-kind breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct SandboxExecutionProfile {
    pub total_nodes: usize,
    pub unique_kinds: usize,
    pub top_kinds: Vec<(String, usize)>,
    pub output_dim: usize,
    pub output_log_lines: usize,
    pub loop_iterations: usize,
    pub elapsed_us: u64,
    pub throughput_ops: f64,
    pub computation_ratio: f64,
}

/// Report from batch execution of multiple node sequences.
#[derive(Debug, Clone, Serialize)]
pub struct SandboxBatchReport {
    pub total_runs: usize,
    pub successful: usize,
    pub failed: usize,
    pub total_elapsed_us: u64,
    pub total_nodes_executed: usize,
    pub per_run_summaries: Vec<SandboxExecutionSummary>,
}

impl SandboxBatchReport {
    /// Return the success rate as a fraction [0.0, 1.0].
    pub fn success_rate(&self) -> f64 {
        if self.total_runs == 0 {
            return 0.0;
        }
        self.successful as f64 / self.total_runs as f64
    }

    /// Return the average elapsed time per run in microseconds.
    pub fn avg_elapsed_us(&self) -> f64 {
        if self.total_runs == 0 {
            return 0.0;
        }
        self.total_elapsed_us as f64 / self.total_runs as f64
    }

    /// Return the average nodes executed per run.
    pub fn avg_nodes_per_run(&self) -> f64 {
        if self.total_runs == 0 {
            return 0.0;
        }
        self.total_nodes_executed as f64 / self.total_runs as f64
    }

    /// Validate the batch report for consistency.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.total_runs == 0 {
            issues.push("batch report has zero total runs".to_string());
        }
        if self.successful + self.failed != self.total_runs {
            issues.push(format!(
                "success({}) + failed({}) != total_runs({})",
                self.successful, self.failed, self.total_runs
            ));
        }
        if self.per_run_summaries.len() != self.total_runs {
            issues.push(format!(
                "per_run_summaries count ({}) != total_runs ({})",
                self.per_run_summaries.len(),
                self.total_runs
            ));
        }
        issues
    }
}

/// Estimate the resource requirements for executing a set of NDA nodes
/// without actually running them.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceEstimate {
    pub node_count: usize,
    pub matrix_count: usize,
    pub norm_count: usize,
    pub loop_count: usize,
    pub variable_count: usize,
    pub max_depth: usize,
    pub estimated_memory_bytes: usize,
    pub validation_issues: Vec<String>,
}

/// Estimate resource requirements for a node sequence without executing it.
pub fn estimate_resource_usage(nodes: &[NdaNode]) -> ResourceEstimate {
    let mut matrix_count = 0usize;
    let mut norm_count = 0usize;
    let mut loop_count = 0usize;
    let mut variable_count = 0usize;
    let mut max_depth = 0usize;

    fn walk(
        node: &NdaNode,
        depth: usize,
        max_depth: &mut usize,
        matrices: &mut usize,
        norms: &mut usize,
        loops: &mut usize,
        vars: &mut usize,
    ) {
        if depth > *max_depth {
            *max_depth = depth;
        }
        match node {
            NdaNode::Matrix { .. } => *matrices += 1,
            NdaNode::Norm { .. } => *norms += 1,
            NdaNode::Loop { body, .. } => {
                *loops += 1;
                for child in body {
                    walk(child, depth + 1, max_depth, matrices, norms, loops, vars);
                }
            }
            NdaNode::While { body, cond, .. } => {
                *loops += 1;
                walk(cond, depth + 1, max_depth, matrices, norms, loops, vars);
                for child in body {
                    walk(child, depth + 1, max_depth, matrices, norms, loops, vars);
                }
            }
            NdaNode::If {
                cond,
                then_body,
                else_body,
            } => {
                walk(cond, depth + 1, max_depth, matrices, norms, loops, vars);
                for child in then_body {
                    walk(child, depth + 1, max_depth, matrices, norms, loops, vars);
                }
                if let Some(eb) = else_body {
                    for child in eb {
                        walk(child, depth + 1, max_depth, matrices, norms, loops, vars);
                    }
                }
            }
            NdaNode::Scope { children } => {
                for child in children {
                    walk(child, depth + 1, max_depth, matrices, norms, loops, vars);
                }
            }
            NdaNode::Let { init, .. } => {
                *vars += 1;
                walk(init, depth + 1, max_depth, matrices, norms, loops, vars);
            }
            NdaNode::Store { value, .. } => {
                *vars += 1;
                walk(value, depth + 1, max_depth, matrices, norms, loops, vars);
            }
            NdaNode::Compare { lhs, rhs, .. }
            | NdaNode::Add { lhs, rhs }
            | NdaNode::Math { lhs, rhs, .. }
            | NdaNode::Dot { lhs, rhs }
            | NdaNode::Poke { addr: lhs, value: rhs }
            | NdaNode::Gemv { matrix: lhs, vector: rhs }
            | NdaNode::Atomic { addr: lhs, val: rhs, .. } => {
                walk(lhs, depth + 1, max_depth, matrices, norms, loops, vars);
                walk(rhs, depth + 1, max_depth, matrices, norms, loops, vars);
            }
            NdaNode::Bitwise { lhs, rhs, .. } => {
                walk(lhs, depth + 1, max_depth, matrices, norms, loops, vars);
                if let Some(r) = rhs {
                    walk(r, depth + 1, max_depth, matrices, norms, loops, vars);
                }
            }
            NdaNode::VecOp { operand, .. }
            | NdaNode::Print { source: operand }
            | NdaNode::Return { value: operand }
            | NdaNode::MathFunc { operand, .. }
            | NdaNode::Peek { addr: operand }
            | NdaNode::Alloc { size: operand }
            | NdaNode::Free { addr: operand }
            | NdaNode::Cast { operand, .. } => {
                walk(operand, depth + 1, max_depth, matrices, norms, loops, vars);
            }
            NdaNode::Syscall { args, .. } | NdaNode::GpuDispatch { args, .. } => {
                for arg in args {
                    walk(arg, depth + 1, max_depth, matrices, norms, loops, vars);
                }
            }
            // Leaf nodes.
            NdaNode::Call { .. }
            | NdaNode::Int { .. }
            | NdaNode::Float { .. }
            | NdaNode::Load { .. }
            | NdaNode::Break
            | NdaNode::Spawn { .. }
            | NdaNode::RegInt { .. }
            | NdaNode::Triple { .. } => {}
        }
    }

    for node in nodes {
        walk(
            node,
            0,
            &mut max_depth,
            &mut matrix_count,
            &mut norm_count,
            &mut loop_count,
            &mut variable_count,
        );
    }

    // Rough memory estimate: each node ~128 bytes + matrix/norm buffers.
    let estimated_memory = nodes.len() * 128 + matrix_count * 4096 + norm_count * 512;

    let mut issues = Vec::new();
    if nodes.is_empty() {
        issues.push("empty node sequence".to_string());
    }
    if loop_count > 100 {
        issues.push(format!("deeply nested loops: {} loop nodes", loop_count));
    }
    if max_depth > 50 {
        issues.push(format!("very deep tree: depth {}", max_depth));
    }

    ResourceEstimate {
        node_count: nodes.len(),
        matrix_count,
        norm_count,
        loop_count,
        variable_count,
        max_depth,
        estimated_memory_bytes: estimated_memory,
        validation_issues: issues,
    }
}

pub struct NdaSandbox;

impl NdaSandbox {
    pub fn run(nodes: &[NdaNode], conditioning_vec: &[f32], site_map: &SiteMap) -> SandboxResult {
        let t_start = Instant::now();

        // Pre-execution credential boundary audit.
        // The sandbox isolates computation but NOT the inherited environment.
        // Log a warning if credentials are still accessible to JIT code.
        let boundary = crate::credential_guard::CredentialBoundaryAudit::run();
        if let Some(warning) = boundary.warning_message() {
            log::warn!("{}", warning);
        }

        let executed_nodes = 0;
        let matrix_count = 0;
        let norm_count = 0;

        // Convert conditioning_vec to NdaVec
        let current_vec = NdaVec::from_f32_slice(conditioning_vec);

        // Pre-register variable name hashes to slot indexes
        let registry = crate::compiler::nda_jit::VarRegistry::new();
        for node in nodes {
            crate::compiler::nda_jit::pre_register_variables(node, &registry);
        }
        let total_slots = registry.total_slots();

        // We use AssertUnwindSafe because we capture references.
        // Catch panics to guarantee the program never crashes during sandboxing of generated code.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut state = ExecutionState {
                current_vec,
                registry,
                variables: vec![None; total_slots],
                executed_nodes: 0,
                matrix_count: 0,
                norm_count: 0,
                loop_count: 0,
                output_log: Vec::new(),
                kind_counts: HashMap::new(),
                loop_iterations: 0,
            };
            match state.execute_sequence(nodes, site_map) {
                Ok(_) => Ok(state),
                Err(e) => Err(e),
            }
        }));

        let elapsed_us = t_start.elapsed().as_micros() as u64;

        match result {
            Ok(Ok(state)) => {
                let out_f32 = state.current_vec.to_f32_vec();
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
                    kind_counts: state.kind_counts,
                    output_log: state.output_log,
                    loop_iterations: state.loop_iterations,
                }
            }
            Ok(Err(err_msg)) => SandboxResult {
                executed_nodes,
                matrix_count,
                norm_count,
                output_vec: vec![],
                output_dim: 0,
                panicked: false,
                error: Some(err_msg),
                elapsed_us,
                kind_counts: HashMap::new(),
                output_log: Vec::new(),
                loop_iterations: 0,
            },
            Err(panic_err) => {
                let err_msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_err.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic during NDA execution".to_string()
                };
                SandboxResult {
                    executed_nodes,
                    matrix_count,
                    norm_count,
                    output_vec: vec![],
                    output_dim: 0,
                    panicked: true,
                    error: Some(format!("Panic: {}", err_msg)),
                    elapsed_us,
                    kind_counts: HashMap::new(),
                    output_log: Vec::new(),
                    loop_iterations: 0,
                }
            }
        }
    }

    /// Execute multiple node sequences in batch and return a structured report.
    pub fn run_batch(
        runs: &[(&[NdaNode], &[f32])],
        site_map: &SiteMap,
    ) -> (Vec<SandboxResult>, SandboxBatchReport) {
        let t_start = Instant::now();
        let mut results = Vec::with_capacity(runs.len());
        let mut per_run_summaries = Vec::with_capacity(runs.len());
        let mut successful = 0;
        let mut failed = 0;
        let mut total_nodes_executed = 0;

        for (nodes, conditioning_vec) in runs {
            let result = Self::run(nodes, conditioning_vec, site_map);
            if result.is_success() {
                successful += 1;
            } else {
                failed += 1;
            }
            total_nodes_executed += result.executed_nodes;
            per_run_summaries.push(result.execution_summary());
            results.push(result);
        }

        let total_elapsed_us = t_start.elapsed().as_micros() as u64;

        let report = SandboxBatchReport {
            total_runs: runs.len(),
            successful,
            failed,
            total_elapsed_us,
            total_nodes_executed,
            per_run_summaries,
        };

        (results, report)
    }
}

/// Maximum iterations for `While` loops (safety limit to prevent infinite loops).
const MAX_LOOP_ITERATIONS: u32 = 10_000_000;

/// Signal for early exit from loops or functions.
#[derive(Debug)]
enum ControlFlow {
    Continue,
    Break,
    Return,
}

struct ExecutionState {
    current_vec: NdaVec,
    registry: crate::compiler::nda_jit::VarRegistry,
    /// Variable bindings: slot_index → Option<NdaVec>
    variables: Vec<Option<NdaVec>>,
    executed_nodes: usize,
    matrix_count: usize,
    norm_count: usize,
    loop_count: usize,
    /// Accumulated print output (collected, not printed during sandbox execution).
    output_log: Vec<String>,
    /// Per-node-kind execution counts.
    kind_counts: HashMap<String, usize>,
    /// Total loop iterations executed.
    loop_iterations: usize,
}

fn jit_val_to_nda_vec(jv: crate::compiler::nda_jit::JitVal) -> NdaVec {
    match jv {
        crate::compiler::nda_jit::JitVal::Vector(v) => (*v).clone(),
        crate::compiler::nda_jit::JitVal::Float(val) => NdaVec::from_f32_slice(&[val]),
        crate::compiler::nda_jit::JitVal::Scalar(val, scale) => {
            let actual = (val as f32) * 2.0f32.powi(scale as i32);
            NdaVec::from_f32_slice(&[actual])
        }
    }
}

impl ExecutionState {
    fn execute_sequence(
        &mut self,
        nodes: &[NdaNode],
        site_map: &SiteMap,
    ) -> Result<ControlFlow, String> {
        for node in nodes {
            let cf = self.execute_node(node, site_map)?;
            match cf {
                ControlFlow::Continue => {}
                ControlFlow::Break | ControlFlow::Return => return Ok(cf),
            }
        }
        Ok(ControlFlow::Continue)
    }

    /// Evaluate a node and return its result as an NdaVec, without modifying current_vec.
    /// Used for sub-expressions (conditions, arguments, etc.).
    fn eval_node(&mut self, node: &NdaNode, site_map: &SiteMap) -> Result<NdaVec, String> {
        let saved = self.current_vec.clone();
        self.execute_node(node, site_map)?;
        let result = self.current_vec.clone();
        self.current_vec = saved;
        Ok(result)
    }

    /// Check if a vector is "truthy": sum of raw values > 0.
    /// A vector of all +1/+2 is truthy. A vector of all -1/-2 is falsy.
    fn is_truthy(v: &NdaVec) -> bool {
        crate::compiler::nda_jit::JitState::is_truthy(v)
    }

    fn execute_node(&mut self, node: &NdaNode, site_map: &SiteMap) -> Result<ControlFlow, String> {
        self.executed_nodes += 1;
        let kind = node_kind_name(node);
        *self.kind_counts.entry(kind).or_insert(0) += 1;
        match node {
            // ── Original computation nodes ────────────────────────────────
            NdaNode::Matrix {
                rows,
                cols,
                scale,
                sign,
                extra,
            } => {
                let r = *rows as usize;
                let c = *cols as usize;
                if self.current_vec.len != c {
                    return Err(format!(
                        "Dimension mismatch in Matrix: input len {} != matrix cols {}",
                        self.current_vec.len, c
                    ));
                }

                let scale_f32 = 2.0f32.powi(*scale as i32);
                let mat = NdaMatrix::new_quad(r, c, scale_f32, sign.clone(), extra.clone());

                self.current_vec = crate::nda_int::nda_gemv_nda_to_nda(&mat, &self.current_vec);
                self.matrix_count += 1;
            }
            NdaNode::Norm { size, weight, bias } => {
                let sz = *size as usize;
                if self.current_vec.len != sz {
                    return Err(format!(
                        "Dimension mismatch in Norm: input len {} != norm size {}",
                        self.current_vec.len, sz
                    ));
                }

                let w_vec = NdaVec {
                    len: sz,
                    log2_scale: 0,
                    sign: weight.clone().into(),
                    extra: bias.clone().into(),
                };

                self.current_vec = crate::nda_int::rms_norm_nda(&self.current_vec, &w_vec, 14);
                self.norm_count += 1;
            }
            NdaNode::Call { target } => {
                if let Some(target_node) = site_map.get_node(*target) {
                    self.execute_node(&target_node, site_map)?;
                }
                // If not found in site map, treat as identity (do nothing).
            }
            NdaNode::Int { value } => {
                self.current_vec = NdaVec::from_i32_slice(&[*value], 0);
            }
            NdaNode::Scope { children } => {
                let cf = self.execute_sequence(children, site_map)?;
                if matches!(cf, ControlFlow::Return) {
                    return Ok(cf);
                }
            }

            // ── Control flow ─────────────────────────────────────────────
            NdaNode::Loop { count, body } => {
                self.loop_count += 1;
                for _ in 0..*count {
                    let cf = self.execute_sequence(body, site_map)?;
                    self.loop_iterations += 1;
                    match cf {
                        ControlFlow::Break => break,
                        ControlFlow::Return => return Ok(ControlFlow::Return),
                        ControlFlow::Continue => {}
                    }
                }
            }
            NdaNode::While { cond, body } => {
                self.loop_count += 1;
                let mut iterations = 0u32;
                loop {
                    // Evaluate condition
                    let cond_val = self.eval_node(cond, site_map)?;
                    if !Self::is_truthy(&cond_val) {
                        break;
                    }
                    // Execute body
                    let cf = self.execute_sequence(body, site_map)?;
                    self.loop_iterations += 1;
                    match cf {
                        ControlFlow::Break => break,
                        ControlFlow::Return => return Ok(ControlFlow::Return),
                        ControlFlow::Continue => {}
                    }
                    iterations += 1;
                    if iterations >= MAX_LOOP_ITERATIONS {
                        return Err(format!(
                            "While loop exceeded MAX_LOOP_ITERATIONS ({})",
                            MAX_LOOP_ITERATIONS
                        ));
                    }
                }
            }
            NdaNode::If {
                cond,
                then_body,
                else_body,
            } => {
                let cond_val = self.eval_node(cond, site_map)?;
                if Self::is_truthy(&cond_val) {
                    let cf = self.execute_sequence(then_body, site_map)?;
                    if !matches!(cf, ControlFlow::Continue) {
                        return Ok(cf);
                    }
                } else if let Some(eb) = else_body {
                    let cf = self.execute_sequence(eb, site_map)?;
                    if !matches!(cf, ControlFlow::Continue) {
                        return Ok(cf);
                    }
                }
            }
            NdaNode::Compare { op, lhs, rhs } => {
                let lhs_val = self.eval_node(lhs, site_map)?;
                let rhs_val = self.eval_node(rhs, site_map)?;

                // Wrap in JitVal
                let lhs_jv = crate::compiler::nda_jit::JitVal::Vector(std::sync::Arc::new(lhs_val));
                let rhs_jv = crate::compiler::nda_jit::JitVal::Vector(std::sync::Arc::new(rhs_val));

                // Invoke fast byte-level compare
                let res_jv = crate::compiler::nda_jit::compare_vals(*op, &lhs_jv, &rhs_jv);

                // Unwrap resulting NdaVec
                self.current_vec = jit_val_to_nda_vec(res_jv);
            }
            NdaNode::Break => {
                return Ok(ControlFlow::Break);
            }

            // ── Variables ────────────────────────────────────────────────
            NdaNode::Let { name_hash, init } => {
                let val = self.eval_node(init, site_map)?;
                let slot = self.registry.get_or_create_slot(*name_hash);
                if slot >= self.variables.len() {
                    self.variables.resize(slot + 1, None);
                }
                self.variables[slot] = Some(val.clone());
                self.current_vec = val;
            }
            NdaNode::Load { name_hash } => {
                let slot = self.registry.get_or_create_slot(*name_hash);
                if let Some(Some(val)) = self.variables.get(slot) {
                    self.current_vec = val.clone();
                } else {
                    return Err(format!("Undefined variable: {:016x}", name_hash));
                }
            }
            NdaNode::Store { name_hash, value } => {
                let val = self.eval_node(value, site_map)?;
                let slot = self.registry.get_or_create_slot(*name_hash);
                if slot >= self.variables.len() {
                    self.variables.resize(slot + 1, None);
                }
                self.variables[slot] = Some(val.clone());
                self.current_vec = val;
            }

            // ── Arithmetic ──────────────────────────────────────────────
            NdaNode::Add { lhs, rhs } => {
                let lhs_val = self.eval_node(lhs, site_map)?;
                let rhs_val = self.eval_node(rhs, site_map)?;

                let lhs_jv = crate::compiler::nda_jit::JitVal::Vector(std::sync::Arc::new(lhs_val));
                let rhs_jv = crate::compiler::nda_jit::JitVal::Vector(std::sync::Arc::new(rhs_val));

                let res_jv = crate::compiler::nda_jit::add_vals(&lhs_jv, &rhs_jv);
                self.current_vec = jit_val_to_nda_vec(res_jv);
            }
            NdaNode::VecOp { op, operand } => {
                let val = self.eval_node(operand, site_map)?;
                let jv = crate::compiler::nda_jit::JitVal::Vector(std::sync::Arc::new(val));
                let res_jv = crate::compiler::nda_jit::apply_vec_op(*op, &jv);
                self.current_vec = jit_val_to_nda_vec(res_jv);
            }

            // ── I/O ─────────────────────────────────────────────────────
            NdaNode::Print { source } => {
                let val = self.eval_node(source, site_map)?;
                let f32_vals = val.to_f32_vec();
                let formatted = if f32_vals.len() == 1 {
                    format!("{}", f32_vals[0])
                } else if f32_vals.len() <= 16 {
                    format!("{:?}", f32_vals)
                } else {
                    format!("[{} elements: {:?}...]", f32_vals.len(), &f32_vals[..8])
                };
                self.output_log.push(formatted.clone());
                eprintln!("[nda] print: {}", formatted);
            }
            NdaNode::Return { value } => {
                let val = self.eval_node(value, site_map)?;
                self.current_vec = val;
                return Ok(ControlFlow::Return);
            }
            NdaNode::Bitwise { op, lhs, rhs } => {
                let l = self.eval_node(lhs, site_map)?;
                if *op == BitwiseOp::Not {
                    let sign: Vec<u8> = l.sign.iter().map(|b| !b).collect();
                    let extra: Vec<u8> = l.extra.iter().map(|b| !b).collect();
                    self.current_vec = NdaVec {
                        len: l.len,
                        log2_scale: l.log2_scale,
                        sign: sign.into(),
                        extra: extra.into(),
                    };
                } else if let Some(r_node) = rhs {
                    let r = self.eval_node(r_node, site_map)?;
                    // Element-wise bitwise over the vectors' raw integer codes,
                    // re-encoded into the quaternary NDA representation.
                    let len = l.len.min(r.len);
                    let out: Vec<i32> = (0..len)
                        .map(|i| {
                            let a = l.get_raw(i);
                            let b = r.get_raw(i);
                            match op {
                                BitwiseOp::And => a & b,
                                BitwiseOp::Or => a | b,
                                BitwiseOp::Xor => a ^ b,
                                BitwiseOp::Shl => a.wrapping_shl(b as u32),
                                BitwiseOp::Shr => a.wrapping_shr(b as u32),
                                BitwiseOp::Not => !a,
                            }
                        })
                        .collect();
                    self.current_vec = NdaVec::from_i32_slice(&out, l.log2_scale);
                }
            }
            NdaNode::Float { value } => {
                self.current_vec = NdaVec::from_f32_slice(&[*value]);
            }
            NdaNode::Math { op, lhs, rhs } => {
                let l = self.eval_node(lhs, site_map)?.to_f32_vec();
                let r = self.eval_node(rhs, site_map)?.to_f32_vec();
                // Element-wise arithmetic with scalar broadcast (len-1 operand).
                let n = l.len().max(r.len());
                let pick = |v: &[f32], i: usize| -> f32 {
                    if v.len() == 1 {
                        v[0]
                    } else {
                        v.get(i).copied().unwrap_or(0.0)
                    }
                };
                let out: Vec<f32> = (0..n)
                    .map(|i| {
                        let a = pick(&l, i);
                        let b = pick(&r, i);
                        match op {
                            MathOp::Add => a + b,
                            MathOp::Sub => a - b,
                            MathOp::Mul => a * b,
                            MathOp::Div => {
                                if b != 0.0 {
                                    a / b
                                } else {
                                    0.0
                                }
                            }
                        }
                    })
                    .collect();
                self.current_vec = NdaVec::from_f32_slice(&out);
            }
            NdaNode::MathFunc { func, operand } => {
                let val = self.eval_node(operand, site_map)?;
                let f32s = val.to_f32_vec();
                let res: Vec<f32> = f32s
                    .iter()
                    .map(|&x| match func {
                        MathFuncKind::Sin => x.sin(),
                        MathFuncKind::Cos => x.cos(),
                        MathFuncKind::Sqrt => x.sqrt(),
                        MathFuncKind::Exp => x.exp(),
                    })
                    .collect();
                self.current_vec = NdaVec::from_f32_slice(&res);
            }
            NdaNode::Peek { addr } => {
                let _a = self.eval_node(addr, site_map)?;
                self.current_vec = NdaVec::from_f32_slice(&[0.0]);
            }
            NdaNode::Poke { addr, value } => {
                let _a = self.eval_node(addr, site_map)?;
                let _v = self.eval_node(value, site_map)?;
            }
            NdaNode::Gemv { matrix, vector } => {
                let mat_vec = self.eval_node(matrix, site_map)?;
                let vec = self.eval_node(vector, site_map)?;
                let cols = vec.len;
                if let Some(rows) = mat_vec.len.checked_div(cols) {
                    let n_mat = NdaMatrix::new_quad(
                        rows,
                        cols,
                        2.0f32.powi(mat_vec.log2_scale as i32),
                        mat_vec.sign.to_vec(),
                        mat_vec.extra.to_vec(),
                    );
                    self.current_vec = crate::nda_int::nda_gemv_nda_to_nda(&n_mat, &vec);
                }
            }
            NdaNode::Dot { lhs, rhs } => {
                let l = self.eval_node(lhs, site_map)?;
                let r = self.eval_node(rhs, site_map)?;
                let l_f = l.to_f32_vec();
                let r_f = r.to_f32_vec();
                let dot: f32 = l_f.iter().zip(r_f.iter()).map(|(x, y)| x * y).sum();
                self.current_vec = NdaVec::from_f32_slice(&[dot]);
            }
            NdaNode::Syscall { args, .. } => {
                for arg in args {
                    let _ = self.eval_node(arg, site_map)?;
                }
                self.current_vec = NdaVec::from_f32_slice(&[0.0]);
            }
            NdaNode::Spawn { .. } => {
                self.current_vec = NdaVec::from_f32_slice(&[1.0]);
            }
            NdaNode::Atomic { val, .. } => {
                let v = self.eval_node(val, site_map)?;
                self.current_vec = v;
            }
            NdaNode::Alloc { size } => {
                let _s = self.eval_node(size, site_map)?;
                self.current_vec = NdaVec::from_f32_slice(&[2048.0]);
            }
            NdaNode::Free { addr } => {
                let _a = self.eval_node(addr, site_map)?;
            }
            NdaNode::RegInt { .. } => {}
            NdaNode::Cast { operand, .. } => {
                let val = self.eval_node(operand, site_map)?;
                self.current_vec = val;
            }
            NdaNode::GpuDispatch { args, .. } => {
                for arg in args {
                    let _ = self.eval_node(arg, site_map)?;
                }
                self.current_vec = NdaVec::from_f32_slice(&[1.0]);
            }
            NdaNode::Triple { .. } => {
                // Triple nodes represent semantic metadata and are ignored during evaluation.
            }
        }
        Ok(ControlFlow::Continue)
    }
}

// ─── Node kind classification ──────────────────────────────────────────────────

/// Return a human-readable kind name for a node (used in profiling counts).
fn node_kind_name(node: &NdaNode) -> String {
    match node {
        NdaNode::Matrix { .. } => "Matrix".into(),
        NdaNode::Norm { .. } => "Norm".into(),
        NdaNode::Call { .. } => "Call".into(),
        NdaNode::Int { .. } => "Int".into(),
        NdaNode::Scope { .. } => "Scope".into(),
        NdaNode::Loop { .. } => "Loop".into(),
        NdaNode::While { .. } => "While".into(),
        NdaNode::If { .. } => "If".into(),
        NdaNode::Compare { .. } => "Compare".into(),
        NdaNode::Let { .. } => "Let".into(),
        NdaNode::Load { .. } => "Load".into(),
        NdaNode::Store { .. } => "Store".into(),
        NdaNode::Add { .. } => "Add".into(),
        NdaNode::VecOp { .. } => "VecOp".into(),
        NdaNode::Print { .. } => "Print".into(),
        NdaNode::Return { .. } => "Return".into(),
        NdaNode::Break => "Break".into(),
        NdaNode::Bitwise { .. } => "Bitwise".into(),
        NdaNode::Float { .. } => "Float".into(),
        NdaNode::Math { .. } => "Math".into(),
        NdaNode::MathFunc { .. } => "MathFunc".into(),
        NdaNode::Peek { .. } => "Peek".into(),
        NdaNode::Poke { .. } => "Poke".into(),
        NdaNode::Gemv { .. } => "Gemv".into(),
        NdaNode::Dot { .. } => "Dot".into(),
        NdaNode::Syscall { .. } => "Syscall".into(),
        NdaNode::Spawn { .. } => "Spawn".into(),
        NdaNode::Atomic { .. } => "Atomic".into(),
        NdaNode::Alloc { .. } => "Alloc".into(),
        NdaNode::Free { .. } => "Free".into(),
        NdaNode::RegInt { .. } => "RegInt".into(),
        NdaNode::Cast { .. } => "Cast".into(),
        NdaNode::GpuDispatch { .. } => "GpuDispatch".into(),
        NdaNode::Triple { .. } => "Triple".into(),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site_map::verifier::NdaNode;

    #[test]
    fn sandbox_chains_matrix_output_to_norm_input() {
        let input = vec![1.0f32; 896];
        let site_map = SiteMap::open(&std::env::temp_dir().join("sandbox_test_sm_1"), 0).unwrap();

        let m1 = NdaNode::Matrix {
            rows: 128,
            cols: 896,
            scale: 0,
            sign: vec![0xAA; 128 * 112],
            extra: vec![0x55; 128 * 112],
        };

        let n1 = NdaNode::Norm {
            size: 128,
            weight: vec![0xFF; 16],
            bias: vec![0x00; 16],
        };

        let result = NdaSandbox::run(&[m1, n1], &input, &site_map);
        assert!(!result.panicked);
        assert!(result.error.is_none());
        assert_eq!(result.output_dim, 128);
        assert_eq!(result.executed_nodes, 2);
    }

    #[test]
    fn sandbox_catches_shape_panic_in_catch_unwind() {
        let input = vec![1.0f32; 896];
        let site_map = SiteMap::open(&std::env::temp_dir().join("sandbox_test_sm_2"), 0).unwrap();

        let m1 = NdaNode::Matrix {
            rows: 128,
            cols: 128,
            scale: 0,
            sign: vec![0xAA; 128 * 16],
            extra: vec![0x55; 128 * 16],
        };

        let result = NdaSandbox::run(&[m1], &input, &site_map);
        assert!(!result.panicked);
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("Dimension mismatch"));
    }

    #[test]
    fn scope_rejects_orthogonal_output() {
        use super::scope_validator::ScopeValidator;
        let cond = vec![1.0f32; 10];
        let out = vec![-1.0f32; 10];
        let val = ScopeValidator::validate(&out, &cond, 0.1);
        assert!(!val.passed);
        assert!(val.similarity < 0.0);
    }

    #[test]
    fn scope_accepts_aligned_output() {
        use super::scope_validator::ScopeValidator;
        let cond = vec![1.0f32, 2.0, 3.0];
        let out = vec![1.0f32, 2.0, 3.0];
        let val = ScopeValidator::validate(&out, &cond, 0.5);
        assert!(val.passed);
        assert!((val.similarity - 1.0).abs() < 1e-5);
    }

    #[test]
    #[allow(clippy::needless_range_loop)]
    fn test_nda_vs_native_rust_performance() {
        use std::time::Instant;

        let input = vec![1.0f32; 896];
        let site_map = SiteMap::open(&std::env::temp_dir().join("sandbox_perf_sm"), 0).unwrap();

        let mut nodes = Vec::new();

        let mut shapes = Vec::new();
        shapes.push((128usize, 896usize));
        for _ in 0..22 {
            shapes.push((128, 128));
        }
        shapes.push((896, 128));

        for &(r, c) in &shapes {
            let bitmap_bytes = r * c.div_ceil(8);
            nodes.push(NdaNode::Matrix {
                rows: r as u16,
                cols: c as u16,
                scale: 0,
                sign: vec![0xAA; bitmap_bytes],
                extra: vec![0x55; bitmap_bytes],
            });
        }

        // Warmup
        let _ = NdaSandbox::run(&nodes, &input, &site_map);

        let iters = 50;
        let t0 = Instant::now();
        for _ in 0..iters {
            let res = NdaSandbox::run(&nodes, &input, &site_map);
            std::hint::black_box(res);
        }
        let sandbox_duration = t0.elapsed() / iters;

        let mut native_mats = Vec::new();
        for &(r, c) in &shapes {
            let bitmap_bytes = r * c.div_ceil(8);
            native_mats.push(NdaMatrix::new_quad(
                r,
                c,
                1.0,
                vec![0xAA; bitmap_bytes],
                vec![0x55; bitmap_bytes],
            ));
        }

        let mut current_vec = NdaVec::from_f32_slice(&input);
        for mat in &native_mats {
            current_vec = crate::nda_int::nda_gemv_nda_to_nda(mat, &current_vec);
        }

        let t1 = Instant::now();
        for _ in 0..iters {
            let mut vec = NdaVec::from_f32_slice(&input);
            for mat in &native_mats {
                vec = crate::nda_int::nda_gemv_nda_to_nda(mat, &vec);
            }
            std::hint::black_box(vec);
        }
        let native_duration = t1.elapsed() / iters;

        let mut f32_mats = Vec::new();
        for &(r, c) in &shapes {
            f32_mats.push(vec![0.5f32; r * c]);
        }

        let mut f32_vec = input.clone();
        for (i, &(r, c)) in shapes.iter().enumerate() {
            let mut out = vec![0.0f32; r];
            let mat = &f32_mats[i];
            for row in 0..r {
                let mut sum = 0.0;
                let base = row * c;
                for col in 0..c {
                    sum += mat[base + col] * f32_vec[col];
                }
                out[row] = sum;
            }
            f32_vec = out;
        }

        let t2 = Instant::now();
        for _ in 0..iters {
            let mut vec = input.clone();
            for (i, &(r, c)) in shapes.iter().enumerate() {
                let mut out = vec![0.0f32; r];
                let mat = &f32_mats[i];
                for row in 0..r {
                    let mut sum = 0.0;
                    let base = row * c;
                    for col in 0..c {
                        sum += mat[base + col] * vec[col];
                    }
                    out[row] = sum;
                }
                vec = out;
            }
            std::hint::black_box(vec);
        }
        let f32_duration = t2.elapsed() / iters;

        println!();
        println!("Execution comparison for a 24-layer Matrix Chain:");
        println!("  1. NDA Sandbox (Interpretive) : {:?}", sandbox_duration);
        println!("  2. NDA Native Rust (Direct)   : {:?}", native_duration);
        println!("  3. Standard F32 GEMV (Direct) : {:?}", f32_duration);
        println!(
            "  - Sandbox overhead            : {:.1}%",
            (sandbox_duration.as_nanos() as f64 / native_duration.as_nanos() as f64 - 1.0) * 100.0
        );
        println!(
            "  - NDA Speedup vs F32 GEMV     : {:.1}x",
            f32_duration.as_nanos() as f64 / native_duration.as_nanos() as f64
        );
        println!();
    }

    #[test]
    fn sandbox_result_tracks_kind_counts() {
        let input = vec![1.0f32; 896];
        let site_map = SiteMap::open(&std::env::temp_dir().join("sandbox_kind_sm"), 0).unwrap();

        let m1 = NdaNode::Matrix {
            rows: 128,
            cols: 896,
            scale: 0,
            sign: vec![0xAA; 128 * 112],
            extra: vec![0x55; 128 * 112],
        };
        let n1 = NdaNode::Norm {
            size: 128,
            weight: vec![0xFF; 16],
            bias: vec![0x00; 16],
        };

        let result = NdaSandbox::run(&[m1, n1], &input, &site_map);
        assert!(result.is_success());
        assert_eq!(*result.kind_counts.get("Matrix").unwrap_or(&0), 1);
        assert_eq!(*result.kind_counts.get("Norm").unwrap_or(&0), 1);
        assert_eq!(result.executed_nodes, 2);
    }

    #[test]
    fn sandbox_result_top_kinds() {
        let mut result = SandboxResult {
            executed_nodes: 10,
            matrix_count: 3,
            norm_count: 2,
            output_vec: vec![],
            output_dim: 0,
            panicked: false,
            error: None,
            elapsed_us: 100,
            kind_counts: HashMap::new(),
            output_log: Vec::new(),
            loop_iterations: 0,
        };
        result.kind_counts.insert("Matrix".into(), 5);
        result.kind_counts.insert("Norm".into(), 3);
        result.kind_counts.insert("Int".into(), 2);

        let top = result.top_kinds(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "Matrix");
        assert_eq!(top[0].1, 5);
        assert_eq!(top[1].0, "Norm");
        assert_eq!(top[1].1, 3);
    }

    #[test]
    fn sandbox_result_is_success() {
        let ok = SandboxResult {
            executed_nodes: 1,
            matrix_count: 0,
            norm_count: 0,
            output_vec: vec![1.0],
            output_dim: 1,
            panicked: false,
            error: None,
            elapsed_us: 10,
            kind_counts: HashMap::new(),
            output_log: Vec::new(),
            loop_iterations: 0,
        };
        assert!(ok.is_success());

        let panicked = SandboxResult {
            panicked: true,
            error: Some("Panic: test".into()),
            ..ok.clone()
        };
        assert!(!panicked.is_success());

        let errored = SandboxResult {
            panicked: false,
            error: Some("Dimension mismatch".into()),
            ..ok.clone()
        };
        assert!(!errored.is_success());
    }

    #[test]
    fn sandbox_result_serializable() {
        let result = SandboxResult {
            executed_nodes: 5,
            matrix_count: 2,
            norm_count: 1,
            output_vec: vec![1.0, 2.0],
            output_dim: 2,
            panicked: false,
            error: None,
            elapsed_us: 500,
            kind_counts: {
                let mut m = HashMap::new();
                m.insert("Matrix".into(), 2);
                m.insert("Norm".into(), 1);
                m
            },
            output_log: vec!["42".into()],
            loop_iterations: 0,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"executed_nodes\":5"));
        assert!(json.contains("\"output_log\":[\"42\"]"));
    }

    #[test]
    fn scope_validation_has_distance_metrics() {
        use super::scope_validator::ScopeValidator;
        let cond = vec![1.0f32, 2.0, 3.0];
        let out = vec![1.0f32, 2.0, 3.0];
        let val = ScopeValidator::validate(&out, &cond, 0.5);
        assert!(val.passed);
        assert!((val.euclidean_distance - 0.0).abs() < 1e-5);
        assert!((val.manhattan_distance - 0.0).abs() < 1e-5);
        assert_eq!(val.vector_dim, 3);
    }

    #[test]
    fn scope_validation_distance_for_different_vecs() {
        use super::scope_validator::ScopeValidator;
        let cond = vec![0.0f32; 4];
        let out = vec![1.0f32, 0.0, 0.0, 0.0];
        let val = ScopeValidator::validate(&out, &cond, 0.5);
        // Euclidean: sqrt(1) = 1.0
        assert!((val.euclidean_distance - 1.0).abs() < 1e-5);
        // Manhattan: |1| + |0| + |0| + |0| = 1.0
        assert!((val.manhattan_distance - 1.0).abs() < 1e-5);
        assert_eq!(val.vector_dim, 4);
    }

    #[test]
    fn scope_validation_serializable() {
        use super::scope_validator::ScopeValidator;
        let cond = vec![1.0f32, 2.0];
        let out = vec![1.0f32, 2.0];
        let val = ScopeValidator::validate(&out, &cond, 0.5);
        let json = serde_json::to_string(&val).unwrap();
        assert!(json.contains("\"euclidean_distance\""));
        assert!(json.contains("\"manhattan_distance\""));
        assert!(json.contains("\"vector_dim\":2"));
    }

    #[test]
    fn node_kind_name_covers_all_variants() {
        // Spot-check a few key kinds
        assert_eq!(node_kind_name(&NdaNode::Int { value: 0 }), "Int");
        assert_eq!(node_kind_name(&NdaNode::Break), "Break");
        let scope = NdaNode::Scope { children: vec![] };
        assert_eq!(node_kind_name(&scope), "Scope");
    }

    // ─── Sandbox validation & summary tests ────────────────────────────────

    #[test]
    fn sandbox_result_validate_clean() {
        let result = SandboxResult {
            executed_nodes: 5,
            matrix_count: 2,
            norm_count: 1,
            output_vec: vec![1.0, 2.0],
            output_dim: 2,
            panicked: false,
            error: None,
            elapsed_us: 500,
            kind_counts: HashMap::new(),
            output_log: Vec::new(),
            loop_iterations: 100,
        };
        let warnings = result.validate();
        assert!(warnings.is_empty());
    }

    #[test]
    fn sandbox_result_validate_detects_issues() {
        let result = SandboxResult {
            executed_nodes: 0,
            matrix_count: 0,
            norm_count: 0,
            output_vec: vec![],
            output_dim: 0,
            panicked: false,
            error: None,
            elapsed_us: 10,
            kind_counts: HashMap::new(),
            output_log: Vec::new(),
            loop_iterations: 0,
        };
        let warnings = result.validate();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.contains("No nodes")));
    }

    #[test]
    fn sandbox_result_validate_detects_panic() {
        let result = SandboxResult {
            executed_nodes: 1,
            matrix_count: 0,
            norm_count: 0,
            output_vec: vec![],
            output_dim: 0,
            panicked: true,
            error: Some("Panic: test".to_string()),
            elapsed_us: 10,
            kind_counts: HashMap::new(),
            output_log: Vec::new(),
            loop_iterations: 0,
        };
        let warnings = result.validate();
        assert!(warnings.iter().any(|w| w.contains("panicked")));
        assert!(warnings.iter().any(|w| w.contains("error")));
    }

    #[test]
    fn sandbox_execution_summary_serializes() {
        let summary = SandboxExecutionSummary {
            success: true,
            executed_nodes: 10,
            matrix_count: 5,
            norm_count: 3,
            output_dim: 128,
            elapsed_us: 1000,
            loop_iterations: 50,
            unique_kinds: 4,
            output_log_lines: 2,
            has_error: false,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"executed_nodes\":10"));
    }

    #[test]
    fn sandbox_batch_report_serializes() {
        let report = SandboxBatchReport {
            total_runs: 3,
            successful: 2,
            failed: 1,
            total_elapsed_us: 5000,
            total_nodes_executed: 30,
            per_run_summaries: vec![],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"total_runs\":3"));
        assert!(json.contains("\"successful\":2"));
        assert!(json.contains("\"failed\":1"));
    }

    #[test]
    fn sandbox_execution_summary_from_result() {
        let result = SandboxResult {
            executed_nodes: 5,
            matrix_count: 2,
            norm_count: 1,
            output_vec: vec![1.0],
            output_dim: 1,
            panicked: false,
            error: None,
            elapsed_us: 500,
            kind_counts: {
                let mut m = HashMap::new();
                m.insert("Matrix".into(), 2);
                m.insert("Norm".into(), 1);
                m
            },
            output_log: vec!["test".into()],
            loop_iterations: 10,
        };
        let summary = result.execution_summary();
        assert!(summary.success);
        assert_eq!(summary.executed_nodes, 5);
        assert_eq!(summary.unique_kinds, 2);
        assert_eq!(summary.output_log_lines, 1);
    }

    // ─── New diagnostic tests ──────────────────────────────────────────────────

    #[test]
    fn throughput_ops_per_sec_zero_elapsed() {
        let result = SandboxResult {
            executed_nodes: 10,
            matrix_count: 0,
            norm_count: 0,
            output_vec: vec![],
            output_dim: 0,
            panicked: false,
            error: None,
            elapsed_us: 0,
            kind_counts: HashMap::new(),
            output_log: Vec::new(),
            loop_iterations: 0,
        };
        assert_eq!(result.throughput_ops_per_sec(), 0.0);
    }

    #[test]
    fn throughput_ops_per_sec_normal() {
        let result = SandboxResult {
            executed_nodes: 1000,
            matrix_count: 0,
            norm_count: 0,
            output_vec: vec![],
            output_dim: 0,
            panicked: false,
            error: None,
            elapsed_us: 1_000_000, // 1 second
            kind_counts: HashMap::new(),
            output_log: Vec::new(),
            loop_iterations: 0,
        };
        let tps = result.throughput_ops_per_sec();
        assert!((tps - 1000.0).abs() < 1.0);
    }

    #[test]
    fn computation_ratio_no_nodes() {
        let result = SandboxResult {
            executed_nodes: 0,
            matrix_count: 0,
            norm_count: 0,
            output_vec: vec![],
            output_dim: 0,
            panicked: false,
            error: None,
            elapsed_us: 0,
            kind_counts: HashMap::new(),
            output_log: Vec::new(),
            loop_iterations: 0,
        };
        assert_eq!(result.computation_ratio(), 0.0);
    }

    #[test]
    fn computation_ratio_mixed() {
        let result = SandboxResult {
            executed_nodes: 10,
            matrix_count: 3,
            norm_count: 2,
            output_vec: vec![],
            output_dim: 0,
            panicked: false,
            error: None,
            elapsed_us: 100,
            kind_counts: HashMap::new(),
            output_log: Vec::new(),
            loop_iterations: 0,
        };
        let ratio = result.computation_ratio();
        assert!((ratio - 0.5).abs() < 0.01); // 5/10 = 0.5
    }

    #[test]
    fn execution_profile_serializes() {
        let result = SandboxResult {
            executed_nodes: 10,
            matrix_count: 3,
            norm_count: 2,
            output_vec: vec![1.0],
            output_dim: 1,
            panicked: false,
            error: None,
            elapsed_us: 500,
            kind_counts: {
                let mut m = HashMap::new();
                m.insert("Matrix".into(), 5);
                m.insert("Norm".into(), 3);
                m.insert("Int".into(), 2);
                m
            },
            output_log: vec!["line1".into()],
            loop_iterations: 10,
        };
        let profile = result.execution_profile();
        assert_eq!(profile.total_nodes, 10);
        assert_eq!(profile.unique_kinds, 3);
        assert_eq!(profile.top_kinds[0].0, "Matrix");
        assert_eq!(profile.top_kinds[0].1, 5);
        assert!(profile.throughput_ops > 0.0);
        let json = serde_json::to_string(&profile).unwrap();
        assert!(json.contains("\"total_nodes\":10"));
    }

    #[test]
    fn batch_report_success_rate() {
        let report = SandboxBatchReport {
            total_runs: 10,
            successful: 7,
            failed: 3,
            total_elapsed_us: 10000,
            total_nodes_executed: 100,
            per_run_summaries: vec![],
        };
        assert!((report.success_rate() - 0.7).abs() < 0.01);
        assert!((report.avg_elapsed_us() - 1000.0).abs() < 0.01);
        assert!((report.avg_nodes_per_run() - 10.0).abs() < 0.01);
    }

    #[test]
    fn batch_report_zero_runs() {
        let report = SandboxBatchReport {
            total_runs: 0,
            successful: 0,
            failed: 0,
            total_elapsed_us: 0,
            total_nodes_executed: 0,
            per_run_summaries: vec![],
        };
        assert_eq!(report.success_rate(), 0.0);
        assert_eq!(report.avg_elapsed_us(), 0.0);
        assert_eq!(report.avg_nodes_per_run(), 0.0);
    }

    #[test]
    fn batch_report_validate_clean() {
        let report = SandboxBatchReport {
            total_runs: 2,
            successful: 1,
            failed: 1,
            total_elapsed_us: 5000,
            total_nodes_executed: 20,
            per_run_summaries: vec![
                SandboxExecutionSummary {
                    success: true,
                    executed_nodes: 10,
                    matrix_count: 1,
                    norm_count: 0,
                    output_dim: 1,
                    elapsed_us: 2000,
                    loop_iterations: 0,
                    unique_kinds: 1,
                    output_log_lines: 0,
                    has_error: false,
                },
                SandboxExecutionSummary {
                    success: false,
                    executed_nodes: 10,
                    matrix_count: 0,
                    norm_count: 0,
                    output_dim: 0,
                    elapsed_us: 3000,
                    loop_iterations: 0,
                    unique_kinds: 0,
                    output_log_lines: 0,
                    has_error: true,
                },
            ],
        };
        let issues = report.validate();
        assert!(issues.is_empty());
    }

    #[test]
    fn batch_report_validate_detects_imbalance() {
        let report = SandboxBatchReport {
            total_runs: 3,
            successful: 1,
            failed: 1, // 1 + 1 != 3
            total_elapsed_us: 1000,
            total_nodes_executed: 10,
            per_run_summaries: vec![],
        };
        let issues = report.validate();
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| i.contains("success")));
    }

    #[test]
    fn batch_report_validate_detects_summary_mismatch() {
        let report = SandboxBatchReport {
            total_runs: 2,
            successful: 2,
            failed: 0,
            total_elapsed_us: 1000,
            total_nodes_executed: 10,
            per_run_summaries: vec![], // should have 2
        };
        let issues = report.validate();
        assert!(issues.iter().any(|i| i.contains("per_run_summaries")));
    }

    #[test]
    fn estimate_resource_empty_nodes() {
        let est = estimate_resource_usage(&[]);
        assert_eq!(est.node_count, 0);
        assert!(!est.validation_issues.is_empty());
    }

    #[test]
    fn estimate_resource_simple_program() {
        let nodes = vec![
            NdaNode::Int { value: 42 },
            NdaNode::Float { value: 3.14 },
        ];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.node_count, 2);
        assert_eq!(est.matrix_count, 0);
        assert_eq!(est.norm_count, 0);
        assert_eq!(est.loop_count, 0);
        assert!(est.validation_issues.is_empty());
    }

    #[test]
    fn estimate_resource_counts_matrices_and_norms() {
        let nodes = vec![
            NdaNode::Matrix {
                rows: 128,
                cols: 896,
                scale: 0,
                sign: vec![0xAA; 128 * 112],
                extra: vec![0x55; 128 * 112],
            },
            NdaNode::Norm {
                size: 128,
                weight: vec![0xFF; 16],
                bias: vec![0x00; 16],
            },
        ];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.matrix_count, 1);
        assert_eq!(est.norm_count, 1);
        assert!(est.estimated_memory_bytes > 0);
    }

    #[test]
    fn estimate_resource_counts_loops_and_vars() {
        let nodes = vec![
            NdaNode::Let {
                name_hash: 0x1234,
                init: Box::new(NdaNode::Int { value: 0 }),
            },
            NdaNode::Loop {
                count: 10,
                body: vec![NdaNode::Int { value: 1 }],
            },
        ];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.loop_count, 1);
        assert_eq!(est.variable_count, 1);
        assert!(est.max_depth > 0);
    }

    #[test]
    fn estimate_resource_nested_scopes() {
        let nodes = vec![NdaNode::Scope {
            children: vec![NdaNode::Scope {
                children: vec![NdaNode::Scope {
                    children: vec![NdaNode::Int { value: 1 }],
                }],
            }],
        }];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.max_depth, 3);
    }

    #[test]
    fn estimate_resource_serializes() {
        let nodes = vec![NdaNode::Int { value: 1 }];
        let est = estimate_resource_usage(&nodes);
        let json = serde_json::to_string(&est).unwrap();
        assert!(json.contains("\"node_count\":1"));
    }

    // ── Helper: build a SandboxResult quickly ────────────────────────────

    fn make_result(
        executed: usize, matrix: usize, norm: usize, dim: usize,
        panicked: bool, error: Option<String>, elapsed: u64,
        kinds: Vec<(&str, usize)>, log: Vec<String>, loop_iter: usize,
    ) -> SandboxResult {
        let mut km = HashMap::new();
        for (k, v) in kinds { km.insert(k.to_string(), v); }
        SandboxResult {
            executed_nodes: executed, matrix_count: matrix, norm_count: norm,
            output_vec: vec![0.0; dim], output_dim: dim,
            panicked, error, elapsed_us: elapsed,
            kind_counts: km, output_log: log, loop_iterations: loop_iter,
        }
    }

    // ── SandboxResult: top_kinds edge cases ──────────────────────────────

    #[test]
    fn top_kinds_empty() {
        let r = make_result(0, 0, 0, 0, false, None, 0, vec![], vec![], 0);
        assert!(r.top_kinds(5).is_empty());
    }

    #[test]
    fn top_kinds_n_larger_than_available() {
        let r = make_result(3, 1, 0, 0, false, None, 10,
            vec![("Matrix", 2), ("Norm", 1)], vec![], 0);
        let top = r.top_kinds(10);
        assert_eq!(top.len(), 2);
    }

    #[test]
    fn top_kinds_zero_n() {
        let r = make_result(3, 1, 0, 0, false, None, 10,
            vec![("Matrix", 2)], vec![], 0);
        assert!(r.top_kinds(0).is_empty());
    }

    #[test]
    fn top_kinds_tie_breaking() {
        let r = make_result(6, 0, 0, 0, false, None, 10,
            vec![("A", 2), ("B", 2), ("C", 2)], vec![], 0);
        let top = r.top_kinds(3);
        assert_eq!(top.len(), 3);
        // All have count 2; order may vary but all present
        let total: usize = top.iter().map(|&(_, c)| c).sum();
        assert_eq!(total, 6);
    }

    // ── SandboxResult: validate edge cases ───────────────────────────────

    #[test]
    fn validate_detects_high_loop_iterations() {
        let r = make_result(5, 0, 0, 1, false, None, 100, vec![], vec![], 2_000_000);
        let w = r.validate();
        assert!(w.iter().any(|w| w.contains("High loop")));
    }

    #[test]
    fn validate_exactly_million_loops_ok() {
        let r = make_result(5, 0, 0, 1, false, None, 100, vec![], vec![], 1_000_000);
        let w = r.validate();
        // 1_000_000 is NOT > 1_000_000, so no warning
        assert!(!w.iter().any(|w| w.contains("High loop")));
    }

    #[test]
    fn validate_detects_zero_output_dim() {
        let r = make_result(5, 0, 0, 0, false, None, 100, vec![], vec![], 0);
        let w = r.validate();
        assert!(w.iter().any(|w| w.contains("Output dimension is 0")));
    }

    #[test]
    fn validate_panicked_and_error_both_reported() {
        let r = make_result(1, 0, 0, 0, true, Some("boom".into()), 10, vec![], vec![], 0);
        let w = r.validate();
        assert!(w.iter().any(|w| w.contains("panicked")));
        assert!(w.iter().any(|w| w.contains("error")));
    }

    #[test]
    fn validate_clean_with_high_loops_has_warning() {
        let r = make_result(1, 0, 0, 1, false, None, 100, vec![], vec![], 5_000_000);
        let w = r.validate();
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("5000000"));
    }

    // ── SandboxResult: throughput ────────────────────────────────────────

    #[test]
    fn throughput_small_elapsed() {
        let r = make_result(100, 0, 0, 0, false, None, 1, vec![], vec![], 0);
        let tps = r.throughput_ops_per_sec();
        assert!(tps > 0.0);
        assert!((tps - 100_000_000.0).abs() < 1.0); // 100 / (1us = 1e-6s)
    }

    #[test]
    fn throughput_large_elapsed() {
        let r = make_result(10, 0, 0, 0, false, None, 10_000_000, vec![], vec![], 0);
        let tps = r.throughput_ops_per_sec();
        // 10 / (10s) = 1.0 ops/sec
        assert!((tps - 1.0).abs() < 0.01);
    }

    // ── SandboxResult: computation_ratio ─────────────────────────────────

    #[test]
    fn computation_ratio_all_matrix() {
        let r = make_result(10, 10, 0, 0, false, None, 100, vec![], vec![], 0);
        assert!((r.computation_ratio() - 1.0).abs() < 0.01);
    }

    #[test]
    fn computation_ratio_all_norm() {
        let r = make_result(10, 0, 10, 0, false, None, 100, vec![], vec![], 0);
        assert!((r.computation_ratio() - 1.0).abs() < 0.01);
    }

    #[test]
    fn computation_ratio_none_computation() {
        let r = make_result(10, 0, 0, 0, false, None, 100, vec![], vec![], 0);
        assert_eq!(r.computation_ratio(), 0.0);
    }

    // ── SandboxResult: execution_profile ─────────────────────────────────

    #[test]
    fn execution_profile_empty_kinds() {
        let r = make_result(0, 0, 0, 0, false, None, 0, vec![], vec![], 0);
        let p = r.execution_profile();
        assert_eq!(p.total_nodes, 0);
        assert_eq!(p.unique_kinds, 0);
        assert!(p.top_kinds.is_empty());
    }

    #[test]
    fn execution_profile_sorted_by_count() {
        let r = make_result(10, 0, 0, 0, false, None, 100,
            vec![("A", 1), ("B", 5), ("C", 3)], vec![], 0);
        let p = r.execution_profile();
        assert_eq!(p.top_kinds[0].0, "B");
        assert_eq!(p.top_kinds[0].1, 5);
        assert_eq!(p.top_kinds[1].0, "C");
        assert_eq!(p.top_kinds[2].0, "A");
    }

    #[test]
    fn execution_profile_captures_log_lines() {
        let r = make_result(5, 0, 0, 1, false, None, 500,
            vec![("X", 5)], vec!["a".into(), "b".into(), "c".into()], 0);
        let p = r.execution_profile();
        assert_eq!(p.output_log_lines, 3);
    }

    #[test]
    fn execution_profile_captures_loop_iterations() {
        let r = make_result(5, 0, 0, 1, false, None, 500,
            vec![("X", 5)], vec![], 42);
        let p = r.execution_profile();
        assert_eq!(p.loop_iterations, 42);
    }

    // ── SandboxResult: struct derives ────────────────────────────────────

    #[test]
    fn result_clone_is_independent() {
        let r = make_result(5, 2, 1, 1, false, None, 100,
            vec![("Matrix", 2)], vec![], 0);
        let mut cloned = r.clone();
        cloned.executed_nodes = 999;
        assert_eq!(r.executed_nodes, 5);
    }

    #[test]
    fn result_debug_format() {
        let r = make_result(5, 2, 1, 1, false, None, 100,
            vec![("Matrix", 2)], vec![], 0);
        let debug = format!("{:?}", r);
        assert!(debug.contains("executed_nodes"));
        assert!(debug.contains("5"));
    }

    // ── SandboxBatchReport: detailed calculations ────────────────────────

    #[test]
    fn batch_report_all_successful() {
        let report = SandboxBatchReport {
            total_runs: 5, successful: 5, failed: 0,
            total_elapsed_us: 5000, total_nodes_executed: 50,
            per_run_summaries: vec![],
        };
        assert!((report.success_rate() - 1.0).abs() < 1e-6);
        assert!((report.avg_elapsed_us() - 1000.0).abs() < 0.01);
        assert!((report.avg_nodes_per_run() - 10.0).abs() < 0.01);
    }

    #[test]
    fn batch_report_all_failed() {
        let report = SandboxBatchReport {
            total_runs: 3, successful: 0, failed: 3,
            total_elapsed_us: 3000, total_nodes_executed: 0,
            per_run_summaries: vec![],
        };
        assert!(report.success_rate().abs() < 1e-6);
    }

    #[test]
    fn batch_report_single_run() {
        let report = SandboxBatchReport {
            total_runs: 1, successful: 1, failed: 0,
            total_elapsed_us: 500, total_nodes_executed: 10,
            per_run_summaries: vec![],
        };
        assert!((report.success_rate() - 1.0).abs() < 1e-6);
        assert!((report.avg_elapsed_us() - 500.0).abs() < 0.01);
        assert!((report.avg_nodes_per_run() - 10.0).abs() < 0.01);
    }

    // ── SandboxBatchReport: validate edge cases ──────────────────────────

    #[test]
    fn batch_report_validate_zero_runs() {
        let report = SandboxBatchReport {
            total_runs: 0, successful: 0, failed: 0,
            total_elapsed_us: 0, total_nodes_executed: 0,
            per_run_summaries: vec![],
        };
        let issues = report.validate();
        assert!(issues.iter().any(|i| i.contains("zero total runs")));
    }

    #[test]
    fn batch_report_validate_all_three_issues() {
        let report = SandboxBatchReport {
            total_runs: 0, successful: 1, failed: 1,
            total_elapsed_us: 0, total_nodes_executed: 0,
            per_run_summaries: vec![
                SandboxExecutionSummary {
                    success: true, executed_nodes: 1, matrix_count: 0,
                    norm_count: 0, output_dim: 1, elapsed_us: 10,
                    loop_iterations: 0, unique_kinds: 0,
                    output_log_lines: 0, has_error: false,
                },
            ],
        };
        let issues = report.validate();
        assert_eq!(issues.len(), 3); // zero runs, imbalance, summary mismatch
    }

    // ── SandboxBatchReport: struct derives ───────────────────────────────

    #[test]
    fn batch_report_clone_is_independent() {
        let report = SandboxBatchReport {
            total_runs: 5, successful: 3, failed: 2,
            total_elapsed_us: 5000, total_nodes_executed: 50,
            per_run_summaries: vec![],
        };
        let mut cloned = report.clone();
        cloned.total_runs = 999;
        assert_eq!(report.total_runs, 5);
    }

    #[test]
    fn batch_report_debug_format() {
        let report = SandboxBatchReport {
            total_runs: 3, successful: 2, failed: 1,
            total_elapsed_us: 3000, total_nodes_executed: 30,
            per_run_summaries: vec![],
        };
        let debug = format!("{:?}", report);
        assert!(debug.contains("total_runs"));
        assert!(debug.contains("successful"));
    }

    // ── SandboxExecutionSummary: struct derives ──────────────────────────

    #[test]
    fn execution_summary_clone_is_independent() {
        let summary = SandboxExecutionSummary {
            success: true, executed_nodes: 10, matrix_count: 5,
            norm_count: 3, output_dim: 128, elapsed_us: 1000,
            loop_iterations: 50, unique_kinds: 4,
            output_log_lines: 2, has_error: false,
        };
        let mut cloned = summary.clone();
        cloned.executed_nodes = 999;
        assert_eq!(summary.executed_nodes, 10);
    }

    #[test]
    fn execution_summary_debug_format() {
        let summary = SandboxExecutionSummary {
            success: false, executed_nodes: 0, matrix_count: 0,
            norm_count: 0, output_dim: 0, elapsed_us: 0,
            loop_iterations: 0, unique_kinds: 0,
            output_log_lines: 0, has_error: true,
        };
        let debug = format!("{:?}", summary);
        assert!(debug.contains("success"));
        assert!(debug.contains("has_error"));
    }

    #[test]
    fn execution_summary_json_all_fields() {
        let summary = SandboxExecutionSummary {
            success: true, executed_nodes: 10, matrix_count: 5,
            norm_count: 3, output_dim: 128, elapsed_us: 1000,
            loop_iterations: 50, unique_kinds: 4,
            output_log_lines: 2, has_error: false,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("executed_nodes"));
        assert!(json.contains("matrix_count"));
        assert!(json.contains("norm_count"));
        assert!(json.contains("output_dim"));
        assert!(json.contains("elapsed_us"));
        assert!(json.contains("loop_iterations"));
        assert!(json.contains("unique_kinds"));
        assert!(json.contains("output_log_lines"));
        assert!(json.contains("has_error"));
    }

    // ── SandboxExecutionProfile: struct derives ──────────────────────────

    #[test]
    fn execution_profile_clone_is_independent() {
        let profile = SandboxExecutionProfile {
            total_nodes: 10, unique_kinds: 3,
            top_kinds: vec![("A".into(), 5), ("B".into(), 3)],
            output_dim: 128, output_log_lines: 2,
            loop_iterations: 10, elapsed_us: 500,
            throughput_ops: 20000.0, computation_ratio: 0.8,
        };
        let mut cloned = profile.clone();
        cloned.total_nodes = 999;
        assert_eq!(profile.total_nodes, 10);
    }

    #[test]
    fn execution_profile_json_all_fields() {
        let profile = SandboxExecutionProfile {
            total_nodes: 10, unique_kinds: 2,
            top_kinds: vec![("Matrix".into(), 7)],
            output_dim: 64, output_log_lines: 1,
            loop_iterations: 5, elapsed_us: 200,
            throughput_ops: 50000.0, computation_ratio: 0.7,
        };
        let json = serde_json::to_string(&profile).unwrap();
        assert!(json.contains("total_nodes"));
        assert!(json.contains("unique_kinds"));
        assert!(json.contains("top_kinds"));
        assert!(json.contains("throughput_ops"));
        assert!(json.contains("computation_ratio"));
    }

    // ── ResourceEstimate: struct derives ─────────────────────────────────

    #[test]
    fn resource_estimate_clone_is_independent() {
        let est = estimate_resource_usage(&[NdaNode::Int { value: 1 }]);
        let mut cloned = est.clone();
        cloned.node_count = 999;
        assert_eq!(est.node_count, 1);
    }

    #[test]
    fn resource_estimate_debug_format() {
        let est = estimate_resource_usage(&[NdaNode::Int { value: 1 }]);
        let debug = format!("{:?}", est);
        assert!(debug.contains("node_count"));
        assert!(debug.contains("estimated_memory_bytes"));
    }

    #[test]
    fn resource_estimate_json_all_fields() {
        let est = estimate_resource_usage(&[NdaNode::Int { value: 1 }]);
        let json = serde_json::to_string(&est).unwrap();
        assert!(json.contains("node_count"));
        assert!(json.contains("matrix_count"));
        assert!(json.contains("norm_count"));
        assert!(json.contains("loop_count"));
        assert!(json.contains("variable_count"));
        assert!(json.contains("max_depth"));
        assert!(json.contains("estimated_memory_bytes"));
        assert!(json.contains("validation_issues"));
    }

    // ── node_kind_name: more variants ────────────────────────────────────

    #[test]
    fn node_kind_name_matrix() {
        let n = NdaNode::Matrix {
            rows: 1, cols: 1, scale: 0,
            sign: vec![0], extra: vec![0],
        };
        assert_eq!(node_kind_name(&n), "Matrix");
    }

    #[test]
    fn node_kind_name_norm() {
        let n = NdaNode::Norm { size: 1, weight: vec![], bias: vec![] };
        assert_eq!(node_kind_name(&n), "Norm");
    }

    #[test]
    fn node_kind_name_add() {
        let n = NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        };
        assert_eq!(node_kind_name(&n), "Add");
    }

    #[test]
    fn node_kind_name_float() {
        assert_eq!(node_kind_name(&NdaNode::Float { value: 1.0 }), "Float");
    }

    #[test]
    fn node_kind_name_let_variant() {
        let n = NdaNode::Let {
            name_hash: 0,
            init: Box::new(NdaNode::Int { value: 0 }),
        };
        assert_eq!(node_kind_name(&n), "Let");
    }

    // ── estimate_resource: more node types ───────────────────────────────

    #[test]
    fn estimate_resource_while_loop() {
        let nodes = vec![NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![NdaNode::Int { value: 2 }],
        }];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.loop_count, 1);
    }

    #[test]
    fn estimate_resource_if_node() {
        let nodes = vec![NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 2 }],
            else_body: Some(vec![NdaNode::Int { value: 3 }]),
        }];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.node_count, 1);
        assert!(est.max_depth >= 1);
    }

    #[test]
    fn estimate_resource_let_counts_variable() {
        let nodes = vec![
            NdaNode::Let { name_hash: 1, init: Box::new(NdaNode::Int { value: 0 }) },
            NdaNode::Let { name_hash: 2, init: Box::new(NdaNode::Float { value: 0.0 }) },
        ];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.variable_count, 2);
    }

    #[test]
    fn estimate_resource_validation_clean_for_valid_program() {
        let nodes = vec![
            NdaNode::Int { value: 42 },
            NdaNode::Float { value: 3.14 },
            NdaNode::Break,
        ];
        let est = estimate_resource_usage(&nodes);
        assert!(est.validation_issues.is_empty());
    }

    // ── Block 155: sandbox/mod.rs comprehensive expansion ───────────────────

    // ─── JSON key count tests ───────────────────────────────────────────────

    #[test]
    fn sandbox_result_json_key_count() {
        let r = make_result(1, 0, 0, 0, false, None, 0, vec![], vec![], 0);
        let json = serde_json::to_string(&r).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        // 10 fields: executed_nodes, matrix_count, norm_count, output_vec,
        //            output_dim, panicked, error, elapsed_us, kind_counts,
        //            output_log, loop_iterations
        assert_eq!(val.as_object().unwrap().len(), 11);
    }

    #[test]
    fn sandbox_execution_summary_json_key_count() {
        let s = SandboxExecutionSummary {
            success: true, executed_nodes: 0, matrix_count: 0,
            norm_count: 0, output_dim: 0, elapsed_us: 0,
            loop_iterations: 0, unique_kinds: 0,
            output_log_lines: 0, has_error: false,
        };
        let json = serde_json::to_string(&s).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 10);
    }

    #[test]
    fn sandbox_execution_profile_json_key_count() {
        let p = SandboxExecutionProfile {
            total_nodes: 0, unique_kinds: 0, top_kinds: vec![],
            output_dim: 0, output_log_lines: 0, loop_iterations: 0,
            elapsed_us: 0, throughput_ops: 0.0, computation_ratio: 0.0,
        };
        let json = serde_json::to_string(&p).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 9);
    }

    #[test]
    fn sandbox_batch_report_json_key_count() {
        let r = SandboxBatchReport {
            total_runs: 0, successful: 0, failed: 0,
            total_elapsed_us: 0, total_nodes_executed: 0,
            per_run_summaries: vec![],
        };
        let json = serde_json::to_string(&r).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 6);
    }

    #[test]
    fn resource_estimate_json_key_count() {
        let est = estimate_resource_usage(&[NdaNode::Int { value: 1 }]);
        let json = serde_json::to_string(&est).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 8);
    }

    // ─── JSON value tests ───────────────────────────────────────────────────

    #[test]
    fn sandbox_result_json_values() {
        let r = make_result(42, 5, 3, 8, false, None, 999,
            vec![("Matrix", 5)], vec!["hello".into()], 77);
        let json = serde_json::to_string(&r).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["executed_nodes"], 42);
        assert_eq!(val["matrix_count"], 5);
        assert_eq!(val["norm_count"], 3);
        assert_eq!(val["output_dim"], 8);
        assert_eq!(val["panicked"], false);
        assert_eq!(val["elapsed_us"], 999);
        assert_eq!(val["loop_iterations"], 77);
        assert!(val["output_log"].is_array());
        assert_eq!(val["output_log"][0], "hello");
    }

    #[test]
    fn sandbox_result_json_error_case() {
        let r = make_result(0, 0, 0, 0, true, Some("boom".into()), 10, vec![], vec![], 0);
        let json = serde_json::to_string(&r).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["panicked"], true);
        assert_eq!(val["error"], "boom");
    }

    // ─── Clone independence tests ───────────────────────────────────────────

    #[test]
    fn sandbox_result_clone_kind_counts_independence() {
        let r = make_result(5, 2, 1, 1, false, None, 100,
            vec![("Matrix", 2), ("Norm", 1)], vec![], 0);
        let mut cloned = r.clone();
        cloned.kind_counts.insert("Extra".to_string(), 99);
        assert!(!r.kind_counts.contains_key("Extra"));
    }

    #[test]
    fn sandbox_result_clone_output_log_independence() {
        let r = make_result(5, 0, 0, 1, false, None, 100,
            vec![], vec!["orig".into()], 0);
        let mut cloned = r.clone();
        cloned.output_log.push("extra".into());
        assert_eq!(r.output_log.len(), 1);
        assert_eq!(cloned.output_log.len(), 2);
    }

    #[test]
    fn execution_summary_clone_all_fields() {
        let s = SandboxExecutionSummary {
            success: true, executed_nodes: 10, matrix_count: 5,
            norm_count: 3, output_dim: 128, elapsed_us: 1000,
            loop_iterations: 50, unique_kinds: 4,
            output_log_lines: 2, has_error: false,
        };
        let mut cloned = s.clone();
        cloned.success = false;
        cloned.has_error = true;
        cloned.loop_iterations = 999;
        assert!(s.success);
        assert!(!s.has_error);
        assert_eq!(s.loop_iterations, 50);
    }

    #[test]
    fn execution_profile_clone_top_kinds_independence() {
        let p = SandboxExecutionProfile {
            total_nodes: 10, unique_kinds: 2,
            top_kinds: vec![("A".into(), 5), ("B".into(), 3)],
            output_dim: 64, output_log_lines: 1,
            loop_iterations: 5, elapsed_us: 200,
            throughput_ops: 50000.0, computation_ratio: 0.8,
        };
        let mut cloned = p.clone();
        cloned.top_kinds.push(("C".into(), 1));
        assert_eq!(p.top_kinds.len(), 2);
        assert_eq!(cloned.top_kinds.len(), 3);
    }

    #[test]
    fn resource_estimate_clone_validation_independence() {
        let est = estimate_resource_usage(&[]);
        let mut cloned = est.clone();
        cloned.validation_issues.push("extra".into());
        assert_ne!(est.validation_issues.len(), cloned.validation_issues.len());
    }

    // ─── Debug format tests ─────────────────────────────────────────────────

    #[test]
    fn sandbox_result_debug_contains_fields() {
        let r = make_result(42, 5, 3, 8, false, None, 999,
            vec![("Matrix", 5)], vec![], 77);
        let debug = format!("{:?}", r);
        assert!(debug.contains("executed_nodes"));
        assert!(debug.contains("42"));
        assert!(debug.contains("matrix_count"));
        assert!(debug.contains("loop_iterations"));
    }

    #[test]
    fn execution_summary_debug_contains_fields() {
        let s = SandboxExecutionSummary {
            success: false, executed_nodes: 0, matrix_count: 0,
            norm_count: 0, output_dim: 0, elapsed_us: 0,
            loop_iterations: 0, unique_kinds: 0,
            output_log_lines: 0, has_error: true,
        };
        let debug = format!("{:?}", s);
        assert!(debug.contains("SandboxExecutionSummary"));
        assert!(debug.contains("has_error"));
    }

    #[test]
    fn execution_profile_debug_contains_fields() {
        let p = SandboxExecutionProfile {
            total_nodes: 10, unique_kinds: 2,
            top_kinds: vec![("A".into(), 5)],
            output_dim: 64, output_log_lines: 1,
            loop_iterations: 5, elapsed_us: 200,
            throughput_ops: 50000.0, computation_ratio: 0.8,
        };
        let debug = format!("{:?}", p);
        assert!(debug.contains("SandboxExecutionProfile"));
        assert!(debug.contains("throughput_ops"));
    }

    // ─── Pretty JSON tests ──────────────────────────────────────────────────

    #[test]
    fn sandbox_result_pretty_json() {
        let r = make_result(1, 0, 0, 1, false, None, 10, vec![], vec![], 0);
        let pretty = serde_json::to_string_pretty(&r).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("executed_nodes"));
    }

    #[test]
    fn batch_report_pretty_json() {
        let r = SandboxBatchReport {
            total_runs: 1, successful: 1, failed: 0,
            total_elapsed_us: 100, total_nodes_executed: 5,
            per_run_summaries: vec![],
        };
        let pretty = serde_json::to_string_pretty(&r).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("total_runs"));
    }

    // ─── node_kind_name: remaining variants ─────────────────────────────────

    #[test]
    fn node_kind_name_call() {
        assert_eq!(node_kind_name(&NdaNode::Call { target: 0 }), "Call");
    }

    #[test]
    fn node_kind_name_loop_variant() {
        let n = NdaNode::Loop { count: 1, body: vec![] };
        assert_eq!(node_kind_name(&n), "Loop");
    }

    #[test]
    fn node_kind_name_while() {
        let n = NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![],
        };
        assert_eq!(node_kind_name(&n), "While");
    }

    #[test]
    fn node_kind_name_if() {
        let n = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![],
            else_body: None,
        };
        assert_eq!(node_kind_name(&n), "If");
    }

    #[test]
    fn node_kind_name_compare() {
        use crate::site_map::verifier::CmpOp;
        let n = NdaNode::Compare {
            op: CmpOp::Eq,
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        };
        assert_eq!(node_kind_name(&n), "Compare");
    }

    #[test]
    fn node_kind_name_load() {
        assert_eq!(node_kind_name(&NdaNode::Load { name_hash: 0 }), "Load");
    }

    #[test]
    fn node_kind_name_store() {
        let n = NdaNode::Store {
            name_hash: 0,
            value: Box::new(NdaNode::Int { value: 0 }),
        };
        assert_eq!(node_kind_name(&n), "Store");
    }

    #[test]
    fn node_kind_name_vec_op() {
        use crate::site_map::verifier::VecOpKind;
        let n = NdaNode::VecOp {
            op: VecOpKind::Negate,
            operand: Box::new(NdaNode::Int { value: 1 }),
        };
        assert_eq!(node_kind_name(&n), "VecOp");
    }

    #[test]
    fn node_kind_name_print() {
        let n = NdaNode::Print {
            source: Box::new(NdaNode::Int { value: 42 }),
        };
        assert_eq!(node_kind_name(&n), "Print");
    }

    #[test]
    fn node_kind_name_return() {
        let n = NdaNode::Return {
            value: Box::new(NdaNode::Int { value: 0 }),
        };
        assert_eq!(node_kind_name(&n), "Return");
    }

    #[test]
    fn node_kind_name_bitwise() {
        let n = NdaNode::Bitwise {
            op: BitwiseOp::And,
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Some(Box::new(NdaNode::Int { value: 2 })),
        };
        assert_eq!(node_kind_name(&n), "Bitwise");
    }

    #[test]
    fn node_kind_name_math() {
        let n = NdaNode::Math {
            op: MathOp::Add,
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        };
        assert_eq!(node_kind_name(&n), "Math");
    }

    #[test]
    fn node_kind_name_mathfunc() {
        let n = NdaNode::MathFunc {
            func: MathFuncKind::Sin,
            operand: Box::new(NdaNode::Int { value: 1 }),
        };
        assert_eq!(node_kind_name(&n), "MathFunc");
    }

    #[test]
    fn node_kind_name_peek() {
        let n = NdaNode::Peek {
            addr: Box::new(NdaNode::Int { value: 0 }),
        };
        assert_eq!(node_kind_name(&n), "Peek");
    }

    #[test]
    fn node_kind_name_poke() {
        let n = NdaNode::Poke {
            addr: Box::new(NdaNode::Int { value: 0 }),
            value: Box::new(NdaNode::Int { value: 0 }),
        };
        assert_eq!(node_kind_name(&n), "Poke");
    }

    #[test]
    fn node_kind_name_gemv() {
        let n = NdaNode::Gemv {
            matrix: Box::new(NdaNode::Int { value: 0 }),
            vector: Box::new(NdaNode::Int { value: 0 }),
        };
        assert_eq!(node_kind_name(&n), "Gemv");
    }

    #[test]
    fn node_kind_name_dot() {
        let n = NdaNode::Dot {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        };
        assert_eq!(node_kind_name(&n), "Dot");
    }

    #[test]
    fn node_kind_name_syscall() {
        let n = NdaNode::Syscall { num: 0, args: vec![] };
        assert_eq!(node_kind_name(&n), "Syscall");
    }

    #[test]
    fn node_kind_name_spawn() {
        assert_eq!(node_kind_name(&NdaNode::Spawn { scope_hash: 0 }), "Spawn");
    }

    #[test]
    fn node_kind_name_atomic() {
        let n = NdaNode::Atomic {
            op: crate::site_map::verifier::AtomicOp::Cas,
            addr: Box::new(NdaNode::Int { value: 0 }),
            val: Box::new(NdaNode::Int { value: 0 }),
        };
        assert_eq!(node_kind_name(&n), "Atomic");
    }

    #[test]
    fn node_kind_name_alloc() {
        let n = NdaNode::Alloc {
            size: Box::new(NdaNode::Int { value: 1024 }),
        };
        assert_eq!(node_kind_name(&n), "Alloc");
    }

    #[test]
    fn node_kind_name_free() {
        let n = NdaNode::Free {
            addr: Box::new(NdaNode::Int { value: 0 }),
        };
        assert_eq!(node_kind_name(&n), "Free");
    }

    #[test]
    fn node_kind_name_reg_int() {
        assert_eq!(node_kind_name(&NdaNode::RegInt { vector: 0, handler_hash: 0 }), "RegInt");
    }

    #[test]
    fn node_kind_name_cast() {
        use crate::site_map::verifier::TypeKind;
        let n = NdaNode::Cast {
            from_type: TypeKind::Int,
            to_type: TypeKind::Float,
            operand: Box::new(NdaNode::Int { value: 0 }),
        };
        assert_eq!(node_kind_name(&n), "Cast");
    }

    #[test]
    fn node_kind_name_gpu_dispatch() {
        let n = NdaNode::GpuDispatch { shader_hash: 0, args: vec![] };
        assert_eq!(node_kind_name(&n), "GpuDispatch");
    }

    #[test]
    fn node_kind_name_triple() {
        let n = NdaNode::Triple {
            subject_hash: 0, predicate_id: 0, object_hash: 0,
        };
        assert_eq!(node_kind_name(&n), "Triple");
    }

    // ─── estimate_resource: more complex programs ───────────────────────────

    #[test]
    fn estimate_resource_bitwise_nodes() {
        let nodes = vec![NdaNode::Bitwise {
            op: BitwiseOp::And,
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Some(Box::new(NdaNode::Int { value: 2 })),
        }];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.node_count, 1);
        assert!(est.validation_issues.is_empty());
    }

    #[test]
    fn estimate_resource_compare_add_math() {
        use crate::site_map::verifier::CmpOp;
        let nodes = vec![
            NdaNode::Compare {
                op: CmpOp::Eq,
                lhs: Box::new(NdaNode::Int { value: 1 }),
                rhs: Box::new(NdaNode::Int { value: 2 }),
            },
            NdaNode::Add {
                lhs: Box::new(NdaNode::Int { value: 3 }),
                rhs: Box::new(NdaNode::Int { value: 4 }),
            },
            NdaNode::Math {
                op: MathOp::Mul,
                lhs: Box::new(NdaNode::Int { value: 5 }),
                rhs: Box::new(NdaNode::Int { value: 6 }),
            },
        ];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.node_count, 3);
    }

    #[test]
    fn estimate_resource_syscall_gpu_dispatch() {
        let nodes = vec![
            NdaNode::Syscall { num: 1, args: vec![NdaNode::Int { value: 42 }] },
            NdaNode::GpuDispatch { shader_hash: 0, args: vec![NdaNode::Int { value: 1 }] },
        ];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.node_count, 2);
    }

    #[test]
    fn estimate_resource_deeply_nested_loops_warning() {
        let mut body = vec![NdaNode::Int { value: 1 }];
        for _ in 0..110 {
            body = vec![NdaNode::Loop { count: 1, body }];
        }
        let est = estimate_resource_usage(&body);
        assert!(est.validation_issues.iter().any(|i| i.contains("deeply nested")));
    }

    #[test]
    fn estimate_resource_memory_formula() {
        let nodes = vec![
            NdaNode::Matrix {
                rows: 1, cols: 1, scale: 0,
                sign: vec![0], extra: vec![0],
            },
            NdaNode::Norm { size: 1, weight: vec![], bias: vec![] },
            NdaNode::Int { value: 0 },
        ];
        let est = estimate_resource_usage(&nodes);
        // 3 * 128 + 1 * 4096 + 1 * 512 = 384 + 4096 + 512 = 4992
        assert_eq!(est.estimated_memory_bytes, 4992);
    }

    #[test]
    fn estimate_resource_store_counts_variable() {
        let nodes = vec![NdaNode::Store {
            name_hash: 42,
            value: Box::new(NdaNode::Int { value: 0 }),
        }];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.variable_count, 1);
    }

    #[test]
    fn estimate_resource_dot_gemv_counted() {
        let nodes = vec![
            NdaNode::Dot {
                lhs: Box::new(NdaNode::Int { value: 1 }),
                rhs: Box::new(NdaNode::Int { value: 2 }),
            },
            NdaNode::Gemv {
                matrix: Box::new(NdaNode::Int { value: 0 }),
                vector: Box::new(NdaNode::Int { value: 0 }),
            },
        ];
        let est = estimate_resource_usage(&nodes);
        assert_eq!(est.node_count, 2);
    }

    // ─── Sandbox execution tests ────────────────────────────────────────────

    #[test]
    fn sandbox_executes_int_node() {
        let input = vec![1.0f32; 4];
        let site_map = SiteMap::open(
            &std::env::temp_dir().join("sandbox_int_sm"), 0
        ).unwrap();
        let nodes = vec![NdaNode::Int { value: 42 }];
        let result = NdaSandbox::run(&nodes, &input, &site_map);
        assert!(result.is_success());
        assert_eq!(result.executed_nodes, 1);
        assert_eq!(*result.kind_counts.get("Int").unwrap_or(&0), 1);
    }

    #[test]
    fn sandbox_executes_float_node() {
        let input = vec![1.0f32; 4];
        let site_map = SiteMap::open(
            &std::env::temp_dir().join("sandbox_float_sm"), 0
        ).unwrap();
        let nodes = vec![NdaNode::Float { value: 3.14 }];
        let result = NdaSandbox::run(&nodes, &input, &site_map);
        assert!(result.is_success());
        assert_eq!(*result.kind_counts.get("Float").unwrap_or(&0), 1);
    }

    #[test]
    fn sandbox_executes_scope_node() {
        let input = vec![1.0f32; 4];
        let site_map = SiteMap::open(
            &std::env::temp_dir().join("sandbox_scope_sm"), 0
        ).unwrap();
        let nodes = vec![NdaNode::Scope {
            children: vec![
                NdaNode::Int { value: 1 },
                NdaNode::Int { value: 2 },
            ],
        }];
        let result = NdaSandbox::run(&nodes, &input, &site_map);
        assert!(result.is_success());
        // Scope + 2 Int children = 3 executed nodes
        assert_eq!(result.executed_nodes, 3);
    }

    #[test]
    fn sandbox_executes_loop_node() {
        let input = vec![1.0f32; 4];
        let site_map = SiteMap::open(
            &std::env::temp_dir().join("sandbox_loop_sm"), 0
        ).unwrap();
        let nodes = vec![NdaNode::Loop {
            count: 3,
            body: vec![NdaNode::Int { value: 1 }],
        }];
        let result = NdaSandbox::run(&nodes, &input, &site_map);
        assert!(result.is_success());
        assert_eq!(result.loop_iterations, 3);
    }

    #[test]
    fn sandbox_executes_spawn_node() {
        let input = vec![1.0f32; 4];
        let site_map = SiteMap::open(
            &std::env::temp_dir().join("sandbox_spawn_sm"), 0
        ).unwrap();
        let nodes = vec![NdaNode::Spawn { scope_hash: 0 }];
        let result = NdaSandbox::run(&nodes, &input, &site_map);
        assert!(result.is_success());
        assert_eq!(*result.kind_counts.get("Spawn").unwrap_or(&0), 1);
    }

    #[test]
    fn sandbox_executes_reg_int_node() {
        let input = vec![1.0f32; 4];
        let site_map = SiteMap::open(
            &std::env::temp_dir().join("sandbox_regint_sm"), 0
        ).unwrap();
        let nodes = vec![NdaNode::RegInt { vector: 0, handler_hash: 42 }];
        let result = NdaSandbox::run(&nodes, &input, &site_map);
        assert!(result.is_success());
        assert_eq!(*result.kind_counts.get("RegInt").unwrap_or(&0), 1);
    }

    #[test]
    fn sandbox_executes_triple_node() {
        let input = vec![1.0f32; 4];
        let site_map = SiteMap::open(
            &std::env::temp_dir().join("sandbox_triple_sm"), 0
        ).unwrap();
        let nodes = vec![NdaNode::Triple { subject_hash: 1, predicate_id: 2, object_hash: 3 }];
        let result = NdaSandbox::run(&nodes, &input, &site_map);
        assert!(result.is_success());
        assert_eq!(*result.kind_counts.get("Triple").unwrap_or(&0), 1);
    }

    // ─── execution_summary field accuracy ───────────────────────────────────

    #[test]
    fn execution_summary_field_values() {
        let r = make_result(50, 10, 5, 128, false, None, 2000,
            vec![("Matrix", 10), ("Norm", 5), ("Int", 3)],
            vec!["line1".into(), "line2".into()], 25);
        let s = r.execution_summary();
        assert!(s.success);
        assert_eq!(s.executed_nodes, 50);
        assert_eq!(s.matrix_count, 10);
        assert_eq!(s.norm_count, 5);
        assert_eq!(s.output_dim, 128);
        assert_eq!(s.elapsed_us, 2000);
        assert_eq!(s.loop_iterations, 25);
        assert_eq!(s.unique_kinds, 3);
        assert_eq!(s.output_log_lines, 2);
        assert!(!s.has_error);
    }

    #[test]
    fn execution_summary_error_case() {
        let r = make_result(0, 0, 0, 0, false, Some("dim mismatch".into()), 100,
            vec![], vec![], 0);
        let s = r.execution_summary();
        assert!(!s.success);
        assert!(s.has_error);
    }
}
