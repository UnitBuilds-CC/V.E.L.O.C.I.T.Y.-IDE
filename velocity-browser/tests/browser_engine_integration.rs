//! Browser engine integration tests.
//!
//! Exercises cross-module workflows in the velocity-browser crate:
//! HTML parsing, DOM construction, AES-GCM crypto, layout, SVG,
//! WebGL context management, canvas, and rasterizer operations.

use velocity_browser::dom::mutation_observer::{MutationRecord, MutationType};
use velocity_browser::dom::{DomTree, MutationBatcher, SlabDomTree};
use velocity_browser::engine::{
    CanvasElement, PixelBuffer, StealthHumanBehavior, SvgPathBuilder, WebGLContext,
    WebGpuComputeEngine,
};
use velocity_browser::layout::grid_solver::GridTrack;
use velocity_browser::layout::GridTrackSolver;
use velocity_browser::net::aes_gcm::{aes256_gcm_decrypt, aes256_gcm_encrypt};
use velocity_browser::parser::{Html5Tokenizer, StreamJitTokenizer};

// ─── HTML Tokenizer ──────────────────────────────────────────────────────────

#[test]
fn html5_tokenizer_handles_basic_tags() {
    let mut tok = Html5Tokenizer::new("<div>hello</div>");
    let tokens = tok.tokenize();
    // Should produce at least: open-tag, text, close-tag
    assert!(
        tokens.len() >= 3,
        "expected >= 3 tokens, got {}",
        tokens.len()
    );
}

#[test]
fn html5_tokenizer_self_closing_tag() {
    let mut tok = Html5Tokenizer::new("<img src='x.png'/>");
    let tokens = tok.tokenize();
    assert!(!tokens.is_empty(), "should produce at least one token");
}

#[test]
fn html5_tokenizer_empty_input() {
    let mut tok = Html5Tokenizer::new("");
    let tokens = tok.tokenize();
    assert!(tokens.is_empty(), "empty input should produce no tokens");
}

#[test]
fn stream_jit_tokenizer_chunked_input() {
    let mut tok = StreamJitTokenizer::new();
    let t1 = tok.tokenize_stream_chunk(b"<p>chunk");
    let t2 = tok.tokenize_stream_chunk(b" one</p>");
    // Both chunks should produce tokens
    assert!(!t1.is_empty(), "first chunk should produce tokens");
    assert!(!t2.is_empty(), "second chunk should produce tokens");
}

// ─── DOM Tree ────────────────────────────────────────────────────────────────

#[test]
fn dom_tree_create_element_and_append() {
    let mut tree = DomTree::new(Vec::new());
    let root = tree.create_element("div");
    let child = tree.create_element("span");
    tree.append_child(root, child);
    // Verify via get_node that the child has the correct parent
    let child_node = tree.get_node(child).unwrap();
    assert_eq!(child_node.parent, Some(root));
}

#[test]
fn dom_tree_extract_page_title() {
    let mut tree = DomTree::new(Vec::new());
    let html = tree.create_element("html");
    let head = tree.create_element("head");
    let title = tree.create_element("title");
    let title_text = tree.create_text_node("My Page");
    tree.append_child(html, head);
    tree.append_child(head, title);
    tree.append_child(title, title_text);
    let page_title = tree.extract_page_title();
    assert_eq!(page_title, "My Page");
}

#[test]
fn slab_dom_tree_basic_operations() {
    let mut slab = SlabDomTree::new(64);
    let root = slab.root_slot;
    let c1 = slab.append_child(root, "span");
    let c2 = slab.append_child(root, "p");
    // Traverse: DFS should include all three nodes
    let dfs = slab.dfs();
    assert!(dfs.contains(&root));
    assert!(dfs.contains(&c1));
    assert!(dfs.contains(&c2));
}

#[test]
fn mutation_batcher_push_and_flush() {
    let mut batcher = MutationBatcher::new();
    batcher.push_mutation(MutationRecord {
        mutation_type: MutationType::ChildList,
        target_node_id: 0,
        added_nodes: vec![1, 2],
        removed_nodes: vec![],
        attribute_name: None,
        old_value: None,
    });
    assert_eq!(batcher.pending_count(), 1);
    let records = batcher.flush_batch();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].added_nodes.len(), 2);
}

// ─── AES-256-GCM Crypto ─────────────────────────────────────────────────────

#[test]
fn aes_gcm_encrypt_decrypt_roundtrip() {
    let key = [0x42u8; 32];
    let nonce = [0x01u8; 12];
    let aad = b"additional data";
    let plaintext = b"hello world, encrypted!";

    let (ciphertext, tag) = aes256_gcm_encrypt(&key, &nonce, aad, plaintext);
    let decrypted = aes256_gcm_decrypt(&key, &nonce, aad, &ciphertext, &tag);
    assert_eq!(decrypted, Some(plaintext.to_vec()));
}

#[test]
fn aes_gcm_tampered_ciphertext_fails() {
    let key = [0x42u8; 32];
    let nonce = [0x01u8; 12];
    let aad = b"test";
    let plaintext = b"secret message";

    let (mut ciphertext, tag) = aes256_gcm_encrypt(&key, &nonce, aad, plaintext);
    // Tamper with ciphertext
    if let Some(byte) = ciphertext.first_mut() {
        *byte ^= 0xFF;
    }
    let result = aes256_gcm_decrypt(&key, &nonce, aad, &ciphertext, &tag);
    assert!(
        result.is_none(),
        "tampered ciphertext should fail decryption"
    );
}

#[test]
fn aes_gcm_wrong_key_fails() {
    let key1 = [0x01u8; 32];
    let key2 = [0x02u8; 32];
    let nonce = [0x03u8; 12];
    let aad = b"";
    let plaintext = b"data";

    let (ciphertext, tag) = aes256_gcm_encrypt(&key1, &nonce, aad, plaintext);
    let result = aes256_gcm_decrypt(&key2, &nonce, aad, &ciphertext, &tag);
    assert!(result.is_none(), "wrong key should fail decryption");
}

#[test]
fn aes_gcm_empty_plaintext() {
    let key = [0xAAu8; 32];
    let nonce = [0xBBu8; 12];
    let aad = b"aad";

    let (ciphertext, tag) = aes256_gcm_encrypt(&key, &nonce, aad, b"");
    let decrypted = aes256_gcm_decrypt(&key, &nonce, aad, &ciphertext, &tag);
    assert_eq!(decrypted, Some(vec![]), "empty plaintext should roundtrip");
}

// ─── SVG Path Builder ────────────────────────────────────────────────────────

#[test]
fn svg_path_builder_moveto_and_lineto() {
    let commands = SvgPathBuilder::new()
        .move_to(10.0, 20.0)
        .line_to(100.0, 200.0)
        .build();
    assert!(commands.len() >= 2, "should have at least MoveTo + LineTo");
}

#[test]
fn svg_path_builder_relative_commands() {
    let commands = SvgPathBuilder::new()
        .move_to(0.0, 0.0)
        .line_to_rel(50.0, 50.0)
        .line_to_rel(50.0, 0.0)
        .build();
    assert_eq!(commands.len(), 3);
}

// ─── Layout: Grid Track Solver ───────────────────────────────────────────────

#[test]
fn grid_track_solver_fixed_and_flex() {
    let tracks = vec![
        GridTrack {
            flex_fraction: 0.0,
            px_size: 200.0,
            min_size: None,
            max_size: None,
            is_auto: false,
            span: 1,
        },
        GridTrack {
            flex_fraction: 1.0,
            px_size: 0.0,
            min_size: None,
            max_size: None,
            is_auto: false,
            span: 1,
        },
    ];
    let sizes = GridTrackSolver::solve_tracks(800.0, &tracks);
    assert_eq!(sizes.len(), 2);
    assert!(
        (sizes[0] - 200.0).abs() < 0.01,
        "fixed track should be 200px"
    );
    assert!(
        (sizes[1] - 600.0).abs() < 0.01,
        "flex track should fill remaining 600px"
    );
}

#[test]
fn grid_track_solver_all_flex() {
    let tracks = vec![
        GridTrack {
            flex_fraction: 1.0,
            px_size: 0.0,
            min_size: None,
            max_size: None,
            is_auto: false,
            span: 1,
        },
        GridTrack {
            flex_fraction: 2.0,
            px_size: 0.0,
            min_size: None,
            max_size: None,
            is_auto: false,
            span: 1,
        },
    ];
    let sizes = GridTrackSolver::solve_tracks(900.0, &tracks);
    assert_eq!(sizes.len(), 2);
    assert!((sizes[0] - 300.0).abs() < 0.01, "1fr of 3fr total = 300");
    assert!((sizes[1] - 600.0).abs() < 0.01, "2fr of 3fr total = 600");
}

// ─── WebGL Context ───────────────────────────────────────────────────────────

#[test]
fn webgl_context_create_and_clear() {
    let mut ctx = WebGLContext::new(800, 600);
    ctx.clear(0.0, 0.0, 0.0, 1.0);
    // Should not panic; context is valid
}

#[test]
fn webgl_create_program() {
    let mut ctx = WebGLContext::new(256, 256);
    let prog_id = ctx.create_program(
        "attribute vec4 pos; void main() { gl_Position = pos; }",
        "void main() { gl_FragColor = vec4(1.0); }",
    );
    assert!(prog_id > 0, "program id should be positive");
}

// ─── Canvas ──────────────────────────────────────────────────────────────────

#[test]
fn canvas_element_creation() {
    let canvas = CanvasElement::new("c1", "2d", 200, 200);
    assert_eq!(canvas.width, 200);
    assert_eq!(canvas.height, 200);
}

// ─── Software Rasterizer (PixelBuffer) ──────────────────────────────────────

#[test]
fn pixel_buffer_basic_fill() {
    let mut buf = PixelBuffer::new(64, 64);
    buf.clear(255, 255, 255, 255);
    buf.fill_rect(0, 0, 32, 32, 255, 0, 0, 255);
    let pixel = buf.get_pixel(16, 16);
    assert_eq!(pixel, [255, 0, 0, 255], "filled pixel should be red");
}

#[test]
fn pixel_buffer_out_of_bounds_safe() {
    let mut buf = PixelBuffer::new(32, 32);
    buf.clear(0, 0, 0, 0);
    // Writing outside bounds should not panic (bounds-checked internally)
    buf.fill_rect(100, 100, 10, 10, 255, 0, 0, 255);
}

// ─── Stealth Human Behavior ─────────────────────────────────────────────────

#[test]
fn stealth_human_bezier_generates_points() {
    let points = StealthHumanBehavior::generate_bezier_trajectory((0.0, 0.0), (100.0, 100.0), 10);
    assert!(
        points.len() >= 2,
        "should generate intermediate bezier points"
    );
    // First point should be near start
    assert!((points[0].x - 0.0).abs() < 1.0);
    assert!((points[0].y - 0.0).abs() < 1.0);
}

#[test]
fn stealth_human_typing_jitter() {
    let jitter = StealthHumanBehavior::compute_typing_jitter(50);
    assert!(!jitter.is_empty(), "should produce timing jitter values");
    // All values should be reasonable (non-negative delays)
    for &val in &jitter {
        assert!(val < 10_000, "jitter values should be < 10s in ms");
    }
}

// ─── WebGPU Compute ─────────────────────────────────────────────────────────

#[test]
fn webgpu_buffer_create_and_dispatch() {
    let mut gpu = WebGpuComputeEngine::new();
    let buf_id = gpu.create_buffer(256);
    assert!(buf_id > 0);
    assert!(gpu.dispatch_compute("test_shader", (1, 1, 1)));
}

#[test]
fn webgpu_multiple_buffers() {
    let mut gpu = WebGpuComputeEngine::new();
    let b1 = gpu.create_buffer(128);
    let b2 = gpu.create_buffer(256);
    let b3 = gpu.create_buffer(512);
    assert_ne!(b1, b2);
    assert_ne!(b2, b3);
}
