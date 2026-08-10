// bin/run_nda.rs — Native runner for the NDA programming language
//
// Usage:
//   cargo run --bin run_nda -- <file.nda>             # JIT mode (default)
//   cargo run --bin run_nda -- <file.nda> --sandbox   # interpreter fallback
//   cargo run --bin run_nda -- <file.nda> --dim 512   # custom input dimension
#![allow(warnings)]

use std::env;
use std::fs;
use std::sync::Arc;

use velocity_ide::compiler::nda_jit;
use velocity_ide::compiler::nda_parser::compile;
use velocity_ide::sandbox::NdaSandbox;
use velocity_ide::site_map::{NdaNode, SiteMap};

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo run --bin run_nda -- <file.nda> [--sandbox] [--dim N]");
        std::process::exit(1);
    }

    let file_path = &args[1];
    log::info!("run_nda: loading '{}'", file_path);

    // Parse flags.
    let use_sandbox = args.iter().any(|a| a == "--sandbox");
    let mut dim = 896usize;
    for i in 2..args.len() {
        if args[i] == "--dim" && i + 1 < args.len() {
            if let Ok(d) = args[i + 1].parse::<usize>() {
                dim = d;
            }
        }
    }

    // ── Load source ────────────────────────────────────────────────────────────
    let source = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to read '{}': {}", file_path, e);
            eprintln!("Error reading '{}': {}", file_path, e);
            std::process::exit(1);
        }
    };
    log::info!("run_nda: source loaded ({} bytes)", source.len());

    println!("[compiler] Compiling '{}' → NDA AST ...", file_path);

    let (program, final_hashes) = match compile(&source) {
        Ok(r) => {
            log::info!("run_nda: compilation successful");
            r
        }
        Err(e) => {
            log::error!("Compilation failed: {}", e);
            eprintln!("Compilation Error: {}", e);
            std::process::exit(1);
        }
    };

    // ── Build site map (Merkle KV store) ───────────────────────────────────────
    // Use PID-unique temp dir to avoid races when multiple run_nda instances
    // execute in parallel (e.g. E2E test suites).
    let sm_dir = env::temp_dir().join(format!("nda_run_sm_{}", std::process::id()));
    if sm_dir.exists() {
        let _ = fs::remove_dir_all(&sm_dir);
    }
    let mut site_map = match SiteMap::open(&sm_dir, 0) {
        Ok(sm) => sm,
        Err(e) => {
            eprintln!("Runtime Error: failed to initialise SiteMap: {}", e);
            std::process::exit(1);
        }
    };

    let mut func_count = 0;
    if let NdaNode::Scope { children } = &program {
        for func in children {
            if let Err(e) = site_map.put_program(func) {
                eprintln!("Runtime Error: failed to register function: {}", e);
                std::process::exit(1);
            }
            func_count += 1;
        }
    }
    if let Err(e) = site_map.flush() {
        eprintln!("Runtime Error: failed to flush site map: {}", e);
        std::process::exit(1);
    }
    println!(
        "[compiler] Registered {} function(s) in Merkle site map.",
        func_count
    );

    // ── Locate main ────────────────────────────────────────────────────────────
    let main_hash = match final_hashes.get("main") {
        Some(&h) => h,
        None => {
            eprintln!("Runtime Error: no 'main' function found.");
            std::process::exit(1);
        }
    };

    let main_node = match site_map.get_node(main_hash) {
        Some(node) => node,
        None => {
            eprintln!(
                "Runtime Error: failed to retrieve main node (hash {:016x})",
                main_hash
            );
            std::process::exit(1);
        }
    };
    let nodes = match &main_node {
        NdaNode::Scope { children } => children.clone(),
        _ => {
            eprintln!("Runtime Error: 'main' is not a Scope.");
            std::process::exit(1);
        }
    };

    let conditioning_vec = vec![1.0f32; dim];
    println!("AST Nodes: {:#?}", nodes);

    println!(
        "[runtime] Execution mode : {}",
        if use_sandbox {
            "Interpreter (sandbox)"
        } else {
            "JIT"
        }
    );
    println!("[runtime] JIT tier       : {}", nda_jit::jit_tier_info());
    println!(
        "[runtime] Starting 'main' (hash: {:016x}) with input dim {}...",
        main_hash, dim
    );
    println!("─────────────────────────────────────────────────────────────");

    if use_sandbox {
        // ── Interpreter path ──────────────────────────────────────────────────
        let res = NdaSandbox::run(&nodes, &conditioning_vec, &site_map);
        println!("─────────────────────────────────────────────────────────────");
        if let Some(err) = res.error {
            eprintln!("[runtime] Execution failed: {}", err);
            std::process::exit(1);
        }
        println!("[runtime] Execution completed (interpreter):");
        println!("  Nodes executed : {}", res.executed_nodes);
        println!("  Matrix GEMVs   : {}", res.matrix_count);
        println!("  Norm ops       : {}", res.norm_count);
        println!("  Duration       : {} µs", res.elapsed_us);
        println!("  Output dim     : {}", res.output_dim);
        println!(
            "  Output vector  : {:?}",
            &res.output_vec[..res.output_vec.len().min(8)]
        );
    } else {
        // ── JIT path ──────────────────────────────────────────────────────────
        println!("[jit] Compiling AST to native closure chain...");
        let t_compile = std::time::Instant::now();
        let program = nda_jit::compile(&nodes);
        let compile_us = t_compile.elapsed().as_micros();
        println!(
            "[jit] Compiled {} node(s) in {} µs.",
            program.nodes_compiled, compile_us
        );
        println!(
            "[jit] Native ASM kernel : {}",
            if program.has_asm_kernel {
                "YES (x86-64 AVX2)"
            } else {
                "NO (pure-Rust fallback)"
            }
        );

        let res = program.run(&conditioning_vec, &site_map);

        println!("─────────────────────────────────────────────────────────────");
        if let Some(err) = res.error {
            eprintln!("[jit] Execution failed: {}", err);
            std::process::exit(1);
        }
        println!("[runtime] Execution completed (JIT):");
        println!("  Nodes compiled : {}", res.nodes_compiled);
        println!("  Compile time   : {} µs", compile_us);
        println!("  Run duration   : {} µs", res.elapsed_us);
        println!("  Output dim     : {}", res.output_dim);
        println!(
            "  Output vector  : {:?}",
            &res.output_vec[..res.output_vec.len().min(8)]
        );
    }
}
