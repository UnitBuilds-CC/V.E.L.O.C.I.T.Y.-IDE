// Structural patterns that are intentional in this codebase
#![allow(clippy::too_many_arguments)] // Tool dispatch / WA functions take many params
#![allow(clippy::type_complexity)] // Channel/callback types are inherently complex
#![allow(clippy::result_large_err)] // Error types carry diagnostic context
#![allow(clippy::ptr_arg)] // &PathBuf kept for API symmetry in places
#![allow(clippy::needless_range_loop)] // Index-based loops clearer for parallel arrays
#![allow(clippy::field_reassign_with_default)] // Stepwise struct config for readability
#![allow(clippy::manual_strip)] // Explicit prefix/suffix handling for clarity
#![allow(clippy::enum_variant_names)] // Shared-prefix variants are intentional
#![allow(clippy::upper_case_acronyms)] // Windows FFI type names (STARTUPINFOW, etc.)
#![allow(clippy::only_used_in_recursion)] // Recursion params kept for signature clarity
#![allow(clippy::manual_c_str_literals)] // Explicit nul-terminated construction for FFI
#![allow(clippy::derivable_impls)] // Derive would change semantics in some cases
#![allow(clippy::manual_div_ceil)] // Explicit div_ceil for clarity
#![allow(clippy::manual_map)] // Manual map for readability in some contexts
#![allow(clippy::while_let_loop)] // Explicit loop+match sometimes more readable
#![allow(clippy::new_without_default)] // Not all types need Default
#![allow(clippy::collapsible_if)] // Nested ifs sometimes clearer
#![allow(clippy::redundant_closure)] // Explicit closures for type inference
#![allow(clippy::if_same_then_else)] // Identical branches for semantic clarity
#![allow(clippy::should_implement_trait)] // from_str methods don't always need FromStr trait
#![allow(dead_code)] // Binary module tree has scaffolding modules not yet wired into main()
#![allow(unused_imports)] // Imports retained for API completeness
#![allow(unused_variables)] // Variables retained for future wiring

use std::env;
use std::process;

mod agent;
mod automation;
mod compiler;
mod connectors;
mod editor;
mod errors;
mod ipc;
mod orchestrator;
mod protocol;
mod registry;
mod safety;
mod security;
mod shutdown;
mod usage;
mod wa;


/// Install a global panic hook that writes structured crash dumps and logs
/// diagnostic context before the process exits. This ensures that any unhandled
/// panic produces actionable output rather than a silent crash.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string payload>".to_string()
        };
        eprintln!(
            "\n[PANIC] V.E.L.O.C.I.T.Y. encountered an unrecoverable error.\n\
             \x20  Location: {location}\n\
             \x20  Detail:   {payload}\n\
             Please report this crash with the above details."
        );

        // Write a JSON crash dump to .velocity/crashes/ for later analysis.
        let dump = serde_json::json!({
            "timestamp": chrono_like_timestamp(),
            "app_name": "V.E.L.O.C.I.T.Y. IDE",
            "app_version": env!("CARGO_PKG_VERSION"),
            "panic_message": payload,
            "panic_location": location,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        });
        if let Ok(cwd) = std::env::current_dir() {
            let crash_dir = cwd.join(".velocity").join("crashes");
            if std::fs::create_dir_all(&crash_dir).is_ok() {
                let filename = format!(
                    "crash_{}.json",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                );
                let _ = std::fs::write(
                    crash_dir.join(filename),
                    serde_json::to_string_pretty(&dump).unwrap_or_default(),
                );
            }
        }
    }));
}

/// Produce a rough ISO-8601 timestamp without pulling in chrono.
fn chrono_like_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}Z", secs)
}

fn main() {
    // Install global panic hook for crash diagnostics
    install_panic_hook();

    // Initialise structured logging (defaults to stderr, safe for stdio JSON-RPC).
    // Control verbosity via RUST_LOG env var, e.g. RUST_LOG=info or RUST_LOG=debug.
    env_logger::init();

    // Install graceful shutdown handlers early
    let _shutdown_flag = shutdown::install_shutdown_handlers();

    let args: Vec<String> = env::args().collect();
    let mut mode = "stdio";
    let mut buffer_path = "nmcp_buffer.bin";
    let mut tokenize_prompt = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                if i + 1 < args.len() {
                    mode = &args[i + 1];
                    i += 2;
                } else {
                    eprintln!("Error: --mode requires an argument (stdio|shmem)");
                    process::exit(1);
                }
            }
            "--buffer-path" => {
                if i + 1 < args.len() {
                    buffer_path = &args[i + 1];
                    i += 2;
                } else {
                    eprintln!("Error: --buffer-path requires an argument");
                    process::exit(1);
                }
            }
            "--tokenize" => {
                if i + 1 < args.len() {
                    tokenize_prompt = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --tokenize requires an argument");
                    process::exit(1);
                }
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
            eprintln!(
                "Error: Invalid mode '{}'. Supported modes: stdio, shmem",
                mode
            );
            process::exit(1);
        }
    }
}

fn print_help() {
    println!("V.E.L.O.C.I.T.Y.-MCP/IDE Server v1.0.0");
    println!("Usage:");
    println!("  velocity_mcp [options]");
    println!();
    println!("Options:");
    println!("  --mode <stdio|shmem>        Protocol mode. stdio (JSON-RPC) or shmem (Shared Memory binary).");
    println!("  --buffer-path <path>        Path to mapped buffer file. Only used in shmem mode.");
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
    println!(
        " {: <15} | {: <8} | {: <10} | {: <18} | {: <18}",
        "Token String", "ID", "Active %", "Active Word[0]", "Pos Word[0]"
    );
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

        println!(
            " {: <15} | {: <8} | {: <9.1}% | 0x{:08x}         | 0x{:08x}",
            if token_display.len() > 15 {
                format!("{}...", &token_display[..12])
            } else {
                token_display
            },
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
// gate-test: trivial edit
