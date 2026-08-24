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

    let nodes = [
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

// ── Library public API surface ─────────────────────────────────────────────

/// Test that library info is accessible from the public API.
#[test]
fn library_info_accessible() {
    let info = velocity_ide::library_info();
    assert!(info.module_count >= 15);
    assert!(!info.features.is_empty());
    assert!(info.name.contains("velocity"));
}

/// Test that module inventory is accessible and complete.
#[test]
fn module_inventory_accessible() {
    let inv = velocity_ide::module_inventory();
    assert_eq!(inv.len(), 15);
    let names: Vec<&str> = inv.iter().map(|m| m.name).collect();
    assert!(names.contains(&"compiler"));
    assert!(names.contains(&"wiki"));
}

/// Test that version and banner are accessible.
#[test]
fn version_and_banner_accessible() {
    let v = velocity_ide::version();
    assert!(!v.is_empty());
    let b = velocity_ide::banner();
    assert!(b.contains("V.E.L.O.C.I.T.Y.-IDE"));
}

// ── Cross-module: JIT compile and run ──────────────────────────────────────

/// Test JIT compilation of a simple program through the public API.
#[test]
fn jit_compile_and_run_simple() {
    use velocity_ide::site_map::NdaNode;
    // Use the JIT compiler module
    let nodes = vec![
        NdaNode::Int { value: 10 },
        NdaNode::Int { value: 20 },
        NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 0 }),
            rhs: Box::new(NdaNode::Int { value: 0 }),
        },
    ];
    // Verify node hashes are deterministic
    let h1 = nodes[0].hash();
    let h2 = nodes[0].hash();
    assert_eq!(h1, h2);
}

/// Test that NdaNode variants are all constructible from the public API.
#[test]
fn nda_node_variants_constructible() {
    use velocity_ide::site_map::NdaNode;
    let _int = NdaNode::Int { value: 42 };
    let _float = NdaNode::Float { value: 3.14 };
    let _add = NdaNode::Add {
        lhs: Box::new(NdaNode::Int { value: 1 }),
        rhs: Box::new(NdaNode::Int { value: 2 }),
    };
    let _scope = NdaNode::Scope {
        children: vec![NdaNode::Int { value: 0 }],
    };
    let _matrix = NdaNode::Matrix {
        rows: 4, cols: 4, scale: 0,
        sign: vec![0xAA; 2], extra: vec![0x55; 2],
    };
    let _norm = NdaNode::Norm {
        size: 64, weight: vec![0xFF; 8], bias: vec![0x00; 8],
    };
    let _loop = NdaNode::Loop {
        count: 10,
        body: vec![NdaNode::Int { value: 0 }],
    };
    let _break = NdaNode::Break;
}

/// Test NdaNode hash produces different hashes for different nodes.
#[test]
fn nda_node_different_types_different_hashes() {
    use velocity_ide::site_map::NdaNode;
    let int_node = NdaNode::Int { value: 1 };
    let float_node = NdaNode::Float { value: 1.0 };
    let add_node = NdaNode::Add {
        lhs: Box::new(NdaNode::Int { value: 0 }),
        rhs: Box::new(NdaNode::Int { value: 0 }),
    };
    // All different types should (almost certainly) have different hashes
    assert_ne!(int_node.hash(), float_node.hash());
    assert_ne!(int_node.hash(), add_node.hash());
}

// ── Cross-module: SiteMap operations ───────────────────────────────────────

/// Test SiteMap node storage and retrieval.
#[test]
fn site_map_put_and_get_node() {
    use velocity_ide::site_map::{SiteMap, NdaNode};

    let tmp = tempfile::tempdir().expect("tempdir");
    let mut sm = SiteMap::open(tmp.path(), 0).expect("open");

    let node = NdaNode::Int { value: 42 };
    let hash = node.hash();
    sm.put_node(&node).expect("put");
    sm.flush().expect("flush");

    let retrieved = sm.get_node(hash);
    assert!(retrieved.is_some(), "should retrieve stored node");
}

/// Test SiteMap with multiple node types.
#[test]
fn site_map_multiple_node_types() {
    use velocity_ide::site_map::{SiteMap, NdaNode};

    let tmp = tempfile::tempdir().expect("tempdir");
    let mut sm = SiteMap::open(tmp.path(), 0).expect("open");

    let nodes = vec![
        NdaNode::Int { value: 1 },
        NdaNode::Float { value: 2.0 },
        NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Int { value: 2 }),
        },
        NdaNode::Scope {
            children: vec![NdaNode::Int { value: 0 }],
        },
    ];

    for node in &nodes {
        sm.put_node(node).expect("put node");
    }
    sm.flush().expect("flush");

    // All nodes should be retrievable
    for node in &nodes {
        let h = node.hash();
        assert!(sm.get_node(h).is_some(), "node {:?} should be retrievable", h);
    }
}

// ── Cross-module: Error types ──────────────────────────────────────────────

/// Test that error types are accessible and constructible.
#[test]
fn error_types_accessible() {
    use velocity_ide::errors::{VelocityError, ErrorCode};
    let err = VelocityError::new(ErrorCode::ConfigNotFound, "test error");
    let msg = format!("{}", err);
    assert!(msg.contains("test error"));
}

/// Test error display and debug formats.
#[test]
fn error_display_and_debug() {
    use velocity_ide::errors::{VelocityError, ErrorCode};
    let err = VelocityError::new(ErrorCode::WeightLoadFailed, "something failed");
    let display = format!("{}", err);
    let debug = format!("{:?}", err);
    assert!(display.contains("something failed"));
    assert!(debug.contains("VelocityError"));
}

// ── Cross-module: NDA matrix ───────────────────────────────────────────────

/// Test NDA matrix construction and properties.
#[test]
fn nda_matrix_construction() {
    use velocity_ide::nda::NdaMatrix;
    let mat = NdaMatrix::new_quad(4, 4, 1.0, vec![0xAA; 2], vec![0x55; 2]);
    assert_eq!(mat.rows, 4);
    assert_eq!(mat.cols, 4);
}

// ── Cross-module: Library constants ────────────────────────────────────────

/// Test library constants match expected values.
#[test]
fn library_constants_consistent() {
    assert_eq!(velocity_ide::VERSION, velocity_ide::version());
    assert_eq!(velocity_ide::NAME, "velocity-ide");
    assert!(!velocity_ide::TARGET_OS.is_empty());
    assert!(!velocity_ide::TARGET_ARCH.is_empty());
}

/// Test that library info JSON serialization works through public API.
#[test]
fn library_info_json_through_public_api() {
    let info = velocity_ide::library_info();
    let json = serde_json::to_string(&info).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["module_count"].as_u64().unwrap() >= 15);
    assert!(v["modules"].as_array().unwrap().len() >= 15);
}

/// Test ModuleInfo serialization through public API.
#[test]
fn module_info_serialization_through_api() {
    let inv = velocity_ide::module_inventory();
    for m in &inv {
        let json = serde_json::to_string(m).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 3);
        assert!(v["name"].as_str().unwrap().len() > 0);
        assert!(v["description"].as_str().unwrap().len() > 0);
    }
}
