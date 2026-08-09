use velocity_browser::engine::WebGpuComputeEngine;
use velocity_browser::parser::StreamJitTokenizer;
use velocity_browser::session::BrowserSession;
use velocity_browser::session_swarm::SwarmSessionOrchestrator;

#[test]
fn test_52_pure_rust_engine_modules() {
    let mut gpu = WebGpuComputeEngine::new();
    let bid = gpu.create_buffer(1024);
    assert_eq!(bid, 1);
    assert!(gpu.dispatch_compute("compute_shader", (1, 1, 1)));

    let mut swarm = SwarmSessionOrchestrator::new();
    let tab = swarm.spawn_swarm_tab("swarm_tab_1");
    assert_eq!(tab.session_id, "swarm_tab_1");
    assert_eq!(swarm.active_swarm_count(), 1);

    let mut stream_tokenizer = StreamJitTokenizer::new();
    let stream_tokens = stream_tokenizer.tokenize_stream_chunk(b"<div>stream</div>");
    assert_eq!(stream_tokens.len(), 3); // open tag + text + close tag

    let mut session = BrowserSession::new("sess_52_modules".to_string());
    let _ = session.load_html(
        "https://example.com",
        "<html><body><button id='b1'>Submit</button></body></html>",
    );
    let pred = session.predict_action();
    assert!(pred.is_some());
    assert_eq!(pred.unwrap().action_type, "click");
}
