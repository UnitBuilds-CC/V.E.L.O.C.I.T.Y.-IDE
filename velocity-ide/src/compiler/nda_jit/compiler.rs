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
