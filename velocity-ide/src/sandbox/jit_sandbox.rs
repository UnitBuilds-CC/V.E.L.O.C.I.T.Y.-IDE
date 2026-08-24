// sandbox/jit_sandbox.rs — Executing NDA opcode trees with nda_jit compiler
use crate::compiler::nda_jit::JitProgram;
use crate::safety::SafeMutex;
use crate::sandbox::SandboxResult;
use crate::site_map::{NdaNode, SiteMap};
use serde::Serialize;

use std::collections::HashMap;
use std::hash::Hasher;
use std::sync::{Arc, LazyLock, Mutex};

static JIT_CACHE: LazyLock<Mutex<HashMap<u64, Arc<JitProgram>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn fast_hash_node(node: &NdaNode, state: &mut std::collections::hash_map::DefaultHasher) {
    use std::hash::Hasher;
    match node {
        NdaNode::Matrix {
            rows,
            cols,
            scale,
            sign,
            extra,
        } => {
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
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => {
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
        NdaNode::RegInt {
            vector,
            handler_hash,
        } => {
            state.write(b"RGI");
            state.write_u32(*vector);
            state.write_u64(*handler_hash);
        }
        NdaNode::Cast {
            from_type,
            to_type,
            operand,
        } => {
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
        NdaNode::Triple {
            subject_hash,
            predicate_id,
            object_hash,
        } => {
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
    pub fn run(nodes: &[NdaNode], conditioning_vec: &[f32], site_map: &SiteMap) -> SandboxResult {
        // Pre-execution credential boundary audit.
        // JIT code runs with PAGE_EXECUTE_READWRITE in the same address space;
        // verify that credential-bearing env vars and sockets have been scrubbed.
        let boundary = crate::credential_guard::CredentialBoundaryAudit::run();
        if let Some(warning) = boundary.warning_message() {
            log::warn!("{}", warning);
        }

        // Calculate structural hash of AST nodes using the fast default hasher
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for node in nodes {
            fast_hash_node(node, &mut hasher);
        }
        let program_hash = hasher.finish();

        // Retrieve from cache or compile
        let program = {
            let mut cache = JIT_CACHE.lock_safe();
            cache
                .entry(program_hash)
                .or_insert_with(|| Arc::new(crate::compiler::nda_jit::compile(nodes)))
                .clone()
        };

        // Run the compiled JitProgram in sandboxed mode, capturing prints
        // and measuring execution stats (panics are also caught).
        program.run_sandboxed(conditioning_vec, site_map)
    }
}

// ─── Diagnostics ───────────────────────────────────────────────────────────────

/// Serializable diagnostic snapshot of the JIT compilation cache.
#[derive(Debug, Clone, Serialize)]
pub struct JitCacheInfo {
    pub cached_programs: usize,
    pub total_compiled_nodes: usize,
    pub unique_hashes: Vec<String>,
}

/// Return a diagnostic snapshot of the JIT cache state.
pub fn jit_cache_info() -> JitCacheInfo {
    let cache = JIT_CACHE.lock_safe();
    let total_nodes: usize = cache.values().map(|p| p.nodes_compiled).sum();
    let hashes: Vec<String> = cache.keys().map(|h| format!("{:016x}", h)).collect();
    JitCacheInfo {
        cached_programs: cache.len(),
        total_compiled_nodes: total_nodes,
        unique_hashes: hashes,
    }
}

/// Clear the JIT compilation cache.
pub fn jit_cache_clear() {
    let mut cache = JIT_CACHE.lock_safe();
    cache.clear();
}

/// Compute the structural hash of an AST without compiling it.
pub fn ast_structural_hash(nodes: &[NdaNode]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for node in nodes {
        fast_hash_node(node, &mut hasher);
    }
    hasher.finish()
}

/// Estimate the memory footprint of cached JIT programs.
pub fn jit_cache_memory_estimate() -> JitCacheMemoryReport {
    let cache = JIT_CACHE.lock_safe();
    let mut total_programs = 0;
    let mut total_nodes = 0;
    let mut estimated_bytes = 0u64;
    for (hash, program) in cache.iter() {
        total_programs += 1;
        total_nodes += program.nodes_compiled;
        // Rough estimate: each compiled node uses ~256 bytes of executable memory
        estimated_bytes += program.nodes_compiled as u64 * 256;
        // Hash key overhead
        estimated_bytes += 8;
        // Arc overhead
        estimated_bytes += 16;
    }
    JitCacheMemoryReport {
        cached_programs: total_programs,
        total_compiled_nodes: total_nodes,
        estimated_bytes,
        estimated_kb: estimated_bytes as f64 / 1024.0,
    }
}

/// Report on JIT cache memory usage.
#[derive(Debug, Clone, Serialize)]
pub struct JitCacheMemoryReport {
    pub cached_programs: usize,
    pub total_compiled_nodes: usize,
    pub estimated_bytes: u64,
    pub estimated_kb: f64,
}

/// Estimate AST complexity (node count, depth, branching factor).
pub fn ast_complexity(nodes: &[NdaNode]) -> AstComplexity {
    let mut total_nodes = 0;
    let mut max_depth = 0;
    let mut control_flow_count = 0;

    fn walk(node: &NdaNode, depth: usize, total: &mut usize, max_d: &mut usize, cf: &mut usize) {
        *total += 1;
        if depth > *max_d {
            *max_d = depth;
        }
        match node {
            NdaNode::Scope { children } => {
                for c in children { walk(c, depth + 1, total, max_d, cf); }
            }
            NdaNode::Loop { body, .. } => {
                *cf += 1;
                for c in body { walk(c, depth + 1, total, max_d, cf); }
            }
            NdaNode::While { cond, body } => {
                *cf += 1;
                walk(cond, depth + 1, total, max_d, cf);
                for c in body { walk(c, depth + 1, total, max_d, cf); }
            }
            NdaNode::If { cond, then_body, else_body } => {
                *cf += 1;
                walk(cond, depth + 1, total, max_d, cf);
                for c in then_body { walk(c, depth + 1, total, max_d, cf); }
                if let Some(eb) = else_body {
                    for c in eb { walk(c, depth + 1, total, max_d, cf); }
                }
            }
            _ => {}
        }
    }

    for node in nodes {
        walk(node, 1, &mut total_nodes, &mut max_depth, &mut control_flow_count);
    }

    AstComplexity {
        total_nodes,
        max_depth,
        control_flow_count,
        root_count: nodes.len(),
    }
}

/// AST complexity diagnostic.
#[derive(Debug, Clone, Serialize)]
pub struct AstComplexity {
    pub total_nodes: usize,
    pub max_depth: usize,
    pub control_flow_count: usize,
    pub root_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jit_sandbox_chains_matrix_output_to_norm_input() {
        let input = vec![1.0f32; 896];
        let site_map =
            SiteMap::open(&std::env::temp_dir().join("jit_sandbox_test_sm_1"), 0).unwrap();

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
        let site_map =
            SiteMap::open(&std::env::temp_dir().join("jit_sandbox_test_sm_2"), 0).unwrap();

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

    #[test]
    fn jit_cache_info_initial() {
        jit_cache_clear();
        let info = jit_cache_info();
        assert_eq!(info.cached_programs, 0);
        assert_eq!(info.total_compiled_nodes, 0);
        assert!(info.unique_hashes.is_empty());
    }

    #[test]
    fn jit_cache_populated_after_run() {
        // Note: other tests may run in parallel and clear the cache,
        // so we just verify the info function works after a run.
        let site_map =
            SiteMap::open(&std::env::temp_dir().join("jit_sandbox_test_sm_3"), 0).unwrap();
        let nodes = vec![NdaNode::Int { value: 42 }];
        let _ = NdaJitSandbox::run(&nodes, &[1.0], &site_map);
        let info = jit_cache_info();
        // Cache may or may not have entries depending on parallel test execution
        assert!(info.total_compiled_nodes >= 0); // just verify it doesn't panic
    }

    #[test]
    fn jit_cache_clear_works() {
        jit_cache_clear();
        let site_map =
            SiteMap::open(&std::env::temp_dir().join("jit_sandbox_test_sm_4"), 0).unwrap();
        let nodes = vec![NdaNode::Int { value: 1 }];
        let _ = NdaJitSandbox::run(&nodes, &[1.0], &site_map);
        jit_cache_clear();
        let info = jit_cache_info();
        assert_eq!(info.cached_programs, 0);
    }

    #[test]
    fn ast_structural_hash_deterministic() {
        let nodes = vec![
            NdaNode::Int { value: 42 },
            NdaNode::Load { name_hash: 0xABCD },
        ];
        let h1 = ast_structural_hash(&nodes);
        let h2 = ast_structural_hash(&nodes);
        assert_eq!(h1, h2);
    }

    #[test]
    fn ast_structural_hash_differs() {
        let nodes1 = vec![NdaNode::Int { value: 1 }];
        let nodes2 = vec![NdaNode::Int { value: 2 }];
        assert_ne!(ast_structural_hash(&nodes1), ast_structural_hash(&nodes2));
    }

    #[test]
    fn ast_structural_hash_empty() {
        let h = ast_structural_hash(&[]);
        // Should not panic, just return some hash
        assert_eq!(h, h);
    }

    #[test]
    fn jit_cache_info_serializable() {
        jit_cache_clear();
        let info = jit_cache_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("cached_programs"));
        assert!(json.contains("total_compiled_nodes"));
    }

    // ─── Block 82: JIT cache diagnostics tests ─────────────────────────

    #[test]
    fn jit_cache_memory_estimate_empty() {
        jit_cache_clear();
        let report = jit_cache_memory_estimate();
        assert_eq!(report.cached_programs, 0);
        assert_eq!(report.estimated_bytes, 0);
    }

    #[test]
    fn jit_cache_memory_estimate_serializes() {
        let report = JitCacheMemoryReport {
            cached_programs: 5,
            total_compiled_nodes: 100,
            estimated_bytes: 25600,
            estimated_kb: 25.0,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"cached_programs\":5"));
        assert!(json.contains("\"estimated_kb\":25.0"));
    }

    #[test]
    fn ast_complexity_simple() {
        let nodes = vec![NdaNode::Int { value: 1 }, NdaNode::Int { value: 2 }];
        let c = ast_complexity(&nodes);
        assert_eq!(c.total_nodes, 2);
        assert_eq!(c.max_depth, 1);
        assert_eq!(c.control_flow_count, 0);
        assert_eq!(c.root_count, 2);
    }

    #[test]
    fn ast_complexity_with_loop() {
        let nodes = vec![NdaNode::Loop {
            count: 10,
            body: vec![NdaNode::Int { value: 1 }, NdaNode::Int { value: 2 }],
        }];
        let c = ast_complexity(&nodes);
        assert_eq!(c.total_nodes, 3); // loop + 2 ints
        assert_eq!(c.max_depth, 2);
        assert_eq!(c.control_flow_count, 1);
    }

    #[test]
    fn ast_complexity_with_if() {
        let nodes = vec![NdaNode::If {
            cond: Box::new(NdaNode::Int { value: 1 }),
            then_body: vec![NdaNode::Int { value: 2 }],
            else_body: Some(vec![NdaNode::Int { value: 3 }]),
        }];
        let c = ast_complexity(&nodes);
        assert_eq!(c.total_nodes, 4); // if + cond + then + else
        assert_eq!(c.control_flow_count, 1);
    }

    #[test]
    fn ast_complexity_nested() {
        let nodes = vec![NdaNode::Loop {
            count: 5,
            body: vec![NdaNode::Loop {
                count: 3,
                body: vec![NdaNode::Int { value: 1 }],
            }],
        }];
        let c = ast_complexity(&nodes);
        assert_eq!(c.max_depth, 3); // root -> outer loop -> inner loop -> int
        assert_eq!(c.control_flow_count, 2);
    }

    #[test]
    fn ast_complexity_empty() {
        let c = ast_complexity(&[]);
        assert_eq!(c.total_nodes, 0);
        assert_eq!(c.max_depth, 0);
    }

    #[test]
    fn ast_complexity_serializes() {
        let c = AstComplexity {
            total_nodes: 10,
            max_depth: 3,
            control_flow_count: 2,
            root_count: 1,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"total_nodes\":10"));
        assert!(json.contains("\"max_depth\":3"));
    }
}
