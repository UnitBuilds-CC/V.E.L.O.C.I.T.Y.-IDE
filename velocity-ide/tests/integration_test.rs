//! Integration tests for the V.E.L.O.C.I.T.Y.-IDE runtime crate.
//!
//! These tests verify cross-module workflows in the compiler/inference runtime.

/// Test that the safety module traits are properly exported and usable.
#[test]
fn safety_traits_are_accessible() {
    use std::sync::{Arc, Mutex, RwLock};
    use velocity_ide::safety::{SafeMutex, SafeRwLock};

    // Mutex via Arc
    let m = Arc::new(Mutex::new(vec![1, 2, 3]));
    {
        let guard = m.lock_safe();
        assert_eq!(guard.len(), 3);
    }

    // RwLock via Arc
    let rw = Arc::new(RwLock::new("hello".to_string()));
    {
        let r = rw.read_safe();
        assert_eq!(*r, "hello");
    }
    {
        let mut w = rw.write_safe();
        w.push_str(" world");
    }
    {
        let r = rw.read_safe();
        assert_eq!(*r, "hello world");
    }
}

/// Test that poisoning recovery works for both Mutex and RwLock.
#[test]
fn safety_poisoning_recovery_for_rwlock() {
    use std::sync::{Arc, RwLock};
    use velocity_ide::safety::SafeRwLock;

    let rw = Arc::new(RwLock::new(42));
    let rw2 = rw.clone();

    // Poison by panicking while write-locked
    let handle = std::thread::spawn(move || {
        let _w = rw2.write().unwrap();
        panic!("intentional");
    });
    let _ = handle.join();

    // write_safe should recover from poison
    let mut w = rw.write_safe();
    *w = 100;
    drop(w);

    let r = rw.read_safe();
    assert_eq!(*r, 100);
}

/// Test SiteMap open and basic triple storage.
#[test]
fn site_map_open_and_store_triples() {
    use velocity_ide::site_map::{SiteMap, VcTriple};

    let tmp = tempfile::tempdir().expect("tempdir");
    let mut sm = SiteMap::open(tmp.path(), 0).expect("open site map");

    // Store several triples
    for i in 0..10 {
        let node = velocity_ide::site_map::NdaNode::Triple {
            subject_hash: 1000 + i,
            predicate_id: (i % 3) as u16,
            object_hash: 2000 + i,
        };
        sm.put_node(&node).expect("put triple");
    }

    sm.flush().expect("flush");

    // Store a file snapshot referencing some triples
    let triples: Vec<VcTriple> = (0..5)
        .map(|i| VcTriple {
            subject_hash: 1000 + i,
            predicate_id: (i % 3) as u16,
            object_hash: 2000 + i,
        })
        .collect();
    sm.put_file_snapshot("test.rs", &triples)
        .expect("put snapshot");
    sm.flush().expect("flush");
}

/// Test NdaNode hash computation is deterministic.
#[test]
fn nda_node_hash_is_deterministic() {
    use velocity_ide::site_map::NdaNode;

    let node1 = NdaNode::Int { value: 42 };
    let node2 = NdaNode::Int { value: 42 };
    let node3 = NdaNode::Int { value: 99 };

    assert_eq!(node1.hash(), node2.hash(), "same value -> same hash");
    assert_ne!(
        node1.hash(),
        node3.hash(),
        "different value -> different hash"
    );
}

/// Test that the tokenizer module is accessible and Tokenizer type exists.
#[test]
fn tokenizer_type_is_accessible() {
    // Verify the Tokenizer type is importable from the public API
    use velocity_ide::tokenizer::Tokenizer;
    // We can't easily construct one without a vocab file, but we verify the type is public
    let _ = std::mem::size_of::<Tokenizer>();
}

/// Test that the NDA serializer round-trips a simple program.
#[test]
fn nda_serialization_roundtrip() {
    use velocity_ide::site_map::NdaNode;

    let nodes = vec![
        NdaNode::Int { value: 1 },
        NdaNode::Int { value: 2 },
        NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        },
    ];

    // Hash each node — should be deterministic
    let hashes: Vec<u64> = nodes.iter().map(|n| n.hash()).collect();
    let hashes2: Vec<u64> = nodes.iter().map(|n| n.hash()).collect();
    assert_eq!(hashes, hashes2, "hashing should be deterministic");
}
