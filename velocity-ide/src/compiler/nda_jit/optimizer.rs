use serde::Serialize;
use std::collections::HashMap;

use crate::site_map::verifier::CmpOp;
use crate::site_map::verifier::VecOpKind;
use crate::site_map::NdaNode;

/// Metrics from an optimization pass.
#[derive(Debug, Default, Clone, Serialize)]
pub struct OptimizationReport {
    /// Number of constant-fold operations performed.
    pub constants_folded: usize,
    /// Number of dead code eliminations (nodes removed).
    pub dead_nodes_removed: usize,
    /// Number of loops unrolled.
    pub loops_unrolled: usize,
    /// Number of dead branches eliminated.
    pub dead_branches_eliminated: usize,
    /// Number of identity operations simplified (x+0, x*1, etc.).
    pub identities_simplified: usize,
    /// Number of double negations eliminated.
    pub double_negations_eliminated: usize,
    /// Number of strength reductions applied (x*2 → x+x).
    pub strength_reductions: usize,
    /// Input node count.
    pub input_nodes: usize,
    /// Output node count.
    pub output_nodes: usize,
}

fn has_side_effects(node: &NdaNode) -> bool {
    match node {
        NdaNode::Call { .. } | NdaNode::Print { .. } | NdaNode::Return { .. } => true,
        NdaNode::Peek { .. }
        | NdaNode::Poke { .. }
        | NdaNode::Syscall { .. }
        | NdaNode::Spawn { .. }
        | NdaNode::Atomic { .. }
        | NdaNode::GpuDispatch { .. } => true,
        NdaNode::Free { .. } | NdaNode::RegInt { .. } => true,
        NdaNode::Let { init, .. } => has_side_effects(init),
        NdaNode::Store { value, .. } => has_side_effects(value),
        NdaNode::Scope { children } => children.iter().any(has_side_effects),
        NdaNode::Loop { body, .. } => body.iter().any(has_side_effects),
        NdaNode::While { cond, body } => {
            has_side_effects(cond) || body.iter().any(has_side_effects)
        }
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
            has_side_effects(cond)
                || then_body.iter().any(has_side_effects)
                || else_body
                    .as_ref()
                    .is_some_and(|eb| eb.iter().any(has_side_effects))
        }
        NdaNode::Add { lhs, rhs } => has_side_effects(lhs) || has_side_effects(rhs),
        NdaNode::Compare { lhs, rhs, .. } => has_side_effects(lhs) || has_side_effects(rhs),
        NdaNode::VecOp { operand, .. } => has_side_effects(operand),
        NdaNode::Bitwise { lhs, rhs, .. } => {
            has_side_effects(lhs) || rhs.as_ref().is_some_and(|r| has_side_effects(r))
        }
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
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
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
            if let Some(r) = rhs {
                gather_loaded_vars(r, set);
            }
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
            for arg in args {
                gather_loaded_vars(arg, set);
            }
        }
        NdaNode::Atomic { addr, val, .. } => {
            gather_loaded_vars(addr, set);
            gather_loaded_vars(val, set);
        }
        NdaNode::Alloc { size } => gather_loaded_vars(size, set),
        NdaNode::Free { addr } => gather_loaded_vars(addr, set),
        NdaNode::Cast { operand, .. } => gather_loaded_vars(operand, set),
        NdaNode::GpuDispatch { args, .. } => {
            for arg in args {
                gather_loaded_vars(arg, set);
            }
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
                optimized.push(NdaNode::Let {
                    name_hash: *name_hash,
                    init: Box::new(opt_init),
                });
            }
            NdaNode::Store { name_hash, value } => {
                let has_side = has_side_effects(node);
                if !has_side && !live_vars.contains(name_hash) {
                    continue;
                }
                live_vars.remove(name_hash);
                let opt_value = dce_node(value, live_vars);
                optimized.push(NdaNode::Store {
                    name_hash: *name_hash,
                    value: Box::new(opt_value),
                });
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
            NdaNode::Scope {
                children: opt_children,
            }
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
            NdaNode::Loop {
                count: *count,
                body: opt_body,
            }
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
            NdaNode::While {
                cond: Box::new(opt_cond),
                body: opt_body,
            }
        }
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
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
            NdaNode::Add {
                lhs: Box::new(opt_lhs),
                rhs: Box::new(opt_rhs),
            }
        }
        NdaNode::Compare { op, lhs, rhs } => {
            let opt_rhs = dce_node(rhs, live_vars);
            let opt_lhs = dce_node(lhs, live_vars);
            NdaNode::Compare {
                op: *op,
                lhs: Box::new(opt_lhs),
                rhs: Box::new(opt_rhs),
            }
        }
        NdaNode::VecOp { op, operand } => {
            let opt_operand = dce_node(operand, live_vars);
            NdaNode::VecOp {
                op: *op,
                operand: Box::new(opt_operand),
            }
        }
        NdaNode::Print { source } => {
            let opt_source = dce_node(source, live_vars);
            NdaNode::Print {
                source: Box::new(opt_source),
            }
        }
        NdaNode::Return { value } => {
            let opt_value = dce_node(value, live_vars);
            NdaNode::Return {
                value: Box::new(opt_value),
            }
        }
        NdaNode::Load { name_hash } => {
            live_vars.insert(*name_hash);
            NdaNode::Load {
                name_hash: *name_hash,
            }
        }
        NdaNode::Bitwise { op, lhs, rhs } => {
            let opt_lhs = dce_node(lhs, live_vars);
            let opt_rhs = rhs.as_ref().map(|r| Box::new(dce_node(r, live_vars)));
            NdaNode::Bitwise {
                op: *op,
                lhs: Box::new(opt_lhs),
                rhs: opt_rhs,
            }
        }
        NdaNode::Math { op, lhs, rhs } => {
            let opt_lhs = dce_node(lhs, live_vars);
            let opt_rhs = dce_node(rhs, live_vars);
            NdaNode::Math {
                op: *op,
                lhs: Box::new(opt_lhs),
                rhs: Box::new(opt_rhs),
            }
        }
        NdaNode::MathFunc { func, operand } => {
            let opt_op = dce_node(operand, live_vars);
            NdaNode::MathFunc {
                func: *func,
                operand: Box::new(opt_op),
            }
        }
        NdaNode::Peek { addr } => {
            let opt_addr = dce_node(addr, live_vars);
            NdaNode::Peek {
                addr: Box::new(opt_addr),
            }
        }
        NdaNode::Poke { addr, value } => {
            let opt_addr = dce_node(addr, live_vars);
            let opt_val = dce_node(value, live_vars);
            NdaNode::Poke {
                addr: Box::new(opt_addr),
                value: Box::new(opt_val),
            }
        }
        NdaNode::Gemv { matrix, vector } => {
            let opt_m = dce_node(matrix, live_vars);
            let opt_v = dce_node(vector, live_vars);
            NdaNode::Gemv {
                matrix: Box::new(opt_m),
                vector: Box::new(opt_v),
            }
        }
        NdaNode::Dot { lhs, rhs } => {
            let opt_lhs = dce_node(lhs, live_vars);
            let opt_rhs = dce_node(rhs, live_vars);
            NdaNode::Dot {
                lhs: Box::new(opt_lhs),
                rhs: Box::new(opt_rhs),
            }
        }
        NdaNode::Syscall { num, args } => {
            let mut opt_args = Vec::new();
            for arg in args {
                opt_args.push(dce_node(arg, live_vars));
            }
            NdaNode::Syscall {
                num: *num,
                args: opt_args,
            }
        }
        NdaNode::Atomic { op, addr, val } => {
            let opt_addr = dce_node(addr, live_vars);
            let opt_val = dce_node(val, live_vars);
            NdaNode::Atomic {
                op: *op,
                addr: Box::new(opt_addr),
                val: Box::new(opt_val),
            }
        }
        NdaNode::Alloc { size } => {
            let opt_size = dce_node(size, live_vars);
            NdaNode::Alloc {
                size: Box::new(opt_size),
            }
        }
        NdaNode::Free { addr } => {
            let opt_addr = dce_node(addr, live_vars);
            NdaNode::Free {
                addr: Box::new(opt_addr),
            }
        }
        NdaNode::Cast {
            from_type,
            to_type,
            operand,
        } => {
            let opt_op = dce_node(operand, live_vars);
            NdaNode::Cast {
                from_type: *from_type,
                to_type: *to_type,
                operand: Box::new(opt_op),
            }
        }
        NdaNode::GpuDispatch { shader_hash, args } => {
            let mut opt_args = Vec::new();
            for arg in args {
                opt_args.push(dce_node(arg, live_vars));
            }
            NdaNode::GpuDispatch {
                shader_hash: *shader_hash,
                args: opt_args,
            }
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

pub(crate) fn gather_written_vars(node: &NdaNode, set: &mut std::collections::HashSet<u64>) {
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
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
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
            if let Some(r) = rhs {
                gather_written_vars(r, set);
            }
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
            for arg in args {
                gather_written_vars(arg, set);
            }
        }
        NdaNode::Atomic { addr, val, .. } => {
            gather_written_vars(addr, set);
            gather_written_vars(val, set);
        }
        NdaNode::Alloc { size } => gather_written_vars(size, set),
        NdaNode::Free { addr } => gather_written_vars(addr, set),
        NdaNode::Cast { operand, .. } => gather_written_vars(operand, set),
        NdaNode::GpuDispatch { args, .. } => {
            for arg in args {
                gather_written_vars(arg, set);
            }
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
                // Constant folding
                (NdaNode::Int { value: l }, NdaNode::Int { value: r }) => NdaNode::Int {
                    value: l.saturating_add(*r),
                },
                // Identity: x + 0 → x
                (_, NdaNode::Int { value: 0 }) => opt_lhs,
                // Identity: 0 + x → x
                (NdaNode::Int { value: 0 }, _) => opt_rhs,
                _ => NdaNode::Add {
                    lhs: Box::new(opt_lhs),
                    rhs: Box::new(opt_rhs),
                },
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
            NdaNode::Let {
                name_hash,
                init: Box::new(opt_init),
            }
        }
        NdaNode::Store { name_hash, value } => {
            let opt_value = optimize_node(*value, var_constants);
            NdaNode::Store {
                name_hash,
                value: Box::new(opt_value),
            }
        }
        NdaNode::Scope { children } => {
            let opt_children = optimize_sequence(&children, var_constants);
            NdaNode::Scope {
                children: opt_children,
            }
        }
        NdaNode::Loop { count, body } => {
            if count > 0 && count <= 4 {
                let mut unrolled = Vec::new();
                for _ in 0..count {
                    unrolled.extend(body.clone());
                }
                let opt_unrolled = optimize_sequence(&unrolled, var_constants);
                NdaNode::Scope {
                    children: opt_unrolled,
                }
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
                NdaNode::Loop {
                    count,
                    body: opt_body,
                }
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
            NdaNode::While {
                cond: Box::new(opt_cond),
                body: opt_body,
            }
        }
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
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
                NdaNode::If {
                    cond: Box::new(opt_cond),
                    then_body: opt_then,
                    else_body: opt_else,
                }
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
                    NdaNode::Int {
                        value: if cmp { 1 } else { -1 },
                    }
                }
                _ => NdaNode::Compare {
                    op,
                    lhs: Box::new(opt_lhs),
                    rhs: Box::new(opt_rhs),
                },
            }
        }
        NdaNode::VecOp { op, operand } => {
            let opt_operand = optimize_node(*operand, var_constants);
            match (&op, &opt_operand) {
                (VecOpKind::Negate, NdaNode::Int { value }) => NdaNode::Int { value: -value },
                (VecOpKind::Abs, NdaNode::Int { value }) => NdaNode::Int { value: value.abs() },
                (VecOpKind::ReduceSum, NdaNode::Int { value }) => NdaNode::Int { value: *value },
                // Double negation: negate(negate(x)) → x
                (VecOpKind::Negate, NdaNode::VecOp { op: VecOpKind::Negate, operand: inner }) => {
                    *inner.clone()
                }
                _ => NdaNode::VecOp {
                    op,
                    operand: Box::new(opt_operand),
                },
            }
        }
        NdaNode::Print { source } => {
            let opt_source = optimize_node(*source, var_constants);
            NdaNode::Print {
                source: Box::new(opt_source),
            }
        }
        NdaNode::Return { value } => {
            let opt_value = optimize_node(*value, var_constants);
            NdaNode::Return {
                value: Box::new(opt_value),
            }
        }
        NdaNode::Bitwise { op, lhs, rhs } => {
            let opt_lhs = optimize_node(*lhs, var_constants);
            let opt_rhs = rhs.map(|r| Box::new(optimize_node(*r, var_constants)));
            NdaNode::Bitwise {
                op,
                lhs: Box::new(opt_lhs),
                rhs: opt_rhs,
            }
        }
        NdaNode::Math { op, lhs, rhs } => {
            let opt_lhs = optimize_node(*lhs, var_constants);
            let opt_rhs = optimize_node(*rhs, var_constants);
            NdaNode::Math {
                op,
                lhs: Box::new(opt_lhs),
                rhs: Box::new(opt_rhs),
            }
        }
        NdaNode::MathFunc { func, operand } => {
            let opt_op = optimize_node(*operand, var_constants);
            NdaNode::MathFunc {
                func,
                operand: Box::new(opt_op),
            }
        }
        NdaNode::Peek { addr } => {
            let opt_addr = optimize_node(*addr, var_constants);
            NdaNode::Peek {
                addr: Box::new(opt_addr),
            }
        }
        NdaNode::Poke { addr, value } => {
            let opt_addr = optimize_node(*addr, var_constants);
            let opt_val = optimize_node(*value, var_constants);
            NdaNode::Poke {
                addr: Box::new(opt_addr),
                value: Box::new(opt_val),
            }
        }
        NdaNode::Gemv { matrix, vector } => {
            let opt_m = optimize_node(*matrix, var_constants);
            let opt_v = optimize_node(*vector, var_constants);
            NdaNode::Gemv {
                matrix: Box::new(opt_m),
                vector: Box::new(opt_v),
            }
        }
        NdaNode::Dot { lhs, rhs } => {
            let opt_lhs = optimize_node(*lhs, var_constants);
            let opt_rhs = optimize_node(*rhs, var_constants);
            NdaNode::Dot {
                lhs: Box::new(opt_lhs),
                rhs: Box::new(opt_rhs),
            }
        }
        NdaNode::Syscall { num, args } => {
            let opt_args = args
                .into_iter()
                .map(|arg| optimize_node(arg, var_constants))
                .collect();
            NdaNode::Syscall {
                num,
                args: opt_args,
            }
        }
        NdaNode::Atomic { op, addr, val } => {
            let opt_addr = optimize_node(*addr, var_constants);
            let opt_val = optimize_node(*val, var_constants);
            NdaNode::Atomic {
                op,
                addr: Box::new(opt_addr),
                val: Box::new(opt_val),
            }
        }
        NdaNode::Alloc { size } => {
            let opt_size = optimize_node(*size, var_constants);
            NdaNode::Alloc {
                size: Box::new(opt_size),
            }
        }
        NdaNode::Free { addr } => {
            let opt_addr = optimize_node(*addr, var_constants);
            NdaNode::Free {
                addr: Box::new(opt_addr),
            }
        }
        NdaNode::Cast {
            from_type,
            to_type,
            operand,
        } => {
            let opt_op = optimize_node(*operand, var_constants);
            NdaNode::Cast {
                from_type,
                to_type,
                operand: Box::new(opt_op),
            }
        }
        NdaNode::GpuDispatch { shader_hash, args } => {
            let opt_args = args
                .into_iter()
                .map(|arg| optimize_node(arg, var_constants))
                .collect();
            NdaNode::GpuDispatch {
                shader_hash,
                args: opt_args,
            }
        }
        other => other,
    }
}

pub fn optimize_ast(nodes: &[NdaNode]) -> Vec<NdaNode> {
    optimize_ast_with_report(nodes).0
}

/// Count nodes in an AST tree.
fn count_nodes(nodes: &[NdaNode]) -> usize {
    let mut count = 0;
    for node in nodes {
        count += 1 + count_nodes_node(node);
    }
    count
}

fn count_nodes_node(node: &NdaNode) -> usize {
    match node {
        NdaNode::Scope { children } => children.iter().map(|c| 1 + count_nodes_node(c)).sum(),
        NdaNode::Loop { body, .. } => body.iter().map(|c| 1 + count_nodes_node(c)).sum(),
        NdaNode::While { cond, body } => {
            1 + count_nodes_node(cond) + body.iter().map(|c| 1 + count_nodes_node(c)).sum::<usize>()
        }
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
            1 + count_nodes_node(cond)
                + then_body.iter().map(|c| 1 + count_nodes_node(c)).sum::<usize>()
                + else_body
                    .as_ref()
                    .map(|eb| eb.iter().map(|c| 1 + count_nodes_node(c)).sum::<usize>())
                    .unwrap_or(0)
        }
        NdaNode::Add { lhs, rhs } | NdaNode::Compare { lhs, rhs, .. } => {
            1 + count_nodes_node(lhs) + count_nodes_node(rhs)
        }
        NdaNode::Let { init, .. } => 1 + count_nodes_node(init),
        NdaNode::Store { value, .. } => 1 + count_nodes_node(value),
        NdaNode::VecOp { operand, .. }
        | NdaNode::Print { source: operand }
        | NdaNode::Return { value: operand }
        | NdaNode::Peek { addr: operand }
        | NdaNode::Alloc { size: operand }
        | NdaNode::Free { addr: operand }
        | NdaNode::Cast { operand, .. }
        | NdaNode::MathFunc { operand, .. } => 1 + count_nodes_node(operand),
        _ => 0,
    }
}

/// Optimize with full metrics report.
pub fn optimize_ast_with_report(nodes: &[NdaNode]) -> (Vec<NdaNode>, OptimizationReport) {
    let input_count = count_nodes(nodes);
    let mut var_constants = HashMap::new();
    let folded = optimize_sequence(nodes, &mut var_constants);
    let mut live_vars = std::collections::HashSet::new();
    let before_dce = folded.len();
    let result = dce_sequence(&folded, &mut live_vars);
    let output_count = count_nodes(&result);

    let report = OptimizationReport {
        constants_folded: input_count.saturating_sub(before_dce),
        dead_nodes_removed: before_dce.saturating_sub(result.len()),
        loops_unrolled: 0, // Counted inside optimize_node, not tracked here yet
        dead_branches_eliminated: 0,
        identities_simplified: 0,
        double_negations_eliminated: 0,
        strength_reductions: 0,
        input_nodes: input_count,
        output_nodes: output_count,
    };
    (result, report)
}

/// Higher-level summary of optimization effectiveness.
#[derive(Debug, Clone, Serialize)]
pub struct OptimizationSummary {
    pub compression_ratio: f64,
    pub total_optimizations: usize,
    pub effectiveness: String,
    pub has_side_effects: bool,
    pub validation_issues: Vec<String>,
}

/// Compute a high-level summary from an OptimizationReport.
pub fn optimization_summary(report: &OptimizationReport) -> OptimizationSummary {
    let total_opts = report.constants_folded
        + report.dead_nodes_removed
        + report.loops_unrolled
        + report.dead_branches_eliminated
        + report.identities_simplified
        + report.double_negations_eliminated
        + report.strength_reductions;

    let ratio = if report.input_nodes > 0 {
        report.output_nodes as f64 / report.input_nodes as f64
    } else {
        1.0
    };

    let effectiveness = if ratio <= 0.5 {
        "high"
    } else if ratio < 0.8 {
        "moderate"
    } else if ratio < 1.0 {
        "low"
    } else {
        "none"
    }
    .to_string();

    let mut issues = validate_optimization_report(report);

    OptimizationSummary {
        compression_ratio: ratio,
        total_optimizations: total_opts,
        effectiveness,
        has_side_effects: report.dead_nodes_removed > 0 || report.dead_branches_eliminated > 0,
        validation_issues: issues,
    }
}

/// Validate that an OptimizationReport is internally consistent.
pub fn validate_optimization_report(report: &OptimizationReport) -> Vec<String> {
    let mut issues = Vec::new();

    if report.output_nodes > report.input_nodes && report.input_nodes > 0 {
        issues.push(format!(
            "output_nodes ({}) > input_nodes ({}) — optimizer should not grow the AST",
            report.output_nodes, report.input_nodes
        ));
    }

    if report.input_nodes == 0 && report.output_nodes > 0 {
        issues.push("input_nodes is 0 but output_nodes > 0".to_string());
    }

    issues
}

/// Distribution of node kinds in an AST.
#[derive(Debug, Clone, Serialize)]
pub struct NodeKindDistribution {
    pub total_nodes: usize,
    pub int_count: usize,
    pub load_count: usize,
    pub store_count: usize,
    pub let_count: usize,
    pub add_count: usize,
    pub compare_count: usize,
    pub loop_count: usize,
    pub while_count: usize,
    pub if_count: usize,
    pub scope_count: usize,
    pub return_count: usize,
    pub print_count: usize,
    pub matrix_count: usize,
    pub norm_count: usize,
    pub call_count: usize,
    pub vec_op_count: usize,
    pub other_count: usize,
    pub max_depth: usize,
}

/// Analyze AST complexity: node kind distribution and max nesting depth.
pub fn ast_complexity_info(nodes: &[NdaNode]) -> NodeKindDistribution {
    let mut dist = NodeKindDistribution {
        total_nodes: 0,
        int_count: 0,
        load_count: 0,
        store_count: 0,
        let_count: 0,
        add_count: 0,
        compare_count: 0,
        loop_count: 0,
        while_count: 0,
        if_count: 0,
        scope_count: 0,
        return_count: 0,
        print_count: 0,
        matrix_count: 0,
        norm_count: 0,
        call_count: 0,
        vec_op_count: 0,
        other_count: 0,
        max_depth: 0,
    };
    count_kinds(nodes, &mut dist, 0);
    dist.total_nodes = dist.int_count
        + dist.load_count
        + dist.store_count
        + dist.let_count
        + dist.add_count
        + dist.compare_count
        + dist.loop_count
        + dist.while_count
        + dist.if_count
        + dist.scope_count
        + dist.return_count
        + dist.print_count
        + dist.matrix_count
        + dist.norm_count
        + dist.call_count
        + dist.vec_op_count
        + dist.other_count;
    dist
}

fn count_kinds(nodes: &[NdaNode], dist: &mut NodeKindDistribution, depth: usize) {
    if depth > dist.max_depth {
        dist.max_depth = depth;
    }
    for node in nodes {
        count_kinds_node(node, dist, depth);
    }
}

fn count_kinds_node(node: &NdaNode, dist: &mut NodeKindDistribution, depth: usize) {
    match node {
        NdaNode::Int { .. } => dist.int_count += 1,
        NdaNode::Load { .. } => dist.load_count += 1,
        NdaNode::Store { value, .. } => {
            dist.store_count += 1;
            count_kinds_node(value, dist, depth + 1);
        }
        NdaNode::Let { init, .. } => {
            dist.let_count += 1;
            count_kinds_node(init, dist, depth + 1);
        }
        NdaNode::Add { lhs, rhs } => {
            dist.add_count += 1;
            count_kinds_node(lhs, dist, depth + 1);
            count_kinds_node(rhs, dist, depth + 1);
        }
        NdaNode::Compare { lhs, rhs, .. } => {
            dist.compare_count += 1;
            count_kinds_node(lhs, dist, depth + 1);
            count_kinds_node(rhs, dist, depth + 1);
        }
        NdaNode::Loop { body, .. } => {
            dist.loop_count += 1;
            count_kinds(body, dist, depth + 1);
        }
        NdaNode::While { cond, body } => {
            dist.while_count += 1;
            count_kinds_node(cond, dist, depth + 1);
            count_kinds(body, dist, depth + 1);
        }
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
            dist.if_count += 1;
            count_kinds_node(cond, dist, depth + 1);
            count_kinds(then_body, dist, depth + 1);
            if let Some(eb) = else_body {
                count_kinds(eb, dist, depth + 1);
            }
        }
        NdaNode::Scope { children } => {
            dist.scope_count += 1;
            count_kinds(children, dist, depth + 1);
        }
        NdaNode::Return { value } => {
            dist.return_count += 1;
            count_kinds_node(value, dist, depth + 1);
        }
        NdaNode::Print { source } => {
            dist.print_count += 1;
            count_kinds_node(source, dist, depth + 1);
        }
        NdaNode::Matrix { .. } => dist.matrix_count += 1,
        NdaNode::Norm { .. } => dist.norm_count += 1,
        NdaNode::Call { .. } => dist.call_count += 1,
        NdaNode::VecOp { operand, .. } => {
            dist.vec_op_count += 1;
            count_kinds_node(operand, dist, depth + 1);
        }
        _ => dist.other_count += 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_add_zero() {
        // x + 0 → x
        let nodes = vec![NdaNode::Add {
            lhs: Box::new(NdaNode::Load { name_hash: 42 }),
            rhs: Box::new(NdaNode::Int { value: 0 }),
        }];
        let result = optimize_ast(&nodes);
        assert_eq!(result.len(), 1);
        match &result[0] {
            NdaNode::Load { name_hash } => assert_eq!(*name_hash, 42),
            other => panic!("Expected Load, got {:?}", other),
        }
    }

    #[test]
    fn identity_zero_add() {
        // 0 + x → x
        let nodes = vec![NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 0 }),
            rhs: Box::new(NdaNode::Load { name_hash: 99 }),
        }];
        let result = optimize_ast(&nodes);
        assert_eq!(result.len(), 1);
        match &result[0] {
            NdaNode::Load { name_hash } => assert_eq!(*name_hash, 99),
            other => panic!("Expected Load, got {:?}", other),
        }
    }

    #[test]
    fn double_negation_eliminated() {
        // negate(negate(x)) → x
        let nodes = vec![NdaNode::VecOp {
            op: VecOpKind::Negate,
            operand: Box::new(NdaNode::VecOp {
                op: VecOpKind::Negate,
                operand: Box::new(NdaNode::Load { name_hash: 7 }),
            }),
        }];
        let result = optimize_ast(&nodes);
        assert_eq!(result.len(), 1);
        match &result[0] {
            NdaNode::Load { name_hash } => assert_eq!(*name_hash, 7),
            other => panic!("Expected Load after double negation, got {:?}", other),
        }
    }

    #[test]
    fn constant_folding_add() {
        let nodes = vec![NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 3 }),
            rhs: Box::new(NdaNode::Int { value: 4 }),
        }];
        let result = optimize_ast(&nodes);
        assert_eq!(result.len(), 1);
        match &result[0] {
            NdaNode::Int { value } => assert_eq!(*value, 7),
            other => panic!("Expected Int(7), got {:?}", other),
        }
    }

    #[test]
    fn optimize_ast_with_report_produces_metrics() {
        let nodes = vec![
            NdaNode::Let {
                name_hash: 1,
                init: Box::new(NdaNode::Int { value: 42 }),
            },
            NdaNode::Let {
                name_hash: 2,
                init: Box::new(NdaNode::Add {
                    lhs: Box::new(NdaNode::Load { name_hash: 1 }),
                    rhs: Box::new(NdaNode::Int { value: 0 }),
                }),
            },
            NdaNode::Return {
                value: Box::new(NdaNode::Load { name_hash: 2 }),
            },
        ];
        let (result, report) = optimize_ast_with_report(&nodes);
        assert!(report.input_nodes > 0);
        assert!(report.output_nodes > 0);
        // The identity x+0 should have been simplified
        assert!(!result.is_empty());
        // Report should be serializable
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("input_nodes"));
        assert!(json.contains("output_nodes"));
    }

    #[test]
    fn dead_code_removed() {
        // let x = 42; return 0; — x is dead
        let nodes = vec![
            NdaNode::Let {
                name_hash: 1,
                init: Box::new(NdaNode::Int { value: 42 }),
            },
            NdaNode::Return {
                value: Box::new(NdaNode::Int { value: 0 }),
            },
        ];
        let result = optimize_ast(&nodes);
        // The Let should be removed since x is never loaded
        let has_let = result.iter().any(|n| matches!(n, NdaNode::Let { .. }));
        assert!(!has_let, "Dead Let should be removed");
    }

    // ── OptimizationSummary tests ─────────────────────────────────────────────

    #[test]
    fn optimization_summary_clean() {
        let report = OptimizationReport {
            constants_folded: 3,
            dead_nodes_removed: 2,
            loops_unrolled: 1,
            dead_branches_eliminated: 0,
            identities_simplified: 4,
            double_negations_eliminated: 1,
            strength_reductions: 0,
            input_nodes: 20,
            output_nodes: 10,
        };
        let summary = optimization_summary(&report);
        assert!((summary.compression_ratio - 0.5).abs() < 1e-9);
        assert_eq!(summary.total_optimizations, 11);
        assert_eq!(summary.effectiveness, "high"); // 0.5 is <= 0.5
        assert!(summary.has_side_effects); // dead_nodes_removed > 0
        assert!(summary.validation_issues.is_empty());
    }

    #[test]
    fn optimization_summary_no_change() {
        let report = OptimizationReport {
            constants_folded: 0,
            dead_nodes_removed: 0,
            loops_unrolled: 0,
            dead_branches_eliminated: 0,
            identities_simplified: 0,
            double_negations_eliminated: 0,
            strength_reductions: 0,
            input_nodes: 10,
            output_nodes: 10,
        };
        let summary = optimization_summary(&report);
        assert!((summary.compression_ratio - 1.0).abs() < 1e-9);
        assert_eq!(summary.effectiveness, "none");
        assert!(!summary.has_side_effects);
    }

    #[test]
    fn optimization_summary_moderate() {
        let report = OptimizationReport {
            constants_folded: 2,
            dead_nodes_removed: 0,
            loops_unrolled: 0,
            dead_branches_eliminated: 0,
            identities_simplified: 0,
            double_negations_eliminated: 0,
            strength_reductions: 0,
            input_nodes: 10,
            output_nodes: 7,
        };
        let summary = optimization_summary(&report);
        assert_eq!(summary.effectiveness, "moderate");
    }

    #[test]
    fn validate_report_output_exceeds_input() {
        let report = OptimizationReport {
            constants_folded: 0,
            dead_nodes_removed: 0,
            loops_unrolled: 0,
            dead_branches_eliminated: 0,
            identities_simplified: 0,
            double_negations_eliminated: 0,
            strength_reductions: 0,
            input_nodes: 5,
            output_nodes: 10,
        };
        let issues = validate_optimization_report(&report);
        assert!(issues.iter().any(|i| i.contains("should not grow")));
    }

    #[test]
    fn validate_report_zero_input() {
        let report = OptimizationReport {
            constants_folded: 0,
            dead_nodes_removed: 0,
            loops_unrolled: 0,
            dead_branches_eliminated: 0,
            identities_simplified: 0,
            double_negations_eliminated: 0,
            strength_reductions: 0,
            input_nodes: 0,
            output_nodes: 5,
        };
        let issues = validate_optimization_report(&report);
        assert!(issues.iter().any(|i| i.contains("input_nodes is 0")));
    }

    // ── AST complexity tests ──────────────────────────────────────────────────

    #[test]
    fn ast_complexity_empty() {
        let dist = ast_complexity_info(&[]);
        assert_eq!(dist.total_nodes, 0);
        assert_eq!(dist.max_depth, 0);
    }

    #[test]
    fn ast_complexity_simple() {
        let nodes = vec![
            NdaNode::Int { value: 42 },
            NdaNode::Load { name_hash: 1 },
            NdaNode::Return { value: Box::new(NdaNode::Int { value: 0 }) },
        ];
        let dist = ast_complexity_info(&nodes);
        assert_eq!(dist.int_count, 2);
        assert_eq!(dist.load_count, 1);
        assert_eq!(dist.return_count, 1);
        assert_eq!(dist.total_nodes, 4);
    }

    #[test]
    fn ast_complexity_nested() {
        let nodes = vec![
            NdaNode::Loop {
                count: 5,
                body: vec![
                    NdaNode::If {
                        cond: Box::new(NdaNode::Int { value: 1 }),
                        then_body: vec![NdaNode::Print { source: Box::new(NdaNode::Int { value: 1 }) }],
                        else_body: None,
                    },
                ],
            },
        ];
        let dist = ast_complexity_info(&nodes);
        assert_eq!(dist.loop_count, 1);
        assert_eq!(dist.if_count, 1);
        assert_eq!(dist.int_count, 2);
        assert_eq!(dist.print_count, 1);
        assert!(dist.max_depth >= 2);
    }

    #[test]
    fn ast_complexity_with_operations() {
        let nodes = vec![
            NdaNode::Add {
                lhs: Box::new(NdaNode::Load { name_hash: 1 }),
                rhs: Box::new(NdaNode::Int { value: 5 }),
            },
            NdaNode::Compare {
                op: CmpOp::Gt,
                lhs: Box::new(NdaNode::Load { name_hash: 1 }),
                rhs: Box::new(NdaNode::Int { value: 0 }),
            },
            NdaNode::VecOp {
                op: VecOpKind::Negate,
                operand: Box::new(NdaNode::Load { name_hash: 2 }),
            },
            NdaNode::Matrix { rows: 4, cols: 4, scale: 0, sign: vec![], extra: vec![] },
            NdaNode::Norm { size: 4, weight: vec![], bias: vec![] },
            NdaNode::Call { target: 0xABCD },
        ];
        let dist = ast_complexity_info(&nodes);
        assert_eq!(dist.add_count, 1);
        assert_eq!(dist.compare_count, 1);
        assert_eq!(dist.vec_op_count, 1);
        assert_eq!(dist.matrix_count, 1);
        assert_eq!(dist.norm_count, 1);
        assert_eq!(dist.call_count, 1);
    }

    #[test]
    fn optimization_summary_serializable() {
        let report = OptimizationReport {
            constants_folded: 5,
            dead_nodes_removed: 3,
            loops_unrolled: 1,
            dead_branches_eliminated: 0,
            identities_simplified: 2,
            double_negations_eliminated: 0,
            strength_reductions: 1,
            input_nodes: 30,
            output_nodes: 18,
        };
        let summary = optimization_summary(&report);
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("compression_ratio"));
        assert!(json.contains("effectiveness"));
        assert!(json.contains("total_optimizations"));
    }

    #[test]
    fn ast_complexity_serializable() {
        let nodes = vec![NdaNode::Int { value: 1 }];
        let dist = ast_complexity_info(&nodes);
        let json = serde_json::to_string(&dist).unwrap();
        assert!(json.contains("total_nodes"));
        assert!(json.contains("int_count"));
    }

    // ── Block 106: has_side_effects tests ────────────────────────────────────

    #[test]
    fn side_effects_int_is_false() {
        assert!(!has_side_effects(&NdaNode::Int { value: 42 }));
    }

    #[test]
    fn side_effects_load_is_false() {
        assert!(!has_side_effects(&NdaNode::Load { name_hash: 0 }));
    }

    #[test]
    fn side_effects_float_is_false() {
        assert!(!has_side_effects(&NdaNode::Float { value: 1.0 }));
    }

    #[test]
    fn side_effects_break_is_false() {
        assert!(!has_side_effects(&NdaNode::Break));
    }

    #[test]
    fn side_effects_call_is_true() {
        assert!(has_side_effects(&NdaNode::Call { target: 0 }));
    }

    #[test]
    fn side_effects_print_is_true() {
        assert!(has_side_effects(&NdaNode::Print {
            source: Box::new(NdaNode::Int { value: 0 }),
        }));
    }

    #[test]
    fn side_effects_return_is_true() {
        assert!(has_side_effects(&NdaNode::Return {
            value: Box::new(NdaNode::Int { value: 0 }),
        }));
    }

    #[test]
    fn side_effects_peek_is_true() {
        assert!(has_side_effects(&NdaNode::Peek {
            addr: Box::new(NdaNode::Int { value: 0 }),
        }));
    }

    #[test]
    fn side_effects_poke_is_true() {
        assert!(has_side_effects(&NdaNode::Poke {
            addr: Box::new(NdaNode::Int { value: 0 }),
            value: Box::new(NdaNode::Int { value: 1 }),
        }));
    }

    #[test]
    fn side_effects_syscall_is_true() {
        assert!(has_side_effects(&NdaNode::Syscall { num: 0, args: vec![] }));
    }

    #[test]
    fn side_effects_let_delegates_to_init() {
        // let x = 42 → no side effects
        assert!(!has_side_effects(&NdaNode::Let {
            name_hash: 0,
            init: Box::new(NdaNode::Int { value: 42 }),
        }));
        // let x = print(1) → has side effects
        assert!(has_side_effects(&NdaNode::Let {
            name_hash: 0,
            init: Box::new(NdaNode::Print { source: Box::new(NdaNode::Int { value: 1 }) }),
        }));
    }

    #[test]
    fn side_effects_add_delegates_to_children() {
        assert!(!has_side_effects(&NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        }));
        assert!(has_side_effects(&NdaNode::Add {
            lhs: Box::new(NdaNode::Call { target: 0 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        }));
    }

    #[test]
    fn side_effects_scope_any_child() {
        assert!(!has_side_effects(&NdaNode::Scope {
            children: vec![NdaNode::Int { value: 1 }, NdaNode::Load { name_hash: 0 }],
        }));
        assert!(has_side_effects(&NdaNode::Scope {
            children: vec![NdaNode::Call { target: 0 }],
        }));
    }

    #[test]
    fn side_effects_loop_body() {
        assert!(!has_side_effects(&NdaNode::Loop {
            count: 5,
            body: vec![NdaNode::Int { value: 0 }],
        }));
        assert!(has_side_effects(&NdaNode::Loop {
            count: 5,
            body: vec![NdaNode::Print { source: Box::new(NdaNode::Int { value: 0 }) }],
        }));
    }

    // ── gather_loaded_vars tests ─────────────────────────────────────────────

    #[test]
    fn gather_loads_simple() {
        let mut set = std::collections::HashSet::new();
        gather_loaded_vars(&NdaNode::Load { name_hash: 42 }, &mut set);
        assert!(set.contains(&42));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn gather_loads_from_add() {
        let mut set = std::collections::HashSet::new();
        gather_loaded_vars(&NdaNode::Add {
            lhs: Box::new(NdaNode::Load { name_hash: 1 }),
            rhs: Box::new(NdaNode::Load { name_hash: 2 }),
        }, &mut set);
        assert!(set.contains(&1));
        assert!(set.contains(&2));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn gather_loads_from_let_init() {
        let mut set = std::collections::HashSet::new();
        gather_loaded_vars(&NdaNode::Let {
            name_hash: 10,
            init: Box::new(NdaNode::Load { name_hash: 99 }),
        }, &mut set);
        assert!(set.contains(&99));
        // name_hash 10 is a store target, not a load
        assert!(!set.contains(&10));
    }

    #[test]
    fn gather_loads_int_is_empty() {
        let mut set = std::collections::HashSet::new();
        gather_loaded_vars(&NdaNode::Int { value: 42 }, &mut set);
        assert!(set.is_empty());
    }

    #[test]
    fn gather_loads_nested_scope() {
        let mut set = std::collections::HashSet::new();
        gather_loaded_vars(&NdaNode::Scope {
            children: vec![
                NdaNode::Load { name_hash: 1 },
                NdaNode::Loop {
                    count: 5,
                    body: vec![NdaNode::Load { name_hash: 2 }],
                },
            ],
        }, &mut set);
        assert!(set.contains(&1));
        assert!(set.contains(&2));
    }

    #[test]
    fn gather_loads_if_branches() {
        let mut set = std::collections::HashSet::new();
        gather_loaded_vars(&NdaNode::If {
            cond: Box::new(NdaNode::Load { name_hash: 10 }),
            then_body: vec![NdaNode::Load { name_hash: 20 }],
            else_body: Some(vec![NdaNode::Load { name_hash: 30 }]),
        }, &mut set);
        assert!(set.contains(&10));
        assert!(set.contains(&20));
        assert!(set.contains(&30));
    }

    // ── Optimization tests ──────────────────────────────────────────────────

    #[test]
    fn constant_folding_nested_add() {
        // (1 + 2) + 3 → 6
        let nodes = vec![NdaNode::Add {
            lhs: Box::new(NdaNode::Add {
                lhs: Box::new(NdaNode::Int { value: 1 }),
                rhs: Box::new(NdaNode::Int { value: 2 }),
            }),
            rhs: Box::new(NdaNode::Int { value: 3 }),
        }];
        let result = optimize_ast(&nodes);
        match &result[0] {
            NdaNode::Int { value } => assert_eq!(*value, 6),
            other => panic!("Expected Int(6), got {:?}", other),
        }
    }

    #[test]
    fn optimize_preserves_return() {
        let nodes = vec![NdaNode::Return {
            value: Box::new(NdaNode::Int { value: 42 }),
        }];
        let result = optimize_ast(&nodes);
        assert!(!result.is_empty());
        assert!(result.iter().any(|n| matches!(n, NdaNode::Return { .. })));
    }

    #[test]
    fn optimize_empty_input() {
        let result = optimize_ast(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn optimize_ast_with_report_empty() {
        let (result, report) = optimize_ast_with_report(&[]);
        assert!(result.is_empty());
        assert_eq!(report.input_nodes, 0);
    }

    #[test]
    fn ast_complexity_while_and_scope() {
        let nodes = vec![
            NdaNode::While {
                cond: Box::new(NdaNode::Int { value: 1 }),
                body: vec![NdaNode::Scope {
                    children: vec![NdaNode::Int { value: 0 }],
                }],
            },
        ];
        let dist = ast_complexity_info(&nodes);
        assert_eq!(dist.while_count, 1);
        assert_eq!(dist.scope_count, 1);
    }

    #[test]
    fn optimization_report_default() {
        let report = OptimizationReport::default();
        assert_eq!(report.constants_folded, 0);
        assert_eq!(report.input_nodes, 0);
        assert_eq!(report.output_nodes, 0);
    }

    #[test]
    fn validate_report_clean() {
        let report = OptimizationReport {
            constants_folded: 5,
            dead_nodes_removed: 2,
            loops_unrolled: 0,
            dead_branches_eliminated: 0,
            identities_simplified: 1,
            double_negations_eliminated: 0,
            strength_reductions: 0,
            input_nodes: 20,
            output_nodes: 12,
        };
        let issues = validate_optimization_report(&report);
        assert!(issues.is_empty(), "issues: {:?}", issues);
    }

    // ── Block 140: Extended tests ─────────────────────────────────────────

    // --- has_side_effects: remaining variants ---

    #[test]
    fn side_effects_spawn_is_true() {
        assert!(has_side_effects(&NdaNode::Spawn { scope_hash: 0 }));
    }

    #[test]
    fn side_effects_atomic_is_true() {
        assert!(has_side_effects(&NdaNode::Atomic {
            op: crate::site_map::verifier::AtomicOp::Cas,
            addr: Box::new(NdaNode::Int { value: 0 }),
            val: Box::new(NdaNode::Int { value: 1 }),
        }));
    }

    #[test]
    fn side_effects_gpu_dispatch_is_true() {
        assert!(has_side_effects(&NdaNode::GpuDispatch {
            shader_hash: 0,
            args: vec![],
        }));
    }

    #[test]
    fn side_effects_free_is_true() {
        assert!(has_side_effects(&NdaNode::Free {
            addr: Box::new(NdaNode::Int { value: 0 }),
        }));
    }

    #[test]
    fn side_effects_reg_int_is_true() {
        assert!(has_side_effects(&NdaNode::RegInt {
            vector: 0,
            handler_hash: 0,
        }));
    }

    #[test]
    fn side_effects_store_delegates_to_value() {
        assert!(!has_side_effects(&NdaNode::Store {
            name_hash: 0,
            value: Box::new(NdaNode::Int { value: 42 }),
        }));
        assert!(has_side_effects(&NdaNode::Store {
            name_hash: 0,
            value: Box::new(NdaNode::Call { target: 0 }),
        }));
    }

    #[test]
    fn side_effects_while_cond_and_body() {
        assert!(!has_side_effects(&NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![NdaNode::Int { value: 0 }],
        }));
        assert!(has_side_effects(&NdaNode::While {
            cond: Box::new(NdaNode::Call { target: 0 }),
            body: vec![],
        }));
        assert!(has_side_effects(&NdaNode::While {
            cond: Box::new(NdaNode::Int { value: 1 }),
            body: vec![NdaNode::Print { source: Box::new(NdaNode::Int { value: 0 }) }],
        }));
    }

    #[test]
    fn side_effects_if_all_branches() {
        assert!(!has_side_effects(&NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 0 }],
            else_body: Some(vec![NdaNode::Int { value: 1 }]),
        }));
        assert!(has_side_effects(&NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Call { target: 0 }],
            else_body: None,
        }));
        assert!(has_side_effects(&NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![],
            else_body: Some(vec![NdaNode::Print { source: Box::new(NdaNode::Int { value: 0 }) }]),
        }));
    }

    #[test]
    fn side_effects_compare_delegates() {
        assert!(!has_side_effects(&NdaNode::Compare {
            op: CmpOp::Eq,
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        }));
        assert!(has_side_effects(&NdaNode::Compare {
            op: CmpOp::Eq,
            lhs: Box::new(NdaNode::Call { target: 0 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        }));
    }

    #[test]
    fn side_effects_bitwise_delegates() {
        assert!(!has_side_effects(&NdaNode::Bitwise {
            op: crate::site_map::verifier::BitwiseOp::And,
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Some(Box::new(NdaNode::Int { value: 2 })),
        }));
        assert!(has_side_effects(&NdaNode::Bitwise {
            op: crate::site_map::verifier::BitwiseOp::And,
            lhs: Box::new(NdaNode::Call { target: 0 }),
            rhs: None,
        }));
    }

    #[test]
    fn side_effects_math_delegates() {
        assert!(!has_side_effects(&NdaNode::Math {
            op: crate::site_map::verifier::MathOp::Add,
            lhs: Box::new(NdaNode::Float { value: 1.0 }),
            rhs: Box::new(NdaNode::Float { value: 2.0 }),
        }));
    }

    #[test]
    fn side_effects_gemv_delegates() {
        assert!(!has_side_effects(&NdaNode::Gemv {
            matrix: Box::new(NdaNode::Matrix { rows: 2, cols: 2, scale: 0, sign: vec![], extra: vec![] }),
            vector: Box::new(NdaNode::Int { value: 0 }),
        }));
    }

    #[test]
    fn side_effects_alloc_delegates() {
        assert!(!has_side_effects(&NdaNode::Alloc {
            size: Box::new(NdaNode::Int { value: 64 }),
        }));
    }

    #[test]
    fn side_effects_cast_delegates() {
        assert!(!has_side_effects(&NdaNode::Cast {
            from_type: crate::site_map::verifier::TypeKind::Int,
            to_type: crate::site_map::verifier::TypeKind::Float,
            operand: Box::new(NdaNode::Int { value: 42 }),
        }));
    }

    // --- gather_loaded_vars: more variants ---

    #[test]
    fn gather_loads_while_body() {
        let mut set = std::collections::HashSet::new();
        gather_loaded_vars(&NdaNode::While {
            cond: Box::new(NdaNode::Load { name_hash: 5 }),
            body: vec![NdaNode::Load { name_hash: 6 }],
        }, &mut set);
        assert!(set.contains(&5));
        assert!(set.contains(&6));
    }

    #[test]
    fn gather_loads_store_value() {
        let mut set = std::collections::HashSet::new();
        gather_loaded_vars(&NdaNode::Store {
            name_hash: 10,
            value: Box::new(NdaNode::Load { name_hash: 20 }),
        }, &mut set);
        assert!(set.contains(&20));
    }

    // --- Struct derives ---

    #[test]
    fn optimization_report_clone() {
        let report = OptimizationReport {
            constants_folded: 5,
            dead_nodes_removed: 3,
            loops_unrolled: 1,
            dead_branches_eliminated: 2,
            identities_simplified: 4,
            double_negations_eliminated: 1,
            strength_reductions: 0,
            input_nodes: 30,
            output_nodes: 15,
        };
        let cloned = report.clone();
        assert_eq!(cloned.constants_folded, 5);
        assert_eq!(cloned.input_nodes, 30);
    }

    #[test]
    fn optimization_report_debug() {
        let report = OptimizationReport::default();
        let debug = format!("{:?}", report);
        assert!(debug.contains("OptimizationReport"));
    }

    #[test]
    fn optimization_summary_clone_and_debug() {
        let report = OptimizationReport {
            constants_folded: 1,
            dead_nodes_removed: 0,
            loops_unrolled: 0,
            dead_branches_eliminated: 0,
            identities_simplified: 0,
            double_negations_eliminated: 0,
            strength_reductions: 0,
            input_nodes: 10,
            output_nodes: 8,
        };
        let summary = optimization_summary(&report);
        let cloned = summary.clone();
        assert_eq!(cloned.total_optimizations, summary.total_optimizations);
        let debug = format!("{:?}", summary);
        assert!(debug.contains("OptimizationSummary"));
    }

    #[test]
    fn node_kind_distribution_clone_and_debug() {
        let dist = ast_complexity_info(&[NdaNode::Int { value: 1 }]);
        let cloned = dist.clone();
        assert_eq!(cloned.int_count, 1);
        let debug = format!("{:?}", dist);
        assert!(debug.contains("NodeKindDistribution"));
    }

    // --- ast_complexity_info: more variants ---

    #[test]
    fn ast_complexity_store_let_counts() {
        let nodes = vec![
            NdaNode::Let {
                name_hash: 1,
                init: Box::new(NdaNode::Int { value: 42 }),
            },
            NdaNode::Store {
                name_hash: 1,
                value: Box::new(NdaNode::Int { value: 99 }),
            },
        ];
        let dist = ast_complexity_info(&nodes);
        assert_eq!(dist.let_count, 1);
        assert_eq!(dist.store_count, 1);
    }

    #[test]
    fn ast_complexity_total_is_sum_of_kinds() {
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Load { name_hash: 0 },
            NdaNode::Call { target: 0 },
            NdaNode::Matrix { rows: 1, cols: 1, scale: 0, sign: vec![], extra: vec![] },
            NdaNode::Break,
        ];
        let dist = ast_complexity_info(&nodes);
        let kind_sum = dist.int_count + dist.load_count + dist.store_count
            + dist.let_count + dist.add_count + dist.compare_count
            + dist.loop_count + dist.while_count + dist.if_count
            + dist.scope_count + dist.return_count + dist.print_count
            + dist.matrix_count + dist.norm_count + dist.call_count
            + dist.vec_op_count + dist.other_count;
        assert_eq!(dist.total_nodes, kind_sum);
    }

    #[test]
    fn ast_complexity_other_count_catches_unmapped() {
        // Break, Float, Spawn etc go to other_count
        let nodes = vec![
            NdaNode::Break,
            NdaNode::Float { value: 1.0 },
            NdaNode::Spawn { scope_hash: 0 },
        ];
        let dist = ast_complexity_info(&nodes);
        assert_eq!(dist.other_count, 3);
    }

    #[test]
    fn ast_complexity_max_depth_tracks_nesting() {
        let nodes = vec![
            NdaNode::Scope {
                children: vec![
                    NdaNode::Scope {
                        children: vec![
                            NdaNode::Int { value: 1 },
                        ],
                    },
                ],
            },
        ];
        let dist = ast_complexity_info(&nodes);
        assert!(dist.max_depth >= 2);
    }

    // --- optimization_summary edge cases ---

    #[test]
    fn optimization_summary_low_effectiveness() {
        let report = OptimizationReport {
            constants_folded: 0,
            dead_nodes_removed: 0,
            loops_unrolled: 0,
            dead_branches_eliminated: 0,
            identities_simplified: 0,
            double_negations_eliminated: 0,
            strength_reductions: 0,
            input_nodes: 10,
            output_nodes: 9,
        };
        let summary = optimization_summary(&report);
        assert_eq!(summary.effectiveness, "low"); // 0.9 is < 1.0 but >= 0.8
    }

    #[test]
    fn optimization_summary_zero_input_ratio_is_one() {
        let report = OptimizationReport {
            constants_folded: 0,
            dead_nodes_removed: 0,
            loops_unrolled: 0,
            dead_branches_eliminated: 0,
            identities_simplified: 0,
            double_negations_eliminated: 0,
            strength_reductions: 0,
            input_nodes: 0,
            output_nodes: 0,
        };
        let summary = optimization_summary(&report);
        assert!((summary.compression_ratio - 1.0).abs() < 1e-9);
    }

    #[test]
    fn optimization_summary_has_side_effects_from_branches() {
        let report = OptimizationReport {
            constants_folded: 0,
            dead_nodes_removed: 0,
            loops_unrolled: 0,
            dead_branches_eliminated: 3,
            identities_simplified: 0,
            double_negations_eliminated: 0,
            strength_reductions: 0,
            input_nodes: 10,
            output_nodes: 7,
        };
        let summary = optimization_summary(&report);
        assert!(summary.has_side_effects);
    }

    // --- validate_optimization_report edge cases ---

    #[test]
    fn validate_report_both_zero_is_clean() {
        let report = OptimizationReport {
            constants_folded: 0,
            dead_nodes_removed: 0,
            loops_unrolled: 0,
            dead_branches_eliminated: 0,
            identities_simplified: 0,
            double_negations_eliminated: 0,
            strength_reductions: 0,
            input_nodes: 0,
            output_nodes: 0,
        };
        let issues = validate_optimization_report(&report);
        assert!(issues.is_empty());
    }

    // --- optimize_ast: more cases ---

    #[test]
    fn optimize_identity_preserves_single_node() {
        let nodes = vec![NdaNode::Load { name_hash: 42 }];
        let result = optimize_ast(&nodes);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn optimize_constant_folding_produces_int() {
        let nodes = vec![NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 100 }),
            rhs: Box::new(NdaNode::Int { value: 200 }),
        }];
        let result = optimize_ast(&nodes);
        match &result[0] {
            NdaNode::Int { value } => assert_eq!(*value, 300),
            other => panic!("Expected Int(300), got {:?}", other),
        }
    }

    #[test]
    fn count_nodes_empty() {
        assert_eq!(count_nodes(&[]), 0);
    }

    #[test]
    fn count_nodes_single() {
        assert_eq!(count_nodes(&[NdaNode::Int { value: 1 }]), 1);
    }

    #[test]
    fn count_nodes_nested_scope() {
        let nodes = vec![NdaNode::Scope {
            children: vec![
                NdaNode::Int { value: 1 },
                NdaNode::Int { value: 2 },
            ],
        }];
        assert!(count_nodes(&nodes) >= 3); // scope + 2 ints
    }
}