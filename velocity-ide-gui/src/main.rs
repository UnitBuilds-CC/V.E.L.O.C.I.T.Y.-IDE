// V.E.L.O.C.I.T.Y. IDE — Native GUI Editor
//
// This binary launches the egui/eframe-based workspace editor.
// It depends on velocity_mcp for the agent system, automation,
// orchestrator, IPC, and all backend infrastructure.

#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::result_large_err)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::manual_strip)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::only_used_in_recursion)]
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::derivable_impls)]
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::manual_map)]
#![allow(clippy::while_let_loop)]
#![allow(clippy::new_without_default)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::should_implement_trait)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use eframe::egui;
use std::process;

use velocity_mcp::agent;
use velocity_mcp::automation;
use velocity_mcp::compiler;
use velocity_mcp::editor;
use velocity_mcp::ipc;
use velocity_ide::site_map::{NdaNode, SiteMap, VcTriple};

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
        site_map.put_node(&triple).map_err(|e: anyhow::Error| e.to_string())?;
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

fn hash_str(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
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

// ─── Main ──────────────────────────────────────────────────────────────────

fn main() {
    // Install global panic hook for crash diagnostics
    install_panic_hook();

    // Initialise structured logging
    env_logger::init();

    // Install graceful shutdown handlers
    let _shutdown_flag = velocity_mcp::shutdown::install_shutdown_handlers();

    let args: Vec<String> = std::env::args().collect();
    let mut workspace_arg: Option<std::path::PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--workspace" => {
                if i + 1 < args.len() {
                    workspace_arg = Some(std::path::PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    eprintln!("Error: --workspace requires a path argument");
                    process::exit(1);
                }
            }
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            _ => {
                // Ignore unknown args for forward compatibility
                i += 1;
            }
        }
    }

    println!("Starting V.E.L.O.C.I.T.Y. Native IDE Editor...");

    // GPU / Vulkan initialization
    let mut gpu_name = "None".to_string();
    match velocity_ide::compiler::driver::VulkanDriver::init() {
        Ok(driver) => {
            let _ = driver.run_diagnostics();
            gpu_name = driver.device_name();
            let diagnostic_weights = vec![1, -1, 0, 1, 1];
            if let Ok(shader) =
                velocity_mcp::compiler::jit::JitCompiler::compile_inlined_weights(&diagnostic_weights)
            {
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

    let (ui_tx, agent_rx) = crossbeam_channel::unbounded();
    let (agent_tx, ui_rx) = crossbeam_channel::unbounded();

    // Determine workspace root
    let workspace_root = if let Some(workspace) = workspace_arg {
        workspace
    } else {
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
    };

    // Ensure workspace directory exists
    if let Err(err) = std::fs::create_dir_all(&workspace_root) {
        eprintln!(
            "Failed to create workspace directory {}: {}",
            workspace_root.display(),
            err
        );
        process::exit(1);
    }

    let workspace_root_agent = workspace_root.clone();

    // Ensure the .velocity folder exists
    let dot_velocity = workspace_root.join(".velocity");
    if let Err(err) = std::fs::create_dir_all(&dot_velocity) {
        eprintln!(
            "Failed to initialize workspace state directory {}: {}",
            dot_velocity.display(),
            err
        );
        process::exit(1);
    }

    // Initialize the MediatorArena
    let mediator = std::sync::Arc::new(automation::MediatorArena::new());
    let mediator_clone = mediator.clone();
    let presence_file_path = resolve_presence_file(&workspace_root);

    // Spawn Telemetry Server on a Shared Memory segment
    let shmem_path = dot_velocity.join("telemetry_shmem.bin");
    let shmem_path_server = shmem_path.clone();
    let shmem_path_watcher = shmem_path.clone();
    let presence_file_path_server = presence_file_path.clone();

    // Open SiteMap for semantic queries inside telemetry callbacks
    let site_map = automation::open_workspace_site_map(&workspace_root)
        .map(std::sync::Mutex::new)
        .ok();
    const PRESENCE_LOCK_TTL: std::time::Duration = std::time::Duration::from_secs(2);

    std::thread::spawn(move || {
        if let Ok(mut server) = ipc::telemetry_share::TelemetryServer::open(&shmem_path_server) {
            println!("[server] Telemetry Server listening on shared memory segment.");
            let _ = server.listen(|req| {
                match req {
                    ipc::telemetry_share::TelemetryRequest::AstUpdate {
                        file_path,
                        triples,
                    } => {
                        let start_time = std::time::Instant::now();
                        println!(
                            "[server] Received AST update for {}: {} triples",
                            file_path,
                            triples.len()
                        );

                        let warning = if let Some(sm) = &site_map {
                            match sm.lock() {
                                Ok(mut guard) => {
                                    match persist_ast_update(&mut guard, &file_path, &triples) {
                                        Ok(()) => None,
                                        Err(err) => Some(format!(
                                            "Failed to persist AST update for {}: {}",
                                            file_path, err
                                        )),
                                    }
                                }
                                Err(err) => Some(format!(
                                    "Failed to lock SiteMap for AST update {}: {}",
                                    file_path, err
                                )),
                            }
                        } else {
                            Some(
                                "SiteMap unavailable; AST update was not persisted".to_string(),
                            )
                        };
                        if let Some(message) = &warning {
                            eprintln!("[server] {}", message);
                        }

                        let elapsed = start_time.elapsed().as_micros() as u64;
                        ipc::telemetry_share::TELEMETRY_LATENCY_US
                            .store(elapsed, std::sync::atomic::Ordering::Relaxed);

                        ipc::telemetry_share::TelemetryResponse {
                            success: warning.is_none(),
                            warning,
                        }
                    }
                    ipc::telemetry_share::TelemetryRequest::AstDelete { file_path } => {
                        let start_time = std::time::Instant::now();
                        println!("[server] Received AST delete for {}", file_path);

                        let warning = if let Some(sm) = &site_map {
                            match sm.lock() {
                                Ok(mut guard) => {
                                    match remove_ast_update(&mut guard, &file_path) {
                                        Ok(()) => None,
                                        Err(err) => Some(format!(
                                            "Failed to remove AST update for {}: {}",
                                            file_path, err
                                        )),
                                    }
                                }
                                Err(err) => Some(format!(
                                    "Failed to lock SiteMap for AST delete {}: {}",
                                    file_path, err
                                )),
                            }
                        } else {
                            Some(
                                "SiteMap unavailable; AST delete was not persisted".to_string(),
                            )
                        };
                        if let Some(message) = &warning {
                            eprintln!("[server] {}", message);
                        }

                        let elapsed = start_time.elapsed().as_micros() as u64;
                        ipc::telemetry_share::TELEMETRY_LATENCY_US
                            .store(elapsed, std::sync::atomic::Ordering::Relaxed);

                        ipc::telemetry_share::TelemetryResponse {
                            success: warning.is_none(),
                            warning,
                        }
                    }
                    ipc::telemetry_share::TelemetryRequest::PresenceUpdate {
                        cursor_line,
                        cursor_col: _,
                    } => {
                        let start_time = std::time::Instant::now();

                        let file_path = presence_file_path_server.clone();
                        let line_range =
                            (cursor_line.saturating_sub(5), cursor_line.saturating_add(5));
                        let agent_id = "Agent_Thread".to_string();

                        let mut warning = None;
                        mediator_clone.prune_stale_locks(PRESENCE_LOCK_TTL);
                        mediator_clone.release_locks_for_agent(&agent_id);
                        if let Some(sm) = &site_map {
                            if let Ok(guard) = sm.lock() {
                                if let Err(conflict) = mediator_clone.acquire_lock(
                                    file_path,
                                    line_range,
                                    agent_id.clone(),
                                    &guard,
                                ) {
                                    let warning_msg =
                                        mediator_clone.resolve_conflict(&conflict);
                                    println!("[mediator] Conflict detected! {}", warning_msg);
                                    warning = Some(warning_msg);
                                }
                            }
                        }

                        let elapsed = start_time.elapsed().as_micros() as u64;
                        ipc::telemetry_share::TELEMETRY_LATENCY_US
                            .store(elapsed, std::sync::atomic::Ordering::Relaxed);

                        ipc::telemetry_share::TelemetryResponse {
                            success: true,
                            warning,
                        }
                    }
                }
            });
        }
    });

    // Spawn AST File Watcher
    automation::spawn_ast_watcher(workspace_root.clone(), shmem_path_watcher);

    std::thread::spawn(move || {
        agent::run_agent_thread(workspace_root_agent, ui_rx, ui_tx);
    });

    automation::spawn_build_watcher(workspace_root.clone(), 5);

    // Launch eframe GUI
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

    let mediator_gui = mediator.clone();
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
                mediator_gui,
            )) as Box<dyn eframe::App>)
        }),
    ) {
        eprintln!("Failed to launch GUI editor: {:?}", e);
        process::exit(1);
    }
}

fn print_help() {
    println!("V.E.L.O.C.I.T.Y. IDE GUI v1.0.0");
    println!("Usage:");
    println!("  velocity_ide_gui [options]");
    println!();
    println!("Options:");
    println!("  --workspace <path>   Open this directory as the workspace root");
    println!("  -h, --help           Print this help screen");
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
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
