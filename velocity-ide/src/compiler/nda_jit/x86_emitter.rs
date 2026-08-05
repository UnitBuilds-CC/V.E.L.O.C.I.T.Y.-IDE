use std::sync::{Arc, Mutex};

use crate::nda::NdaMatrix;
use crate::nda_int::NdaVec;
use crate::site_map::verifier::{CmpOp, VecOpKind};
use crate::site_map::NdaNode;

use super::compiler::compile_interpreter_sequence;
use super::optimizer::gather_written_vars;
use super::types::{run_sequence, JitControlFlow, JitFn, JitState, JitVal, VarRegistry};

pub use super::exec_page::ExecPage;

pub struct X86Emitter {
    pub buf: Vec<u8>,
}

impl Default for X86Emitter {
    fn default() -> Self {
        Self::new()
    }
}

impl X86Emitter {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn emit(&mut self, byte: u8) {
        self.buf.push(byte);
    }

    pub fn emit_slice(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn push_rbp(&mut self) {
        self.emit(0x55);
    }

    pub fn pop_rbp(&mut self) {
        self.emit(0x5D);
    }

    pub fn mov_rbp_rsp(&mut self) {
        self.emit_slice(&[0x48, 0x89, 0xE5]);
    }

    pub fn ret(&mut self) {
        self.emit(0xC3);
    }

    pub fn mov_eax_imm32(&mut self, imm: i32) {
        self.emit(0xB8);
        self.emit_slice(&imm.to_le_bytes());
    }
}

pub fn asm_gemv_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        true
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

pub fn gemv_native(mat: &NdaMatrix, input: &NdaVec) -> NdaVec {
    crate::nda_int::nda_gemv_nda_to_nda(mat, input)
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
                VecOpKind::Negate | VecOpKind::Abs | VecOpKind::ReduceSum
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
        // `Return` must stay on the interpreter path: the native block can
        // only jump to its own epilogue and cannot signal JitControlFlow::Return
        // back to run_sequence, so sibling nodes would wrongly keep executing.
        _ => false,
    }
}

pub fn count_nodes(node: &NdaNode) -> usize {
    match node {
        NdaNode::Scope { children } => 1 + children.iter().map(count_nodes).sum::<usize>(),
        NdaNode::Loop { body, .. } => 1 + body.iter().map(count_nodes).sum::<usize>(),
        NdaNode::While { cond, body } => {
            1 + count_nodes(cond) + body.iter().map(count_nodes).sum::<usize>()
        }
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
            1 + count_nodes(cond)
                + then_body.iter().map(count_nodes).sum::<usize>()
                + else_body
                    .as_ref()
                    .map_or(0, |eb| eb.iter().map(count_nodes).sum::<usize>())
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

#[cfg(not(target_os = "windows"))]
const REG_VARS: u8 = 7; // RDI

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
        (reg_code << 3) | base_code
    } else if (-128..=127).contains(&disp) {
        0x40 | (reg_code << 3) | base_code
    } else {
        0x80 | (reg_code << 3) | base_code
    };
    emitter.emit(0x8B);
    emitter.emit(modrm);
    if disp != 0 {
        if (-128..=127).contains(&disp) {
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
        (reg_code << 3) | base_code
    } else if (-128..=127).contains(&disp) {
        0x40 | (reg_code << 3) | base_code
    } else {
        0x80 | (reg_code << 3) | base_code
    };
    emitter.emit(0x89);
    emitter.emit(modrm);
    if disp != 0 {
        if (-128..=127).contains(&disp) {
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

pub use super::symbolic_loop::detect_and_compile_symbolic_loop;

#[allow(clippy::too_many_arguments)]
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
                emitter.emit_slice(&[0x89, 0xC3]);
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
                let modrm = 0xC0 | ((src_reg as u8 & 7) << 3);
                emitter.emit_slice(&[0x44, 0x89, modrm]);
                *stack_depth = 1;
            } else if d == 1 {
                emitter.emit_slice(&[0x89, 0xC3]);
                let modrm = 0xC0 | ((src_reg as u8 & 7) << 3);
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
                return Err(
                    "Let initialization must leave exactly 1 value on the stack".to_string()
                );
            }
            let modrm = 0xC0 | (dest_reg as u8 & 7);
            emitter.emit_slice(&[0x41, 0x89, modrm]);
        }
        NdaNode::Store { name_hash, value } => {
            let slot = registry.get_or_create_slot(*name_hash);
            if slot >= 4 {
                return Err("Variable slot index >= 4 not supported in register JIT".to_string());
            }
            let dest_reg = 12 + slot;

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
                            let modrm = 0xC0 | (dest_reg as u8 & 7);
                            emitter.emit_slice(&[0x41, 0xFF, modrm]);
                            pattern_matched = true;
                        } else if *val == -1 {
                            let modrm = 0xC0 | (1 << 3) | (dest_reg as u8 & 7);
                            emitter.emit_slice(&[0x41, 0xFF, modrm]);
                            pattern_matched = true;
                        }
                    } else if let NdaNode::Load {
                        name_hash: other_hash,
                    } = other
                    {
                        let other_slot = registry.get_or_create_slot(*other_hash);
                        if other_slot < 4 {
                            let src_reg = 12 + other_slot;
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
                let modrm = 0xC0 | (dest_reg as u8 & 7);
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
            emitter.emit_slice(&[0x01, 0xD8]);
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
            emitter.emit_slice(&[0x39, 0xC3]);
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
                    emitter.emit_slice(&[0xF7, 0xD8]);
                }
                VecOpKind::Abs => {
                    emitter.emit(0x99);
                    emitter.emit_slice(&[0x31, 0xD0]);
                    emitter.emit_slice(&[0x29, 0xD0]);
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

            emitter.emit_slice(&[0x83, 0xF8, 0x00]);
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
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
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

            emitter.emit_slice(&[0x83, 0xF8, 0x00]);
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

pub fn pre_register_variables(node: &NdaNode, registry: &VarRegistry) {
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
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
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

/// Walk a pure-scalar block collecting runtime load obligations:
/// - every loaded hash (native code can only represent scalar bindings, so a
///   non-scalar binding at runtime forces the interpreter fallback), and
/// - loads on the always-executed path with no earlier definition in the
///   block, which must error as "undefined variable" like the interpreter.
fn gather_load_checks(
    node: &NdaNode,
    registry: &VarRegistry,
    defined: &mut std::collections::HashSet<u64>,
    loaded: &mut std::collections::HashSet<u64>,
    checks: &mut Vec<(usize, u64)>,
    always: bool,
) {
    match node {
        NdaNode::Load { name_hash } => {
            loaded.insert(*name_hash);
            if always && !defined.contains(name_hash) {
                checks.push((registry.get_or_create_slot(*name_hash), *name_hash));
            }
        }
        NdaNode::Let { name_hash, init } => {
            gather_load_checks(init, registry, defined, loaded, checks, always);
            if always {
                defined.insert(*name_hash);
            }
        }
        NdaNode::Store { name_hash, value } => {
            gather_load_checks(value, registry, defined, loaded, checks, always);
            if always {
                defined.insert(*name_hash);
            }
        }
        NdaNode::Scope { children } => {
            for child in children {
                gather_load_checks(child, registry, defined, loaded, checks, always);
            }
        }
        NdaNode::Loop { body, .. } => {
            for child in body {
                gather_load_checks(child, registry, defined, loaded, checks, false);
            }
        }
        NdaNode::While { cond, body } => {
            // The condition runs at least once; the body may never run.
            gather_load_checks(cond, registry, defined, loaded, checks, always);
            for child in body {
                gather_load_checks(child, registry, defined, loaded, checks, false);
            }
        }
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
            gather_load_checks(cond, registry, defined, loaded, checks, always);
            for child in then_body {
                gather_load_checks(child, registry, defined, loaded, checks, false);
            }
            if let Some(eb) = else_body {
                for child in eb {
                    gather_load_checks(child, registry, defined, loaded, checks, false);
                }
            }
        }
        NdaNode::Add { lhs, rhs } | NdaNode::Compare { lhs, rhs, .. } => {
            gather_load_checks(lhs, registry, defined, loaded, checks, always);
            gather_load_checks(rhs, registry, defined, loaded, checks, always);
        }
        NdaNode::VecOp { operand, .. } => {
            gather_load_checks(operand, registry, defined, loaded, checks, always);
        }
        _ => {}
    }
}

pub fn compile_scalar_block(nodes: &[NdaNode], registry: &VarRegistry) -> Option<JitFn> {
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

        // Loads on the always-executed path must observe the same bindings as
        // the interpreter: definitions earlier in the block mask them, any
        // other unbound load is a runtime error.
        let mut defined = std::collections::HashSet::new();
        let mut loaded_hashes = std::collections::HashSet::new();
        let mut load_checks: Vec<(usize, u64)> = Vec::new();
        for node in nodes {
            gather_load_checks(
                node,
                registry,
                &mut defined,
                &mut loaded_hashes,
                &mut load_checks,
                true,
            );
        }
        let all_loaded_slots: Vec<usize> = loaded_hashes
            .iter()
            .map(|h| registry.get_or_create_slot(*h))
            .collect();
        let mut written_hashes = std::collections::HashSet::new();
        for node in nodes {
            gather_written_vars(node, &mut written_hashes);
        }
        let written_slots: Vec<usize> = written_hashes
            .iter()
            .map(|h| registry.get_or_create_slot(*h))
            .collect();

        let mut emitter = X86Emitter::new();
        let mut loop_depth = 0;
        let mut loop_ends = Vec::new();
        let mut jumps_to_patch = Vec::new();
        let mut label_positions = std::collections::HashMap::new();
        let mut next_label_id = 0;
        let mut stack_depth = 0;

        emitter.push_rbp();
        emitter.emit(0x53);
        emitter.emit_slice(&[0x41, 0x54]);
        emitter.emit_slice(&[0x41, 0x55]);
        emitter.emit_slice(&[0x41, 0x56]);
        emitter.emit_slice(&[0x41, 0x57]);
        emitter.mov_rbp_rsp();
        emitter.emit_slice(&[0x48, 0x83, 0xEC, 0x80]);

        #[cfg(target_os = "windows")]
        emitter.emit_slice(&[0x4D, 0x89, 0xC2]);
        #[cfg(not(target_os = "windows"))]
        emitter.emit_slice(&[0x49, 0x89, 0xD2]);

        let total_slots = registry.total_slots();
        if total_slots > 4 {
            return None;
        }
        if total_slots > 0 {
            emit_mov_reg_rcx_disp(&mut emitter, 12, REG_VARS, 0);
        }
        if total_slots > 1 {
            emit_mov_reg_rcx_disp(&mut emitter, 13, REG_VARS, 4);
        }
        if total_slots > 2 {
            emit_mov_reg_rcx_disp(&mut emitter, 14, REG_VARS, 8);
        }
        if total_slots > 3 {
            emit_mov_reg_rcx_disp(&mut emitter, 15, REG_VARS, 12);
        }

        let epilogue_label = next_label_id;
        next_label_id += 1;

        for node in nodes {
            if compile_scalar_node(
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
            )
            .is_err()
            {
                return None;
            }
        }

        label_positions.insert(epilogue_label, emitter.buf.len());

        if total_slots > 0 {
            emit_mov_rcx_disp_reg(&mut emitter, REG_VARS, 0, 12);
        }
        if total_slots > 1 {
            emit_mov_rcx_disp_reg(&mut emitter, REG_VARS, 4, 13);
        }
        if total_slots > 2 {
            emit_mov_rcx_disp_reg(&mut emitter, REG_VARS, 8, 14);
        }
        if total_slots > 3 {
            emit_mov_rcx_disp_reg(&mut emitter, REG_VARS, 12, 15);
        }

        if stack_depth == 1 {
            #[cfg(target_os = "windows")]
            emitter.emit_slice(&[0x42, 0x89, 0x04, 0x92]);
            #[cfg(not(target_os = "windows"))]
            emitter.emit_slice(&[0x42, 0x89, 0x04, 0x96]);
            emitter.emit_slice(&[0x49, 0xFF, 0xC2]);
        } else if stack_depth == 2 {
            #[cfg(target_os = "windows")]
            emitter.emit_slice(&[0x42, 0x89, 0x1C, 0x92]);
            #[cfg(not(target_os = "windows"))]
            emitter.emit_slice(&[0x42, 0x89, 0x1C, 0x96]);
            emitter.emit_slice(&[0x49, 0xFF, 0xC2]);
            #[cfg(target_os = "windows")]
            emitter.emit_slice(&[0x42, 0x89, 0x04, 0x92]);
            #[cfg(not(target_os = "windows"))]
            emitter.emit_slice(&[0x42, 0x89, 0x04, 0x96]);
            emitter.emit_slice(&[0x49, 0xFF, 0xC2]);
        }

        emitter.emit_slice(&[0x4C, 0x89, 0xD0]);
        emitter.emit_slice(&[0x48, 0x89, 0xEC]);
        emitter.emit_slice(&[0x41, 0x5F]);
        emitter.emit_slice(&[0x41, 0x5E]);
        emitter.emit_slice(&[0x41, 0x5D]);
        emitter.emit_slice(&[0x41, 0x5C]);
        emitter.emit(0x5B);
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
        let mut page = ExecPage::allocate(code_len)?;
        page.write(0, &emitter.buf);

        let page = Arc::new(page);
        let page_ptr = page.as_ptr();

        type ScalarJitFunc =
            unsafe extern "C" fn(vars: *mut i32, stack: *mut i32, init_len: i32) -> i32;
        let func: ScalarJitFunc = unsafe { std::mem::transmute(page_ptr) };

        let total_slots = registry.total_slots();
        let num_nodes = nodes.len();
        let fallback_nodes = nodes.to_vec();
        let fallback_registry = registry.clone();
        let fallback_fns: Arc<Mutex<Option<Vec<JitFn>>>> = Arc::new(Mutex::new(None));
        Some(Arc::new(
            #[allow(clippy::needless_range_loop)]
            move |state: &mut JitState<'_>| {
                // Undefined loads on the always-executed path error exactly like
                // the interpreter closure path does.
                for (slot, hash) in &load_checks {
                    if state
                        .variables
                        .get(*slot)
                        .and_then(|opt| opt.as_ref())
                        .is_none()
                    {
                        return Err(format!("undefined variable (hash {:016x})", hash));
                    }
                }
                // The native block only represents scalars; if any load would
                // observe a vector/float binding, interpret the block instead.
                let needs_fallback = all_loaded_slots.iter().any(|slot| {
                    matches!(
                        state.variables.get(*slot),
                        Some(Some(JitVal::Vector(_))) | Some(Some(JitVal::Float(_)))
                    )
                });
                if needs_fallback {
                    let mut guard = fallback_fns.lock().unwrap();
                    if guard.is_none() {
                        *guard = Some(compile_interpreter_sequence(
                            &fallback_nodes,
                            &mut 0usize,
                            &fallback_registry,
                        ));
                    }
                    let fns = guard.as_ref().unwrap().clone();
                    drop(guard);
                    return run_sequence(&fns, state);
                }

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
                // Only write back slots this block may have stored to; read-only
                // slots keep their original bindings (None stays None so unbound
                // loads keep erroring, and vector/float values survive).
                for slot in &written_slots {
                    state.variables[*slot] = Some(JitVal::Scalar(temp_vars[*slot], 0));
                }

                // The native code only touches stack entries at/above the initial
                // depth, so keep the original JitVals below and append the new
                // scalar results instead of rebuilding the whole stack.
                state.stack.truncate(initial_stack_len);
                for i in initial_stack_len..final_len as usize {
                    state.stack.push(JitVal::Scalar(temp_stack[i], 0));
                }

                let _keep_alive = &page;

                Ok(JitControlFlow::Continue)
            },
        ))
    }
}
