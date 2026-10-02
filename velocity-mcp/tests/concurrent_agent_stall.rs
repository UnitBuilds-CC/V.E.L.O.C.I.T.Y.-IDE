//! Stall-detection integration test for the concurrent agent substrate.
//!
//! Fires N workers using the same shape as `spawn_live_worker` (thread +
//! shared lock map + result channel), then asserts (a) every worker reports
//! back, (b) the whole fan-out completes inside a wall-clock ceiling, and
//! (c) no single worker takes dramatically longer than the median - a "stall".
//! This is the scheduler-behaviour test that raw throughput benches can't
//! answer, because criterion averages over samples while a stall lives in
//! the tail.
//!
//! Run: `cargo test -p velocity_mcp --test concurrent_agent_stall --release`

use crossbeam_channel::bounded;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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

/// 30 concurrent agents, each running 200 lock-churn cycles. Fails if any
/// worker is missing, if wall-clock exceeds the ceiling, or if the slowest
/// worker is more than 4x the median (a genuine stall signal).
#[test]
fn thirty_concurrent_agents_no_stall() {
    const N: usize = 30;
    const CYCLES: usize = 200;
    // Sized generously so a loaded CI runner still passes; catches hangs,
    // not millisecond jitter. 20s for 30 threads * 200 lock cycles is very
    // loose unless something is genuinely stalled.
    const WALL_LIMIT: Duration = Duration::from_secs(20);
    const STALL_MULTIPLIER: f64 = 4.0;

    let locks: Locks = Arc::new(Mutex::new(HashMap::new()));
    let (tx, rx) = bounded::<(usize, Duration)>(N);
    let barrier = Arc::new(Barrier::new(N));
    let wall_start = Instant::now();

    let mut handles = Vec::with_capacity(N);
    for i in 0..N {
        let locks = locks.clone();
        let barrier = barrier.clone();
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            let t0 = Instant::now();
            let agent = format!("agent-{i}");
            for c in 0..CYCLES {
                let path = PathBuf::from(format!("src/mod_{c}.rs"));
                acquire(&locks, &path, &agent);
                release(&locks, &path, &agent);
            }
            let _ = tx.send((i, t0.elapsed()));
        }));
    }
    drop(tx);

    let mut elapsed: Vec<Duration> = Vec::with_capacity(N);
    for (_id, d) in rx.iter() {
        elapsed.push(d);
    }
    let mut panicked = 0;
    for h in handles {
        if h.join().is_err() {
            panicked += 1;
        }
    }
    let wall = wall_start.elapsed();

    assert_eq!(
        elapsed.len(),
        N,
        "expected {N} worker reports, got {}",
        elapsed.len()
    );
    assert_eq!(panicked, 0, "some workers panicked");
    assert!(
        wall <= WALL_LIMIT,
        "wall-clock {wall:?} exceeded ceiling {WALL_LIMIT:?} (possible scheduler stall)"
    );

    elapsed.sort();
    let median = elapsed[N / 2];
    let max = *elapsed.last().unwrap();
    let ratio = max.as_secs_f64() / median.as_secs_f64().max(f64::EPSILON);
    assert!(
        ratio < STALL_MULTIPLIER,
        "stall detected: max {max:?} vs median {median:?} = {ratio:.2}x (limit {STALL_MULTIPLIER}x)"
    );
}

/// Sanity: after a full fan-out, every acquired lock must be released. A
/// leftover entry would mean the worker's critical section leaked, which
/// over time starves subsequent workers (a class of "stall" that manifests
/// hours later, not on the first run).
#[test]
fn shared_lock_map_is_empty_after_fanout() {
    const N: usize = 20;
    let locks: Locks = Arc::new(Mutex::new(HashMap::new()));
    let barrier = Arc::new(Barrier::new(N));
    let mut handles = Vec::with_capacity(N);
    for i in 0..N {
        let locks = locks.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            let agent = format!("agent-{i}");
            for c in 0..50 {
                let path = PathBuf::from(format!("f_{c}.rs"));
                acquire(&locks, &path, &agent);
                release(&locks, &path, &agent);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let remaining = locks.lock().unwrap().len();
    assert_eq!(
        remaining, 0,
        "lock leak: {remaining} entries still held after all workers exited"
    );
}
