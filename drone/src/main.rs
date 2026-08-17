//! Velocity Drone CLI — lightweight portable agent endpoint.
//!
//! Usage:
//!   velocity-drone                              # Start with defaults
//!   velocity-drone --port 9191 --name "My Drone"
//!   velocity-drone --workspace /path/to/workdir

use std::path::PathBuf;

use velocity_drone::core::{DroneCore, DroneIdentity};
use velocity_drone::server::DroneServer;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut port: u16 = 9191;
    let mut host: String = "127.0.0.1".to_string();
    let mut name: Option<String> = None;
    let mut workspace: Option<PathBuf> = None;
    let mut capabilities: Option<Vec<String>> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                port = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(9191);
            }
            "--host" => {
                i += 1;
                host = args
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| "127.0.0.1".to_string());
            }
            "--name" => {
                i += 1;
                name = args.get(i).cloned();
            }
            "--workspace" => {
                i += 1;
                workspace = args.get(i).map(PathBuf::from);
            }
            "--capabilities" => {
                i += 1;
                let mut caps = Vec::new();
                while i < args.len() && !args[i].starts_with("--") {
                    caps.push(args[i].clone());
                    i += 1;
                }
                capabilities = Some(caps);
                continue; // Don't increment i again.
            }
            "--help" | "-h" => {
                print_help();
                return;
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // Determine workspace.
    let workspace = workspace.unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".velocity_drone")
    });
    std::fs::create_dir_all(&workspace).ok();

    // Determine name.
    let name = name.unwrap_or_else(|| {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "velocity-drone".to_string())
    });

    // Create identity (load from disk or create fresh).
    let mut identity = DroneIdentity::load_or_create(&name, port, &workspace);

    // Override capabilities if specified.
    if let Some(caps) = capabilities {
        identity.capabilities = caps;
    }

    // Create core and server.
    let core = DroneCore::new(identity, workspace);
    let server = DroneServer::new(core, &host, port);

    // Start serving (blocking).
    if let Err(e) = server.serve() {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}

fn print_help() {
    println!(
        "Velocity Drone \u{2014} Lightweight portable agent endpoint

USAGE:
    velocity-drone [OPTIONS]

OPTIONS:
    --host <ADDR>              Bind address (default: 127.0.0.1)
    --port <PORT>              Port to listen on (default: 9191)
    --name <NAME>              Drone name (default: hostname)
    --workspace <PATH>         Workspace directory (default: ./.velocity_drone)
    --capabilities <CAP>...    Advertised capabilities (space-separated)
    -h, --help                 Show this help

EXAMPLES:
    velocity-drone
    velocity-drone --port 9191 --name \"Build Machine\"
    velocity-drone --host 0.0.0.0 --port 9191   # Listen on all interfaces
    velocity-drone --workspace /home/user/drone --capabilities file_execution test_runner

PROTOCOL:
    See DRONE_PROTOCOL.md for the full HTTP/JSON API specification.",
    );
}
