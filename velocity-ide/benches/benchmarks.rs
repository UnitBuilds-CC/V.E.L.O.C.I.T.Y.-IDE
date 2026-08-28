//! Benchmark suite for Velocity IDE.
//!
//! Run with: `cargo bench --workspace`
//! Or use just: `just bench`

use std::hint::black_box;
use std::time::Duration;

/// Benchmark NDA vector operations.
fn bench_nda_vec_ops(c: &mut criterion::Criterion) {
    use velocity_ide::nda::NdaVec;

    let mut group = c.benchmark_group("nda_vec");

    let a = NdaVec::from_f32_slice(&[1.0; 896]);
    let b = NdaVec::from_f32_slice(&[2.0; 896]);

    group.bench_function("add_896", |bencher| {
        bencher.iter(|| black_box(&a).add(black_box(&b)))
    });

    let a_small = NdaVec::from_f32_slice(&[1.0; 64]);
    let b_small = NdaVec::from_f32_slice(&[2.0; 64]);

    group.bench_function("add_64", |bencher| {
        bencher.iter(|| black_box(&a_small).add(black_box(&b_small)))
    });

    group.finish();
}

/// Benchmark tokenizer encoding.
fn bench_tokenizer(c: &mut criterion::Criterion) {
    use velocity_ide::tokenizer::Tokenizer;

    let mut group = c.benchmark_group("tokenizer");

    // Create a minimal tokenizer for benchmarking
    let vocab: Vec<String> = (0..256).map(|i| format!("tok_{}", i)).collect();
    let merges: Vec<(String, String)> = vec![
        ("tok_1".into(), "tok_2".into()),
        ("tok_3".into(), "tok_4".into()),
    ];
    let tokenizer = Tokenizer::new(vocab, merges);

    let input = "tok_1 tok_2 tok_3 tok_4 tok_5";

    group.bench_function("encode_short", |bencher| {
        bencher.iter(|| tokenizer.encode(black_box(input)))
    });

    group.finish();
}

/// Benchmark library metadata operations.
fn bench_library_info(c: &mut criterion::Criterion) {
    use velocity_ide::library_info;

    let mut group = c.benchmark_group("library");

    group.bench_function("library_info", |bencher| {
        bencher.iter(|| library_info())
    });

    group.bench_function("library_info_serialize", |bencher| {
        let info = library_info();
        bencher.iter(|| serde_json::to_string(black_box(&info)))
    });

    group.finish();
}

criterion::criterion_group!(
    benches,
    bench_nda_vec_ops,
    bench_tokenizer,
    bench_library_info,
);
criterion::criterion_main!(benches);
