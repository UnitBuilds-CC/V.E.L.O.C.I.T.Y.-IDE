# Drone Subsystem

The `velocity-drone` crate (3 source files) implements a lightweight portable agent endpoint for cross-device V.E.L.O.C.I.T.Y. collaboration. A drone is a single binary that can be deployed on any machine to participate in the Velocity peer protocol without requiring the full IDE.

---

## Overview

A drone provides a minimal HTTP-based agent endpoint that can:
- Receive and execute tasks from the IDE or other drones
- Transfer files between peers
- Report execution results
- Maintain identity across restarts via persisted configuration

### Module Structure

```
drone/
├── src/
│   ├── main.rs       # Entry point: parse CLI args, start server
│   ├── lib.rs        # Module root
│   ├── core.rs       # DroneCore: identity, file transfers, task execution, deployment
│   ├── safety.rs     # SafeMutex: panic-safe mutex wrapper
│   └── server.rs     # HTTP server built on std::net::TcpListener
├── tests/
│   └── integration_test.rs
├── Cargo.toml
└── DRONE_PROTOCOL.md # Full protocol specification
```

---

## Drone Identity & Configuration

### DroneIdentity (`core.rs`)

```rust
pub struct DroneIdentity {
    pub id: String,            // SHA-256 derived: "drone_{hex}"
    pub name: String,          // Human-readable name
    pub port: u16,             // HTTP listen port
    pub environment: String,   // "{os}-{arch}"
    pub capabilities: Vec<String>,  // ["file_execution", "test_runner", "build_system", "general"]
    pub first_seen: u64,       // Unix timestamp of first registration
    pub start_time: u64,       // Unix timestamp of current session start
}
```

Identity persistence:
- Stored at `.velocity/drone_identity.json`
- `load_or_create()` attempts to load persisted identity, falling back to fresh generation
- Port is updated on each restart; ID is stable across restarts

---

## DroneCore — Core Logic

### DroneCore (`core.rs`)

The central coordinator for drone operations:

```rust
pub struct DroneCore {
    identity: DroneIdentity,
    workspace: PathBuf,
    // File transfer state, task queue, deployment registry
}
```

**Key operations**:

| Operation | Description |
|-----------|-------------|
| File Transfer | Receive files from peers, store in workspace |
| Task Execution | Execute received tasks (build, test, run) |
| Deployment | Deploy artifacts to target locations |
| Status Reporting | Report execution results to requesting peer |

### Task Execution Model

Tasks received via HTTP are executed in the drone's workspace:
1. Parse task request from HTTP body
2. Validate task against drone capabilities
3. Execute task (file operation, build command, test runner)
4. Capture output and exit status
5. Return structured result via HTTP response

---

## HTTP Server

### Server Implementation (`server.rs`)

Built entirely on `std::net::TcpListener` — no external HTTP framework dependency:

```rust
struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}
```

**Security constraints**:
- Maximum request body size: 16 MB (`MAX_BODY_SIZE`)
- Oversized requests rejected early before body read
- Connection close after each response (HTTP/1.0 style)

### Protocol Endpoints

Per `DRONE_PROTOCOL.md`, the drone implements these HTTP endpoints:

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/identity` | Return drone identity JSON |
| GET | `/status` | Return current status and capabilities |
| POST | `/execute` | Execute a task in the workspace |
| POST | `/upload` | Receive file from peer |
| GET | `/download` | Retrieve file from workspace |
| POST | `/deploy` | Deploy artifact to target |

---

## Safety Module

### SafeMutex (`safety.rs`)

Panic-safe mutex wrapper ensuring poisoned locks don't crash the drone:

```rust
pub struct SafeMutex<T> {
    inner: Mutex<T>,
}
```

Provides `with()` method that automatically recovers from poisoned mutex state by replacing the inner value.

---

## Dependencies

The drone has minimal dependencies for portability:

| Dependency | Purpose |
|------------|---------|
| `serde` + `serde_json` | JSON serialization for protocol messages |
| `sha2` | Identity hash generation |
| `base64` | File transfer encoding |

No async runtime, no HTTP framework, no TLS — the drone is designed to be deployable as a single static binary.

---

## Use Cases

1. **Remote Build Agent**: Deploy drone on CI machine, IDE sends build tasks
2. **Cross-Device Testing**: Drone on test machine executes test suites
3. **Distributed Execution**: Multiple drones collaborate on large tasks
4. **Lightweight Agent**: Drone as minimal agent endpoint without IDE overhead

---

## See Also

- [Multi-Agent Task Orchestrator](multi_agent_orchestrator.md) — How the IDE orchestrates drone tasks
- [System Overview](../architecture/system_overview.md) — Full workspace architecture
