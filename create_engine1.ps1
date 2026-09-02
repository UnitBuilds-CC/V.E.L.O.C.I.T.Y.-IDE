$engBase = "c:\Users\visse\OneDrive\Documents\Velocity-IDE\Velocity-IDE\shared\velocity-workflow-engine"

# Cargo.toml
@"
[package]
name = "velocity-workflow-engine"
version = "0.1.0"
edition = "2021"
description = "Velocity Workflow Engine — WAL-backed, batched, concurrent execution"
license = "MIT"

[dependencies]
velocity-workflow-core = { path = "../velocity-workflow-core" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.0", features = ["v4", "serde"] }
thiserror = "1.0"
async-trait = "0.1"
tokio = { version = "1.0", features = ["full"] }
tracing = "0.1"
rusqlite = { version = "0.31", features = ["bundled"] }

[dev-dependencies]
tokio = { version = "1.0", features = ["full", "test-util"] }
tempfile = "3.0"
"@ | Set-Content -Path "$engBase\Cargo.toml" -Encoding UTF8
Write-Host "Created engine Cargo.toml"
