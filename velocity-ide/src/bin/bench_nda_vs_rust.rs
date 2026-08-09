// bench_nda_vs_rust.rs — Head-to-head: NDA Sandbox vs Native Rust
//
// This benchmark answers the question: "If we wrote a program in NDA and
// executed it, how does it compare vs if it were written in Rust?"
//
// NDA is a matrix/vector computation engine (not a general-purpose language),
// so we compare equivalent _computations_ — the kind of work each excels at.
//
// Benchmarks:
//   1. Iterated vector accumulation (NDA's "counting loop" equivalent)
//   2. Matrix-vector multiply chain (NDA's native domain)
//   3. Pure Rust counting loop baseline (for reference)
//   4. Bitwise popcount throughput (NDA's core primitive)

#![allow(warnings)]

use std::hint::black_box;
use std::time::{Duration, Instant};

// We reference the main crate as a library
use velocity_ide::nda::{quantize_activations_v2_quad, NdaMatrix, NDA_V2_QUAD};
use velocity_ide::nda_int::{combine_log2_scales, nda_gemv_nda_to_nda, rms_norm_nda, NdaVec};
use velocity_ide::sandbox::NdaSandbox;
use velocity_ide::site_map::{NdaNode, SiteMap};

fn main() {
    println!();
    println!("╔═══════════════════════════════════════════════════════════════════╗");
    println!("║     V.E.L.O.C.I.T.Y.-IDE  —  NDA vs Rust Benchmark Suite       ║");
    println!("╠═══════════════════════════════════════════════════════════════════╣");
    println!("║  NDA: 2-bit {{-2,-1,+1,+2}} pure-integer bitwise engine          ║");
    println!("║  Rust: Native f32 / i64 compiled code (release mode)            ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝");
    println!();

    bench_1_counting_loop();
    bench_2_vector_accumulation();
    bench_3_matrix_chain();
    bench_4_popcount_throughput();
    bench_5_sandbox_vs_direct();
    bench_6_jit_vs_rust();
    bench_7_scalar_loop_jit();

    println!("═══════════════════════════════════════════════════════════════════");
    println!();
}

// ─── Benchmark 1: Counting Loop ──────────────────────────────────────────────
// NDA has no "loop" opcode — it's a dataflow engine. So we compare:
//   Rust: `for i in 0..N { sum += i; }`
//   NDA equivalent: N iterated vector additions (each is a popcount + shift)

fn bench_1_counting_loop() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│  Benchmark 1: Counting Loop (1 to 1,000,000)                   │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    let n = 1_000_000u64;
    let iters = 20;

    // ── Rust: Native loop ────────────────────────────────────────────────────
    // Warmup
    let mut sum: u64 = 0;
    for i in 0..n {
        sum = sum.wrapping_add(i);
    }
    black_box(sum);

    let t0 = Instant::now();
    for _ in 0..iters {
        let mut s: u64 = 0;
        for i in 0..n {
            s = s.wrapping_add(i);
        }
        black_box(s);
    }
    let rust_dur = t0.elapsed() / iters;

    // ── NDA: Iterated vector add (128-element vectors) ───────────────────────
    // NDA's "loop iteration" = one vector residual add per step.
    // We do N/128 iterations of 128-wide vector ops to process N elements.
    let vec_width = 128usize;
    let nda_iters_count = n as usize / vec_width; // ~7812 iterations

    let mut acc_vec = NdaVec::zeros(vec_width, 0);
    let delta = NdaVec::from_i32_slice(&vec![1i32; vec_width], 0);
    // Warmup
    for _ in 0..nda_iters_count {
        velocity_ide::nda_int::nda_vec_add_inplace(&mut acc_vec, &delta);
    }
    black_box(&acc_vec);

    let t1 = Instant::now();
    for _ in 0..iters {
        let mut v = NdaVec::zeros(vec_width, 0);
        for _ in 0..nda_iters_count {
            velocity_ide::nda_int::nda_vec_add_inplace(&mut v, &delta);
        }
        black_box(&v);
    }
    let nda_dur = t1.elapsed() / iters;

    println!("│                                                                 │");
    println!(
        "│  Rust native loop (u64 add)  : {:>12?}                     │",
        rust_dur
    );
    println!(
        "│  NDA vector-add equiv        : {:>12?}                     │",
        nda_dur
    );
    let ratio = nda_dur.as_nanos() as f64 / rust_dur.as_nanos().max(1) as f64;
    println!(
        "│  Ratio (NDA / Rust)          : {:>8.1}x slower               │",
        ratio
    );
    println!("│                                                                 │");
    println!("│  ⚠ NDA is NOT designed for scalar loops — it's a vector engine │");
    println!("│    This shows the overhead of NDA's encode/decode per step.     │");
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
}

// ─── Benchmark 2: Vector Accumulation (NDA's domain) ─────────────────────────
// Same computation but at NDA's natural granularity: wide vectors.

fn bench_2_vector_accumulation() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│  Benchmark 2: Wide Vector Accumulation (896-wide, 1000 steps)  │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    let width = 896usize;
    let steps = 1000;
    let iters = 50;

    // ── Rust: f32 vector add ─────────────────────────────────────────────────
    let rust_delta: Vec<f32> = (0..width).map(|i| (i as f32) * 0.001).collect();
    let mut rust_acc = vec![0.0f32; width];
    for _ in 0..steps {
        for j in 0..width {
            rust_acc[j] += rust_delta[j];
        }
    }
    black_box(&rust_acc);

    let t0 = Instant::now();
    for _ in 0..iters {
        let mut acc = vec![0.0f32; width];
        for _ in 0..steps {
            for j in 0..width {
                acc[j] += rust_delta[j];
            }
        }
        black_box(&acc);
    }
    let rust_dur = t0.elapsed() / iters;

    // ── NDA: NdaVec add ──────────────────────────────────────────────────────
    let nda_delta = NdaVec::from_f32_slice(&rust_delta);
    let mut nda_acc = NdaVec::zeros(width, nda_delta.log2_scale);
    for _ in 0..steps {
        velocity_ide::nda_int::nda_vec_add_inplace(&mut nda_acc, &nda_delta);
    }
    black_box(&nda_acc);

    let t1 = Instant::now();
    for _ in 0..iters {
        let mut acc = NdaVec::zeros(width, nda_delta.log2_scale);
        for _ in 0..steps {
            velocity_ide::nda_int::nda_vec_add_inplace(&mut acc, &nda_delta);
        }
        black_box(&acc);
    }
    let nda_dur = t1.elapsed() / iters;

    println!("│                                                                 │");
    println!(
        "│  Rust f32 vector add         : {:>12?}                     │",
        rust_dur
    );
    println!(
        "│  NDA vector add (2-bit)      : {:>12?}                     │",
        nda_dur
    );
    let ratio = nda_dur.as_nanos() as f64 / rust_dur.as_nanos().max(1) as f64;
    if ratio > 1.0 {
        println!(
            "│  Ratio                       : {:>8.1}x slower               │",
            ratio
        );
    } else {
        println!(
            "│  Ratio                       : {:>8.1}x FASTER               │",
            1.0 / ratio
        );
    }
    println!("│                                                                 │");
    println!("│  NDA trades precision for throughput: 2 bits vs 32 bits.        │");
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
}

// ─── Benchmark 3: Matrix-Vector Multiply Chain (NDA's Sweet Spot) ────────────
// This is what NDA was DESIGNED for: GEMV with bitwise popcount kernels.

fn bench_3_matrix_chain() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│  Benchmark 3: 24-Layer GEMV Chain (NDA's Sweet Spot)           │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    let iters = 30;
    let input = vec![1.0f32; 896];

    // Build 24-layer shapes: 896→128, 22×(128→128), 128→896
    let mut shapes: Vec<(usize, usize)> = Vec::new();
    shapes.push((128, 896));
    for _ in 0..22 {
        shapes.push((128, 128));
    }
    shapes.push((896, 128));

    // ── NDA: Native GEMV chain (popcount kernel) ─────────────────────────────
    let nda_mats: Vec<NdaMatrix> = shapes
        .iter()
        .map(|&(r, c)| {
            let bm = r * ((c + 7) / 8);
            NdaMatrix::new_quad(r, c, 1.0, vec![0xAA; bm], vec![0x55; bm])
        })
        .collect();

    // Warmup
    let mut v = NdaVec::from_f32_slice(&input);
    for mat in &nda_mats {
        v = nda_gemv_nda_to_nda(mat, &v);
    }
    black_box(&v);

    let t0 = Instant::now();
    for _ in 0..iters {
        let mut vec = NdaVec::from_f32_slice(&input);
        for mat in &nda_mats {
            vec = nda_gemv_nda_to_nda(mat, &vec);
        }
        black_box(&vec);
    }
    let nda_dur = t0.elapsed() / iters;

    // ── Rust: f32 GEMV chain (scalar multiply-add) ───────────────────────────
    let f32_mats: Vec<Vec<f32>> = shapes.iter().map(|&(r, c)| vec![0.5f32; r * c]).collect();

    // Warmup
    let mut fv = input.clone();
    for (i, &(r, c)) in shapes.iter().enumerate() {
        let mut out = vec![0.0f32; r];
        let mat = &f32_mats[i];
        for row in 0..r {
            let mut sum = 0.0f32;
            let base = row * c;
            for col in 0..c {
                sum += mat[base + col] * fv[col];
            }
            out[row] = sum;
        }
        fv = out;
    }
    black_box(&fv);

    let t1 = Instant::now();
    for _ in 0..iters {
        let mut vec = input.clone();
        for (i, &(r, c)) in shapes.iter().enumerate() {
            let mut out = vec![0.0f32; r];
            let mat = &f32_mats[i];
            for row in 0..r {
                let mut sum = 0.0f32;
                let base = row * c;
                for col in 0..c {
                    sum += mat[base + col] * vec[col];
                }
                out[row] = sum;
            }
            vec = out;
        }
        black_box(&vec);
    }
    let f32_dur = t1.elapsed() / iters;

    println!("│                                                                 │");
    println!(
        "│  NDA popcount GEMV (2-bit)   : {:>12?}                     │",
        nda_dur
    );
    println!(
        "│  Rust f32 scalar GEMV        : {:>12?}                     │",
        f32_dur
    );
    let speedup = f32_dur.as_nanos() as f64 / nda_dur.as_nanos().max(1) as f64;
    if speedup >= 1.0 {
        println!(
            "│  NDA Speedup vs Rust f32     : {:>8.1}x FASTER ★            │",
            speedup
        );
    } else {
        println!(
            "│  NDA vs Rust f32             : {:>8.1}x slower               │",
            1.0 / speedup
        );
    }
    println!("│                                                                 │");

    // Memory comparison
    let nda_bytes: usize = nda_mats.iter().map(|m| m.sign.len() + m.extra.len()).sum();
    let f32_bytes: usize = f32_mats.iter().map(|m| m.len() * 4).sum();
    println!(
        "│  NDA memory (2-bit weights)  : {:>8.1} KB                     │",
        nda_bytes as f64 / 1024.0
    );
    println!(
        "│  Rust f32 memory             : {:>8.1} KB                     │",
        f32_bytes as f64 / 1024.0
    );
    println!(
        "│  Memory savings              : {:>8.1}x                       │",
        f32_bytes as f64 / nda_bytes.max(1) as f64
    );
    println!("│                                                                 │");
    println!("│  ★ NDA trades precision for speed: 2 bits = no multiplications │");
    println!("│    GEMV becomes XOR + popcount — pure integer, SIMD-friendly.  │");
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
}

// ─── Benchmark 4: Raw Popcount Throughput ────────────────────────────────────
// The fundamental primitive: how fast is NDA's core operation?

fn bench_4_popcount_throughput() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│  Benchmark 4: Core Primitive — Popcount Throughput             │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    let n_bytes = 1_000_000usize;
    let iters = 100;

    let data_a: Vec<u8> = (0..n_bytes).map(|i| (i * 0xAB) as u8).collect();
    let data_b: Vec<u8> = (0..n_bytes).map(|i| (i * 0xCD) as u8).collect();

    // ── NDA-style: XOR + popcount (this is the core GEMV kernel) ─────────────
    let mut total_pop = 0u64;
    for i in 0..n_bytes {
        total_pop += (data_a[i] ^ data_b[i]).count_ones() as u64;
    }
    black_box(total_pop);

    let t0 = Instant::now();
    for _ in 0..iters {
        let mut pop = 0u64;
        for i in 0..n_bytes {
            pop += (data_a[i] ^ data_b[i]).count_ones() as u64;
        }
        black_box(pop);
    }
    let pop_dur = t0.elapsed() / iters;
    let pop_gops = (n_bytes * 8) as f64 / pop_dur.as_nanos() as f64; // bits/ns = Gbps

    // ── Rust f32: Multiply-add (equivalent dense operation) ──────────────────
    let fa: Vec<f32> = (0..n_bytes).map(|i| (i as f32) * 0.001).collect();
    let fb: Vec<f32> = (0..n_bytes).map(|i| (i as f32) * 0.002).collect();

    let mut fma_sum = 0.0f32;
    for i in 0..n_bytes {
        fma_sum += fa[i] * fb[i];
    }
    black_box(fma_sum);

    let t1 = Instant::now();
    for _ in 0..iters {
        let mut s = 0.0f32;
        for i in 0..n_bytes {
            s += fa[i] * fb[i];
        }
        black_box(s);
    }
    let fma_dur = t1.elapsed() / iters;
    let fma_gops = n_bytes as f64 / fma_dur.as_nanos() as f64; // ops/ns = Gops

    println!("│                                                                 │");
    println!(
        "│  XOR + popcount (1M bytes)   : {:>12?}  ({:.1} Gbit/s)  │",
        pop_dur, pop_gops
    );
    println!(
        "│  f32 multiply-add (1M elems) : {:>12?}  ({:.1} Gflop/s) │",
        fma_dur, fma_gops
    );
    let speedup = fma_dur.as_nanos() as f64 / pop_dur.as_nanos().max(1) as f64;
    println!(
        "│  Popcount throughput advantage: {:>8.1}x                       │",
        speedup
    );
    println!("│                                                                 │");
    println!("│  Each NDA byte processes 8 elements simultaneously.            │");
    println!("│  Popcount is a single CPU instruction (POPCNT) on x86.         │");
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
}

// ─── Benchmark 5: Sandbox Interpreter vs Direct Calls ────────────────────────
// Shows the overhead of NDA's interpretive sandbox layer.

fn bench_5_sandbox_vs_direct() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│  Benchmark 5: NDA Sandbox (Interpreted) vs Direct Rust Calls   │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    let iters = 50;
    let input = vec![1.0f32; 896];
    let sm_dir = std::env::temp_dir().join("bench_nda_vs_rust_sm");
    let site_map = SiteMap::open(&sm_dir, 0).unwrap();

    // Build a small 4-layer program
    let shapes = vec![(128usize, 896usize), (128, 128), (128, 128), (896, 128)];

    let nodes: Vec<NdaNode> = shapes
        .iter()
        .map(|&(r, c)| {
            let bm = r * ((c + 7) / 8);
            NdaNode::Matrix {
                rows: r as u16,
                cols: c as u16,
                scale: 0,
                sign: vec![0xAA; bm],
                extra: vec![0x55; bm],
            }
        })
        .collect();

    // Warmup
    let _ = NdaSandbox::run(&nodes, &input, &site_map);

    // ── Sandbox (interpreted) ────────────────────────────────────────────────
    let t0 = Instant::now();
    for _ in 0..iters {
        let res = NdaSandbox::run(&nodes, &input, &site_map);
        black_box(res);
    }
    let sandbox_dur = t0.elapsed() / iters;

    // ── Direct Rust calls (same NDA kernels, no interpreter) ─────────────────
    let nda_mats: Vec<NdaMatrix> = shapes
        .iter()
        .map(|&(r, c)| {
            let bm = r * ((c + 7) / 8);
            NdaMatrix::new_quad(r, c, 1.0, vec![0xAA; bm], vec![0x55; bm])
        })
        .collect();

    let mut v = NdaVec::from_f32_slice(&input);
    for mat in &nda_mats {
        v = nda_gemv_nda_to_nda(mat, &v);
    }
    black_box(&v);

    let t1 = Instant::now();
    for _ in 0..iters {
        let mut vec = NdaVec::from_f32_slice(&input);
        for mat in &nda_mats {
            vec = nda_gemv_nda_to_nda(mat, &vec);
        }
        black_box(&vec);
    }
    let direct_dur = t1.elapsed() / iters;

    let overhead =
        (sandbox_dur.as_nanos() as f64 / direct_dur.as_nanos().max(1) as f64 - 1.0) * 100.0;

    println!("│                                                                 │");
    println!(
        "│  NDA Sandbox (interpreted)   : {:>12?}                     │",
        sandbox_dur
    );
    println!(
        "│  NDA Direct Rust calls       : {:>12?}                     │",
        direct_dur
    );
    println!(
        "│  Sandbox overhead            : {:>8.1}%                       │",
        overhead
    );
    println!("│                                                                 │");
    println!("│  The sandbox adds catch_unwind + node dispatch overhead.       │");
    println!("│  Core NDA kernels run at the same speed in both paths.         │");
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
}

// ─── Benchmark 6: JIT vs Rust (NEW) ──────────────────────────────────────────
// Now that we have a JIT compiler, how close does NDA get to native Rust?
//
// This is the key benchmark: 3 modes of the same computation:
//   A) NDA Sandbox (interpreted match dispatch)    — our old baseline
//   B) NDA JIT     (closure chain + AVX2 GEMV)    — Phase 4 result
//   C) Rust f32 GEMV (scalar, hand-written)        — conventional baseline
//   D) Rust f32 GEMV with SIMD hint               — best-case Rust
//
// Program: 4-layer 128-wide GEMV chain (same as bench_5).
// We report: compile time (once), per-execution time, and break-even point.

fn bench_6_jit_vs_rust() {
    use std::sync::Arc;
    use velocity_ide::compiler::nda_jit;

    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│  Benchmark 6: JIT vs Interpreter vs Native Rust (4-Layer GEMV) │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    let iters = 200usize;
    let input = vec![1.0f32; 896];
    let shapes = vec![(128usize, 896usize), (128, 128), (128, 128), (896, 128)];

    // ── Build shared NDA node list ──────────────────────────────────────────
    let nodes: Vec<NdaNode> = shapes
        .iter()
        .map(|&(r, c)| {
            let bm = r * ((c + 7) / 8);
            NdaNode::Matrix {
                rows: r as u16,
                cols: c as u16,
                scale: 0,
                sign: vec![0xAA; bm],
                extra: vec![0x55; bm],
            }
        })
        .collect();

    let sm_dir = std::env::temp_dir().join("bench6_sm");
    let sm = SiteMap::open(&sm_dir, 0).unwrap();

    // ══════════════════════════════════════════════════════════════════════
    // A) NDA JIT — compile phase
    // ══════════════════════════════════════════════════════════════════════
    let t_compile = Instant::now();
    let jit_prog = nda_jit::compile(&nodes);
    let compile_us = t_compile.elapsed().as_micros() as u64;

    // Warmup
    let _ = jit_prog.run(&input, &sm);

    // Run phase
    let t0 = Instant::now();
    for _ in 0..iters {
        let r = jit_prog.run(&input, &sm);
        black_box(r);
    }
    let jit_dur = t0.elapsed() / iters as u32;

    // ══════════════════════════════════════════════════════════════════════
    // B) NDA Sandbox (interpreted)
    // ══════════════════════════════════════════════════════════════════════
    let sm_plain = SiteMap::open(&std::env::temp_dir().join("bench6_sm2"), 0).unwrap();
    // Warmup
    let _ = NdaSandbox::run(&nodes, &input, &sm_plain);

    let t1 = Instant::now();
    for _ in 0..iters {
        let r = NdaSandbox::run(&nodes, &input, &sm_plain);
        black_box(r);
    }
    let sandbox_dur = t1.elapsed() / iters as u32;

    // ══════════════════════════════════════════════════════════════════════
    // C) Direct NDA kernel calls (no interpreter, no JIT wrapper)
    // ══════════════════════════════════════════════════════════════════════
    let nda_mats: Vec<NdaMatrix> = shapes
        .iter()
        .map(|&(r, c)| {
            let bm = r * ((c + 7) / 8);
            NdaMatrix::new_quad(r, c, 1.0, vec![0xAA; bm], vec![0x55; bm])
        })
        .collect();

    // Warmup
    let mut wv = NdaVec::from_f32_slice(&input);
    for m in &nda_mats {
        wv = nda_gemv_nda_to_nda(m, &wv);
    }
    black_box(&wv);

    let t2 = Instant::now();
    for _ in 0..iters {
        let mut v = NdaVec::from_f32_slice(&input);
        for m in &nda_mats {
            v = nda_gemv_nda_to_nda(m, &v);
        }
        black_box(&v);
    }
    let direct_nda_dur = t2.elapsed() / iters as u32;

    // ══════════════════════════════════════════════════════════════════════
    // D) Rust f32 scalar GEMV (same layers, 32-bit weights)
    // ══════════════════════════════════════════════════════════════════════
    let f32_mats: Vec<Vec<f32>> = shapes.iter().map(|&(r, c)| vec![0.5f32; r * c]).collect();

    // Warmup
    let mut fv = input.clone();
    for (i, &(r, c)) in shapes.iter().enumerate() {
        let mut out = vec![0.0f32; r];
        for row in 0..r {
            let base = row * c;
            let mut s = 0.0f32;
            for col in 0..c {
                s += f32_mats[i][base + col] * fv[col];
            }
            out[row] = s;
        }
        fv = out;
    }
    black_box(&fv);

    let t3 = Instant::now();
    for _ in 0..iters {
        let mut v = input.clone();
        for (i, &(r, c)) in shapes.iter().enumerate() {
            let mut out = vec![0.0f32; r];
            for row in 0..r {
                let base = row * c;
                let mut s = 0.0f32;
                for col in 0..c {
                    s += f32_mats[i][base + col] * v[col];
                }
                out[row] = s;
            }
            v = out;
        }
        black_box(&v);
    }
    let rust_f32_dur = t3.elapsed() / iters as u32;

    // ══════════════════════════════════════════════════════════════════════
    // E) JIT Sandbox (Compile + Run on the fly)
    // ══════════════════════════════════════════════════════════════════════
    use velocity_ide::sandbox::NdaJitSandbox;

    // Warmup
    let _ = NdaJitSandbox::run(&nodes, &input, &sm);

    let t_jit_sb_comp = Instant::now();
    for _ in 0..iters {
        let r = NdaJitSandbox::run(&nodes, &input, &sm);
        black_box(r);
    }
    let jit_sb_comp_dur = t_jit_sb_comp.elapsed() / iters as u32;

    // ══════════════════════════════════════════════════════════════════════
    // F) JIT Sandbox (Run only - precompiled)
    // ══════════════════════════════════════════════════════════════════════
    // Warmup
    let _ = jit_prog.run_sandboxed(&input, &sm);

    let t_jit_sb_run = Instant::now();
    for _ in 0..iters {
        let r = jit_prog.run_sandboxed(&input, &sm);
        black_box(r);
    }
    let jit_sb_run_dur = t_jit_sb_run.elapsed() / iters as u32;

    // ══════════════════════════════════════════════════════════════════════
    // Results
    // ══════════════════════════════════════════════════════════════════════
    println!("│                                                                 │");
    println!(
        "│  [A] NDA JIT (closure + AVX2)   : {:>12?}                │",
        jit_dur
    );
    println!(
        "│  [B] NDA Sandbox (interpreted)  : {:>12?}                │",
        sandbox_dur
    );
    println!(
        "│  [C] NDA direct kernel calls    : {:>12?}                │",
        direct_nda_dur
    );
    println!(
        "│  [D] Rust f32 scalar GEMV       : {:>12?}                │",
        rust_f32_dur
    );
    println!(
        "│  [E] JIT Sandbox (Compile+Run)  : {:>12?}                │",
        jit_sb_comp_dur
    );
    println!(
        "│  [F] JIT Sandbox (Run only)     : {:>12?}                │",
        jit_sb_run_dur
    );
    println!("│                                                                 │");
    println!(
        "│  JIT compile time (once)        : {:>9} µs                │",
        compile_us
    );
    println!("│                                                                 │");

    // JIT vs Sandbox
    let jit_vs_sb = sandbox_dur.as_nanos() as f64 / jit_dur.as_nanos().max(1) as f64;
    if jit_vs_sb >= 1.0 {
        println!(
            "│  JIT speedup vs Interpreter     : {:>8.1}x FASTER             │",
            jit_vs_sb
        );
    } else {
        println!(
            "│  JIT vs Interpreter             : {:>8.1}x slower              │",
            1.0 / jit_vs_sb
        );
    }

    // JIT Sandbox vs Interpreter Sandbox
    let jit_sb_vs_sb = sandbox_dur.as_nanos() as f64 / jit_sb_comp_dur.as_nanos().max(1) as f64;
    if jit_sb_vs_sb >= 1.0 {
        println!(
            "│  JIT Sandbox speedup vs Interp. : {:>8.1}x FASTER (on-the-fly)   │",
            jit_sb_vs_sb
        );
    } else {
        println!(
            "│  JIT Sandbox vs Interp.         : {:>8.1}x slower (on-the-fly)   │",
            1.0 / jit_sb_vs_sb
        );
    }

    // JIT Sandbox (Run only) vs Interpreter Sandbox
    let jit_sb_run_vs_sb = sandbox_dur.as_nanos() as f64 / jit_sb_run_dur.as_nanos().max(1) as f64;
    if jit_sb_run_vs_sb >= 1.0 {
        println!(
            "│  JIT Sandbox speedup vs Interp. : {:>8.1}x FASTER (run only)     │",
            jit_sb_run_vs_sb
        );
    } else {
        println!(
            "│  JIT Sandbox vs Interp.         : {:>8.1}x slower (run only)     │",
            1.0 / jit_sb_run_vs_sb
        );
    }

    // JIT vs Direct NDA
    let jit_vs_direct = direct_nda_dur.as_nanos() as f64 / jit_dur.as_nanos().max(1) as f64;
    if jit_vs_direct >= 1.0 {
        println!(
            "│  JIT speedup vs Direct NDA      : {:>8.1}x FASTER             │",
            jit_vs_direct
        );
    } else {
        let overhead = (1.0 / jit_vs_direct - 1.0) * 100.0;
        println!(
            "│  JIT overhead vs Direct NDA     : {:>8.1}% (closure wrap cost) │",
            overhead
        );
    }

    // JIT vs Rust f32
    let jit_vs_f32 = rust_f32_dur.as_nanos() as f64 / jit_dur.as_nanos().max(1) as f64;
    if jit_vs_f32 >= 1.0 {
        println!(
            "│  JIT speedup vs Rust f32 GEMV   : {:>8.1}x FASTER ★           │",
            jit_vs_f32
        );
    } else {
        println!(
            "│  JIT vs Rust f32 GEMV           : {:>8.1}x slower              │",
            1.0 / jit_vs_f32
        );
    }

    // Break-even: how many JIT runs to amortise compile cost?
    let break_even_runs = if jit_dur < sandbox_dur {
        let savings_per_run_ns = sandbox_dur.as_nanos().saturating_sub(jit_dur.as_nanos());
        if savings_per_run_ns > 0 {
            (compile_us as u128 * 1000) / savings_per_run_ns + 1
        } else {
            0
        }
    } else {
        u128::MAX
    };

    println!("│                                                                 │");
    if break_even_runs == u128::MAX {
        println!("│  Break-even (compile amort.)    : JIT slower than sandbox      │");
    } else if break_even_runs == 0 {
        println!("│  Break-even (compile amort.)    : immediate                    │");
    } else {
        println!(
            "│  Break-even (compile amort.)    : after {:>4} executions         │",
            break_even_runs
        );
    }

    // Memory footprint comparison
    let nda_weight_bytes: usize = nda_mats.iter().map(|m| m.sign.len() + m.extra.len()).sum();
    let f32_weight_bytes: usize = f32_mats.iter().map(|m| m.len() * 4).sum();
    println!("│                                                                 │");
    println!(
        "│  NDA weight memory (2-bit)      : {:>9.1} KB               │",
        nda_weight_bytes as f64 / 1024.0
    );
    println!(
        "│  Rust f32 weight memory         : {:>9.1} KB               │",
        f32_weight_bytes as f64 / 1024.0
    );
    println!(
        "│  Memory ratio                   : {:>9.1}x smaller          │",
        f32_weight_bytes as f64 / nda_weight_bytes.max(1) as f64
    );
    println!("│                                                                 │");
    println!(
        "│  Platform                       : {}   │",
        nda_jit::jit_tier_info()
    );
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
}

fn bench_7_scalar_loop_jit() {
    use velocity_ide::compiler::nda_jit;
    use velocity_ide::compiler::nda_parser::hash_name;

    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│  Benchmark 7: JIT vs Interpreter vs Native Rust (Scalar Loop)   │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    let iters = 50usize;
    let loop_count = black_box(100_000i32); // 100,000 iterations per execution
    let input = vec![0.0f32];

    let sum_h = hash_name("sum");
    let i_h = hash_name("i");

    // AST for:
    // sum = 0
    // i = 0
    // loop loop_count {
    //   sum = sum + i
    //   i = i + 1
    // }
    // load sum
    let nodes = vec![
        NdaNode::Let {
            name_hash: sum_h,
            init: Box::new(NdaNode::Int { value: 0 }),
        },
        NdaNode::Let {
            name_hash: i_h,
            init: Box::new(NdaNode::Int { value: 0 }),
        },
        NdaNode::Loop {
            count: loop_count as u32,
            body: vec![
                NdaNode::Store {
                    name_hash: sum_h,
                    value: Box::new(NdaNode::Add {
                        lhs: Box::new(NdaNode::Load { name_hash: sum_h }),
                        rhs: Box::new(NdaNode::Load { name_hash: i_h }),
                    }),
                },
                NdaNode::Store {
                    name_hash: i_h,
                    value: Box::new(NdaNode::Add {
                        lhs: Box::new(NdaNode::Load { name_hash: i_h }),
                        rhs: Box::new(NdaNode::Int { value: 1 }),
                    }),
                },
            ],
        },
        NdaNode::Load { name_hash: sum_h },
    ];

    let sm_dir = std::env::temp_dir().join("bench7_sm");
    let sm = SiteMap::open(&sm_dir, 0).unwrap();

    // ══════════════════════════════════════════════════════════════════════
    // A) NDA JIT — compile phase
    // ══════════════════════════════════════════════════════════════════════
    let t_compile = Instant::now();
    let jit_prog = nda_jit::compile(&nodes);
    let compile_us = t_compile.elapsed().as_micros() as u64;

    // Warmup JIT
    let r_jit = jit_prog.run(&input, &sm);
    assert!(r_jit.error.is_none(), "JIT error: {:?}", r_jit.error);

    // Run phase JIT
    let t0 = Instant::now();
    for _ in 0..iters {
        let r = jit_prog.run(&input, &sm);
        black_box(r);
    }
    let jit_dur = t0.elapsed() / iters as u32;

    // ══════════════════════════════════════════════════════════════════════
    // B) NDA Sandbox (interpreted)
    // ══════════════════════════════════════════════════════════════════════
    let sm_plain = SiteMap::open(&std::env::temp_dir().join("bench7_sm2"), 0).unwrap();
    // Warmup Interpreter
    let r_interp = NdaSandbox::run(&nodes, &input, &sm_plain);
    assert!(
        r_interp.error.is_none(),
        "Interpreter error: {:?}",
        r_interp.error
    );

    let t1 = Instant::now();
    for _ in 0..iters {
        let r = NdaSandbox::run(&nodes, &input, &sm_plain);
        black_box(r);
    }
    let sandbox_dur = t1.elapsed() / iters as u32;

    // ══════════════════════════════════════════════════════════════════════
    // C) Rust Native (same logic)
    // ══════════════════════════════════════════════════════════════════════
    // Warmup Rust
    let mut sum = 0i32;
    let mut i = 0i32;
    for _ in 0..loop_count {
        sum = sum.wrapping_add(i);
        i = i.wrapping_add(1);
    }
    black_box(sum);

    let t2 = Instant::now();
    for _ in 0..iters {
        let mut sum = 0i32;
        let mut i = 0i32;
        for _ in 0..loop_count {
            sum = sum.wrapping_add(i);
            i = i.wrapping_add(1);
        }
        black_box(sum);
    }
    let rust_dur = t2.elapsed() / iters as u32;

    // Results
    println!("│                                                                 │");
    println!(
        "│  [A] NDA JIT (native scalar loop) : {:>12?}                │",
        jit_dur
    );
    println!(
        "│  [B] NDA Sandbox (interpreted)    : {:>12?}                │",
        sandbox_dur
    );
    println!(
        "│  [C] Rust Native scalar loop      : {:>12?}                │",
        rust_dur
    );
    println!("│                                                                 │");
    println!(
        "│  JIT compile time (once)          : {:>9} µs                │",
        compile_us
    );
    println!("│                                                                 │");

    // JIT vs Sandbox
    let jit_vs_sb = sandbox_dur.as_nanos() as f64 / jit_dur.as_nanos().max(1) as f64;
    if jit_vs_sb >= 1.0 {
        println!(
            "│  JIT speedup vs Interpreter       : {:>8.1}x FASTER             │",
            jit_vs_sb
        );
    } else {
        println!(
            "│  JIT vs Interpreter               : {:>8.1}x slower              │",
            1.0 / jit_vs_sb
        );
    }

    // JIT vs Rust
    let jit_vs_rust = jit_dur.as_nanos() as f64 / rust_dur.as_nanos().max(1) as f64;
    if jit_vs_rust <= 1.0 {
        println!(
            "│  JIT vs Rust Native               : {:>8.1}x FASTER ★           │",
            1.0 / jit_vs_rust
        );
    } else {
        println!(
            "│  JIT vs Rust Native               : {:>8.1}x slower              │",
            jit_vs_rust
        );
    }
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
}
