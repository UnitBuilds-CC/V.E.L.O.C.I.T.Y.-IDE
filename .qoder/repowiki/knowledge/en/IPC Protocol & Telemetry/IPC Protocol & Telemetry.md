# IPC Protocol & Telemetry

## Classification
- **Category**: Infrastructure / Communication
- **Files**: velocity-mcp/src/ipc/ (4 files), velocity-mcp/src/protocol/ (3 files)
- **Criticality**: High — IDE ↔ MCP server communication backbone

## Summary

Dual-transport MCP protocol (stdio JSON-RPC + 64KB shared memory binary) with production telemetry collection (counters, gauges, histograms, structured JSON logging).

## IPC Shared Memory Layout

```
Offset 0:        State byte (Idle/Request/Processing/Response/Error)
Offset 1..5:     Input length (u32 LE)
Offset 5..9:     Output length (u32 LE)
Offset 10..4096: Input buffer (4KB)
Offset 4096..65536: Output buffer (61KB)
```

## Protocol Modes

- **stdio** (`run_stdio_loop`): JSON-RPC 2.0 over stdin/stdout — external MCP clients
- **shared memory** (`run_shmem_loop`): Binary JSON-RPC over 64KB mmap — IDE host (lower latency)

## Telemetry

- `TelemetryCollector` global singleton via `OnceLock`
- Metrics: Counter, Gauge, Histogram
- Structured logging: JSON LogEvent with levels (Trace→Error)
- Performance spans for critical path timing
- File-based export for offline analysis
