use serde::Serialize;
use std::sync::{Arc, Mutex};

use crate::nda::NdaMatrix;
use crate::nda_int::NdaVec;
use crate::safety::SafeMutex;
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

/// Diagnostic info about the x86 emitter state.
#[derive(Debug, Clone, Serialize)]
pub struct EmitterDiagnostic {
    pub code_bytes: usize,
    pub has_prologue: bool,
    pub has_ret: bool,
    pub validation_issues: Vec<String>,
}

/// Analyze an emitter's buffer for structural correctness.
pub fn emitter_diagnostic(emitter: &X86Emitter) -> EmitterDiagnostic {
    let mut issues = Vec::new();
    let buf = &emitter.buf;

    let has_prologue = buf.len() >= 4 && buf[0] == 0x55 && buf[1..4] == [0x48, 0x89, 0xE5];
    let has_ret = buf.last() == Some(&0xC3);

    if buf.is_empty() {
        issues.push("emitter buffer is empty".into());
    }
    if !buf.is_empty() && !has_prologue {
        issues.push("missing standard prologue (push rbp; mov rbp, rsp)".into());
    }
    if !has_ret {
        issues.push("missing ret (0xC3) at end of buffer".into());
    }

    EmitterDiagnostic {
        code_bytes: buf.len(),
        has_prologue,
        has_ret,
        validation_issues: issues,
    }
}

/// Classification of a node for native compilation eligibility.
#[derive(Debug, Clone, Serialize)]
pub struct NativeCompileInfo {
    pub total_nodes: usize,
    pub native_eligible_nodes: usize,
    pub interpreter_only_nodes: usize,
    pub native_ratio: f64,
    pub is_fully_native: bool,
    pub has_loops: bool,
    pub has_while_loops: bool,
    pub has_conditionals: bool,
    pub has_returns: bool,
    pub variable_count: usize,
    pub validation_issues: Vec<String>,
}

/// Analyze an AST tree for native compilation potential without emitting code.
pub fn native_compile_info(nodes: &[NdaNode]) -> NativeCompileInfo {
    let total = nodes.iter().map(count_nodes).sum::<usize>();
    let native_eligible = nodes.iter().filter(|n| is_pure_scalar(n)).map(count_nodes).sum::<usize>();
    let interpreter_only = total.saturating_sub(native_eligible);
    let native_ratio = if total > 0 { native_eligible as f64 / total as f64 } else { 0.0 };

    let mut has_loops = false;
    let mut has_while_loops = false;
    let mut has_conditionals = false;
    let mut has_returns = false;
    let mut var_names = std::collections::HashSet::new();

    for node in nodes {
        collect_node_stats(node, &mut has_loops, &mut has_while_loops, &mut has_conditionals, &mut has_returns, &mut var_names);
    }

    let mut issues = Vec::new();
    if total == 0 {
        issues.push("empty node list".into());
    }
    if has_returns && nodes.len() > 1 {
        issues.push("return node in multi-node block may prevent sibling execution".into());
    }

    NativeCompileInfo {
        total_nodes: total,
        native_eligible_nodes: native_eligible,
        interpreter_only_nodes: interpreter_only,
        native_ratio,
        is_fully_native: nodes.iter().all(is_pure_scalar),
        has_loops,
        has_while_loops,
        has_conditionals,
        has_returns,
        variable_count: var_names.len(),
        validation_issues: issues,
    }
}

fn collect_node_stats(
    node: &NdaNode,
    has_loops: &mut bool,
    has_while_loops: &mut bool,
    has_conditionals: &mut bool,
    has_returns: &mut bool,
    var_names: &mut std::collections::HashSet<String>,
) {
    match node {
        NdaNode::Loop { .. } => { *has_loops = true; }
        NdaNode::While { .. } => { *has_while_loops = true; *has_loops = true; }
        NdaNode::If { then_body, else_body, .. } => {
            *has_conditionals = true;
            for child in then_body { collect_node_stats(child, has_loops, has_while_loops, has_conditionals, has_returns, var_names); }
            if let Some(eb) = else_body {
                for child in eb { collect_node_stats(child, has_loops, has_while_loops, has_conditionals, has_returns, var_names); }
            }
        }
        NdaNode::Return { .. } => { *has_returns = true; }
        NdaNode::Let { name_hash, init, .. } => {
            var_names.insert(name_hash.to_string());
            collect_node_stats(init, has_loops, has_while_loops, has_conditionals, has_returns, var_names);
        }
        NdaNode::Store { name_hash, value, .. } => {
            var_names.insert(name_hash.to_string());
            collect_node_stats(value, has_loops, has_while_loops, has_conditionals, has_returns, var_names);
        }
        NdaNode::Scope { children } => {
            for child in children { collect_node_stats(child, has_loops, has_while_loops, has_conditionals, has_returns, var_names); }
        }
        NdaNode::Add { lhs, rhs } | NdaNode::Compare { lhs, rhs, .. } => {
            collect_node_stats(lhs, has_loops, has_while_loops, has_conditionals, has_returns, var_names);
            collect_node_stats(rhs, has_loops, has_while_loops, has_conditionals, has_returns, var_names);
        }
        NdaNode::VecOp { operand, .. } | NdaNode::Print { source: operand } => {
            collect_node_stats(operand, has_loops, has_while_loops, has_conditionals, has_returns, var_names);
        }
        _ => {}
    }
}

/// Validate that an emitter's code size is within safe bounds for JIT execution.
pub fn validate_emitter_size(emitter: &X86Emitter, max_bytes: usize) -> Vec<String> {
    let mut issues = Vec::new();
    if emitter.buf.is_empty() {
        issues.push("emitter buffer is empty".into());
    }
    if emitter.buf.len() > max_bytes {
        issues.push(format!(
            "code size {} exceeds maximum {} bytes",
            emitter.buf.len(),
            max_bytes
        ));
    }
    // Check for unreasonably large code (potential infinite emission bug)
    if emitter.buf.len() > 1_000_000 {
        issues.push("code size exceeds 1MB, likely an emission bug".into());
    }
    issues
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
        // SAFETY: `page_ptr` points to executable memory containing valid x86-64 machine
        // code written by the emitter. The transmute converts it to a function pointer
        // matching the calling convention used by the emitted code. The `page` Arc keeps
        // the memory alive for the lifetime of the returned closure.
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
                    let mut guard = fallback_fns.lock_safe();
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

                // SAFETY: `func` is a valid JIT-compiled function pointer. `temp_vars` and
                // `temp_stack` have sufficient capacity. The function returns the new stack length.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emitter_diagnostic_empty() {
        let emitter = X86Emitter::new();
        let diag = emitter_diagnostic(&emitter);
        assert_eq!(diag.code_bytes, 0);
        assert!(!diag.has_prologue);
        assert!(!diag.has_ret);
        assert!(!diag.validation_issues.is_empty());
    }

    #[test]
    fn emitter_diagnostic_valid_minimal() {
        let mut emitter = X86Emitter::new();
        emitter.push_rbp();
        emitter.mov_rbp_rsp();
        emitter.ret();
        let diag = emitter_diagnostic(&emitter);
        assert_eq!(diag.code_bytes, 5); // push_rbp(1) + mov_rbp_rsp(3) + ret(1)
        assert!(diag.has_prologue);
        assert!(diag.has_ret);
        assert!(diag.validation_issues.is_empty());
    }

    #[test]
    fn emitter_diagnostic_missing_ret() {
        let mut emitter = X86Emitter::new();
        emitter.push_rbp();
        emitter.mov_rbp_rsp();
        let diag = emitter_diagnostic(&emitter);
        assert!(!diag.has_ret);
        assert!(diag.validation_issues.iter().any(|i| i.contains("ret")));
    }

    #[test]
    fn emitter_diagnostic_missing_prologue() {
        let mut emitter = X86Emitter::new();
        emitter.emit(0x90); // NOP
        emitter.ret();
        let diag = emitter_diagnostic(&emitter);
        assert!(!diag.has_prologue);
        assert!(diag.validation_issues.iter().any(|i| i.contains("prologue")));
    }

    #[test]
    fn emitter_diagnostic_serializes() {
        let emitter = X86Emitter::new();
        let diag = emitter_diagnostic(&emitter);
        let json = serde_json::to_string(&diag).unwrap();
        assert!(json.contains("\"code_bytes\":0"));
        assert!(json.contains("\"has_prologue\":false"));
    }

    #[test]
    fn native_compile_info_empty() {
        let info = native_compile_info(&[]);
        assert_eq!(info.total_nodes, 0);
        assert_eq!(info.native_eligible_nodes, 0);
        assert!(!info.validation_issues.is_empty());
    }

    #[test]
    fn native_compile_info_pure_scalar() {
        let node = NdaNode::Int { value: 42 };
        let info = native_compile_info(&[node]);
        assert_eq!(info.total_nodes, 1);
        assert_eq!(info.native_eligible_nodes, 1);
        assert!(info.is_fully_native);
        assert!((info.native_ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn native_compile_info_mixed() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Print { source: Box::new(NdaNode::Int { value: 2 }) },
        ];
        let info = native_compile_info(&nodes);
        assert_eq!(info.total_nodes, 3); // Int(1) + Print(1+Int(1))
        assert_eq!(info.native_eligible_nodes, 1); // only top-level Int is pure scalar
        assert!(!info.is_fully_native);
        assert!((info.native_ratio - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn native_compile_info_detects_loops() {
        let node = NdaNode::Loop {
            count: 10,
            body: vec![NdaNode::Int { value: 0 }],
        };
        let info = native_compile_info(&[node]);
        assert!(info.has_loops);
        assert!(!info.has_while_loops);
    }

    #[test]
    fn native_compile_info_detects_while() {
        let node = NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![NdaNode::Int { value: 0 }],
        };
        let info = native_compile_info(&[node]);
        assert!(info.has_while_loops);
        assert!(info.has_loops);
    }

    #[test]
    fn native_compile_info_detects_conditionals() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 2 }],
            else_body: None,
        };
        let info = native_compile_info(&[node]);
        assert!(info.has_conditionals);
    }

    #[test]
    fn native_compile_info_detects_returns() {
        let node = NdaNode::Return {
            value: Box::new(NdaNode::Int { value: 0 }),
        };
        let info = native_compile_info(&[node]);
        assert!(info.has_returns);
    }

    #[test]
    fn native_compile_info_counts_variables() {
        let nodes = vec![
            NdaNode::Let {
                name_hash: 1,
                init: Box::new(NdaNode::Int { value: 1 }),
            },
            NdaNode::Let {
                name_hash: 2,
                init: Box::new(NdaNode::Int { value: 2 }),
            },
            NdaNode::Let {
                name_hash: 1, // duplicate
                init: Box::new(NdaNode::Int { value: 3 }),
            },
        ];
        let info = native_compile_info(&nodes);
        assert_eq!(info.variable_count, 2); // hash 1 and 2
    }

    #[test]
    fn validate_emitter_size_empty() {
        let emitter = X86Emitter::new();
        let issues = validate_emitter_size(&emitter, 1024);
        assert!(issues.iter().any(|i| i.contains("empty")));
    }

    #[test]
    fn validate_emitter_size_valid() {
        let mut emitter = X86Emitter::new();
        emitter.push_rbp();
        emitter.ret();
        let issues = validate_emitter_size(&emitter, 1024);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_emitter_size_too_large() {
        let mut emitter = X86Emitter::new();
        for _ in 0..2000 {
            emitter.emit(0x90);
        }
        let issues = validate_emitter_size(&emitter, 1024);
        assert!(issues.iter().any(|i| i.contains("exceeds maximum")));
    }

    #[test]
    fn validate_emitter_size_huge() {
        let mut emitter = X86Emitter::new();
        emitter.buf.resize(1_000_001, 0x90);
        let issues = validate_emitter_size(&emitter, 2_000_000);
        assert!(issues.iter().any(|i| i.contains("1MB")));
    }

    // ── Block 108: count_nodes tests ────────────────────────────────────────

    #[test]
    fn count_nodes_int() {
        assert_eq!(count_nodes(&NdaNode::Int { value: 42 }), 1);
    }

    #[test]
    fn count_nodes_break() {
        assert_eq!(count_nodes(&NdaNode::Break), 1);
    }

    #[test]
    fn count_nodes_load() {
        assert_eq!(count_nodes(&NdaNode::Load { name_hash: 0 }), 1);
    }

    #[test]
    fn count_nodes_add() {
        let node = NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        };
        assert_eq!(count_nodes(&node), 3); // Add + 2 Int
    }

    #[test]
    fn count_nodes_let() {
        let node = NdaNode::Let {
            name_hash: 0,
            init: Box::new(NdaNode::Int { value: 42 }),
        };
        assert_eq!(count_nodes(&node), 2); // Let + Int
    }

    #[test]
    fn count_nodes_scope() {
        let node = NdaNode::Scope {
            children: vec![
                NdaNode::Int { value: 1 },
                NdaNode::Int { value: 2 },
                NdaNode::Int { value: 3 },
            ],
        };
        assert_eq!(count_nodes(&node), 4); // Scope + 3 Int
    }

    #[test]
    fn count_nodes_loop() {
        let node = NdaNode::Loop {
            count: 10,
            body: vec![NdaNode::Int { value: 0 }],
        };
        assert_eq!(count_nodes(&node), 2); // Loop + Int
    }

    #[test]
    fn count_nodes_if_no_else() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 2 }],
            else_body: None,
        };
        assert_eq!(count_nodes(&node), 3); // If + 2 Int
    }

    #[test]
    fn count_nodes_if_with_else() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 2 }],
            else_body: Some(vec![NdaNode::Int { value: 3 }]),
        };
        assert_eq!(count_nodes(&node), 4); // If + 3 Int
    }

    #[test]
    fn count_nodes_nested() {
        let node = NdaNode::Loop {
            count: 5,
            body: vec![NdaNode::If {
                cond: Box::new(NdaNode::Int { value: 1 }),
                then_body: vec![NdaNode::Add {
                    lhs: Box::new(NdaNode::Load { name_hash: 0 }),
                    rhs: Box::new(NdaNode::Int { value: 1 }),
                }],
                else_body: None,
            }],
        };
        // Loop(1) + If(1) + Int(1) + Add(1) + Load(1) + Int(1) = 6
        assert_eq!(count_nodes(&node), 6);
    }

    #[test]
    fn count_nodes_while() {
        let node = NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![NdaNode::Int { value: 0 }],
        };
        assert_eq!(count_nodes(&node), 3); // While + cond Int + body Int
    }

    #[test]
    fn count_nodes_compare() {
        let node = NdaNode::Compare {
            op: CmpOp::Eq,
            lhs: Box::new(NdaNode::Load { name_hash: 0 }),
            rhs: Box::new(NdaNode::Int { value: 0 }),
        };
        assert_eq!(count_nodes(&node), 3); // Compare + Load + Int
    }

    // ── X86Emitter basic tests ──────────────────────────────────────────────

    #[test]
    fn emitter_new_is_empty() {
        let emitter = X86Emitter::new();
        assert!(emitter.buf.is_empty());
    }

    #[test]
    fn emitter_default_is_empty() {
        let emitter = X86Emitter::default();
        assert!(emitter.buf.is_empty());
    }

    #[test]
    fn emitter_emit_single_byte() {
        let mut emitter = X86Emitter::new();
        emitter.emit(0x90);
        assert_eq!(emitter.buf.len(), 1);
        assert_eq!(emitter.buf[0], 0x90);
    }

    #[test]
    fn emitter_emit_slice() {
        let mut emitter = X86Emitter::new();
        emitter.emit_slice(&[0x48, 0x89, 0xE5]);
        assert_eq!(emitter.buf.len(), 3);
        assert_eq!(emitter.buf, vec![0x48, 0x89, 0xE5]);
    }

    #[test]
    fn emitter_push_rbp_opcode() {
        let mut emitter = X86Emitter::new();
        emitter.push_rbp();
        assert_eq!(emitter.buf, vec![0x55]);
    }

    #[test]
    fn emitter_pop_rbp_opcode() {
        let mut emitter = X86Emitter::new();
        emitter.pop_rbp();
        assert_eq!(emitter.buf, vec![0x5D]);
    }

    #[test]
    fn emitter_mov_rbp_rsp_opcode() {
        let mut emitter = X86Emitter::new();
        emitter.mov_rbp_rsp();
        assert_eq!(emitter.buf, vec![0x48, 0x89, 0xE5]);
    }

    #[test]
    fn emitter_ret_opcode() {
        let mut emitter = X86Emitter::new();
        emitter.ret();
        assert_eq!(emitter.buf, vec![0xC3]);
    }

    #[test]
    fn emitter_mov_eax_imm32_encoding() {
        let mut emitter = X86Emitter::new();
        emitter.mov_eax_imm32(42);
        assert_eq!(emitter.buf[0], 0xB8);
        let imm = i32::from_le_bytes([emitter.buf[1], emitter.buf[2], emitter.buf[3], emitter.buf[4]]);
        assert_eq!(imm, 42);
    }

    #[test]
    fn emitter_mov_eax_negative_imm32() {
        let mut emitter = X86Emitter::new();
        emitter.mov_eax_imm32(-1);
        let imm = i32::from_le_bytes([emitter.buf[1], emitter.buf[2], emitter.buf[3], emitter.buf[4]]);
        assert_eq!(imm, -1);
    }

    #[test]
    fn emitter_standard_prologue() {
        let mut emitter = X86Emitter::new();
        emitter.push_rbp();
        emitter.mov_rbp_rsp();
        assert_eq!(emitter.buf, vec![0x55, 0x48, 0x89, 0xE5]);
    }

    // ── native_compile_info extended tests ──────────────────────────────────

    #[test]
    fn native_compile_info_return_multi_node_warning() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Return { value: Box::new(NdaNode::Int { value: 0 }) },
        ];
        let info = native_compile_info(&nodes);
        assert!(info.validation_issues.iter().any(|i| i.contains("return")));
    }

    #[test]
    fn native_compile_info_serializes() {
        let info = native_compile_info(&[NdaNode::Int { value: 0 }]);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("total_nodes"));
        assert!(json.contains("native_ratio"));
        assert!(json.contains("variable_count"));
    }

    // ── is_pure_scalar via native_compile_info ────────────────────────────────

    #[test]
    fn is_pure_scalar_return_not_eligible() {
        // Return is NOT pure scalar — it must stay on interpreter path
        let node = NdaNode::Return { value: Box::new(NdaNode::Int { value: 0 }) };
        let info = native_compile_info(&[node]);
        assert!(!info.is_fully_native);
        assert_eq!(info.native_eligible_nodes, 0);
    }

    #[test]
    fn is_pure_scalar_print_not_eligible() {
        let node = NdaNode::Print { source: Box::new(NdaNode::Int { value: 1 }) };
        let info = native_compile_info(&[node]);
        assert!(!info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_call_not_eligible() {
        let node = NdaNode::Call { target: 0 };
        let info = native_compile_info(&[node]);
        assert!(!info.is_fully_native);
        assert_eq!(info.native_eligible_nodes, 0);
    }

    #[test]
    fn is_pure_scalar_matrix_not_eligible() {
        let node = NdaNode::Matrix { rows: 4, cols: 4, scale: 0, sign: vec![], extra: vec![] };
        let info = native_compile_info(&[node]);
        assert!(!info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_break_is_eligible() {
        let node = NdaNode::Break;
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
        assert_eq!(info.native_eligible_nodes, 1);
    }

    #[test]
    fn is_pure_scalar_vecop_negate_eligible() {
        let node = NdaNode::VecOp {
            op: VecOpKind::Negate,
            operand: Box::new(NdaNode::Load { name_hash: 0 }),
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_vecop_abs_eligible() {
        let node = NdaNode::VecOp {
            op: VecOpKind::Abs,
            operand: Box::new(NdaNode::Int { value: -5 }),
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_vecop_reduce_sum_eligible() {
        let node = NdaNode::VecOp {
            op: VecOpKind::ReduceSum,
            operand: Box::new(NdaNode::Load { name_hash: 0 }),
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_let_with_int_init() {
        let node = NdaNode::Let {
            name_hash: 1,
            init: Box::new(NdaNode::Int { value: 42 }),
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_store_with_load() {
        let node = NdaNode::Store {
            name_hash: 1,
            value: Box::new(NdaNode::Load { name_hash: 2 }),
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_add_of_loads() {
        let node = NdaNode::Add {
            lhs: Box::new(NdaNode::Load { name_hash: 1 }),
            rhs: Box::new(NdaNode::Load { name_hash: 2 }),
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_compare_of_ints() {
        let node = NdaNode::Compare {
            op: CmpOp::Gt,
            lhs: Box::new(NdaNode::Int { value: 5 }),
            rhs: Box::new(NdaNode::Int { value: 3 }),
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_loop_with_pure_body() {
        let node = NdaNode::Loop {
            count: 10,
            body: vec![NdaNode::Int { value: 0 }],
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_loop_with_return_body_not_eligible() {
        let node = NdaNode::Loop {
            count: 10,
            body: vec![NdaNode::Return { value: Box::new(NdaNode::Int { value: 0 }) }],
        };
        let info = native_compile_info(&[node]);
        assert!(!info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_while_with_pure_cond_and_body() {
        let node = NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![NdaNode::Load { name_hash: 0 }],
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_if_with_else_all_pure() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Load { name_hash: 0 }],
            else_body: Some(vec![NdaNode::Int { value: 0 }]),
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_scope_all_pure() {
        let node = NdaNode::Scope {
            children: vec![
                NdaNode::Int { value: 1 },
                NdaNode::Load { name_hash: 0 },
                NdaNode::Break,
            ],
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
    }

    // ── count_nodes additional variants ───────────────────────────────────────

    #[test]
    fn count_nodes_store() {
        let node = NdaNode::Store {
            name_hash: 1,
            value: Box::new(NdaNode::Int { value: 42 }),
        };
        assert_eq!(count_nodes(&node), 2); // Store + Int
    }

    #[test]
    fn count_nodes_vecop() {
        let node = NdaNode::VecOp {
            op: VecOpKind::Negate,
            operand: Box::new(NdaNode::Load { name_hash: 0 }),
        };
        assert_eq!(count_nodes(&node), 2); // VecOp + Load
    }

    #[test]
    fn count_nodes_print() {
        let node = NdaNode::Print {
            source: Box::new(NdaNode::Int { value: 7 }),
        };
        assert_eq!(count_nodes(&node), 2); // Print + Int
    }

    #[test]
    fn count_nodes_return() {
        let node = NdaNode::Return {
            value: Box::new(NdaNode::Load { name_hash: 0 }),
        };
        assert_eq!(count_nodes(&node), 2); // Return + Load
    }

    #[test]
    fn count_nodes_matrix() {
        let node = NdaNode::Matrix { rows: 4, cols: 4, scale: 0, sign: vec![], extra: vec![] };
        assert_eq!(count_nodes(&node), 1);
    }

    #[test]
    fn count_nodes_norm() {
        let node = NdaNode::Norm { size: 4, weight: vec![], bias: vec![] };
        assert_eq!(count_nodes(&node), 1);
    }

    #[test]
    fn count_nodes_call() {
        let node = NdaNode::Call { target: 0 };
        assert_eq!(count_nodes(&node), 1);
    }

    #[test]
    fn count_nodes_empty_scope() {
        let node = NdaNode::Scope { children: vec![] };
        assert_eq!(count_nodes(&node), 1); // just the Scope itself
    }

    #[test]
    fn count_nodes_empty_loop_body() {
        let node = NdaNode::Loop { count: 5, body: vec![] };
        assert_eq!(count_nodes(&node), 1); // just the Loop itself
    }

    #[test]
    fn count_nodes_deeply_nested() {
        let node = NdaNode::Scope {
            children: vec![
                NdaNode::Loop {
                    count: 3,
                    body: vec![
                        NdaNode::If {
                            cond: Box::new(NdaNode::Int { value: 1 }),
                            then_body: vec![
                                NdaNode::Add {
                                    lhs: Box::new(NdaNode::Load { name_hash: 0 }),
                                    rhs: Box::new(NdaNode::Int { value: 1 }),
                                },
                            ],
                            else_body: Some(vec![
                                NdaNode::Compare {
                                    op: CmpOp::Eq,
                                    lhs: Box::new(NdaNode::Load { name_hash: 0 }),
                                    rhs: Box::new(NdaNode::Int { value: 0 }),
                                },
                            ]),
                        },
                    ],
                },
            ],
        };
        // Scope(1) + Loop(1) + If(1) + Int(1) + Add(1) + Load(1) + Int(1) + Compare(1) + Load(1) + Int(1) = 10
        assert_eq!(count_nodes(&node), 10);
    }

    // ── native_compile_info extended ──────────────────────────────────────────

    #[test]
    fn native_compile_info_store_counts_variable() {
        let nodes = vec![
            NdaNode::Let {
                name_hash: 1,
                init: Box::new(NdaNode::Int { value: 0 }),
            },
            NdaNode::Store {
                name_hash: 1,
                value: Box::new(NdaNode::Add {
                    lhs: Box::new(NdaNode::Load { name_hash: 1 }),
                    rhs: Box::new(NdaNode::Int { value: 1 }),
                }),
            },
        ];
        let info = native_compile_info(&nodes);
        assert_eq!(info.variable_count, 1); // hash 1 only
        assert!(info.is_fully_native);
    }

    #[test]
    fn native_compile_info_if_else_counts_both_branches() {
        let nodes = vec![
            NdaNode::If {
                cond: Box::new(NdaNode::Int { value: 1 }),
                then_body: vec![
                    NdaNode::Let { name_hash: 1, init: Box::new(NdaNode::Int { value: 10 }) },
                ],
                else_body: Some(vec![
                    NdaNode::Let { name_hash: 2, init: Box::new(NdaNode::Int { value: 20 }) },
                ]),
            },
        ];
        let info = native_compile_info(&nodes);
        assert_eq!(info.variable_count, 2); // hash 1 and 2
        assert!(info.has_conditionals);
    }

    #[test]
    fn native_compile_info_nested_loops() {
        let nodes = vec![
            NdaNode::Loop {
                count: 5,
                body: vec![
                    NdaNode::Loop {
                        count: 3,
                        body: vec![NdaNode::Int { value: 0 }],
                    },
                ],
            },
        ];
        let info = native_compile_info(&nodes);
        assert!(info.has_loops);
        assert!(!info.has_while_loops);
    }

    #[test]
    fn native_compile_info_interpreter_only_sum() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Print { source: Box::new(NdaNode::Int { value: 2 }) },
        ];
        let info = native_compile_info(&nodes);
        // total = native_eligible + interpreter_only
        assert_eq!(info.total_nodes, info.native_eligible_nodes + info.interpreter_only_nodes);
    }

    #[test]
    fn native_compile_info_ratio_range() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Int { value: 2 },
            NdaNode::Print { source: Box::new(NdaNode::Int { value: 3 }) },
        ];
        let info = native_compile_info(&nodes);
        assert!(info.native_ratio >= 0.0);
        assert!(info.native_ratio <= 1.0);
    }

    #[test]
    fn native_compile_info_empty_ratio_is_zero() {
        let info = native_compile_info(&[]);
        assert!((info.native_ratio - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn native_compile_info_single_load() {
        let info = native_compile_info(&[NdaNode::Load { name_hash: 42 }]);
        assert_eq!(info.total_nodes, 1);
        assert!(info.is_fully_native);
        assert_eq!(info.variable_count, 0); // Load doesn't define a variable
    }

    // ── emitter_diagnostic edge cases ─────────────────────────────────────────

    #[test]
    fn emitter_diagnostic_only_ret() {
        let mut emitter = X86Emitter::new();
        emitter.ret();
        let diag = emitter_diagnostic(&emitter);
        assert!(!diag.has_prologue);
        assert!(diag.has_ret);
        assert!(diag.validation_issues.iter().any(|i| i.contains("prologue")));
    }

    #[test]
    fn emitter_diagnostic_only_prologue() {
        let mut emitter = X86Emitter::new();
        emitter.push_rbp();
        emitter.mov_rbp_rsp();
        let diag = emitter_diagnostic(&emitter);
        assert!(diag.has_prologue);
        assert!(!diag.has_ret);
        assert!(diag.validation_issues.iter().any(|i| i.contains("ret")));
    }

    #[test]
    fn emitter_diagnostic_wrong_prologue_bytes() {
        let mut emitter = X86Emitter::new();
        // Start with 0x55 (push rbp) but wrong following bytes
        emitter.emit(0x55);
        emitter.emit(0x90); // NOP instead of 0x48
        emitter.emit(0x90);
        emitter.emit(0x90);
        emitter.ret();
        let diag = emitter_diagnostic(&emitter);
        assert!(!diag.has_prologue);
    }

    #[test]
    fn emitter_diagnostic_code_bytes_matches_buf() {
        let mut emitter = X86Emitter::new();
        emitter.emit_slice(&[0x90; 10]);
        emitter.ret();
        let diag = emitter_diagnostic(&emitter);
        assert_eq!(diag.code_bytes, 11);
    }

    #[test]
    fn emitter_diagnostic_clone_debug() {
        let diag = EmitterDiagnostic {
            code_bytes: 42,
            has_prologue: true,
            has_ret: false,
            validation_issues: vec!["test".into()],
        };
        let cloned = diag.clone();
        assert_eq!(cloned.code_bytes, 42);
        assert!(cloned.has_prologue);
        assert!(!cloned.has_ret);
        assert_eq!(cloned.validation_issues.len(), 1);
        let debug = format!("{:?}", diag);
        assert!(debug.contains("EmitterDiagnostic"));
    }

    #[test]
    fn native_compile_info_clone_debug() {
        let info = native_compile_info(&[NdaNode::Int { value: 0 }]);
        let cloned = info.clone();
        assert_eq!(cloned.total_nodes, info.total_nodes);
        let debug = format!("{:?}", info);
        assert!(debug.contains("NativeCompileInfo"));
    }

    // ── asm_gemv_available ────────────────────────────────────────────────────

    #[test]
    fn asm_gemv_available_on_x86_64() {
        #[cfg(target_arch = "x86_64")]
        assert!(asm_gemv_available());
        #[cfg(not(target_arch = "x86_64"))]
        assert!(!asm_gemv_available());
    }

    // ── validate_emitter_size edge cases ──────────────────────────────────────

    #[test]
    fn validate_emitter_size_exact_boundary() {
        let mut emitter = X86Emitter::new();
        emitter.buf.resize(1024, 0x90);
        let issues = validate_emitter_size(&emitter, 1024);
        // 1024 == 1024, NOT > 1024, so no "exceeds" issue
        assert!(!issues.iter().any(|i| i.contains("exceeds maximum")));
    }

    #[test]
    fn validate_emitter_size_one_over() {
        let mut emitter = X86Emitter::new();
        emitter.buf.resize(1025, 0x90);
        let issues = validate_emitter_size(&emitter, 1024);
        assert!(issues.iter().any(|i| i.contains("exceeds maximum")));
    }

    #[test]
    fn validate_emitter_size_exactly_1mb() {
        let mut emitter = X86Emitter::new();
        emitter.buf.resize(1_000_000, 0x90);
        let issues = validate_emitter_size(&emitter, 2_000_000);
        // 1_000_000 is NOT > 1_000_000, so no "1MB" issue
        assert!(!issues.iter().any(|i| i.contains("1MB")));
    }

    // ── emitter mov_eax_imm32 boundary values ─────────────────────────────────

    #[test]
    fn emitter_mov_eax_imm32_zero() {
        let mut emitter = X86Emitter::new();
        emitter.mov_eax_imm32(0);
        assert_eq!(emitter.buf.len(), 5);
        let imm = i32::from_le_bytes([emitter.buf[1], emitter.buf[2], emitter.buf[3], emitter.buf[4]]);
        assert_eq!(imm, 0);
    }

    #[test]
    fn emitter_mov_eax_imm32_max() {
        let mut emitter = X86Emitter::new();
        emitter.mov_eax_imm32(i32::MAX);
        let imm = i32::from_le_bytes([emitter.buf[1], emitter.buf[2], emitter.buf[3], emitter.buf[4]]);
        assert_eq!(imm, i32::MAX);
    }

    #[test]
    fn emitter_mov_eax_imm32_min() {
        let mut emitter = X86Emitter::new();
        emitter.mov_eax_imm32(i32::MIN);
        let imm = i32::from_le_bytes([emitter.buf[1], emitter.buf[2], emitter.buf[3], emitter.buf[4]]);
        assert_eq!(imm, i32::MIN);
    }

    // ── emitter cumulative operations ─────────────────────────────────────────

    #[test]
    fn emitter_multiple_emits_accumulate() {
        let mut emitter = X86Emitter::new();
        emitter.push_rbp();
        emitter.pop_rbp();
        emitter.ret();
        assert_eq!(emitter.buf.len(), 3);
        assert_eq!(emitter.buf, vec![0x55, 0x5D, 0xC3]);
    }

    #[test]
    fn emitter_emit_slice_empty() {
        let mut emitter = X86Emitter::new();
        emitter.emit_slice(&[]);
        assert!(emitter.buf.is_empty());
    }

    // ── Block 201: x86_emitter expanded tests ────────────────────────────────

    #[test]
    fn gemv_native_produces_correct_size() {
        let mat = NdaMatrix::new_quad(4, 8, 0.0, vec![0xAA; 4], vec![0x55; 4]);
        let input = NdaVec::zeros(8, 0);
        let output = gemv_native(&mat, &input);
        assert_eq!(output.len, 4);
    }

    #[test]
    fn gemv_native_output_has_correct_length() {
        let mat = NdaMatrix::new_quad(3, 8, 0.0, vec![0xAA; 3], vec![0x55; 3]);
        let input = NdaVec::zeros(8, 0);
        let output = gemv_native(&mat, &input);
        // Output length should match matrix rows
        assert_eq!(output.len, 3);
    }

    #[test]
    fn is_pure_scalar_vecop_silu_not_eligible() {
        // SiLU is NOT in the eligible list (only Negate, Abs, ReduceSum)
        let node = NdaNode::VecOp {
            op: VecOpKind::SiLU,
            operand: Box::new(NdaNode::Load { name_hash: 0 }),
        };
        let info = native_compile_info(&[node]);
        assert!(!info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_nested_add_of_loads() {
        let node = NdaNode::Add {
            lhs: Box::new(NdaNode::Add {
                lhs: Box::new(NdaNode::Load { name_hash: 1 }),
                rhs: Box::new(NdaNode::Load { name_hash: 2 }),
            }),
            rhs: Box::new(NdaNode::Int { value: 10 }),
        };
        let info = native_compile_info(&[node]);
        assert!(info.is_fully_native);
        assert_eq!(info.total_nodes, 5); // Add(1) + Add(1) + Load(1) + Load(1) + Int(1)
    }

    #[test]
    fn native_compile_info_norm_node() {
        let node = NdaNode::Norm { size: 128, weight: vec![], bias: vec![] };
        let info = native_compile_info(&[node]);
        assert!(!info.is_fully_native); // Norm is interpreter-only
        assert_eq!(info.native_eligible_nodes, 0);
    }

    #[test]
    fn native_compile_info_multiple_variables() {
        let nodes = vec![
            NdaNode::Let { name_hash: 10, init: Box::new(NdaNode::Int { value: 1 }) },
            NdaNode::Let { name_hash: 20, init: Box::new(NdaNode::Int { value: 2 }) },
            NdaNode::Let { name_hash: 30, init: Box::new(NdaNode::Int { value: 3 }) },
            NdaNode::Store { name_hash: 10, value: Box::new(NdaNode::Load { name_hash: 20 }) },
        ];
        let info = native_compile_info(&nodes);
        assert_eq!(info.variable_count, 3); // 10, 20, 30
    }

    #[test]
    fn emitter_diagnostic_short_buffer() {
        let mut emitter = X86Emitter::new();
        emitter.emit(0x55);
        emitter.emit(0x48);
        // Only 2 bytes — too short for prologue check (needs >= 4)
        let diag = emitter_diagnostic(&emitter);
        assert!(!diag.has_prologue);
        assert_eq!(diag.code_bytes, 2);
    }

    #[test]
    fn emitter_diagnostic_three_bytes_no_ret() {
        let mut emitter = X86Emitter::new();
        emitter.emit(0x55);
        emitter.emit(0x48);
        emitter.emit(0x89);
        let diag = emitter_diagnostic(&emitter);
        assert!(!diag.has_prologue); // needs exactly [0x55, 0x48, 0x89, 0xE5]
        assert!(!diag.has_ret);
    }

    #[test]
    fn emitter_diagnostic_validation_issues_count() {
        let emitter = X86Emitter::new();
        let diag = emitter_diagnostic(&emitter);
        // Empty buffer should have at least 2 issues: empty + missing prologue
        assert!(diag.validation_issues.len() >= 2);
    }

    #[test]
    fn count_nodes_empty_while_body() {
        let node = NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![],
        };
        assert_eq!(count_nodes(&node), 2); // While + cond Int
    }

    #[test]
    fn count_nodes_nested_scopes() {
        let node = NdaNode::Scope {
            children: vec![
                NdaNode::Scope {
                    children: vec![
                        NdaNode::Scope {
                            children: vec![NdaNode::Int { value: 0 }],
                        },
                    ],
                },
            ],
        };
        // 3 Scopes + 1 Int = 4
        assert_eq!(count_nodes(&node), 4);
    }

    #[test]
    fn native_compile_info_json_all_fields() {
        let nodes = vec![
            NdaNode::Let { name_hash: 1, init: Box::new(NdaNode::Int { value: 0 }) },
            NdaNode::Loop { count: 5, body: vec![NdaNode::Int { value: 1 }] },
            NdaNode::Print { source: Box::new(NdaNode::Int { value: 2 }) },
        ];
        let info = native_compile_info(&nodes);
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("total_nodes").is_some());
        assert!(json.get("native_eligible_nodes").is_some());
        assert!(json.get("interpreter_only_nodes").is_some());
        assert!(json.get("native_ratio").is_some());
        assert!(json.get("is_fully_native").is_some());
        assert!(json.get("has_loops").is_some());
        assert!(json.get("has_while_loops").is_some());
        assert!(json.get("has_conditionals").is_some());
        assert!(json.get("has_returns").is_some());
        assert!(json.get("variable_count").is_some());
        assert!(json.get("validation_issues").is_some());
    }

    #[test]
    fn native_compile_info_total_equals_sum() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Print { source: Box::new(NdaNode::Int { value: 2 }) },
            NdaNode::Return { value: Box::new(NdaNode::Int { value: 3 }) },
            NdaNode::Loop { count: 3, body: vec![NdaNode::Load { name_hash: 0 }] },
        ];
        let info = native_compile_info(&nodes);
        assert_eq!(
            info.total_nodes,
            info.native_eligible_nodes + info.interpreter_only_nodes
        );
    }

    #[test]
    fn native_compile_info_while_sets_both_flags() {
        let node = NdaNode::While {
            cond: Box::new(NdaNode::Load { name_hash: 0 }),
            body: vec![NdaNode::Int { value: 0 }],
        };
        let info = native_compile_info(&[node]);
        assert!(info.has_loops);
        assert!(info.has_while_loops);
    }

    #[test]
    fn native_compile_info_no_loops_no_whiles() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Load { name_hash: 0 },
        ];
        let info = native_compile_info(&nodes);
        assert!(!info.has_loops);
        assert!(!info.has_while_loops);
    }

    #[test]
    fn emitter_mov_eax_imm32_length() {
        let mut emitter = X86Emitter::new();
        emitter.mov_eax_imm32(0x12345678);
        assert_eq!(emitter.buf.len(), 5); // 1 opcode + 4 bytes
    }

    #[test]
    fn emitter_multiple_mov_eax_last_wins() {
        let mut emitter = X86Emitter::new();
        emitter.mov_eax_imm32(1);
        emitter.mov_eax_imm32(2);
        assert_eq!(emitter.buf.len(), 10); // two 5-byte sequences
        let imm2 = i32::from_le_bytes([emitter.buf[6], emitter.buf[7], emitter.buf[8], emitter.buf[9]]);
        assert_eq!(imm2, 2);
    }

    #[test]
    fn emitter_diagnostic_multiple_issues() {
        // Buffer with wrong prologue bytes AND no ret
        let mut emitter = X86Emitter::new();
        emitter.emit(0x90); // NOP
        emitter.emit(0x90); // NOP
        let diag = emitter_diagnostic(&emitter);
        // Should have: missing prologue + missing ret
        assert!(diag.validation_issues.len() >= 2);
        assert!(!diag.has_prologue);
        assert!(!diag.has_ret);
    }

    #[test]
    fn count_nodes_compare_all_ops() {
        for op in &[CmpOp::Lt, CmpOp::Gt, CmpOp::Eq, CmpOp::Ne, CmpOp::Le, CmpOp::Ge] {
            let node = NdaNode::Compare {
                op: *op,
                lhs: Box::new(NdaNode::Int { value: 1 }),
                rhs: Box::new(NdaNode::Int { value: 2 }),
            };
            assert_eq!(count_nodes(&node), 3);
        }
    }

    #[test]
    fn native_compile_info_if_without_else_no_conditionals_from_scope() {
        // An If always sets has_conditionals regardless
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 0 }],
            else_body: None,
        };
        let info = native_compile_info(&[node]);
        assert!(info.has_conditionals);
    }

    #[test]
    fn is_pure_scalar_if_with_impure_then_body() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Print { source: Box::new(NdaNode::Int { value: 0 }) }],
            else_body: None,
        };
        let info = native_compile_info(&[node]);
        assert!(!info.is_fully_native);
    }

    #[test]
    fn is_pure_scalar_if_with_impure_else_body() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 0 }],
            else_body: Some(vec![NdaNode::Call { target: 0 }]),
        };
        let info = native_compile_info(&[node]);
        assert!(!info.is_fully_native);
    }

    #[test]
    fn native_compile_info_empty_has_no_flags() {
        let info = native_compile_info(&[]);
        assert!(!info.has_loops);
        assert!(!info.has_while_loops);
        assert!(!info.has_conditionals);
        assert!(!info.has_returns);
        assert_eq!(info.variable_count, 0);
    }

    #[test]
    fn validate_emitter_size_serializes() {
        let mut emitter = X86Emitter::new();
        emitter.push_rbp();
        emitter.ret();
        let issues = validate_emitter_size(&emitter, 1024);
        let json = serde_json::to_value(&issues).unwrap();
        assert!(json.is_array());
    }
}
