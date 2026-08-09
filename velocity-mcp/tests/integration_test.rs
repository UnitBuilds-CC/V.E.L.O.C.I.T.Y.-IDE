//! Integration tests for the V.E.L.O.C.I.T.Y. MCP server.
//!
//! These tests verify end-to-end workflows across multiple modules.

/// Test that the tool registry can list all available tools.
#[test]
fn tool_registry_lists_available_tools() {
    let tools = velocity_mcp::registry::get_tools();
    assert!(!tools.is_empty(), "Tool registry should have at least one tool");
}

/// Test that the site map can be opened and basic operations work.
#[test]
fn site_map_basic_operations() {
    use velocity_ide::site_map::SiteMap;

    let tmp = tempfile::tempdir().expect("tempdir");
    let mut sm = SiteMap::open(tmp.path(), 0).expect("open site map");

    // Put a simple triple node
    let node = velocity_ide::site_map::NdaNode::Triple {
        subject_hash: 12345,
        predicate_id: 1,
        object_hash: 67890,
    };
    sm.put_node(&node).expect("put node");

    // Flush and verify no error
    sm.flush().expect("flush");
}

/// Test that the safety module's lock_safe works correctly.
#[test]
fn safety_lock_safe_basic_usage() {
    use std::sync::Mutex;
    use velocity_ide::safety::SafeMutex;

    let m = Mutex::new(42);
    let guard = m.lock_safe();
    assert_eq!(*guard, 42);
}

/// Test that safety lock_safe recovers from poisoning.
#[test]
fn safety_lock_safe_poisoning_recovery() {
    use std::sync::{Arc, Mutex};
    use velocity_ide::safety::SafeMutex;

    let m = Arc::new(Mutex::new(0));
    let m2 = m.clone();

    // Poison the mutex by panicking while held
    let handle = std::thread::spawn(move || {
        let _guard = m2.lock().unwrap();
        panic!("intentional panic to poison mutex");
    });
    let _ = handle.join();

    // lock_safe should recover and still work
    let guard = m.lock_safe();
    assert_eq!(*guard, 0);
}

/// Test crypto seal/open round-trip.
#[test]
fn crypto_seal_open_roundtrip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plaintext = b"integration test secret data";

    let sealed = velocity_mcp::agent::crypto::seal(tmp.path(), b"test_artifact", plaintext);
    assert!(sealed.is_some(), "seal should succeed");

    let opened = velocity_mcp::agent::crypto::open(tmp.path(), b"test_artifact", &sealed.unwrap());
    assert_eq!(opened, plaintext, "round-trip should preserve plaintext");
}

/// Test that different crypto labels produce different keys.
#[test]
fn crypto_different_labels_produce_different_ciphertext() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plaintext = b"same plaintext for both";

    let sealed_a = velocity_mcp::agent::crypto::seal(tmp.path(), b"label_a", plaintext)
        .expect("seal with label_a");
    let sealed_b = velocity_mcp::agent::crypto::seal(tmp.path(), b"label_b", plaintext)
        .expect("seal with label_b");

    // Different labels derive different subkeys, so ciphertexts differ
    assert_ne!(sealed_a, sealed_b, "different labels should produce different ciphertext");

    // Each can only be opened with its own label
    let opened_a = velocity_mcp::agent::crypto::open(tmp.path(), b"label_a", &sealed_a);
    assert_eq!(opened_a, plaintext);

    let opened_b = velocity_mcp::agent::crypto::open(tmp.path(), b"label_b", &sealed_b);
    assert_eq!(opened_b, plaintext);

    // Cross-label open should fail (return empty for NDA envelope)
    let cross = velocity_mcp::agent::crypto::open(tmp.path(), b"label_a", &sealed_b);
    assert!(cross.is_empty(), "wrong label should fail to decrypt");
}

/// Test shared memory buffer creation and read/write.
#[test]
fn shmem_buffer_read_write_roundtrip() {
    use velocity_mcp::ipc::shmem::SharedMemoryBuffer;

    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("test_shmem.bin");

    let mut buf = SharedMemoryBuffer::create_or_open(&path).expect("create shmem");
    buf.write_input("test request payload").expect("write input");

    let read_back = buf.read_input().expect("read input");
    assert_eq!(read_back, "test request payload");
}

/// Test NDA binary frame parsing.
#[test]
fn nmcp_binary_frame_parse_valid() {
    // Construct a valid NMCP frame: 4-byte magic + 32-byte merkle root + payload
    let mut frame = Vec::new();
    frame.extend_from_slice(b"NMCP");
    frame.extend_from_slice(&[0xABu8; 32]); // merkle root
    frame.extend_from_slice(b"hello payload");

    let parsed = velocity_mcp::protocol::nmcp_binary::NmcpBinaryFrame::parse(&frame);
    assert!(parsed.is_ok());
    let f = parsed.unwrap();
    assert_eq!(f.magic, b"NMCP");
    assert_eq!(f.payload, b"hello payload");
}

/// Test NDA binary frame rejects invalid magic.
#[test]
fn nmcp_binary_frame_rejects_invalid_magic() {
    let mut frame = Vec::new();
    frame.extend_from_slice(b"BAAD");
    frame.extend_from_slice(&[0u8; 32]);

    let parsed = velocity_mcp::protocol::nmcp_binary::NmcpBinaryFrame::parse(&frame);
    assert!(parsed.is_err());
}

/// Test NDA binary frame rejects too-short buffer.
#[test]
fn nmcp_binary_frame_rejects_short_buffer() {
    let frame = vec![0u8; 10]; // Too short for header
    let parsed = velocity_mcp::protocol::nmcp_binary::NmcpBinaryFrame::parse(&frame);
    assert!(parsed.is_err());
}
