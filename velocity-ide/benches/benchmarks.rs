//! Benchmark suite for Velocity IDE.
//!
//! Run with: `cargo bench --workspace`
//! Or use just: `just bench`

use std::hint::black_box;
use std::path::PathBuf;

/// Benchmark the quantized NDA matrix-vector product (core inference kernel).
fn bench_nda_gemv(c: &mut criterion::Criterion) {
    use velocity_ide::nda::{nda_gemv, NdaMatrix};

    let mut group = c.benchmark_group("nda_gemv");

    // Synthetic v2-quad matrices with deterministic bitmaps (no RNG needed):
    // sign/extra are the packed 2-bit-per-weight planes, sized (rows*cols)/8 B.
    for (label, rows, cols) in [
        ("gemv_896x896", 896_usize, 896_usize),
        ("gemv_64x64", 64_usize, 64_usize),
    ] {
        let bitmap_bytes = (rows * cols).div_ceil(8);
        let sign = vec![0b1010_1010u8; bitmap_bytes];
        let extra = vec![0b0101_0101u8; bitmap_bytes];
        let matrix = NdaMatrix::new_quad(rows, cols, 1.0, sign, extra);
        let x: Vec<f32> = (0..cols).map(|i| (i as f32) * 0.001).collect();

        group.bench_function(label, |bencher| {
            bencher.iter(|| black_box(nda_gemv(&matrix, &x)))
        });
    }

    group.finish();
}

/// Benchmark tokenizer encoding using the bundled Qwen-coder tokenizer fixture.
fn bench_tokenizer(c: &mut criterion::Criterion) {
    use velocity_ide::tokenizer::Tokenizer;

    // The fixture ships with the repo; skip gracefully if it is absent (e.g. a
    // partial checkout) so `cargo bench` never hard-fails on a missing model.
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../models/qwen-coder-0.5b/tokenizer.json");
    let Ok(tokenizer) = Tokenizer::from_file(&path) else {
        eprintln!("bench_tokenizer: skipping, fixture not found at {path:?}");
        return;
    };

    let mut group = c.benchmark_group("tokenizer");

    let input = "the quick brown fox jumps over the lazy dog";
    group.bench_function("encode_short", |bencher| {
        bencher.iter(|| tokenizer.encode(black_box(input), false))
    });

    group.finish();
}

/// Benchmark library metadata operations.
fn bench_library_info(c: &mut criterion::Criterion) {
    use velocity_ide::library_info;

    let mut group = c.benchmark_group("library");

    group.bench_function("library_info", |bencher| bencher.iter(library_info));

    group.bench_function("library_info_serialize", |bencher| {
        let info = library_info();
        bencher.iter(|| serde_json::to_string(black_box(&info)))
    });

    group.finish();
}

/// Benchmark NDA parser compile() on small/medium inputs.
fn bench_nda_parser(c: &mut criterion::Criterion) {
    use velocity_ide::compiler::nda_parser;

    let mut group = c.benchmark_group("nda_parser");

    let tiny = "let x = 42;";
    group.bench_function("compile_tiny", |b| {
        b.iter(|| nda_parser::compile(black_box(tiny)))
    });

    let medium = (0..50)
        .map(|i| format!("let v{} = {};", i, i * 7))
        .collect::<Vec<_>>()
        .join("\n");
    group.bench_function("compile_medium", |b| {
        b.iter(|| nda_parser::compile(black_box(&medium)))
    });

    // Adversarial: deeply nested braces (parser stress test)
    let nested = format!("{}{}", "{".repeat(20), "}".repeat(20));
    group.bench_function("compile_nested_20", |b| {
        b.iter(|| nda_parser::compile(black_box(&nested)))
    });

    group.finish();
}

/// Benchmark SiteMap node insertion throughput.
fn bench_site_map(c: &mut criterion::Criterion) {
    use velocity_ide::site_map::{NdaNode, SiteMap};

    let tmp = tempfile::tempdir().expect("tempdir");
    let mut sm = SiteMap::open(tmp.path(), 0).expect("open site map");

    let mut group = c.benchmark_group("site_map");

    group.bench_function("insert_1000_int_nodes", |b| {
        b.iter(|| {
            for i in 0..1000u64 {
                let node = NdaNode::Int { value: i as i32 };
                let _ = sm.put_node(&node);
            }
        })
    });

    group.bench_function("insert_1000_triple_nodes", |b| {
        b.iter(|| {
            for i in 0..1000u64 {
                let node = NdaNode::Triple {
                    subject_hash: i,
                    predicate_id: (i % 10) as u16,
                    object_hash: i * 3,
                };
                let _ = sm.put_node(&node);
            }
        })
    });

    group.finish();
}

criterion::criterion_group!(
    benches,
    bench_nda_gemv,
    bench_tokenizer,
    bench_library_info,
    bench_nda_parser,
    bench_site_map,
);
criterion::criterion_main!(benches);
