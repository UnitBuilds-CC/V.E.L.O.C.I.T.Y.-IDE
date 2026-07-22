use std::collections::HashMap;

use crate::site_map::verifier::CmpOp;
use crate::site_map::verifier::VecOpKind;
use crate::site_map::NdaNode;

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
                    .map_or(false, |eb| eb.iter().any(has_side_effects))
        }
        NdaNode::Add { lhs, rhs } => has_side_effects(lhs) || has_side_effects(rhs),
        NdaNode::Compare { lhs, rhs, .. } => has_side_effects(lhs) || has_side_effects(rhs),
        NdaNode::VecOp { operand, .. } => has_side_effects(operand),
        NdaNode::Bitwise { lhs, rhs, .. } => {
            has_side_effects(lhs) || rhs.as_ref().map_or(false, |r| has_side_effects(r))
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

fn dce_sequence(
    nodes: &[NdaNode],
    live_vars: &mut std::collections::HashSet<u64>,
) -> Vec<NdaNode> {
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

fn optimize_sequence(
    nodes: &[NdaNode],
    var_constants: &mut HashMap<u64, i32>,
) -> Vec<NdaNode> {
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
                (NdaNode::Int { value: l }, NdaNode::Int { value: r }) => NdaNode::Int {
                    value: l.saturating_add(*r),
                },
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
                    NdaNode::Scope {
                        children: opt_then,
                    }
                } else if let Some(eb) = else_body {
                    let opt_else = optimize_sequence(&eb, var_constants);
                    NdaNode::Scope {
                        children: opt_else,
                    }
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
                (VecOpKind::Abs, NdaNode::Int { value }) => NdaNode::Int {
                    value: value.abs(),
                },
                (VecOpKind::ReduceSum, NdaNode::Int { value }) => NdaNode::Int { value: *value },
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
    let mut var_constants = HashMap::new();
    let folded = optimize_sequence(nodes, &mut var_constants);
    let mut live_vars = std::collections::HashSet::new();
    dce_sequence(&folded, &mut live_vars)
}
