# Velocity Drone Portable Agent Endpoint

## Classification
- **Category**: Secondary Crate
- **Files**: 3 source files (core.rs, server.rs, safety.rs)
- **Criticality**: Medium — enables cross-device collaboration

## Summary

`velocity-drone` is a lightweight portable agent endpoint deployable as a single binary on any machine. It implements the V.E.L.O.C.I.T.Y. peer protocol via a minimal HTTP server built on `std::net::TcpListener` with no external HTTP framework or async runtime dependency.

## Architecture

```
drone/
├── src/
│   ├── main.rs       # CLI entry point, start server
│   ├── lib.rs        # Module root
│   ├── core.rs       # DroneCore: identity, file transfers, task execution
│   ├── safety.rs     # SafeMutex: panic-safe mutex wrapper
│   └── server.rs     # HTTP server (std::net::TcpListener)
├── tests/
│   └── integration_test.rs
└── DRONE_PROTOCOL.md # Full protocol specification
```

## DroneIdentity

```rust
pub struct DroneIdentity {
    pub id: String,            // SHA-256 derived: "drone_{hex}"
    pub name: String,
    pub port: u16,
    pub environment: String,   // "{os}-{arch}"
    pub capabilities: Vec<String>,  // file_execution, test_runner, build_system, general
    pub first_seen: u64,
    pub start_time: u64,
}
```

Persisted at `.velocity/drone_identity.json`. ID is stable across restarts.

## HTTP Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/identity` | Return drone identity JSON |
| GET | `/status` | Return current status and capabilities |
| POST | `/execute` | Execute a task in the workspace |
| POST | `/upload` | Receive file from peer |
| GET | `/download` | Retrieve file from workspace |
| POST | `/deploy` | Deploy artifact to target |

## Dependencies (Minimal)

- `serde` + `serde_json` — JSON serialization
- `sha2` — Identity hash generation
- `base64` — File transfer encoding

No async runtime, no HTTP framework, no TLS — designed for single static binary deployment.

## Use Cases

1. Remote build agent on CI machine
2. Cross-device test execution
3. Distributed task collaboration
4. Lightweight agent without IDE overhead
