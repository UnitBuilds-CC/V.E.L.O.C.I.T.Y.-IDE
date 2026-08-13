// sandbox/mod.rs — Executing NDA opcode trees with nda_int kernels
#![allow(dead_code, unused)]
pub mod jit_sandbox;
pub mod scope_validator;

pub use jit_sandbox::NdaJitSandbox;

use crate::nda::NdaMatrix;
use crate::nda_int::NdaVec;
use crate::site_map::verifier::{BitwiseOp, MathFuncKind, MathOp};
use crate::site_map::{NdaNode, SiteMap};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct SandboxResult {
    pub executed_nodes: usize,
    pub matrix_count: usize,
    pub norm_count: usize,
    pub output_vec: Vec<f32>, // final output of the execution chain
    pub output_dim: usize,
    pub panicked: bool,
    pub error: Option<String>,
    pub elapsed_us: u64,
}

pub struct NdaSandbox;

impl NdaSandbox {
    pub fn run(nodes: &[NdaNode], conditioning_vec: &[f32], site_map: &SiteMap) -> SandboxResult {
        let t_start = Instant::now();

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
                }
            }
        }
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
}
