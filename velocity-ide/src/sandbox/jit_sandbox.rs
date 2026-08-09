// sandbox/jit_sandbox.rs — Executing NDA opcode trees with nda_jit compiler
use crate::site_map::{NdaNode, SiteMap};
use crate::sandbox::SandboxResult;
use crate::compiler::nda_jit::JitProgram;
use crate::safety::SafeMutex;

use std::sync::{Arc, LazyLock, Mutex};
use std::collections::HashMap;
use std::hash::Hasher;

static JIT_CACHE: LazyLock<Mutex<HashMap<u64, Arc<JitProgram>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn fast_hash_node(node: &NdaNode, state: &mut std::collections::hash_map::DefaultHasher) {
    use std::hash::Hasher;
    match node {
        NdaNode::Matrix { rows, cols, scale, sign, extra } => {
            state.write(b"M");
            state.write_u16(*rows);
            state.write_u16(*cols);
            state.write_i8(*scale);
            state.write(sign);
            state.write(extra);
        }
        NdaNode::Norm { size, weight, bias } => {
            state.write(b"N");
            state.write_u16(*size);
            state.write(weight);
            state.write(bias);
        }
        NdaNode::Call { target } => {
            state.write(b"C");
            state.write_u64(*target);
        }
        NdaNode::Int { value } => {
            state.write(b"I");
            state.write_i32(*value);
        }
        NdaNode::Scope { children } => {
            state.write(b"S");
            state.write_usize(children.len());
            for child in children {
                fast_hash_node(child, state);
            }
        }
        NdaNode::Loop { count, body } => {
            state.write(b"LP");
            state.write_u32(*count);
            state.write_usize(body.len());
            for child in body {
                fast_hash_node(child, state);
            }
        }
        NdaNode::While { cond, body } => {
            state.write(b"WH");
            fast_hash_node(cond, state);
            state.write_usize(body.len());
            for child in body {
                fast_hash_node(child, state);
            }
        }
        NdaNode::If { cond, then_body, else_body } => {
            state.write(b"IF");
            fast_hash_node(cond, state);
            state.write_usize(then_body.len());
            for child in then_body {
                fast_hash_node(child, state);
            }
            if let Some(eb) = else_body {
                state.write(b"EL");
                state.write_usize(eb.len());
                for child in eb {
                    fast_hash_node(child, state);
                }
            }
        }
        NdaNode::Compare { op, lhs, rhs } => {
            state.write(b"CMP");
            state.write_u8(*op as u8);
            fast_hash_node(lhs, state);
            fast_hash_node(rhs, state);
        }
        NdaNode::Let { name_hash, init } => {
            state.write(b"LET");
            state.write_u64(*name_hash);
            fast_hash_node(init, state);
        }
        NdaNode::Load { name_hash } => {
            state.write(b"LD");
            state.write_u64(*name_hash);
        }
        NdaNode::Store { name_hash, value } => {
            state.write(b"ST");
            state.write_u64(*name_hash);
            fast_hash_node(value, state);
        }
        NdaNode::Add { lhs, rhs } => {
            state.write(b"ADD");
            fast_hash_node(lhs, state);
            fast_hash_node(rhs, state);
        }
        NdaNode::VecOp { op, operand } => {
            state.write(b"VOP");
            state.write_u8(*op as u8);
            fast_hash_node(operand, state);
        }
        NdaNode::Print { source } => {
            state.write(b"PRT");
            fast_hash_node(source, state);
        }
        NdaNode::Return { value } => {
            state.write(b"RET");
            fast_hash_node(value, state);
        }
        NdaNode::Break => {
            state.write(b"BRK");
        }
        NdaNode::Bitwise { op, lhs, rhs } => {
            state.write(b"BW");
            state.write_u8(*op as u8);
            fast_hash_node(lhs, state);
            if let Some(r) = rhs {
                state.write(b"R");
                fast_hash_node(r, state);
            }
        }
        NdaNode::Float { value } => {
            state.write(b"FL");
            state.write(&value.to_le_bytes());
        }
        NdaNode::Math { op, lhs, rhs } => {
            state.write(b"MTH");
            state.write_u8(*op as u8);
            fast_hash_node(lhs, state);
            fast_hash_node(rhs, state);
        }
        NdaNode::MathFunc { func, operand } => {
            state.write(b"MFC");
            state.write_u8(*func as u8);
            fast_hash_node(operand, state);
        }
        NdaNode::Peek { addr } => {
            state.write(b"PEK");
            fast_hash_node(addr, state);
        }
        NdaNode::Poke { addr, value } => {
            state.write(b"POK");
            fast_hash_node(addr, state);
            fast_hash_node(value, state);
        }
        NdaNode::Gemv { matrix, vector } => {
            state.write(b"GMV");
            fast_hash_node(matrix, state);
            fast_hash_node(vector, state);
        }
        NdaNode::Dot { lhs, rhs } => {
            state.write(b"DOT");
            fast_hash_node(lhs, state);
            fast_hash_node(rhs, state);
        }
        NdaNode::Syscall { num, args } => {
            state.write(b"SYS");
            state.write_u32(*num);
            state.write_usize(args.len());
            for arg in args {
                fast_hash_node(arg, state);
            }
        }
        NdaNode::Spawn { scope_hash } => {
            state.write(b"SPW");
            state.write_u64(*scope_hash);
        }
        NdaNode::Atomic { op, addr, val } => {
            state.write(b"ATC");
            state.write_u8(*op as u8);
            fast_hash_node(addr, state);
            fast_hash_node(val, state);
        }
        NdaNode::Alloc { size } => {
            state.write(b"ALC");
            fast_hash_node(size, state);
        }
        NdaNode::Free { addr } => {
            state.write(b"FRE");
            fast_hash_node(addr, state);
        }
        NdaNode::RegInt { vector, handler_hash } => {
            state.write(b"RGI");
            state.write_u32(*vector);
            state.write_u64(*handler_hash);
        }
        NdaNode::Cast { from_type, to_type, operand } => {
            state.write(b"CST");
            state.write_u8(*from_type as u8);
            state.write_u8(*to_type as u8);
            fast_hash_node(operand, state);
        }
        NdaNode::GpuDispatch { shader_hash, args } => {
            state.write(b"GPD");
            state.write_u64(*shader_hash);
            state.write_usize(args.len());
            for arg in args {
                fast_hash_node(arg, state);
            }
        }
        NdaNode::Triple { subject_hash, predicate_id, object_hash } => {
            state.write(b"TPL");
            state.write_u64(*subject_hash);
            state.write_u16(*predicate_id);
            state.write_u64(*object_hash);
        }
    }
}

pub struct NdaJitSandbox;

impl NdaJitSandbox {
    /// Execute the sequence of NDA nodes using the JIT compiler.
    pub fn run(
        nodes: &[NdaNode],
        conditioning_vec: &[f32],
        site_map: &SiteMap,
    ) -> SandboxResult {
        // Calculate structural hash of AST nodes using the fast default hasher
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for node in nodes {
            fast_hash_node(node, &mut hasher);
        }
        let program_hash = hasher.finish();

        // Retrieve from cache or compile
        let program = {
            let mut cache = JIT_CACHE.lock_safe();
            cache.entry(program_hash)
                .or_insert_with(|| Arc::new(crate::compiler::nda_jit::compile(nodes)))
                .clone()
        };

        // Run the compiled JitProgram in sandboxed mode, capturing prints
        // and measuring execution stats (panics are also caught).
        program.run_sandboxed(conditioning_vec, site_map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jit_sandbox_chains_matrix_output_to_norm_input() {
        let input = vec![1.0f32; 896];
        let site_map = SiteMap::open(&std::env::temp_dir().join("jit_sandbox_test_sm_1"), 0).unwrap();

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

        let result = NdaJitSandbox::run(&[m1, n1], &input, &site_map);
        assert!(!result.panicked);
        assert!(result.error.is_none());
        assert_eq!(result.output_dim, 128);
        assert_eq!(result.executed_nodes, 2);
    }

    #[test]
    fn jit_sandbox_catches_shape_panic() {
        let input = vec![1.0f32; 896];
        let site_map = SiteMap::open(&std::env::temp_dir().join("jit_sandbox_test_sm_2"), 0).unwrap();

        let m1 = NdaNode::Matrix {
            rows: 128,
            cols: 128,
            scale: 0,
            sign: vec![0xAA; 128 * 16],
            extra: vec![0x55; 128 * 16],
        };

        let result = NdaJitSandbox::run(&[m1], &input, &site_map);
        assert!(!result.panicked);
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("dimension mismatch"));
    }
}
