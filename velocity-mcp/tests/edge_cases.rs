//! Edge case integration tests for V.E.L.O.C.I.T.Y.
//!
//! These tests verify that the system handles edge cases gracefully:
//! empty inputs, overflow conditions, concurrent access, and invalid data.

use std::sync::{Arc, Mutex};
use velocity_ide::safety::{SafeMutex, SafeRwLock};

/// Test that lock_safe handles concurrent poisoning from multiple threads.
#[test]
fn safety_concurrent_poison_and_recover() {
    let m = Arc::new(Mutex::new(0));
    let mut handles = vec![];

    // Spawn 5 threads that each poison the mutex
    for i in 0..5 {
        let m_clone = m.clone();
        handles.push(std::thread::spawn(move || {
            let _guard = m_clone.lock_safe();
            if i % 2 == 0 {
                panic!("intentional panic in thread {}", i);
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    // The mutex should still be usable after multiple poisonings
    let guard = m.lock_safe();
    assert_eq!(*guard, 0);
}

/// Test that RwLock write_safe recovers after poison.
#[test]
fn safety_rwlock_write_after_poison() {
    use std::sync::RwLock;

    let rw = Arc::new(RwLock::new(vec![1, 2, 3]));
    let rw2 = rw.clone();

    let h = std::thread::spawn(move || {
        let _w = rw2.write().unwrap();
        panic!("poison the rwlock");
    });
    let _ = h.join();

    // write_safe should recover
    let mut w = rw.write_safe();
    w.push(4);
    drop(w);

    let r = rw.read_safe();
    assert_eq!(*r, vec![1, 2, 3, 4]);
}

/// Test that try_lock_safe works on an uncontested mutex.
#[test]
fn safety_try_lock_safe_on_uncontested() {
    let m = Mutex::new(42);

    // try_lock_safe should succeed on an uncontested mutex
    let guard = m.try_lock_safe();
    assert!(guard.is_some(), "try_lock_safe should succeed on uncontested mutex");
    assert_eq!(*guard.unwrap(), 42);
}

/// Test crypto with empty plaintext.
#[test]
fn crypto_empty_plaintext() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let sealed = velocity_mcp::agent::crypto::seal(tmp.path(), b"test", b"");
    assert!(sealed.is_some(), "should handle empty plaintext");

    let opened = velocity_mcp::agent::crypto::open(tmp.path(), b"test", &sealed.unwrap());
    assert_eq!(opened, b"", "empty plaintext should round-trip");
}

/// Test crypto with large plaintext.
#[test]
fn crypto_large_plaintext() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plaintext = vec![0xABu8; 1_000_000]; // 1MB
    let sealed = velocity_mcp::agent::crypto::seal(tmp.path(), b"large", &plaintext);
    assert!(sealed.is_some(), "should handle large plaintext");

    let opened = velocity_mcp::agent::crypto::open(tmp.path(), b"large", &sealed.unwrap());
    assert_eq!(opened, plaintext, "large plaintext should round-trip");
}

/// Test shmem with empty input.
#[test]
fn shmem_empty_input() {
    use velocity_mcp::ipc::shmem::SharedMemoryBuffer;

    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("test_empty.bin");

    let mut buf = SharedMemoryBuffer::create_or_open(&path).expect("create shmem");
    buf.write_input("").expect("write empty");

    let read_back = buf.read_input().expect("read empty");
    assert_eq!(read_back, "");
}

/// Test shmem rejects oversized input.
#[test]
fn shmem_rejects_oversized_input() {
    use velocity_mcp::ipc::shmem::SharedMemoryBuffer;

    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("test_oversized.bin");

    let mut buf = SharedMemoryBuffer::create_or_open(&path).expect("create shmem");

    // Try to write more than the buffer limit (4086 bytes for input)
    let oversized = "x".repeat(5000);
    let result = buf.write_input(&oversized);
    assert!(result.is_err(), "should reject oversized input");
}

/// Test NDA binary frame with maximum-size payload.
#[test]
fn nmcp_binary_frame_large_payload() {
    let mut frame = Vec::new();
    frame.extend_from_slice(b"NMCP");
    frame.extend_from_slice(&[0xCDu8; 32]);
    // Add a 10KB payload
    frame.extend_from_slice(&vec![0xABu8; 10_240]);

    let parsed = velocity_mcp::protocol::nmcp_binary::NmcpBinaryFrame::parse(&frame);
    assert!(parsed.is_ok());
    assert_eq!(parsed.unwrap().payload.len(), 10_240);
}

/// Test NDA binary frame with exactly minimum size (36 bytes, no payload).
#[test]
fn nmcp_binary_frame_minimum_size() {
    let frame = vec![b'N', b'M', b'C', b'P'];
    let mut full = frame.clone();
    full.extend_from_slice(&[0u8; 32]); // exactly 36 bytes

    let parsed = velocity_mcp::protocol::nmcp_binary::NmcpBinaryFrame::parse(&full);
    assert!(parsed.is_ok());
    assert!(parsed.unwrap().payload.is_empty());
}

/// Test that SiteMap handles concurrent access safely.
#[test]
fn site_map_concurrent_triple_insertion() {
    use std::thread;
    use velocity_ide::site_map::{NdaNode, SiteMap};

    let tmp = tempfile::tempdir().expect("tempdir");
    let sm = Arc::new(Mutex::new(SiteMap::open(tmp.path(), 0).expect("open site map")));

    let mut handles = vec![];
    for t in 0..4 {
        let sm_clone = sm.clone();
        handles.push(thread::spawn(move || {
            for i in 0..25 {
                let node = NdaNode::Triple {
                    subject_hash: (t * 100 + i) as u64,
                    predicate_id: (i % 5) as u16,
                    object_hash: (t * 200 + i) as u64,
                };
                let mut guard = sm_clone.lock_safe();
                guard.put_node(&node).expect("put node");
            }
        }));
    }

    for h in handles {
        h.join().expect("thread panicked");
    }

    // All 100 triples should have been inserted without panic
    let guard = sm.lock_safe();
    guard.flush().expect("flush");
}

/// Test that NdaNode hash doesn't collide for common values.
#[test]
fn nda_node_hash_no_collisions_for_common_values() {
    use velocity_ide::site_map::NdaNode;
    use std::collections::HashSet;

    let mut hashes = HashSet::new();
    for i in 0..1000 {
        let node = NdaNode::Int { value: i };
        assert!(hashes.insert(node.hash()), "hash collision for Int({})", i);
    }
}
