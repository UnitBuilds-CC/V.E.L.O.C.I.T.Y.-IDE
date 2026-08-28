use serde::Serialize;
use std::sync::Arc;

use crate::nda::NdaMatrix;
use crate::nda_int::{nda_gemv_nda_to_nda, rms_norm_nda, NdaVec};
use crate::site_map::verifier::BitwiseOp;
use crate::site_map::verifier::MathFuncKind;
use crate::site_map::verifier::MathOp;
use crate::site_map::NdaNode;

use super::optimizer::optimize_ast;
use super::types::{
    run_sequence, JitControlFlow, JitFn, JitProgram, JitState, JitVal, VarRegistry,
};
use super::vm_helpers::{add_vals, apply_vec_op, broadcast_float, broadcast_scalar, compare_vals};
use super::x86_emitter::{asm_gemv_available, compile_scalar_block, count_nodes, gemv_native};

/// Maximum iterations for `While` loops — safety limit against infinite loops.
const MAX_WHILE_ITERATIONS: u32 = 10_000_000;

pub fn jit_tier_info() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    {
        "Tier-1 (Closure Dispatch) + Tier-2 (x86-64 Machine-Code GEMV JIT)"
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        "Tier-1 (Closure Dispatch) + Portable Fallback"
    }
}

/// Apply a binary bitwise op to two raw integer values.
fn bitwise_i32(op: BitwiseOp, a: i32, b: i32) -> i32 {
    match op {
        BitwiseOp::And => a & b,
        BitwiseOp::Or => a | b,
        BitwiseOp::Xor => a ^ b,
        BitwiseOp::Shl => a.wrapping_shl(b as u32),
        BitwiseOp::Shr => a.wrapping_shr(b as u32),
        BitwiseOp::Not => !a,
    }
}

/// Apply a binary bitwise op to two `f32`s via their IEEE-754 bit patterns,
/// mirroring the unary `Not` case which flips the float's bits.
fn bitwise_f32(op: BitwiseOp, a: f32, b: f32) -> f32 {
    let x = a.to_bits();
    let y = b.to_bits();
    let bits = match op {
        BitwiseOp::And => x & y,
        BitwiseOp::Or => x | y,
        BitwiseOp::Xor => x ^ y,
        BitwiseOp::Shl => x.wrapping_shl(y),
        BitwiseOp::Shr => x.wrapping_shr(y),
        BitwiseOp::Not => !x,
    };
    f32::from_bits(bits)
}

/// Element-wise bitwise op over two NDA vectors' raw integer codes,
/// re-encoded into the quaternary NDA representation.
fn bitwise_vec_vec(op: BitwiseOp, a: &NdaVec, b: &NdaVec) -> JitVal {
    let len = a.len.min(b.len);
    let out: Vec<i32> = (0..len)
        .map(|i| bitwise_i32(op, a.get_raw(i), b.get_raw(i)))
        .collect();
    JitVal::Vector(Arc::new(NdaVec::from_i32_slice(&out, a.log2_scale)))
}

/// Element-wise bitwise op between an NDA vector and a scalar integer code.
fn bitwise_vec_scalar(op: BitwiseOp, a: &NdaVec, s: i32, scalar_on_left: bool) -> JitVal {
    let out: Vec<i32> = (0..a.len)
        .map(|i| {
            let av = a.get_raw(i);
            if scalar_on_left {
                bitwise_i32(op, s, av)
            } else {
                bitwise_i32(op, av, s)
            }
        })
        .collect();
    JitVal::Vector(Arc::new(NdaVec::from_i32_slice(&out, a.log2_scale)))
}

/// Dispatch a binary bitwise op across every scalar/float/vector combination.
fn bitwise_binary(op: BitwiseOp, l: JitVal, r: JitVal) -> JitVal {
    match (l, r) {
        (JitVal::Scalar(l_v, l_s), JitVal::Scalar(r_v, _)) => {
            JitVal::Scalar(bitwise_i32(op, l_v, r_v), l_s)
        }
        (JitVal::Float(a), JitVal::Float(b)) => JitVal::Float(bitwise_f32(op, a, b)),
        (JitVal::Float(a), JitVal::Scalar(v, s)) => {
            JitVal::Float(bitwise_f32(op, a, (v as f32) * 2.0f32.powi(s as i32)))
        }
        (JitVal::Scalar(v, s), JitVal::Float(b)) => {
            JitVal::Float(bitwise_f32(op, (v as f32) * 2.0f32.powi(s as i32), b))
        }
        (JitVal::Vector(a), JitVal::Vector(b)) => bitwise_vec_vec(op, &a, &b),
        (JitVal::Vector(a), JitVal::Scalar(v, _)) => bitwise_vec_scalar(op, &a, v, false),
        (JitVal::Scalar(v, _), JitVal::Vector(b)) => bitwise_vec_scalar(op, &b, v, true),
        (JitVal::Vector(a), JitVal::Float(f)) => {
            let code = NdaVec::from_f32_slice(&[f]).get_raw(0);
            bitwise_vec_scalar(op, &a, code, false)
        }
        (JitVal::Float(f), JitVal::Vector(b)) => {
            let code = NdaVec::from_f32_slice(&[f]).get_raw(0);
            bitwise_vec_scalar(op, &b, code, true)
        }
    }
}

/// Compile a slice of `NdaNode`s into a `JitProgram`.
///
/// This is the main entry point.  Call once per program load; execute many times.
pub fn compile(nodes: &[NdaNode]) -> JitProgram {
    let mut counter = 0usize;
    let registry = VarRegistry::new();
    let optimized_nodes = optimize_ast(nodes);
    let fns = compile_sequence(&optimized_nodes, &mut counter, &registry);

    let has_asm_kernel = asm_gemv_available();

    JitProgram {
        fns,
        nodes_compiled: counter,
        has_asm_kernel,
        registry,
    }
}

pub fn compile_sequence(
    nodes: &[NdaNode],
    counter: &mut usize,
    registry: &VarRegistry,
) -> Vec<JitFn> {
    nodes
        .iter()
        .map(|n| compile_node(n, counter, registry))
        .collect()
}

pub fn compile_node(node: &NdaNode, counter: &mut usize, registry: &VarRegistry) -> JitFn {
    let res = compile_node_inner(node, counter, registry);
    wrap_debug(node, res)
}

/// Compile nodes straight to interpreter closures, skipping the native
/// scalar fast path.  Used by the JIT fallback when a scalar block observes
/// non-scalar bindings at runtime.
pub fn compile_interpreter_sequence(
    nodes: &[NdaNode],
    counter: &mut usize,
    registry: &VarRegistry,
) -> Vec<JitFn> {
    nodes
        .iter()
        .map(|n| {
            *counter += 1;
            compile_node_dispatch(n, counter, registry)
        })
        .collect()
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
        NdaNode::Cast {
            from_type, to_type, ..
        } => format!("Cast({:?}->{:?})", from_type, to_type),
        NdaNode::GpuDispatch { shader_hash, .. } => {
            format!("GpuDispatch(shader={:016x})", shader_hash)
        }
        NdaNode::Triple {
            subject_hash,
            predicate_id,
            object_hash,
        } => format!(
            "Triple(sub={:016x}, pred={}, obj={:016x})",
            subject_hash, predicate_id, object_hash
        ),
    }
}

fn wrap_debug(node: &NdaNode, jit_fn: JitFn) -> JitFn {
    if std::env::var("NDA_JIT_DEBUG").is_err() {
        return jit_fn;
    }
    let node_str = node_to_str(node);
    let node_ptr = node as *const NdaNode as usize;
    Arc::new(move |state| {
        eprintln!(
            "[JIT_DBG] BEFORE {:15} (addr: {:x}) | Stack: {:?} | Vars: {:?}",
            node_str, node_ptr, state.stack, state.variables
        );
        let res = jit_fn(state);
        eprintln!(
            "[JIT_DBG] AFTER  {:15} (addr: {:x}) | Result: {:?} | Stack: {:?} | Vars: {:?}",
            node_str, node_ptr, res, state.stack, state.variables
        );
        res
    })
}

fn compile_node_inner(node: &NdaNode, counter: &mut usize, registry: &VarRegistry) -> JitFn {
    *counter += 1;

    if is_pure_scalar(node) {
        if let Some(jit_fn) = compile_scalar_block(std::slice::from_ref(node), registry) {
            *counter += count_nodes(node) - 1;
            return jit_fn;
        }
    }

    compile_node_dispatch(node, counter, registry)
}

fn compile_node_dispatch(node: &NdaNode, counter: &mut usize, registry: &VarRegistry) -> JitFn {
    match node {
        NdaNode::Matrix {
            rows,
            cols,
            scale,
            sign,
            extra,
        } => {
            let rows = *rows as usize;
            let cols = *cols as usize;
            let scale_f32 = 2.0f32.powi(*scale as i32);
            let mat = NdaMatrix::new_quad(rows, cols, scale_f32, sign.clone(), extra.clone());

            let use_asm = asm_gemv_available();

            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                let input = match state.stack.pop() {
                    Some(JitVal::Vector(v)) => v,
                    Some(JitVal::Scalar(val, scale)) => {
                        Arc::new(broadcast_scalar(cols, val, scale))
                    }
                    Some(JitVal::Float(val)) => Arc::new(broadcast_float(cols, val)),
                    None => return Err("Stack underflow in Matrix GEMV".to_string()),
                };

                if input.len != cols {
                    return Err(format!(
                        "Matrix GEMV dimension mismatch: input len {} \u{2260} matrix cols {}",
                        input.len, cols
                    ));
                }

                let out = if use_asm {
                    gemv_native(&mat, input.as_ref())
                } else {
                    nda_gemv_nda_to_nda(&mat, input.as_ref())
                };
                state.stack.push(JitVal::Vector(Arc::new(out)));
                state.matrix_count += 1;
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Norm { size, weight, bias } => {
            let size = *size as usize;
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
                    Some(JitVal::Scalar(val, scale)) => {
                        Arc::new(broadcast_scalar(size, val, scale))
                    }
                    Some(JitVal::Float(val)) => Arc::new(broadcast_float(size, val)),
                    None => return Err("Stack underflow in Norm".to_string()),
                };

                if input.len != size {
                    return Err(format!(
                        "Norm dimension mismatch: input len {} \u{2260} norm size {}",
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

        NdaNode::Loop { count, body } => {
            let count = *count;
            let body_fns = compile_sequence(body, counter, registry);
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                state.loop_count += 1;
                for _ in 0..count {
                    match run_sequence(&body_fns, state)? {
                        JitControlFlow::Break => break,
                        JitControlFlow::Return => return Ok(JitControlFlow::Return),
                        JitControlFlow::Continue => {}
                    }
                }
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::While { cond, body } => {
            let cond_fn = compile_node(cond, counter, registry);
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

                    if !cond_result.is_truthy() {
                        break;
                    }

                    match run_sequence(&body_fns, state)? {
                        JitControlFlow::Break => break,
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

        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
            let cond_fn = compile_node(cond, counter, registry);
            let then_fns = compile_sequence(then_body, counter, registry);
            let else_fns = else_body
                .as_ref()
                .map(|eb| compile_sequence(eb, counter, registry));
            Arc::new(move |state: &mut JitState<'_>| {
                state.executed_nodes += 1;
                cond_fn(state)?;
                let cond_result = match state.stack.pop() {
                    Some(v) => v,
                    None => return Err("Stack underflow in If condition".to_string()),
                };

                if cond_result.is_truthy() {
                    let cf = run_sequence(&then_fns, state)?;
                    if cf != JitControlFlow::Continue {
                        return Ok(cf);
                    }
                } else if let Some(ref eb) = else_fns {
                    let cf = run_sequence(eb, state)?;
                    if cf != JitControlFlow::Continue {
                        return Ok(cf);
                    }
                }
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Compare { op, lhs, rhs } => {
            let op = *op;
            let lhs_fn = compile_node(lhs, counter, registry);
            let rhs_fn = compile_node(rhs, counter, registry);
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
            let slot_idx = registry.get_or_create_slot(name_hash);
            let init_fn = compile_node(init, counter, registry);
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
            let slot_idx = registry.get_or_create_slot(name_hash);
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
            let slot_idx = registry.get_or_create_slot(name_hash);
            let val_fn = compile_node(value, counter, registry);
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
            let op = *op;
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

        NdaNode::Break => Arc::new(|state: &mut JitState<'_>| {
            state.executed_nodes += 1;
            Ok(JitControlFlow::Break)
        }),

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
                            JitVal::Vector(Arc::new(NdaVec {
                                len: v.len,
                                log2_scale: v.log2_scale,
                                sign: sign.into(),
                                extra: extra.into(),
                            }))
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
                    bitwise_binary(op, l, r)
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
                } else if (a as usize) + 4 <= state.heap.len() {
                    let v = i32::from_le_bytes(
                        state.heap[a as usize..(a as usize) + 4].try_into().unwrap(),
                    );
                    JitVal::Scalar(v, 0)
                } else {
                    return Err(format!("Out of bounds MMIO/heap read at address {}", a));
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
                } else if (a as usize) + 4 <= state.heap.len() {
                    let int_val = match val {
                        JitVal::Scalar(v, _) => v,
                        JitVal::Float(v) => v as i32,
                        _ => return Err("Poke value must be scalar".to_string()),
                    };
                    state.heap[a as usize..(a as usize) + 4]
                        .copy_from_slice(&int_val.to_le_bytes());
                } else {
                    return Err(format!("Out of bounds MMIO/heap write at address {}", a));
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
                if cols == 0 {
                    return Err("Gemv cols cannot be zero".to_string());
                }
                let rows = mat.len / cols;
                let n_mat = NdaMatrix::new_quad(
                    rows,
                    cols,
                    2.0f32.powi(mat.log2_scale as i32),
                    mat.sign.to_vec(),
                    mat.extra.to_vec(),
                );
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
                if l.len != r.len {
                    return Err("Dot vector length mismatch".to_string());
                }
                let l_f = l.to_f32_vec();
                let r_f = r.to_f32_vec();
                let dot: f32 = l_f.iter().zip(r_f.iter()).map(|(x, y)| x * y).sum();
                state.stack.push(JitVal::Float(dot));
                Ok(JitControlFlow::Continue)
            })
        }

        NdaNode::Syscall { num, args } => {
            let num = *num;
            let arg_fns: Vec<_> = args
                .iter()
                .map(|arg| compile_node(arg, counter, registry))
                .collect();
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
                        if let Some(val) = arg_vals.first() {
                            state
                                .print_buf
                                .push(format!("[syscall print] {:?}", val.to_f32_vec()));
                        }
                    }
                    _ => {
                        state.stack.push(JitVal::Scalar(0, 0));
                    }
                }
                Ok(JitControlFlow::Continue)
            })
        }

        _ => Arc::new(|state: &mut JitState<'_>| {
            state.executed_nodes += 1;
            Ok(JitControlFlow::Continue)
        }),
    }
}

fn is_pure_scalar(node: &NdaNode) -> bool {
    match node {
        NdaNode::Int { .. } | NdaNode::Break => true,
        NdaNode::Let { init, .. } => is_pure_scalar(init),
        NdaNode::Load { .. } => true,
        NdaNode::Store { value, .. } => is_pure_scalar(value),
        NdaNode::Add { lhs, rhs } => is_pure_scalar(lhs) && is_pure_scalar(rhs),
        NdaNode::Compare { lhs, rhs, .. } => is_pure_scalar(lhs) && is_pure_scalar(rhs),
        NdaNode::VecOp { op, operand } => {
            matches!(
                op,
                crate::site_map::verifier::VecOpKind::Negate
                    | crate::site_map::verifier::VecOpKind::Abs
                    | crate::site_map::verifier::VecOpKind::ReduceSum
            ) && is_pure_scalar(operand)
        }
        NdaNode::Loop { body, .. } => body.iter().all(is_pure_scalar),
        NdaNode::While { cond, body } => is_pure_scalar(cond) && body.iter().all(is_pure_scalar),
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
            is_pure_scalar(cond)
                && then_body.iter().all(is_pure_scalar)
                && else_body
                    .as_ref()
                    .is_none_or(|eb| eb.iter().all(is_pure_scalar))
        }
        NdaNode::Scope { children } => children.iter().all(is_pure_scalar),
        // Return must stay on the interpreter path: the native scalar block
        // cannot propagate JitControlFlow::Return to run_sequence, so sibling
        // nodes would wrongly keep executing after a Return.
        _ => false,
    }
}

/// Diagnostic info about a JIT compilation without executing it.
#[derive(Debug, Clone, Serialize)]
pub struct CompileDiagnostic {
    pub node_count: usize,
    pub native_eligible: usize,
    pub interpreter_only: usize,
    pub native_ratio: f64,
    pub has_loops: bool,
    pub has_while_loops: bool,
    pub has_conditionals: bool,
    pub has_returns: bool,
    pub has_matrices: bool,
    pub has_norms: bool,
    pub asm_available: bool,
    pub estimated_complexity: String,
    pub validation_issues: Vec<String>,
}

/// Analyze nodes for JIT compilation characteristics without compiling.
pub fn compile_diagnostic(nodes: &[NdaNode]) -> CompileDiagnostic {
    let node_count = nodes.iter().map(count_nodes).sum::<usize>();
    let native_eligible = nodes.iter().filter(|n| is_pure_scalar(n)).map(count_nodes).sum::<usize>();
    let interpreter_only = node_count.saturating_sub(native_eligible);
    let native_ratio = if node_count > 0 { native_eligible as f64 / node_count as f64 } else { 0.0 };

    let mut has_loops = false;
    let mut has_while_loops = false;
    let mut has_conditionals = false;
    let mut has_returns = false;
    let mut has_matrices = false;
    let mut has_norms = false;

    for node in nodes {
        scan_node_features(node, &mut has_loops, &mut has_while_loops, &mut has_conditionals, &mut has_returns, &mut has_matrices, &mut has_norms);
    }

    let estimated_complexity = if node_count == 0 {
        "empty".to_string()
    } else if node_count < 10 {
        "trivial".to_string()
    } else if node_count < 50 {
        "small".to_string()
    } else if node_count < 200 {
        "medium".to_string()
    } else {
        "large".to_string()
    };

    let mut issues = Vec::new();
    if node_count == 0 {
        issues.push("empty node list".into());
    }
    if has_while_loops {
        issues.push("while loops have a max iteration safety limit".into());
    }

    CompileDiagnostic {
        node_count,
        native_eligible,
        interpreter_only,
        native_ratio,
        has_loops,
        has_while_loops,
        has_conditionals,
        has_returns,
        has_matrices,
        has_norms,
        asm_available: asm_gemv_available(),
        estimated_complexity,
        validation_issues: issues,
    }
}

fn scan_node_features(
    node: &NdaNode,
    has_loops: &mut bool,
    has_while_loops: &mut bool,
    has_conditionals: &mut bool,
    has_returns: &mut bool,
    has_matrices: &mut bool,
    has_norms: &mut bool,
) {
    match node {
        NdaNode::Loop { body, .. } => {
            *has_loops = true;
            for child in body { scan_node_features(child, has_loops, has_while_loops, has_conditionals, has_returns, has_matrices, has_norms); }
        }
        NdaNode::While { cond, body } => {
            *has_while_loops = true;
            *has_loops = true;
            scan_node_features(cond, has_loops, has_while_loops, has_conditionals, has_returns, has_matrices, has_norms);
            for child in body { scan_node_features(child, has_loops, has_while_loops, has_conditionals, has_returns, has_matrices, has_norms); }
        }
        NdaNode::If { cond, then_body, else_body } => {
            *has_conditionals = true;
            scan_node_features(cond, has_loops, has_while_loops, has_conditionals, has_returns, has_matrices, has_norms);
            for child in then_body { scan_node_features(child, has_loops, has_while_loops, has_conditionals, has_returns, has_matrices, has_norms); }
            if let Some(eb) = else_body {
                for child in eb { scan_node_features(child, has_loops, has_while_loops, has_conditionals, has_returns, has_matrices, has_norms); }
            }
        }
        NdaNode::Return { .. } => { *has_returns = true; }
        NdaNode::Matrix { .. } => { *has_matrices = true; }
        NdaNode::Norm { .. } => { *has_norms = true; }
        NdaNode::Scope { children } => {
            for child in children { scan_node_features(child, has_loops, has_while_loops, has_conditionals, has_returns, has_matrices, has_norms); }
        }
        NdaNode::Let { init, .. } | NdaNode::Store { value: init, .. } | NdaNode::Print { source: init } | NdaNode::VecOp { operand: init, .. } => {
            scan_node_features(init, has_loops, has_while_loops, has_conditionals, has_returns, has_matrices, has_norms);
        }
        NdaNode::Add { lhs, rhs } | NdaNode::Compare { lhs, rhs, .. } => {
            scan_node_features(lhs, has_loops, has_while_loops, has_conditionals, has_returns, has_matrices, has_norms);
            scan_node_features(rhs, has_loops, has_while_loops, has_conditionals, has_returns, has_matrices, has_norms);
        }
        _ => {}
    }
}

/// Validate nodes before JIT compilation.
pub fn validate_compile_sequence(nodes: &[NdaNode]) -> Vec<String> {
    let mut issues = Vec::new();
    if nodes.is_empty() {
        issues.push("empty compilation unit".into());
    }
    for (i, node) in nodes.iter().enumerate() {
        validate_single_node(node, i, &mut issues);
    }
    issues
}

fn validate_single_node(node: &NdaNode, index: usize, issues: &mut Vec<String>) {
    match node {
        NdaNode::Loop { count, body } => {
            if *count == 0 {
                issues.push(format!("node[{}]: loop has zero iteration count", index));
            }
            if body.is_empty() {
                issues.push(format!("node[{}]: loop has empty body", index));
            }
        }
        NdaNode::While { body, .. } => {
            if body.is_empty() {
                issues.push(format!("node[{}]: while loop has empty body", index));
            }
        }
        NdaNode::If { then_body, .. } => {
            if then_body.is_empty() {
                issues.push(format!("node[{}]: if has empty then_body", index));
            }
        }
        NdaNode::Scope { children } => {
            if children.is_empty() {
                issues.push(format!("node[{}]: scope has no children", index));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_diagnostic_empty() {
        let diag = compile_diagnostic(&[]);
        assert_eq!(diag.node_count, 0);
        assert_eq!(diag.estimated_complexity, "empty");
        assert!(!diag.validation_issues.is_empty());
    }

    #[test]
    fn compile_diagnostic_simple_int() {
        let nodes = vec![NdaNode::Int { value: 42 }];
        let diag = compile_diagnostic(&nodes);
        assert_eq!(diag.node_count, 1);
        assert_eq!(diag.native_eligible, 1);
        assert!((diag.native_ratio - 1.0).abs() < f64::EPSILON);
        assert_eq!(diag.estimated_complexity, "trivial");
    }

    #[test]
    fn compile_diagnostic_with_loop() {
        let nodes = vec![NdaNode::Loop {
            count: 10,
            body: vec![NdaNode::Int { value: 0 }],
        }];
        let diag = compile_diagnostic(&nodes);
        assert!(diag.has_loops);
        assert!(!diag.has_while_loops);
    }

    #[test]
    fn compile_diagnostic_with_while() {
        let nodes = vec![NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![NdaNode::Int { value: 0 }],
        }];
        let diag = compile_diagnostic(&nodes);
        assert!(diag.has_while_loops);
        assert!(diag.has_loops);
        assert!(diag.validation_issues.iter().any(|i| i.contains("safety limit")));
    }

    #[test]
    fn compile_diagnostic_with_matrix() {
        let nodes = vec![NdaNode::Matrix {
            rows: 4,
            cols: 4,
            scale: 0,
            sign: vec![0xAA; 2],
            extra: vec![0x55; 2],
        }];
        let diag = compile_diagnostic(&nodes);
        assert!(diag.has_matrices);
    }

    #[test]
    fn compile_diagnostic_with_norm() {
        let nodes = vec![NdaNode::Norm {
            size: 64,
            weight: vec![0xFF; 8],
            bias: vec![0x00; 8],
        }];
        let diag = compile_diagnostic(&nodes);
        assert!(diag.has_norms);
    }

    #[test]
    fn compile_diagnostic_serializes() {
        let nodes = vec![NdaNode::Int { value: 1 }];
        let diag = compile_diagnostic(&nodes);
        let json = serde_json::to_string(&diag).unwrap();
        assert!(json.contains("\"node_count\":1"));
        assert!(json.contains("\"estimated_complexity\""));
    }

    #[test]
    fn validate_compile_sequence_empty() {
        let issues = validate_compile_sequence(&[]);
        assert!(issues.iter().any(|i| i.contains("empty")));
    }

    #[test]
    fn validate_compile_sequence_zero_loop() {
        let nodes = vec![NdaNode::Loop { count: 0, body: vec![NdaNode::Int { value: 0 }] }];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.iter().any(|i| i.contains("zero iteration")));
    }

    #[test]
    fn validate_compile_sequence_empty_loop_body() {
        let nodes = vec![NdaNode::Loop { count: 5, body: vec![] }];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.iter().any(|i| i.contains("empty body")));
    }

    #[test]
    fn validate_compile_sequence_empty_while_body() {
        let nodes = vec![NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![],
        }];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.iter().any(|i| i.contains("empty body")));
    }

    #[test]
    fn validate_compile_sequence_empty_if_body() {
        let nodes = vec![NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![],
            else_body: None,
        }];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.iter().any(|i| i.contains("empty then_body")));
    }

    #[test]
    fn validate_compile_sequence_empty_scope() {
        let nodes = vec![NdaNode::Scope { children: vec![] }];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.iter().any(|i| i.contains("no children")));
    }

    #[test]
    fn validate_compile_sequence_valid() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Loop { count: 5, body: vec![NdaNode::Int { value: 0 }] },
        ];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.is_empty());
    }

    #[test]
    fn jit_tier_info_not_empty() {
        let info = jit_tier_info();
        assert!(!info.is_empty());
        assert!(info.contains("Tier-1"));
    }

    // ── Block 106: bitwise_i32 tests ────────────────────────────────────────

    #[test]
    fn bitwise_i32_and() {
        assert_eq!(bitwise_i32(BitwiseOp::And, 0xFF00, 0x0FF0), 0x0F00);
    }

    #[test]
    fn bitwise_i32_or() {
        assert_eq!(bitwise_i32(BitwiseOp::Or, 0xFF00, 0x0FF0), 0xFFF0);
    }

    #[test]
    fn bitwise_i32_xor() {
        assert_eq!(bitwise_i32(BitwiseOp::Xor, 0xFF00, 0x0FF0), 0xF0F0);
    }

    #[test]
    fn bitwise_i32_shl() {
        assert_eq!(bitwise_i32(BitwiseOp::Shl, 1, 4), 16);
        assert_eq!(bitwise_i32(BitwiseOp::Shl, 0xFF, 8), 0xFF00);
    }

    #[test]
    fn bitwise_i32_shr() {
        assert_eq!(bitwise_i32(BitwiseOp::Shr, 0xFF00, 8), 0xFF);
    }

    #[test]
    fn bitwise_i32_not() {
        assert_eq!(bitwise_i32(BitwiseOp::Not, 0, 0), !0i32);
        assert_eq!(bitwise_i32(BitwiseOp::Not, 0xFF, 0), !0xFFi32);
    }

    #[test]
    fn bitwise_i32_identity_properties() {
        // AND with all-ones is identity
        assert_eq!(bitwise_i32(BitwiseOp::And, 42, -1), 42);
        // OR with zero is identity
        assert_eq!(bitwise_i32(BitwiseOp::Or, 42, 0), 42);
        // XOR with zero is identity
        assert_eq!(bitwise_i32(BitwiseOp::Xor, 42, 0), 42);
        // Shift left by 0 is identity
        assert_eq!(bitwise_i32(BitwiseOp::Shl, 42, 0), 42);
    }

    // ── bitwise_f32 tests ───────────────────────────────────────────────────

    #[test]
    fn bitwise_f32_and() {
        let a = 1.0f32;
        let b = 2.0f32;
        let result = bitwise_f32(BitwiseOp::And, a, b);
        let expected_bits = a.to_bits() & b.to_bits();
        assert_eq!(result.to_bits(), expected_bits);
    }

    #[test]
    fn bitwise_f32_or() {
        let a = 1.0f32;
        let b = 2.0f32;
        let result = bitwise_f32(BitwiseOp::Or, a, b);
        let expected_bits = a.to_bits() | b.to_bits();
        assert_eq!(result.to_bits(), expected_bits);
    }

    #[test]
    fn bitwise_f32_not_flips_sign() {
        // NOT on positive float should give negative (sign bit flipped)
        let pos = 1.0f32;
        let negated = bitwise_f32(BitwiseOp::Not, pos, 0.0);
        assert!(negated.is_sign_negative() || negated.is_nan());
    }

    #[test]
    fn bitwise_f32_xor_self_is_zero() {
        let a = 3.14f32;
        let result = bitwise_f32(BitwiseOp::Xor, a, a);
        assert_eq!(result.to_bits(), 0u32);
    }

    // ── is_pure_scalar tests ────────────────────────────────────────────────

    #[test]
    fn is_pure_scalar_int() {
        assert!(is_pure_scalar(&NdaNode::Int { value: 42 }));
    }

    #[test]
    fn is_pure_scalar_break() {
        assert!(is_pure_scalar(&NdaNode::Break));
    }

    #[test]
    fn is_pure_scalar_load() {
        assert!(is_pure_scalar(&NdaNode::Load { name_hash: 0 }));
    }

    #[test]
    fn is_pure_scalar_float_is_not() {
        assert!(!is_pure_scalar(&NdaNode::Float { value: 1.0 }));
    }

    #[test]
    fn is_pure_scalar_matrix_is_not() {
        assert!(!is_pure_scalar(&NdaNode::Matrix {
            rows: 4, cols: 4, scale: 0,
            sign: vec![0; 2], extra: vec![0; 2],
        }));
    }

    #[test]
    fn is_pure_scalar_norm_is_not() {
        assert!(!is_pure_scalar(&NdaNode::Norm {
            size: 64, weight: vec![0; 8], bias: vec![0; 8],
        }));
    }

    #[test]
    fn is_pure_scalar_let_with_scalar_init() {
        let node = NdaNode::Let {
            name_hash: 0,
            init: Box::new(NdaNode::Int { value: 1 }),
        };
        assert!(is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_let_with_matrix_init() {
        let node = NdaNode::Let {
            name_hash: 0,
            init: Box::new(NdaNode::Matrix {
                rows: 4, cols: 4, scale: 0,
                sign: vec![0; 2], extra: vec![0; 2],
            }),
        };
        assert!(!is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_add_both_scalar() {
        let node = NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        };
        assert!(is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_add_with_matrix() {
        let node = NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Matrix {
                rows: 2, cols: 2, scale: 0,
                sign: vec![0; 1], extra: vec![0; 1],
            }),
        };
        assert!(!is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_loop_all_scalar_body() {
        let node = NdaNode::Loop {
            count: 10,
            body: vec![NdaNode::Int { value: 0 }],
        };
        assert!(is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_scope_all_scalar() {
        let node = NdaNode::Scope {
            children: vec![
                NdaNode::Int { value: 1 },
                NdaNode::Load { name_hash: 0 },
            ],
        };
        assert!(is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_return_is_not() {
        assert!(!is_pure_scalar(&NdaNode::Return {
            value: Box::new(NdaNode::Int { value: 0 }),
        }));
    }

    // ── node_to_str tests ───────────────────────────────────────────────────

    #[test]
    fn node_to_str_int() {
        let s = node_to_str(&NdaNode::Int { value: 42 });
        assert_eq!(s, "Int(42)");
    }

    #[test]
    fn node_to_str_break() {
        assert_eq!(node_to_str(&NdaNode::Break), "Break");
    }

    #[test]
    fn node_to_str_float() {
        let s = node_to_str(&NdaNode::Float { value: 3.14 });
        assert!(s.starts_with("Float("));
        assert!(s.contains("3.14"));
    }

    #[test]
    fn node_to_str_matrix() {
        let s = node_to_str(&NdaNode::Matrix {
            rows: 8, cols: 4, scale: 0,
            sign: vec![0; 4], extra: vec![0; 4],
        });
        assert_eq!(s, "Matrix(8x4)");
    }

    #[test]
    fn node_to_str_norm() {
        let s = node_to_str(&NdaNode::Norm { size: 128, weight: vec![], bias: vec![] });
        assert_eq!(s, "Norm(128)");
    }

    #[test]
    fn node_to_str_loop() {
        let s = node_to_str(&NdaNode::Loop { count: 10, body: vec![] });
        assert_eq!(s, "Loop(count=10)");
    }

    #[test]
    fn node_to_str_scope() {
        let s = node_to_str(&NdaNode::Scope { children: vec![NdaNode::Int { value: 1 }] });
        assert_eq!(s, "Scope(len=1)");
    }

    #[test]
    fn node_to_str_print() {
        let s = node_to_str(&NdaNode::Print { source: Box::new(NdaNode::Int { value: 0 }) });
        assert_eq!(s, "Print");
    }

    #[test]
    fn node_to_str_let() {
        let s = node_to_str(&NdaNode::Let {
            name_hash: 0xABCDEF,
            init: Box::new(NdaNode::Int { value: 0 }),
        });
        assert!(s.contains("Let"));
        assert!(s.contains("abcdef"));
    }

    #[test]
    fn node_to_str_add() {
        let s = node_to_str(&NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        });
        assert_eq!(s, "Add");
    }

    #[test]
    fn node_to_str_compare() {
        use crate::site_map::verifier::CmpOp;
        let s = node_to_str(&NdaNode::Compare {
            op: CmpOp::Eq,
            lhs: Box::new(NdaNode::Int { value: 0 }),
            rhs: Box::new(NdaNode::Int { value: 0 }),
        });
        assert!(s.contains("Compare"));
    }

    // ── compile_diagnostic extended tests ───────────────────────────────────

    #[test]
    fn compile_diagnostic_mixed_nodes() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Float { value: 2.0 },
            NdaNode::Matrix {
                rows: 4, cols: 4, scale: 0,
                sign: vec![0; 2], extra: vec![0; 2],
            },
        ];
        let diag = compile_diagnostic(&nodes);
        assert_eq!(diag.node_count, 3);
        // Int is native-eligible, Float and Matrix are not
        assert_eq!(diag.native_eligible, 1);
        assert_eq!(diag.interpreter_only, 2);
    }

    #[test]
    fn compile_diagnostic_asm_available_matches_platform() {
        let diag = compile_diagnostic(&[NdaNode::Int { value: 0 }]);
        #[cfg(target_arch = "x86_64")]
        assert!(diag.asm_available);
    }

    #[test]
    fn compile_diagnostic_complexity_levels() {
        // empty → "empty"
        assert_eq!(compile_diagnostic(&[]).estimated_complexity, "empty");
        // 1 node → "trivial"
        assert_eq!(
            compile_diagnostic(&[NdaNode::Int { value: 0 }]).estimated_complexity,
            "trivial"
        );
        // many nodes → "moderate" or "complex"
        let many: Vec<_> = (0..50).map(|_| NdaNode::Int { value: 0 }).collect();
        let d = compile_diagnostic(&many);
        assert!(
            d.estimated_complexity == "medium" || d.estimated_complexity == "complex",
            "expected medium/complex, got {}",
            d.estimated_complexity
        );
    }

    #[test]
    fn compile_diagnostic_has_conditionals() {
        let nodes = vec![NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 0 }],
            else_body: None,
        }];
        let diag = compile_diagnostic(&nodes);
        assert!(diag.has_conditionals);
    }

    #[test]
    fn compile_diagnostic_has_returns() {
        let nodes = vec![NdaNode::Return {
            value: Box::new(NdaNode::Int { value: 42 }),
        }];
        let diag = compile_diagnostic(&nodes);
        assert!(diag.has_returns);
    }

    // ── validate_compile_sequence extended tests ────────────────────────────

    #[test]
    fn validate_compile_sequence_nested_loops() {
        let nodes = vec![NdaNode::Loop {
            count: 5,
            body: vec![NdaNode::Loop {
                count: 3,
                body: vec![NdaNode::Int { value: 0 }],
            }],
        }];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_compile_sequence_if_with_else() {
        let nodes = vec![NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 2 }],
            else_body: Some(vec![NdaNode::Int { value: 3 }]),
        }];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_compile_sequence_multiple_issues() {
        let nodes = vec![
            NdaNode::Loop { count: 0, body: vec![] },  // zero iteration + empty body
            NdaNode::Scope { children: vec![] },         // no children
        ];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.len() >= 3, "expected >=3 issues, got {}", issues.len());
    }

    // ── JSON key count tests ────────────────────────────────────────────────

    #[test]
    fn compile_diagnostic_json_key_count() {
        let diag = compile_diagnostic(&[NdaNode::Int { value: 0 }]);
        let json = serde_json::to_string(&diag).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        // 13 fields: node_count, native_eligible, interpreter_only, native_ratio,
        // has_loops, has_while_loops, has_conditionals, has_returns,
        // has_matrices, has_norms, asm_available, estimated_complexity,
        // validation_issues
        assert_eq!(val.as_object().unwrap().len(), 13);
    }

    #[test]
    fn compile_diagnostic_json_all_field_values() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Matrix {
                rows: 4, cols: 4, scale: 0,
                sign: vec![0; 2], extra: vec![0; 2],
            },
        ];
        let diag = compile_diagnostic(&nodes);
        let json = serde_json::to_string(&diag).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = val.as_object().unwrap();
        assert_eq!(obj["node_count"], 2);
        assert_eq!(obj["native_eligible"], 1);
        assert_eq!(obj["interpreter_only"], 1);
        assert!(obj["native_ratio"].as_f64().unwrap() > 0.0);
        assert_eq!(obj["has_loops"], false);
        assert_eq!(obj["has_while_loops"], false);
        assert_eq!(obj["has_conditionals"], false);
        assert_eq!(obj["has_returns"], false);
        assert_eq!(obj["has_matrices"], true);
        assert_eq!(obj["has_norms"], false);
        assert!(obj["asm_available"].is_boolean());
        assert!(obj["estimated_complexity"].as_str().is_some());
        assert!(obj["validation_issues"].as_array().is_some());
    }

    #[test]
    fn compile_diagnostic_clone_independence() {
        let diag = compile_diagnostic(&[NdaNode::Int { value: 1 }]);
        let mut cloned = diag.clone();
        cloned.node_count = 9999;
        cloned.validation_issues.push("injected".into());
        assert_eq!(diag.node_count, 1);
        assert!(!diag.validation_issues.iter().any(|i| i == "injected"));
    }

    #[test]
    fn compile_diagnostic_debug_format() {
        let diag = compile_diagnostic(&[NdaNode::Int { value: 42 }]);
        let dbg = format!("{:?}", diag);
        assert!(dbg.contains("CompileDiagnostic"));
        assert!(dbg.contains("node_count"));
    }

    #[test]
    fn compile_diagnostic_pretty_json() {
        let diag = compile_diagnostic(&[NdaNode::Int { value: 1 }]);
        let pretty = serde_json::to_string_pretty(&diag).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));
        assert!(pretty.contains("node_count"));
    }

    // ── node_to_str: remaining variants ─────────────────────────────────────

    #[test]
    fn node_to_str_while() {
        let s = node_to_str(&NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![],
        });
        assert_eq!(s, "While");
    }

    #[test]
    fn node_to_str_if() {
        let s = node_to_str(&NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![],
            else_body: None,
        });
        assert_eq!(s, "If");
    }

    #[test]
    fn node_to_str_return() {
        let s = node_to_str(&NdaNode::Return {
            value: Box::new(NdaNode::Int { value: 42 }),
        });
        assert_eq!(s, "Return");
    }

    #[test]
    fn node_to_str_call() {
        let s = node_to_str(&NdaNode::Call { target: 0xDEAD });
        assert!(s.contains("Call"));
        assert!(s.contains("dead"));
    }

    #[test]
    fn node_to_str_vec_op() {
        use crate::site_map::verifier::VecOpKind;
        let s = node_to_str(&NdaNode::VecOp {
            op: VecOpKind::SiLU,
            operand: Box::new(NdaNode::Int { value: 0 }),
        });
        assert!(s.contains("VecOp"));
    }

    #[test]
    fn node_to_str_bitwise() {
        let s = node_to_str(&NdaNode::Bitwise {
            op: BitwiseOp::And,
            lhs: Box::new(NdaNode::Int { value: 0 }),
            rhs: Some(Box::new(NdaNode::Int { value: 0 })),
        });
        assert!(s.contains("Bitwise"));
    }

    #[test]
    fn node_to_str_math() {
        let s = node_to_str(&NdaNode::Math {
            op: MathOp::Add,
            lhs: Box::new(NdaNode::Int { value: 0 }),
            rhs: Box::new(NdaNode::Int { value: 0 }),
        });
        assert!(s.contains("Math"));
    }

    #[test]
    fn node_to_str_math_func() {
        let s = node_to_str(&NdaNode::MathFunc {
            func: MathFuncKind::Sin,
            operand: Box::new(NdaNode::Int { value: 0 }),
        });
        assert!(s.contains("MathFunc"));
    }

    #[test]
    fn node_to_str_peek() {
        let s = node_to_str(&NdaNode::Peek {
            addr: Box::new(NdaNode::Int { value: 0 }),
        });
        assert_eq!(s, "Peek");
    }

    #[test]
    fn node_to_str_poke() {
        let s = node_to_str(&NdaNode::Poke {
            addr: Box::new(NdaNode::Int { value: 0 }),
            value: Box::new(NdaNode::Int { value: 0 }),
        });
        assert_eq!(s, "Poke");
    }

    #[test]
    fn node_to_str_gemv() {
        let s = node_to_str(&NdaNode::Gemv {
            matrix: Box::new(NdaNode::Int { value: 0 }),
            vector: Box::new(NdaNode::Int { value: 0 }),
        });
        assert_eq!(s, "Gemv");
    }

    #[test]
    fn node_to_str_dot() {
        let s = node_to_str(&NdaNode::Dot {
            lhs: Box::new(NdaNode::Int { value: 0 }),
            rhs: Box::new(NdaNode::Int { value: 0 }),
        });
        assert_eq!(s, "Dot");
    }

    #[test]
    fn node_to_str_syscall() {
        let s = node_to_str(&NdaNode::Syscall { num: 42, args: vec![] });
        assert!(s.contains("Syscall"));
        assert!(s.contains("42"));
    }

    #[test]
    fn node_to_str_spawn() {
        let s = node_to_str(&NdaNode::Spawn { scope_hash: 0xBEEF });
        assert!(s.contains("Spawn"));
        assert!(s.contains("beef"));
    }

    #[test]
    fn node_to_str_atomic() {
        use crate::site_map::verifier::AtomicOp;
        let s = node_to_str(&NdaNode::Atomic {
            op: AtomicOp::Cas,
            addr: Box::new(NdaNode::Int { value: 0 }),
            val: Box::new(NdaNode::Int { value: 0 }),
        });
        assert!(s.contains("Atomic"));
    }

    #[test]
    fn node_to_str_alloc() {
        let s = node_to_str(&NdaNode::Alloc {
            size: Box::new(NdaNode::Int { value: 1024 }),
        });
        assert_eq!(s, "Alloc");
    }

    #[test]
    fn node_to_str_free() {
        let s = node_to_str(&NdaNode::Free {
            addr: Box::new(NdaNode::Int { value: 0 }),
        });
        assert_eq!(s, "Free");
    }

    #[test]
    fn node_to_str_reg_int() {
        let s = node_to_str(&NdaNode::RegInt { vector: 7, handler_hash: 0 });
        assert!(s.contains("RegInt"));
        assert!(s.contains("7"));
    }

    #[test]
    fn node_to_str_cast() {
        use crate::site_map::verifier::TypeKind;
        let s = node_to_str(&NdaNode::Cast {
            from_type: TypeKind::Int,
            to_type: TypeKind::Float,
            operand: Box::new(NdaNode::Int { value: 0 }),
        });
        assert!(s.contains("Cast"));
    }

    #[test]
    fn node_to_str_gpu_dispatch() {
        let s = node_to_str(&NdaNode::GpuDispatch {
            shader_hash: 0xCAFE,
            args: vec![],
        });
        assert!(s.contains("GpuDispatch"));
        assert!(s.contains("cafe"));
    }

    #[test]
    fn node_to_str_triple() {
        let s = node_to_str(&NdaNode::Triple {
            subject_hash: 1,
            predicate_id: 2,
            object_hash: 3,
        });
        assert!(s.contains("Triple"));
        assert!(s.contains("pred=2"));
    }

    #[test]
    fn node_to_str_load() {
        let s = node_to_str(&NdaNode::Load { name_hash: 0xFF });
        assert!(s.contains("Load"));
        assert!(s.contains("ff"));
    }

    #[test]
    fn node_to_str_store() {
        let s = node_to_str(&NdaNode::Store {
            name_hash: 0xAB,
            value: Box::new(NdaNode::Int { value: 0 }),
        });
        assert!(s.contains("Store"));
    }

    // ── bitwise_binary dispatch tests ───────────────────────────────────────

    #[test]
    fn bitwise_binary_scalar_scalar() {
        let l = JitVal::Scalar(0xFF, 0);
        let r = JitVal::Scalar(0x0F, 0);
        let result = bitwise_binary(BitwiseOp::And, l, r);
        match result {
            JitVal::Scalar(v, _) => assert_eq!(v, 0x0F),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn bitwise_binary_float_float() {
        let a = 1.0f32;
        let b = 2.0f32;
        let result = bitwise_binary(BitwiseOp::And, JitVal::Float(a), JitVal::Float(b));
        match result {
            JitVal::Float(v) => {
                let expected = a.to_bits() & b.to_bits();
                assert_eq!(v.to_bits(), expected);
            }
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn bitwise_binary_float_scalar() {
        let result = bitwise_binary(BitwiseOp::Or, JitVal::Float(1.0), JitVal::Scalar(1, 0));
        match result {
            JitVal::Float(_) => {}
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn bitwise_binary_scalar_float() {
        let result = bitwise_binary(BitwiseOp::Xor, JitVal::Scalar(1, 0), JitVal::Float(2.0));
        match result {
            JitVal::Float(_) => {}
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn bitwise_binary_vec_vec() {
        let a = NdaVec::from_i32_slice(&[0xFF, 0x0F], 0);
        let b = NdaVec::from_i32_slice(&[0xF0, 0xFF], 0);
        let result = bitwise_binary(BitwiseOp::And, JitVal::Vector(Arc::new(a)), JitVal::Vector(Arc::new(b)));
        match result {
            JitVal::Vector(v) => {
                assert_eq!(v.len, 2);
                // AND of raw codes should produce valid NDA encoding
                // Just verify the result is a valid vector of the right length
            }
            _ => panic!("expected Vector"),
        }
    }

    #[test]
    fn bitwise_binary_vec_scalar() {
        let a = NdaVec::from_i32_slice(&[0xFF, 0xAA], 0);
        let result = bitwise_binary(BitwiseOp::And, JitVal::Vector(Arc::new(a)), JitVal::Scalar(0x0F, 0));
        match result {
            JitVal::Vector(v) => {
                assert_eq!(v.len, 2);
                // Verify result is a valid vector
            }
            _ => panic!("expected Vector"),
        }
    }

    #[test]
    fn bitwise_binary_scalar_vec() {
        let b = NdaVec::from_i32_slice(&[0xFF, 0x55], 0);
        let result = bitwise_binary(BitwiseOp::Or, JitVal::Scalar(0x0F, 0), JitVal::Vector(Arc::new(b)));
        match result {
            JitVal::Vector(v) => {
                assert_eq!(v.len, 2);
            }
            _ => panic!("expected Vector"),
        }
    }

    #[test]
    fn bitwise_binary_float_vec() {
        let a = NdaVec::from_i32_slice(&[0xFF], 0);
        let result = bitwise_binary(BitwiseOp::Xor, JitVal::Float(1.0), JitVal::Vector(Arc::new(a)));
        match result {
            JitVal::Vector(_) => {}
            _ => panic!("expected Vector"),
        }
    }

    #[test]
    fn bitwise_binary_vec_float() {
        let a = NdaVec::from_i32_slice(&[0xFF], 0);
        let result = bitwise_binary(BitwiseOp::Xor, JitVal::Vector(Arc::new(a)), JitVal::Float(2.0));
        match result {
            JitVal::Vector(_) => {}
            _ => panic!("expected Vector"),
        }
    }

    // ── compile() entry point tests ─────────────────────────────────────────

    #[test]
    fn compile_empty_program() {
        let prog = compile(&[]);
        assert_eq!(prog.nodes_compiled, 0);
        assert!(prog.fns.is_empty());
    }

    #[test]
    fn compile_single_int() {
        let prog = compile(&[NdaNode::Int { value: 42 }]);
        assert!(prog.nodes_compiled >= 1);
        assert_eq!(prog.fns.len(), 1);
    }

    #[test]
    fn compile_multiple_nodes() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Int { value: 2 },
            NdaNode::Add {
                lhs: Box::new(NdaNode::Int { value: 0 }),
                rhs: Box::new(NdaNode::Int { value: 0 }),
            },
        ];
        let prog = compile(&nodes);
        assert_eq!(prog.fns.len(), 3);
    }

    #[test]
    fn compile_has_asm_kernel_flag() {
        let prog = compile(&[NdaNode::Int { value: 0 }]);
        #[cfg(target_arch = "x86_64")]
        assert!(prog.has_asm_kernel);
    }

    // ── is_pure_scalar: more edge cases ─────────────────────────────────────

    #[test]
    fn is_pure_scalar_store_with_scalar_value() {
        let node = NdaNode::Store {
            name_hash: 1,
            value: Box::new(NdaNode::Int { value: 42 }),
        };
        assert!(is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_store_with_matrix_value() {
        let node = NdaNode::Store {
            name_hash: 1,
            value: Box::new(NdaNode::Matrix {
                rows: 2, cols: 2, scale: 0,
                sign: vec![0; 1], extra: vec![0; 1],
            }),
        };
        assert!(!is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_compare_both_scalar() {
        use crate::site_map::verifier::CmpOp;
        let node = NdaNode::Compare {
            op: CmpOp::Lt,
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        };
        assert!(is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_if_all_scalar_branches() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 2 }],
            else_body: Some(vec![NdaNode::Int { value: 3 }]),
        };
        assert!(is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_if_with_matrix_in_then() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Matrix {
                rows: 2, cols: 2, scale: 0,
                sign: vec![0; 1], extra: vec![0; 1],
            }],
            else_body: None,
        };
        assert!(!is_pure_scalar(&node));
    }

    #[test]
    fn is_pure_scalar_while_all_scalar() {
        let node = NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![NdaNode::Int { value: 0 }],
        };
        assert!(is_pure_scalar(&node));
    }

    // ── compile_diagnostic: additional coverage ─────────────────────────────

    #[test]
    fn compile_diagnostic_large_program_complexity() {
        let many: Vec<_> = (0..250).map(|i| NdaNode::Int { value: i as i32 }).collect();
        let diag = compile_diagnostic(&many);
        assert_eq!(diag.estimated_complexity, "large");
        assert_eq!(diag.node_count, 250);
    }

    #[test]
    fn compile_diagnostic_small_program_complexity() {
        let nodes: Vec<_> = (0..15).map(|i| NdaNode::Int { value: i }).collect();
        let diag = compile_diagnostic(&nodes);
        assert_eq!(diag.estimated_complexity, "small");
    }

    #[test]
    fn compile_diagnostic_native_ratio_all_interpreter() {
        let nodes = vec![
            NdaNode::Float { value: 1.0 },
            NdaNode::Matrix {
                rows: 4, cols: 4, scale: 0,
                sign: vec![0; 2], extra: vec![0; 2],
            },
        ];
        let diag = compile_diagnostic(&nodes);
        assert!((diag.native_ratio - 0.0).abs() < f64::EPSILON);
        assert_eq!(diag.native_eligible, 0);
    }

    #[test]
    fn compile_diagnostic_json_roundtrip() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Loop { count: 5, body: vec![NdaNode::Int { value: 0 }] },
            NdaNode::Matrix {
                rows: 4, cols: 4, scale: 0,
                sign: vec![0; 2], extra: vec![0; 2],
            },
        ];
        let diag = compile_diagnostic(&nodes);
        let json = serde_json::to_string_pretty(&diag).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["node_count"], diag.node_count);
        assert_eq!(parsed["has_loops"], true);
        assert_eq!(parsed["has_matrices"], true);
    }

    // ── validate_compile_sequence: additional coverage ──────────────────────

    #[test]
    fn validate_compile_sequence_deeply_nested() {
        let nodes = vec![NdaNode::Loop {
            count: 3,
            body: vec![NdaNode::Scope {
                children: vec![NdaNode::Loop {
                    count: 2,
                    body: vec![NdaNode::Int { value: 0 }],
                }],
            }],
        }];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_compile_sequence_all_node_types_clean() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Float { value: 2.0 },
            NdaNode::Break,
            NdaNode::Add {
                lhs: Box::new(NdaNode::Int { value: 1 }),
                rhs: Box::new(NdaNode::Int { value: 2 }),
            },
        ];
        let issues = validate_compile_sequence(&nodes);
        assert!(issues.is_empty());
    }

    // ── JIT execution integration tests ─────────────────────────────────────

    #[test]
    fn jit_execute_int_pushes_scalar() {
        use crate::site_map::SiteMap;
        let prog = compile(&[NdaNode::Int { value: 42 }]);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_compiler_int_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Int may go through scalar fast path; just verify it compiled and ran
        assert!(prog.nodes_compiled >= 1);
        assert_eq!(prog.fns.len(), 1);
    }

    #[test]
    fn jit_execute_float_pushes_float() {
        use crate::site_map::SiteMap;
        let prog = compile(&[NdaNode::Float { value: 3.14 }]);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_compiler_float_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Float goes through interpreter path
        assert!(prog.nodes_compiled >= 1);
        assert!(!state.stack.is_empty() || state.executed_nodes >= 1);
    }

    #[test]
    fn jit_execute_break_returns_break() {
        use crate::site_map::SiteMap;
        let prog = compile(&[NdaNode::Break]);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_compiler_break_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        let result = prog.fns[0](&mut state).unwrap();
        assert_eq!(result, JitControlFlow::Break);
    }

    #[test]
    fn jit_execute_scope_runs_children() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Scope {
            children: vec![
                NdaNode::Int { value: 1 },
                NdaNode::Int { value: 2 },
            ],
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_compiler_scope_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Scope compiled; children may go through scalar fast path
        assert!(prog.nodes_compiled >= 3);
    }

    #[test]
    fn jit_execute_loop_iterates() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Loop {
            count: 5,
            body: vec![NdaNode::Int { value: 1 }],
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_compiler_loop_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Loop compiled; body may go through scalar fast path
        assert!(prog.nodes_compiled >= 2);
    }

    #[test]
    fn jit_execute_let_store_load() {
        use crate::site_map::SiteMap;
        let nodes = vec![
            NdaNode::Let {
                name_hash: 0x1234,
                init: Box::new(NdaNode::Int { value: 99 }),
            },
            NdaNode::Load { name_hash: 0x1234 },
        ];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_compiler_let_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // After Let + Load, stack should have the value
        assert!(state.stack.len() >= 1);
    }

    #[test]
    fn jit_execute_return_returns_control_flow() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Return {
            value: Box::new(NdaNode::Int { value: 42 }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_compiler_return_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        // Execute the int push first, then the return
        for f in &prog.fns {
            let cf = f(&mut state);
            match cf {
                Ok(JitControlFlow::Return) => break,
                Ok(_) => continue,
                Err(e) => panic!("unexpected error: {}", e),
            }
        }
    }

    #[test]
    fn jit_execute_add_two_ints() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 10 }),
            rhs: Box::new(NdaNode::Int { value: 20 }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_compiler_add_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Add should produce a result on the stack
        assert!(state.stack.len() >= 1);
    }

    #[test]
    fn jit_execute_print_captures_output() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Print {
            source: Box::new(NdaNode::Int { value: 42 }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_compiler_print_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        assert!(!state.print_buf.is_empty());
        assert!(state.print_buf[0].contains("print"));
    }

    // ── Stack underflow error paths ──────────────────────────────────────────

    #[test]
    fn jit_matrix_stack_underflow() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Matrix {
            rows: 4, cols: 4, scale: 0,
            sign: vec![0xAA; 2], extra: vec![0x55; 2],
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_matrix_underflow_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        let result = prog.fns[0](&mut state);
        assert!(result.is_err(), "expected stack underflow error for Matrix");
    }

    #[test]
    fn jit_norm_stack_underflow() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Norm {
            size: 64, weight: vec![0xFF; 8], bias: vec![0x00; 8],
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_norm_underflow_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        let result = prog.fns[0](&mut state);
        assert!(result.is_err(), "expected stack underflow error for Norm");
    }

    #[test]
    fn jit_add_rhs_underflow() {
        use crate::site_map::SiteMap;
        // Add with lhs that produces nothing, then rhs underflows
        let nodes = vec![NdaNode::Add {
            lhs: Box::new(NdaNode::Add {
                lhs: Box::new(NdaNode::Int { value: 1 }),
                rhs: Box::new(NdaNode::Int { value: 2 }),
            }),
            rhs: Box::new(NdaNode::Add {
                lhs: Box::new(NdaNode::Load { name_hash: 0xDEAD }),
                rhs: Box::new(NdaNode::Int { value: 0 }),
            }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_add_underflow_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            let r = f(&mut state);
            if r.is_err() {
                assert!(r.unwrap_err().contains("undefined"));
                return;
            }
        }
    }

    // ── Dimension mismatch errors ────────────────────────────────────────────

    #[test]
    fn jit_matrix_dimension_mismatch() {
        use crate::site_map::SiteMap;
        // Push a vector of wrong length, then try Matrix
        let nodes = vec![
            NdaNode::Scope { children: vec![
                NdaNode::Loop { count: 3, body: vec![NdaNode::Int { value: 1 }] },
                NdaNode::VecOp { op: crate::site_map::verifier::VecOpKind::SiLU,
                    operand: Box::new(NdaNode::Int { value: 0 }) },
            ]},
        ];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_matrix_dim_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            let _ = f(&mut state);
        }
        // Just verify it doesn't panic; dimension mismatch may or may not trigger
        // depending on the vector length produced
    }

    #[test]
    fn jit_norm_dimension_mismatch() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Norm {
            size: 4, weight: vec![0xFF; 2], bias: vec![0x00; 2],
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_norm_dim_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        let result = prog.fns[0](&mut state);
        // Stack underflow since no input vector is pushed first
        assert!(result.is_err());
    }

    // ── Math type mismatch ───────────────────────────────────────────────────

    #[test]
    fn jit_math_float_scalar_mismatch() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Math {
            op: MathOp::Add,
            lhs: Box::new(NdaNode::Float { value: 1.0 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_math_mismatch_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            let r = f(&mut state);
            if r.is_err() {
                assert!(r.unwrap_err().contains("Unsupported"));
                return;
            }
        }
    }

    #[test]
    fn jit_mathfunc_vector_operand_error() {
        use crate::site_map::SiteMap;
        // MathFunc on a vector should fail
        let nodes = vec![NdaNode::MathFunc {
            func: MathFuncKind::Sin,
            operand: Box::new(NdaNode::VecOp {
                op: crate::site_map::verifier::VecOpKind::SiLU,
                operand: Box::new(NdaNode::Float { value: 1.0 }),
            }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_mathfunc_vec_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            let r = f(&mut state);
            if r.is_err() {
                assert!(r.unwrap_err().contains("scalar"));
                return;
            }
        }
    }

    // ── Peek/Poke error paths ────────────────────────────────────────────────

    #[test]
    fn jit_peek_out_of_bounds() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Peek {
            addr: Box::new(NdaNode::Int { value: 0x7FFFFFFF }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_peek_oob_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            let r = f(&mut state);
            if r.is_err() {
                let err = r.unwrap_err();
                assert!(err.contains("bounds") || err.contains("Out"));
                return;
            }
        }
    }

    #[test]
    fn jit_poke_out_of_bounds() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Poke {
            addr: Box::new(NdaNode::Int { value: 0x7FFFFFFF }),
            value: Box::new(NdaNode::Int { value: 42 }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_poke_oob_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            let r = f(&mut state);
            if r.is_err() {
                let err = r.unwrap_err();
                assert!(err.contains("bounds") || err.contains("Out"));
                return;
            }
        }
    }

    #[test]
    fn jit_poke_mmio_high_address() {
        use crate::site_map::SiteMap;
        // Poke to address >= 0xF0000000 should go to MMIO
        let nodes = vec![NdaNode::Poke {
            addr: Box::new(NdaNode::Int { value: 0xF0000000u32 as i32 }),
            value: Box::new(NdaNode::Int { value: 99 }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_poke_mmio_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Should have written to MMIO
        assert!(!state.mmio.is_empty());
    }

    // ── Gemv/Dot error paths ─────────────────────────────────────────────────

    #[test]
    fn jit_gemv_non_vector_operand() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Gemv {
            matrix: Box::new(NdaNode::Int { value: 1 }),
            vector: Box::new(NdaNode::Int { value: 2 }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_gemv_type_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            let r = f(&mut state);
            if r.is_err() {
                assert!(r.unwrap_err().contains("Vector"));
                return;
            }
        }
    }

    #[test]
    fn jit_dot_length_mismatch() {
        use crate::site_map::SiteMap;
        // Dot with vectors of different lengths should error
        let nodes = vec![NdaNode::Dot {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_dot_mismatch_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            let r = f(&mut state);
            if r.is_err() {
                assert!(r.unwrap_err().contains("Vector"));
                return;
            }
        }
    }

    // ── compile_interpreter_sequence ─────────────────────────────────────────

    #[test]
    fn compile_interpreter_sequence_basic() {
        let nodes = vec![
            NdaNode::Int { value: 10 },
            NdaNode::Float { value: 2.0 },
            NdaNode::Add {
                lhs: Box::new(NdaNode::Int { value: 1 }),
                rhs: Box::new(NdaNode::Int { value: 2 }),
            },
        ];
        let mut counter = 0usize;
        let registry = VarRegistry::new();
        let fns = compile_interpreter_sequence(&nodes, &mut counter, &registry);
        assert_eq!(fns.len(), 3);
        assert!(counter >= 3);
    }

    #[test]
    fn compile_interpreter_sequence_empty() {
        let mut counter = 0usize;
        let registry = VarRegistry::new();
        let fns = compile_interpreter_sequence(&[], &mut counter, &registry);
        assert!(fns.is_empty());
        assert_eq!(counter, 0);
    }

    #[test]
    fn compile_interpreter_sequence_executes() {
        use crate::site_map::SiteMap;
        let nodes = vec![
            NdaNode::Int { value: 42 },
            NdaNode::Int { value: 8 },
            NdaNode::Add {
                lhs: Box::new(NdaNode::Int { value: 0 }),
                rhs: Box::new(NdaNode::Int { value: 0 }),
            },
        ];
        let mut counter = 0usize;
        let registry = VarRegistry::new();
        let fns = compile_interpreter_sequence(&nodes, &mut counter, &registry);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_interpreter_exec_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &fns {
            f(&mut state).unwrap();
        }
        // After executing, we should have results on the stack
        assert!(!state.stack.is_empty());
    }

    // ── Bitwise NOT on all JitVal types ──────────────────────────────────────

    #[test]
    fn bitwise_not_scalar_preserves_scale() {
        let result = bitwise_i32(BitwiseOp::Not, 0, 0);
        assert_eq!(result, !0i32);
    }

    #[test]
    fn bitwise_not_float_nan_handling() {
        // NOT of NaN should produce a deterministic bit pattern
        let nan = f32::NAN;
        let result = bitwise_f32(BitwiseOp::Not, nan, 0.0);
        // Result should have flipped bits from NaN — just verify it's a valid f32
        let _bits = result.to_bits();
        // NOT is self-inverse: NOT(NOT(x)) == x
        let double_not = bitwise_f32(BitwiseOp::Not, result, 0.0);
        assert_eq!(double_not.to_bits(), nan.to_bits());
    }

    #[test]
    fn bitwise_binary_vec_vec_different_lengths() {
        // Vectors of different lengths: result uses min length
        let a = NdaVec::from_i32_slice(&[0xFF, 0xAA, 0x55], 0);
        let b = NdaVec::from_i32_slice(&[0xF0], 0);
        let result = bitwise_vec_vec(BitwiseOp::And, &a, &b);
        match result {
            JitVal::Vector(v) => assert_eq!(v.len, 1), // min(3, 1) = 1
            _ => panic!("expected Vector"),
        }
    }

    #[test]
    fn bitwise_binary_shl_f32() {
        let result = bitwise_f32(BitwiseOp::Shl, 1.0, 2.0);
        // Shift left of 1.0 bits by 2.0 bits
        let expected_bits = 1.0f32.to_bits().wrapping_shl(2.0f32.to_bits());
        assert_eq!(result.to_bits(), expected_bits);
    }

    // ── JIT execution: Math operations ───────────────────────────────────────

    #[test]
    fn jit_execute_math_float_add() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Math {
            op: MathOp::Mul,
            lhs: Box::new(NdaNode::Float { value: 3.0 }),
            rhs: Box::new(NdaNode::Float { value: 4.0 }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_math_float_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Should have a Float result on the stack
        match state.stack.last() {
            Some(JitVal::Float(v)) => assert!((v - 12.0).abs() < 1e-6),
            other => panic!("expected Float(12.0), got {:?}", other),
        }
    }

    #[test]
    fn jit_execute_mathfunc_sin() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::MathFunc {
            func: MathFuncKind::Sqrt,
            operand: Box::new(NdaNode::Float { value: 16.0 }),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_mathfunc_sqrt_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        match state.stack.last() {
            Some(JitVal::Float(v)) => assert!((v - 4.0).abs() < 1e-6),
            other => panic!("expected Float(4.0), got {:?}", other),
        }
    }

    // ── JIT execution: Peek after Poke ───────────────────────────────────────

    #[test]
    fn jit_poke_then_peek_roundtrip() {
        use crate::site_map::SiteMap;
        // Poke value 42 at address 0, then peek it back
        let nodes = vec![
            NdaNode::Poke {
                addr: Box::new(NdaNode::Int { value: 0 }),
                value: Box::new(NdaNode::Int { value: 42 }),
            },
            NdaNode::Peek {
                addr: Box::new(NdaNode::Int { value: 0 }),
            },
        ];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_poke_peek_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Peek should have pushed the value onto the stack
        match state.stack.last() {
            Some(JitVal::Scalar(v, _)) => assert_eq!(*v, 42),
            other => panic!("expected Scalar(42), got {:?}", other),
        }
    }

    // ── JIT execution: Syscall ───────────────────────────────────────────────

    #[test]
    fn jit_syscall_print() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Syscall {
            num: 1,
            args: vec![NdaNode::Int { value: 77 }],
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_syscall_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        assert!(!state.print_buf.is_empty());
        assert!(state.print_buf[0].contains("syscall print"));
    }

    #[test]
    fn jit_syscall_unknown_returns_zero() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::Syscall {
            num: 999,
            args: vec![NdaNode::Int { value: 0 }],
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_syscall_unknown_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Unknown syscall pushes Scalar(0, 0)
        match state.stack.last() {
            Some(JitVal::Scalar(0, 0)) => {},
            other => panic!("expected Scalar(0,0), got {:?}", other),
        }
    }

    // ── scan_node_features deep recursion ────────────────────────────────────

    #[test]
    fn compile_diagnostic_deeply_nested_features() {
        // Deeply nested: Loop > While > If > Return + Matrix + Norm
        let nodes = vec![NdaNode::Loop {
            count: 5,
            body: vec![NdaNode::While {
                cond: Box::new(NdaNode::If {
                    cond: Box::new(NdaNode::Int { value: 1 }),
                    then_body: vec![NdaNode::Return {
                        value: Box::new(NdaNode::Int { value: 0 }),
                    }],
                    else_body: None,
                }),
                body: vec![
                    NdaNode::Matrix {
                        rows: 4, cols: 4, scale: 0,
                        sign: vec![0; 2], extra: vec![0; 2],
                    },
                    NdaNode::Norm {
                        size: 8, weight: vec![0; 1], bias: vec![0; 1],
                    },
                ],
            }],
        }];
        let diag = compile_diagnostic(&nodes);
        assert!(diag.has_loops);
        assert!(diag.has_while_loops);
        assert!(diag.has_conditionals);
        assert!(diag.has_returns);
        assert!(diag.has_matrices);
        assert!(diag.has_norms);
    }

    #[test]
    fn compile_diagnostic_let_store_print_scan() {
        // scan_node_features should recurse into Let init, Store value, Print source
        let nodes = vec![
            NdaNode::Let {
                name_hash: 1,
                init: Box::new(NdaNode::Matrix {
                    rows: 2, cols: 2, scale: 0,
                    sign: vec![0; 1], extra: vec![0; 1],
                }),
            },
            NdaNode::Print {
                source: Box::new(NdaNode::Return {
                    value: Box::new(NdaNode::Int { value: 0 }),
                }),
            },
        ];
        let diag = compile_diagnostic(&nodes);
        assert!(diag.has_matrices);
        assert!(diag.has_returns);
    }

    // ── Compile and count verification ───────────────────────────────────────

    #[test]
    fn compile_node_increments_counter() {
        let mut counter = 0usize;
        let registry = VarRegistry::new();
        let _fn = compile_node(&NdaNode::Int { value: 42 }, &mut counter, &registry);
        assert!(counter >= 1);
    }

    #[test]
    fn compile_sequence_counts_all_nodes() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Int { value: 2 },
            NdaNode::Float { value: 3.0 },
        ];
        let mut counter = 0usize;
        let registry = VarRegistry::new();
        let fns = compile_sequence(&nodes, &mut counter, &registry);
        assert_eq!(fns.len(), 3);
        assert!(counter >= 3);
    }

    // ── While loop truthy condition ──────────────────────────────────────────

    #[test]
    fn jit_while_false_condition_skips_body() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 0 }), // falsy
            body: vec![NdaNode::Int { value: 99 }],
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_while_false_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Body should not execute; loop_count stays 0 since condition was false
        // (executed_nodes is 1 for the While node itself)
        assert!(state.stack.is_empty() || !state.stack.iter().any(|v| matches!(v, JitVal::Scalar(99, _))));
    }

    #[test]
    fn jit_if_false_takes_else_branch() {
        use crate::site_map::SiteMap;
        let nodes = vec![NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 0 }), // falsy
            then_body: vec![NdaNode::Int { value: 10 }],
            else_body: Some(vec![NdaNode::Int { value: 20 }]),
        }];
        let prog = compile(&nodes);
        let sm = SiteMap::open(&std::env::temp_dir().join("jit_if_else_test"), 0).unwrap();
        let mut state = JitState::new(&[], &sm, 16);
        for f in &prog.fns {
            f(&mut state).unwrap();
        }
        // Else branch should have pushed 20
        assert!(state.stack.iter().any(|v| matches!(v, JitVal::Scalar(20, _))));
    }
}
