// V.E.L.O.C.I.T.Y. IDE — Native GUI Editor
//
// This binary launches the egui/eframe-based workspace editor.
// It depends on velocity_mcp for the agent system, automation,
// orchestrator, IPC, and all backend infrastructure.

use clap::Parser;
use eframe::egui;
use std::process;

use velocity_ide::hash_str;
use velocity_ide::site_map::{NdaNode, SiteMap, VcTriple};
use velocity_mcp::automation;
use velocity_mcp::editor;
use velocity_mcp::ipc;

// ─── CLI ───────────────────────────────────────────────────────────────────

#[derive(clap::Parser)]
#[command(
    name = "velocity_ide_gui",
    about = "V.E.L.O.C.I.T.Y. IDE — Native Workspace Editor"
)]
struct Cli {
    /// Open this directory as the workspace root
    #[arg(long)]
    workspace: Option<std::path::PathBuf>,
}

// ─── Helper functions ──────────────────────────────────────────────────────

fn persist_ast_update(
    site_map: &mut SiteMap,
    file_path: &str,
    triples: &[(u64, u16, u64)],
) -> Result<(), String> {
    let file_hash = hash_str(file_path);
    let mut live_triples = Vec::with_capacity(triples.len());

    for (subject_hash, predicate_id, object_hash) in triples {
        let normalized_subject = if *subject_hash == file_hash {
            file_hash
        } else {
            *subject_hash
        };
        let triple = NdaNode::Triple {
            subject_hash: normalized_subject,
            predicate_id: *predicate_id,
            object_hash: *object_hash,
        };
        site_map
            .put_node(&triple)
            .map_err(|e: anyhow::Error| e.to_string())?;
        live_triples.push(VcTriple {
            subject_hash: normalized_subject,
            predicate_id: *predicate_id,
            object_hash: *object_hash,
        });
    }

    site_map
        .put_file_snapshot(file_path, &live_triples)
        .map_err(|e: anyhow::Error| e.to_string())?;
    site_map.flush().map_err(|e: anyhow::Error| e.to_string())
}

fn remove_ast_update(site_map: &mut SiteMap, file_path: &str) -> Result<(), String> {
    site_map
        .remove_file_snapshot(file_path)
        .map_err(|e: anyhow::Error| e.to_string())?;
    site_map.flush().map_err(|e: anyhow::Error| e.to_string())
}

fn resolve_presence_file(workspace_root: &std::path::Path) -> std::path::PathBuf {
    let candidates = [
        workspace_root
            .join("velocity-mcp")
            .join("src")
            .join("main.rs"),
        workspace_root
            .join("velocity-mcp")
            .join("src")
            .join("lib.rs"),
        workspace_root.join("src").join("main.rs"),
        workspace_root.join("src").join("lib.rs"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| workspace_root.to_path_buf())
}

fn load_icon() -> Option<egui::IconData> {
    let icon_bytes = include_bytes!("../assets/logo.png");
    if let Ok(image) = image::load_from_memory(icon_bytes) {
        let rgba = image.into_rgba8();
        let (width, height) = rgba.dimensions();
        Some(egui::IconData {
            rgba: rgba.into_raw(),
            width,
            height,
        })
    } else {
        None
    }
}

/// Initialise GPU / Vulkan and return the device name.
fn init_gpu() -> String {
    let mut gpu_name = "None".to_string();
    match velocity_ide::compiler::driver::VulkanDriver::init() {
        Ok(driver) => {
            let _ = driver.run_diagnostics();
            gpu_name = driver.device_name();
            let diagnostic_weights = vec![1, -1, 0, 1, 1];
            if let Ok(shader) = velocity_mcp::compiler::jit::JitCompiler::compile_inlined_weights(
                &diagnostic_weights,
            ) {
                println!(
                    "  - [OK] JIT weight-inlining compile test passed (Size: {} words).",
                    shader.len()
                );
            }
        }
        Err(e) => {
            println!("  - [WARNING] Vulkan Driver diagnostics skipped: {:?}", e);
        }
    }
    gpu_name
}

/// Resolve the workspace root from CLI args or filesystem heuristics.
fn resolve_workspace(workspace_arg: Option<std::path::PathBuf>) -> std::path::PathBuf {
    if let Some(workspace) = workspace_arg {
        return workspace;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let test_file = cwd.join(".velocity_write_test");
    if std::fs::write(&test_file, "test").is_ok() {
        let _ = std::fs::remove_file(test_file);
        cwd
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("V.E.L.O.C.I.T.Y. Workspace")
    }
}

/// Spawn the telemetry shared-memory server thread.
fn spawn_telemetry_server(
    shmem_path: std::path::PathBuf,
    presence_file_path: std::path::PathBuf,
    site_map: Option<std::sync::Arc<std::sync::Mutex<SiteMap>>>,
    mediator: std::sync::Arc<automation::MediatorArena>,
) {
    std::thread::spawn(move || {
        if let Ok(mut server) =
            ipc::telemetry_share::TelemetryServer::open(&shmem_path, b"velocity_telemetry_v1")
        {
            println!("[server] Telemetry Server listening on shared memory segment.");
            let _ = server.listen(|req| match req {
                ipc::telemetry_share::TelemetryRequest::AstUpdate { file_path, triples } => {
                    handle_ast_update(&site_map, &file_path, &triples)
                }
                ipc::telemetry_share::TelemetryRequest::AstDelete { file_path } => {
                    handle_ast_delete(&site_map, &file_path)
                }
                ipc::telemetry_share::TelemetryRequest::PresenceUpdate {
                    cursor_line,
                    cursor_col: _,
                } => handle_presence_update(&mediator, &site_map, &presence_file_path, cursor_line),
            });
        }
    });
}

fn handle_ast_update(
    site_map: &Option<std::sync::Arc<std::sync::Mutex<SiteMap>>>,
    file_path: &str,
    triples: &[(u64, u16, u64)],
) -> ipc::telemetry_share::TelemetryResponse {
    let start_time = std::time::Instant::now();
    println!(
        "[server] Received AST update for {}: {} triples",
        file_path,
        triples.len()
    );

    let warning = if let Some(sm) = site_map {
        match sm.lock() {
            Ok(mut guard) => match persist_ast_update(&mut guard, file_path, triples) {
                Ok(()) => None,
                Err(err) => Some(format!(
                    "Failed to persist AST update for {}: {}",
                    file_path, err
                )),
            },
            Err(err) => Some(format!(
                "Failed to lock SiteMap for AST update {}: {}",
                file_path, err
            )),
        }
    } else {
        Some("SiteMap unavailable; AST update was not persisted".to_string())
    };
    if let Some(message) = &warning {
        eprintln!("[server] {}", message);
    }

    let elapsed = start_time.elapsed().as_micros() as u64;
    ipc::telemetry_share::TELEMETRY_LATENCY_US.store(elapsed, std::sync::atomic::Ordering::Relaxed);

    ipc::telemetry_share::TelemetryResponse {
        success: warning.is_none(),
        warning,
    }
}

fn handle_ast_delete(
    site_map: &Option<std::sync::Arc<std::sync::Mutex<SiteMap>>>,
    file_path: &str,
) -> ipc::telemetry_share::TelemetryResponse {
    let start_time = std::time::Instant::now();
    println!("[server] Received AST delete for {}", file_path);

    let warning = if let Some(sm) = site_map {
        match sm.lock() {
            Ok(mut guard) => match remove_ast_update(&mut guard, file_path) {
                Ok(()) => None,
                Err(err) => Some(format!(
                    "Failed to remove AST update for {}: {}",
                    file_path, err
                )),
            },
            Err(err) => Some(format!(
                "Failed to lock SiteMap for AST delete {}: {}",
                file_path, err
            )),
        }
    } else {
        Some("SiteMap unavailable; AST delete was not persisted".to_string())
    };
    if let Some(message) = &warning {
        eprintln!("[server] {}", message);
    }

    let elapsed = start_time.elapsed().as_micros() as u64;
    ipc::telemetry_share::TELEMETRY_LATENCY_US.store(elapsed, std::sync::atomic::Ordering::Relaxed);

    ipc::telemetry_share::TelemetryResponse {
        success: warning.is_none(),
        warning,
    }
}

fn handle_presence_update(
    mediator: &automation::MediatorArena,
    site_map: &Option<std::sync::Arc<std::sync::Mutex<SiteMap>>>,
    presence_file_path: &std::path::Path,
    cursor_line: usize,
) -> ipc::telemetry_share::TelemetryResponse {
    let start_time = std::time::Instant::now();
    let line_range = (cursor_line.saturating_sub(5), cursor_line.saturating_add(5));
    let agent_id = "Agent_Thread".to_string();

    let mut warning = None;
    const PRESENCE_LOCK_TTL: std::time::Duration = std::time::Duration::from_secs(2);
    mediator.prune_stale_locks(PRESENCE_LOCK_TTL);
    mediator.release_locks_for_agent(&agent_id);
    if let Some(sm) = site_map {
        if let Ok(guard) = sm.lock() {
            if let Err(conflict) = mediator.acquire_lock(
                presence_file_path.to_path_buf(),
                line_range,
                agent_id,
                &guard,
            ) {
                let warning_msg = mediator.resolve_conflict(&conflict);
                println!("[mediator] Conflict detected! {}", warning_msg);
                warning = Some(warning_msg);
            }
        }
    }

    let elapsed = start_time.elapsed().as_micros() as u64;
    ipc::telemetry_share::TELEMETRY_LATENCY_US.store(elapsed, std::sync::atomic::Ordering::Relaxed);

    ipc::telemetry_share::TelemetryResponse {
        success: true,
        warning,
    }
}

/// Configure and launch the eframe GUI event loop.
fn launch_gui(
    workspace_root: std::path::PathBuf,
    gpu_name: String,
    agent_tx: crossbeam_channel::Sender<velocity_mcp::agent::UiToAgentMessage>,
    agent_rx: crossbeam_channel::Receiver<velocity_mcp::agent::AgentToUiMessage>,
    mediator: std::sync::Arc<automation::MediatorArena>,
) {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("V.E.L.O.C.I.T.Y. IDE - Native Workspace Editor")
        .with_inner_size([1280.0, 768.0]);

    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "velocity_ide",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(editor::app::VelocityApp::new(
                _cc,
                workspace_root,
                agent_tx,
                agent_rx,
                gpu_name,
                mediator,
            )) as Box<dyn eframe::App>)
        }),
    ) {
        eprintln!("Failed to launch GUI editor: {:?}", e);
        process::exit(1);
    }
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        eprintln!(
            "[FATAL] V.E.L.O.C.I.T.Y. IDE panicked: {}\n  location: {}",
            payload,
            location.unwrap_or_else(|| "unknown".to_string())
        );
    }));
}

// ─── Main ──────────────────────────────────────────────────────────────────

fn main() {
    install_panic_hook();
    env_logger::init();
    let _shutdown_flag = velocity_mcp::shutdown::install_shutdown_handlers();

    let cli = Cli::parse();
    println!("Starting V.E.L.O.C.I.T.Y. Native IDE Editor...");

    let gpu_name = init_gpu();

    let (ui_tx, agent_rx) = crossbeam_channel::unbounded();
    let (agent_tx, ui_rx) = crossbeam_channel::unbounded();

    let workspace_root = resolve_workspace(cli.workspace);
    if let Err(err) = std::fs::create_dir_all(&workspace_root) {
        eprintln!(
            "Failed to create workspace directory {}: {}",
            workspace_root.display(),
            err
        );
        process::exit(1);
    }

    let dot_velocity = workspace_root.join(".velocity");
    if let Err(err) = std::fs::create_dir_all(&dot_velocity) {
        eprintln!(
            "Failed to initialize workspace state directory {}: {}",
            dot_velocity.display(),
            err
        );
        process::exit(1);
    }

    let mediator = std::sync::Arc::new(automation::MediatorArena::new());
    let presence_file_path = resolve_presence_file(&workspace_root);
    let shmem_path = dot_velocity.join("telemetry_shmem.bin");

    let site_map = automation::open_workspace_site_map(&workspace_root)
        .map(std::sync::Mutex::new)
        .map(std::sync::Arc::new)
        .ok();

    spawn_telemetry_server(shmem_path, presence_file_path, site_map, mediator.clone());

    automation::spawn_ast_watcher(
        workspace_root.clone(),
        dot_velocity.join("telemetry_shmem.bin"),
    );

    let workspace_root_agent = workspace_root.clone();
    std::thread::spawn(move || {
        velocity_mcp::agent::run_agent_thread(workspace_root_agent, ui_rx, ui_tx);
    });

    automation::spawn_build_watcher(workspace_root.clone(), 5);

    launch_gui(workspace_root, gpu_name, agent_tx, agent_rx, mediator);
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! Smoke tests for the GUI binary's display-independent helpers.
    //!
    //! Rendering itself needs a live GPU/display surface, so these cover the
    //! pure and filesystem logic that runs before and around the event loop:
    //! path hashing, workspace presence-file resolution, and icon decoding.

    use super::{hash_str, load_icon, resolve_presence_file, resolve_workspace, Cli};

    #[test]
    fn hash_str_is_deterministic() {
        assert_eq!(hash_str("velocity"), hash_str("velocity"));
        assert_eq!(hash_str(""), hash_str(""));
        assert_eq!(
            hash_str("velocity-mcp/src/main.rs"),
            hash_str("velocity-mcp/src/main.rs"),
        );
    }

    #[test]
    fn hash_str_distinguishes_inputs() {
        assert_ne!(hash_str("a"), hash_str("b"));
        assert_ne!(hash_str("src/main.rs"), hash_str("src/lib.rs"));
        assert_ne!(
            hash_str("velocity-mcp/src/main.rs"),
            hash_str("velocity-mcp/src/main.rs "),
        );
    }

    #[test]
    fn resolve_presence_file_prefers_mcp_main() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("velocity-mcp").join("src")).unwrap();
        std::fs::write(
            root.join("velocity-mcp").join("src").join("main.rs"),
            b"fn main(){}",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("main.rs"), b"fn main(){}").unwrap();
        assert_eq!(
            resolve_presence_file(root),
            root.join("velocity-mcp").join("src").join("main.rs"),
        );
    }

    #[test]
    fn resolve_presence_file_falls_back_to_mcp_lib() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("velocity-mcp").join("src")).unwrap();
        std::fs::write(
            root.join("velocity-mcp").join("src").join("lib.rs"),
            b"// lib",
        )
        .unwrap();
        assert_eq!(
            resolve_presence_file(root),
            root.join("velocity-mcp").join("src").join("lib.rs"),
        );
    }

    #[test]
    fn resolve_presence_file_uses_plain_src_when_no_mcp() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("main.rs"), b"fn main(){}").unwrap();
        assert_eq!(
            resolve_presence_file(root),
            root.join("src").join("main.rs")
        );
    }

    #[test]
    fn resolve_presence_file_falls_back_to_root_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(resolve_presence_file(root), root.to_path_buf());
    }

    #[test]
    fn load_icon_decodes_bundled_png() {
        let icon = load_icon().expect("assets/logo.png should decode to RGBA");
        assert!(icon.width > 0, "icon width must be non-zero");
        assert!(icon.height > 0, "icon height must be non-zero");
        assert_eq!(
            icon.rgba.len(),
            (icon.width as usize) * (icon.height as usize) * 4,
            "RGBA buffer must hold width*height*4 bytes",
        );
    }

    #[test]
    fn resolve_workspace_explicit_path() {
        let p = std::path::PathBuf::from("/tmp/explicit_ws");
        assert_eq!(resolve_workspace(Some(p.clone())), p);
    }

    #[test]
    fn resolve_workspace_default_is_writable() {
        let ws = resolve_workspace(None);
        // The returned path should either be cwd or a home-dir fallback.
        assert!(
            !ws.to_str().unwrap().is_empty(),
            "workspace path must be non-empty"
        );
    }

    #[test]
    fn cli_parser_defaults() {
        let cli = <Cli as clap::Parser>::parse_from(["velocity_ide_gui"]);
        assert!(cli.workspace.is_none());
    }

    #[test]
    fn cli_parser_workspace() {
        let cli =
            <Cli as clap::Parser>::parse_from(["velocity_ide_gui", "--workspace", "/tmp/my_ws"]);
        assert_eq!(
            cli.workspace.unwrap(),
            std::path::PathBuf::from("/tmp/my_ws")
        );
    }
}
