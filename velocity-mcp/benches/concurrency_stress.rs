//! Concurrency-substrate stress benchmarks for the orchestrator worker pool.
//!
//! These benches exercise the shape the real orchestrator uses today: one OS
//! thread per worker assignment, a shared lock-protected edit registry
//! (mirroring `MediatorArena.locks`), a bounded result channel, and a
//! synthetic per-worker workload. They do NOT call into a live LLM provider,
//! so timings reflect the substrate, not network variance.
//!
//! Run: `cargo bench -p velocity_mcp --bench concurrency_stress`
//! CI:  `cargo bench --workspace` picks these up automatically.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use crossbeam_channel::bounded;
use std::collections::HashMap;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// A shared, lock-protected edit registry that mirrors the shape of
/// `MediatorArena.locks`: `path -> agent_ids holding it`. Every worker's
/// acquire/release hits the same inner `Mutex<HashMap<..>>`, so N-way
/// contention on this structure is exactly what a 30-agent fan-out measures.
type Locks = Arc<Mutex<HashMap<PathBuf, Vec<String>>>>;

fn acquire(locks: &Locks, path: &Path, agent: &str) {
    let mut m = locks.lock().unwrap();
    m.entry(path.to_path_buf())
        .or_default()
        .push(agent.to_string());
}

fn release(locks: &Locks, path: &Path, agent: &str) {
    let mut m = locks.lock().unwrap();
    if let Some(v) = m.get_mut(path) {
        v.retain(|a| a != agent);
        if v.is_empty() {
            m.remove(path);
        }
    }
}

/// One worker's life: barrier-sync with peers so the measurement window
/// starts all-at-once, then run K cycles of "acquire a distinct file lock +
/// release", reporting per-worker elapsed on a channel. Same shape as
/// `orchestrator::worker::runner::spawn_live_worker`.
fn run_worker(
    id: usize,
    locks: &Locks,
    barrier: &Barrier,
    cycles: usize,
    tx: &crossbeam_channel::Sender<(usize, Duration)>,
) {
    barrier.wait();
    let start = Instant::now();
    let agent = format!("agent-{id}");
    for i in 0..cycles {
        let path = PathBuf::from(format!("src/module_{i}.rs"));
        acquire(locks, &path, &agent);
        black_box(&path);
        release(locks, &path, &agent);
    }
    let _ = tx.send((id, start.elapsed()));
}

/// Substrate scaling: how wall-clock grows with N concurrent workers doing a
/// fixed number of lock-churn cycles. Ideal scaling is roughly flat in N
/// (workers run in parallel on many cores); any N-superlinearity is lock
/// contention, which is the "scheduler handling concurrency properly" signal.
fn bench_worker_fanout(c: &mut Criterion) {
    let mut group = c.benchmark_group("orchestrator_fanout");
    const CYCLES: usize = 50;
    for &n in &[1usize, 5, 10, 20, 30, 50] {
        group.bench_function(BenchmarkId::new("workers", n), |b| {
            b.iter(|| {
                let locks: Locks = Arc::new(Mutex::new(HashMap::new()));
                let (tx, rx) = bounded::<(usize, Duration)>(n);
                let barrier = Arc::new(Barrier::new(n));
                let mut handles = Vec::with_capacity(n);
                for i in 0..n {
                    let locks = locks.clone();
                    let barrier = barrier.clone();
                    let tx = tx.clone();
                    handles.push(thread::spawn(move || {
                        run_worker(i, &locks, &barrier, CYCLES, &tx);
                    }));
                }
                drop(tx);
                let mut acc: u128 = 0;
                for (_id, d) in rx.iter() {
                    acc += d.as_nanos();
                }
                for h in handles {
                    h.join().unwrap();
                }
                black_box(acc);
            });
        });
    }
    group.finish();
}

/// Crossbeam channel throughput at the buffer shape used in `velocity_serve`
/// (bounded 256) and `spawn_live_worker`'s result mpsc - measures whether the
/// messaging layer itself is a bottleneck at realistic fan-out.
fn bench_channel_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("channel_throughput");
    for &n in &[1usize, 10, 30, 50] {
        group.bench_function(BenchmarkId::new("producers", n), |b| {
            b.iter(|| {
                let (tx, rx) = bounded::<u64>(256);
                let mut handles = Vec::with_capacity(n);
                for i in 0..n {
                    let tx = tx.clone();
                    handles.push(thread::spawn(move || {
                        for k in 0..100u64 {
                            tx.send(black_box(i as u64 * 100 + k)).unwrap();
                        }
                    }));
                }
                drop(tx);
                let sum: u64 = rx.iter().sum();
                for h in handles {
                    h.join().unwrap();
                }
                black_box(sum);
            });
        });
    }
    group.finish();
}

criterion_group!(
    concurrency_benches,
    bench_worker_fanout,
    bench_channel_throughput
);
criterion_main!(concurrency_benches);
