use velocity_browser::js::{WasmInterpreter, WasmValue};
use velocity_browser::layout::{GridTrack, GridTrackSolver};
use velocity_browser::net::InspectorServer;
use velocity_browser::session::BrowserSession;

#[test]
fn test_wasm_execution_and_grid_solver() {
    let mut wasm = WasmInterpreter::new(1);
    wasm.stack.push(WasmValue::I32(15));
    wasm.stack.push(WasmValue::I32(25));
    assert!(wasm.execute_i32_add().is_ok());

    if let Some(WasmValue::I32(sum)) = wasm.stack.pop() {
        assert_eq!(sum, 40);
    } else {
        panic!("Expected i32 Wasm sum");
    }

    let tracks = vec![
        GridTrack { flex_fraction: 0.0, px_size: 100.0 },
        GridTrack { flex_fraction: 1.0, px_size: 0.0 },
        GridTrack { flex_fraction: 2.0, px_size: 0.0 },
    ];
    let track_sizes = GridTrackSolver::solve_tracks(400.0, &tracks);
    assert_eq!(track_sizes[0], 100.0);
    assert_eq!(track_sizes[1], 100.0); // (400 - 100) / 3 * 1
    assert_eq!(track_sizes[2], 200.0); // (400 - 100) / 3 * 2
}

#[test]
fn test_inspector_server_session_integration() {
    let session = BrowserSession::new("sess_inspector".to_string());
    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 200)); // Inspector port predicate
    assert!(state.iter().any(|t| t.predicate_id == 201)); // DevTools attached predicate
}
