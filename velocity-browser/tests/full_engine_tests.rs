use velocity_browser::engine::{DeviceProfile, StealthHumanBehavior};
use velocity_browser::js::{WasmSimdPipeline, WasmV128Vector};
use velocity_browser::layout::{LayoutBox, DisplayMode, ParallelLayoutEngine};
use velocity_browser::net::TlsFingerprintRotator;
use velocity_browser::parser::FastCssParser;
use velocity_browser::session::BrowserSession;

#[test]
fn test_velocity_native_identity_and_48_modules() {
    let profile = DeviceProfile::velocity_native();
    assert!(profile.user_agent.contains("VelocityEngine"));

    let mut rot = TlsFingerprintRotator::velocity_native();
    let tls = rot.rotate_profile();
    assert!(tls.ja3_hash.contains("velocity_native"));

    let fast_rules = FastCssParser::parse_rules_fast("div { color: red; }");
    assert_eq!(fast_rules.len(), 1);

    let layout_engine = ParallelLayoutEngine::new(4);
    let mut root_box = LayoutBox {
        node_id: 0,
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
        padding: [0.0; 4],
        margin: [0.0; 4],
        z_index: 0,
        display: DisplayMode::Block,
        children: Vec::new(),
        is_visible: false,
    };
    layout_engine.compute_parallel_subtrees(&mut root_box);
    assert!(root_box.is_visible);

    let simd = WasmSimdPipeline::new();
    let vec_a = WasmV128Vector { lane_bytes: [1; 16] };
    let vec_b = WasmV128Vector { lane_bytes: [2; 16] };
    let res = simd.execute_vector_add(&vec_a, &vec_b);
    assert_eq!(res.lane_bytes[0], 3);

    let trajectory = StealthHumanBehavior::generate_bezier_trajectory((0.0, 0.0), (100.0, 100.0), 5);
    assert_eq!(trajectory.len(), 6);

    let session = BrowserSession::new("sess_48_modules".to_string());
    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 110));
}
