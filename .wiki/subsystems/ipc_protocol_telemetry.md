# IPC, Protocol & Telemetry

_Inter-process communication via shared memory, MCP wire protocols (JSON-RPC + NMCP binary), and production telemetry collection._

---

## Overview

The `velocity-mcp/src/ipc/` and `velocity-mcp/src/protocol/` modules provide the communication infrastructure between the IDE host process and the MCP server, plus production observability via structured telemetry.

---

## IPC: Shared Memory (`ipc/shmem.rs`, 289 lines)

A cross-process shared memory buffer for zero-copy IPC between the IDE and MCP server.

### Memory Layout

```
Offset 0:        State byte (0=Idle, 1=Host Request, 2=Processing, 3=Response Ready, 4=Error)
Offset 1..5:     Input buffer length (u32 LE)
Offset 5..9:     Output buffer length (u32 LE)
Offset 10..4096: Input request buffer (4KB)
Offset 4096..65536: Output response buffer (61KB)
Total: 65536 bytes (64KB)
```

### Platform Support

- **Windows**: Uses `CreateEventW` / `SetEvent` / `WaitForSingleObject` for native event signaling (no polling)
- **Non-Windows**: Falls back to `memmap2` memory-mapped file with sleep-based polling

### State Machine

```
Idle → Host Request → Server Processing → Response Ready → Idle
                                          ↘ Error
```

---

## IPC: Telemetry (`ipc/telemetry.rs`, 488 lines)

Production telemetry and metrics collection:

### Metric Types

```rust
pub enum Metric {
    Counter { name: String, value: u64 },
    Gauge { name: String, value: f64 },
    Histogram { name: String, values: Vec<f64> },
}
```

### Structured Logging

```rust
pub enum LogLevel { Trace, Debug, Info, Warn, Error }

pub struct LogEvent {
    pub timestamp: u64,
    pub level: LogLevel,
    pub message: String,
    pub module: Option<String>,
    pub fields: HashMap<String, serde_json::Value>,
}
```

### Features

- **Global singleton**: `TelemetryCollector` via `OnceLock`
- **Performance spans**: Timing instrumentation for critical paths
- **File-based export**: JSON telemetry dump for offline analysis
- **Thread-safe**: Atomic counters + Mutex-protected collections

### `ipc/telemetry_share.rs`

Shared-memory telemetry transport — extends the shmem buffer to carry telemetry events from the MCP server back to the IDE host for display.

---

## Protocol: JSON-RPC (`protocol/json_rpc.rs`, 113 lines)

Standard MCP protocol over stdio:

```rust
pub fn run_stdio_loop() -> Result<(), Box<dyn Error>>
```

- Reads newline-delimited JSON-RPC 2.0 requests from stdin
- Dispatches to `registry::call_tool()` for `tools/call`
- Returns tool list for `tools/list`
- Handles `initialize` handshake (protocol version `2024-11-05`)
- Server identifies as `velocity-mcp-rust-server v1.0.0`

---

## Protocol: NMCP Binary (`protocol/nmcp_binary.rs`, 149 lines)

Shared-memory binary protocol variant for lower-latency IPC:

```rust
pub fn run_shmem_loop(buffer_path: &str) -> Result<(), Box<dyn Error>>
```

- Opens the shared memory buffer at the given path
- Blocks on `wait_for_request()` (native event on Windows)
- Reads binary JSON-RPC from the input region
- Dispatches to `registry::call_tool()`
- Writes response to the output region
- Signals response ready via event

### Dual Protocol Support

The MCP server supports two transport modes:
1. **stdio** (`run_stdio_loop`): Standard JSON-RPC over stdin/stdout — used by external MCP clients
2. **shared memory** (`run_shmem_loop`): Binary JSON-RPC over 64KB shared memory — used by the IDE host for lower latency

---

## Key Design Decisions

- **64KB shared memory**: Fixed-size buffer avoids dynamic allocation in the IPC path
- **Native events on Windows**: `CreateEventW`/`WaitForSingleObject` for zero-CPU blocking waits
- **Dual transport**: stdio for compatibility, shmem for performance
- **Global telemetry**: `OnceLock` singleton ensures exactly one collector per process
- **Structured logging**: JSON-formatted events with module tags and arbitrary fields

---

## See Also

- [MCP Tool Registry](mcp_tool_registry.md) — Tool dispatch target for both protocols
- [System Overview](../architecture/system_overview.md) — IPC topology and thread model
- [Multi-Agent Task Orchestrator](multi_agent_orchestrator.md) — Consumer of IPC-delivered tasks
