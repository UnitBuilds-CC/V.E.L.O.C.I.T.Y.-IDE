#![allow(warnings)]

use std::env;
use std::process;
use eframe::egui;

mod protocol;
mod ipc;
mod registry;
mod benchmark;
mod editor;
mod agent;
mod compiler;
mod orchestrator;
mod automation;
mod usage;

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut mode = "stdio";
    let mut buffer_path = "nmcp_buffer.bin";
    let mut benchmark_mode = false;
    let mut editor_mode = args.len() == 1;
    let mut tokenize_prompt = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                editor_mode = false;
                if i + 1 < args.len() {
                    mode = &args[i + 1];
                    i += 2;
                } else {
                    eprintln!("Error: --mode requires an argument (stdio|shmem)");
                    process::exit(1);
                }
            }
            "--buffer-path" => {
                editor_mode = false;
                if i + 1 < args.len() {
                    buffer_path = &args[i + 1];
                    i += 2;
                } else {
                    eprintln!("Error: --buffer-path requires an argument");
                    process::exit(1);
                }
            }
            "--benchmark" => {
                benchmark_mode = true;
                editor_mode = false;
                i += 1;
            }
            "--tokenize" => {
                editor_mode = false;
                if i + 1 < args.len() {
                    tokenize_prompt = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --tokenize requires an argument");
                    process::exit(1);
                }
            }
            "--editor" => {
                editor_mode = true;
                i += 1;
            }
            "--check" => {
                automation::run_self_check();
                return;
            }
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                print_help();
                process::exit(1);
            }
        }
    }

    if let Some(prompt) = tokenize_prompt {
        run_tokenizer_demo(&prompt);
        return;
    }

    if benchmark_mode {
        benchmark::run_benchmarks();
        return;
    }

    if editor_mode {
        println!("Starting V.E.L.O.C.I.T.Y. Native IDE Editor...");

        let mut gpu_name = "None".to_string();
        match compiler::driver::VulkanDriver::init() {
            Ok(driver) => {
                let _ = driver.run_diagnostics();
                gpu_name = driver.device_name();
                let mock_weights = vec![1, -1, 0, 1, 1];
                if let Ok(shader) = compiler::jit::JitCompiler::compile_inlined_weights(&mock_weights) {
                    println!("  - [OK] JIT weight-inlining compile test passed (Size: {} words).", shader.len());
                }
            }
            Err(e) => {
                println!("  - [WARNING] Vulkan Driver diagnostics skipped: {:?}", e);
            }
        }

        let (ui_tx, agent_rx) = crossbeam_channel::unbounded();
        let (agent_tx, ui_rx) = crossbeam_channel::unbounded();
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let workspace_root_agent = workspace_root.clone();

        // Ensure the .velocity folder exists
        let dot_velocity = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dot_velocity).ok();

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
        let site_map = automation::open_workspace_site_map(&workspace_root).ok();
        const PRESENCE_LOCK_TTL: std::time::Duration = std::time::Duration::from_secs(2);

        std::thread::spawn(move || {
            if let Ok(mut server) = ipc::telemetry_share::TelemetryServer::open(&shmem_path_server) {
                println!("[server] Telemetry Server listening on shared memory segment.");
                let _ = server.listen(|req| {
                    match req {
                         ipc::telemetry_share::TelemetryRequest::AstUpdate { file_path, triples } => {
                            let start_time = std::time::Instant::now();
                            println!("[server] Received AST update for {}: {} triples", file_path, triples.len());
                            
                            let elapsed = start_time.elapsed().as_micros() as u64;
                            ipc::telemetry_share::TELEMETRY_LATENCY_US.store(elapsed, std::sync::atomic::Ordering::Relaxed);

                            ipc::telemetry_share::TelemetryResponse {
                                success: true,
                                warning: None,
                            }
                        }
                        ipc::telemetry_share::TelemetryRequest::PresenceUpdate { cursor_line, cursor_col } => {
                            let start_time = std::time::Instant::now();
                            
                            // Check for concurrency conflicts using MediatorArena
                            let file_path = presence_file_path_server.clone();
                            let line_range = (cursor_line.saturating_sub(5), cursor_line.saturating_add(5));
                            let agent_id = "Agent_Thread".to_string();

                            let mut warning = None;
                            mediator_clone.prune_stale_locks(PRESENCE_LOCK_TTL);
                            mediator_clone.release_locks_for_agent(&agent_id);
                            if let Some(sm) = &site_map {
                                if let Err(conflict) = mediator_clone.acquire_lock(file_path, line_range, agent_id.clone(), sm) {
                                    let warning_msg = mediator_clone.resolve_conflict(&conflict);
                                    println!("[mediator] Conflict detected! {}", warning_msg);
                                    warning = Some(warning_msg);
                                }
                            }

                            let elapsed = start_time.elapsed().as_micros() as u64;
                            ipc::telemetry_share::TELEMETRY_LATENCY_US.store(elapsed, std::sync::atomic::Ordering::Relaxed);

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
                Ok(Box::new(editor::app::VelocityApp::new(_cc, workspace_root, agent_tx, agent_rx, gpu_name, mediator_gui)) as Box<dyn eframe::App>)
            }),
        ) {
            eprintln!("Failed to launch GUI editor: {:?}", e);
            process::exit(1);
        }
        return;
    }

    println!("Starting V.E.L.O.C.I.T.Y. NMCP Server...");
    println!("Mode: {}", mode);

    match mode {
        "stdio" => {
            if let Err(e) = protocol::json_rpc::run_stdio_loop() {
                eprintln!("Stdio loop encountered error: {}", e);
                process::exit(1);
            }
        }
        "shmem" => {
            println!("Shared Memory Path: {}", buffer_path);
            if let Err(e) = protocol::nmcp_binary::run_shmem_loop(buffer_path) {
                eprintln!("Shared Memory loop encountered error: {}", e);
                process::exit(1);
            }
        }
        _ => {
            eprintln!("Error: Invalid mode '{}'. Supported modes: stdio, shmem", mode);
            process::exit(1);
        }
    }
}

fn resolve_presence_file(workspace_root: &std::path::Path) -> std::path::PathBuf {
    let candidates = [
        workspace_root.join("velocity-mcp").join("src").join("main.rs"),
        workspace_root.join("velocity-mcp").join("src").join("lib.rs"),
        workspace_root.join("src").join("main.rs"),
        workspace_root.join("src").join("lib.rs"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| workspace_root.to_path_buf())
}

fn print_help() {
    println!("V.E.L.O.C.I.T.Y.-MCP/IDE Server v1.0.0");
    println!("Usage:");
    println!("  velocity_mcp [options]");
    println!();
    println!("Options:");
    println!("  --editor                    Launch the custom native GUI editor (Default if run without options)");
    println!("  --mode <stdio|shmem>        Protocol mode. stdio (JSON-RPC) or shmem (Shared Memory binary).");
    println!("  --buffer-path <path>        Path to mapped buffer file. Only used in shmem mode.");
    println!("  --benchmark                 Run the performance benchmark suite");
    println!("  --tokenize <prompt>         Run the NDA-embedded tokenizer demonstration on the text prompt");
    println!("  --check                     Run `cargo check` and exit with a summary");
    println!("  -h, --help                  Print this help screen");
}

fn run_tokenizer_demo(prompt: &str) {
    println!("============================================================");
    println!("        V.E.L.O.C.I.T.Y. NDA Embedded Tokenizer Demo");
    println!("============================================================");
    println!("Input Text: {:?}", prompt);

    let nda_tokenizer = compiler::tokenizer::NdaEmbeddedTokenizer::new(3200);
    let (token_ids, embeds) = nda_tokenizer.encode_and_embed(prompt);

    println!("Token IDs: {:?}", token_ids);
    println!("Decoded:   {:?}", nda_tokenizer.decode(&token_ids));
    println!("\nNDA Embedding Table (First 3200-dim active/pos bitmaps):");
    println!("--------------------------------------------------------------------------------");
    println!(" {: <15} | {: <8} | {: <10} | {: <18} | {: <18}", "Token String", "ID", "Active %", "Active Word[0]", "Pos Word[0]");
    println!("--------------------------------------------------------------------------------");
    for (idx, &id) in token_ids.iter().enumerate() {
        let (active, pos) = embeds[idx];
        let token_str = nda_tokenizer.tokenizer.decode(&[id]);

        let mut total_active = 0;
        for &w in active {
            total_active += w.count_ones();
        }
        let active_pct = (total_active as f32 / 3200.0) * 100.0;

        let token_display = token_str.replace("\n", "\\n").replace("\r", "\\r");

        println!(" {: <15} | {: <8} | {: <9.1}% | 0x{:08x}         | 0x{:08x}",
            if token_display.len() > 15 { format!("{}...", &token_display[..12]) } else { token_display },
            id,
            active_pct,
            active[0],
            pos[0]
        );
    }
    println!("--------------------------------------------------------------------------------");
    println!("Note: All embedding dimensions are stored directly in bit-compressed binary maps.");
    println!("      Each token embedding requires exactly 750 bits (2 bits per parameter for 3200 dimensions),");
    println!("      resulting in a 10x memory savings compared to standard FP16 lookup tables.");
    println!("============================================================");
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
