# Velocity Drone Protocol Specification

## Overview

A **Velocity Drone** is a lightweight, portable agent endpoint that implements
the V.E.L.O.C.I.T.Y. peer-to-peer collaboration protocol. Drones can be deployed
on any machine, in any language, without requiring the full IDE. They serve as
remote execution endpoints for cross-device development and testing.

## Use Cases

- **E2E Testing**: Deploy drones on multiple machines to test multi-device applications
- **Remote Execution**: Run builds, tests, or commands on remote hardware
- **CI/CD Integration**: Drones as build agents in CI pipelines
- **Edge Computing**: Lightweight agent on IoT devices or edge servers

## Protocol

### Transport

- **HTTP/1.1** over TCP
- **JSON** request/response bodies
- Default port: **9191** (configurable)
- Content-Type: `application/json`

### Endpoints

#### `GET /peer/health`

Health check. Returns the drone's status.

**Response:**
```json
{
  "status": "ok",
  "id": "drone_abc123",
  "name": "My Drone",
  "version": "1.0.0",
  "environment": "linux-x86_64",
  "uptime_secs": 3600,
  "capabilities": ["file_execution", "test_runner", "build_system"]
}
```

#### `GET /peer/identity`

Get the drone's full identity.

**Response:**
```json
{
  "id": "drone_abc123",
  "name": "My Drone",
  "host": "192.168.1.50",
  "port": 9191,
  "version": "1.0.0",
  "environment": "linux-x86_64",
  "capabilities": ["file_execution", "test_runner"],
  "first_seen": 1700000000,
  "last_seen": 1700003600,
  "online": true
}
```

#### `POST /peer/pair`

Request pairing with the drone.

**Request:**
```json
{
  "peer_id": "ide_xyz789",
  "name": "Developer IDE"
}
```

**Response:**
```json
{
  "accepted": true,
  "drone_id": "drone_abc123",
  "drone_name": "My Drone"
}
```

#### `POST /peer/message`

Send a message to the drone.

**Request:**
```json
{
  "id": "msg_001",
  "from": "ide_xyz789",
  "kind": "Chat",
  "payload": {"text": "Hello drone!"}
}
```

**Response:**
```json
{
  "received": true,
  "message_id": "msg_001"
}
```

#### `POST /peer/file/start`

Begin a file transfer to the drone.

**Request:**
```json
{
  "transfer_id": "xfer_001",
  "filename": "app.exe",
  "total_size": 1048576,
  "sha256": "abc123...",
  "total_chunks": 16,
  "instructions": "run {file} --test"
}
```

**Response:**
```json
{
  "accepted": true,
  "transfer_id": "xfer_001",
  "save_path": "/tmp/velocity_drops/xfer_001"
}
```

#### `POST /peer/file/chunk`

Send a file chunk.

**Request:**
```json
{
  "transfer_id": "xfer_001",
  "index": 0,
  "data": "base64encodeddata..."
}
```

**Response:**
```json
{
  "received": true,
  "index": 0
}
```

#### `POST /peer/file/complete`

Signal file transfer completion.

**Request:**
```json
{
  "transfer_id": "xfer_001"
}
```

**Response:**
```json
{
  "complete": true,
  "verified": true,
  "deploy_result": {
    "deployed": true,
    "dest_path": "/workspace/drops/app.exe",
    "execution_output": "[run] app.exe --test\n  exit: 0\n"
  }
}
```

#### `POST /peer/task`

Delegate a task to the drone.

**Request:**
```json
{
  "task_id": "task_001",
  "prompt": "Run the test suite",
  "instructions": "Execute cargo test and report results",
  "attached_files": ["test_data.json"]
}
```

**Response:**
```json
{
  "accepted": true,
  "task_id": "task_001",
  "status": "pending"
}
```

#### `GET /peer/task/{task_id}/status`

Get task execution status.

**Response:**
```json
{
  "task_id": "task_001",
  "status": "completed",
  "progress": 100.0,
  "result": {
    "exit_code": 0,
    "stdout": "test result: ok. 42 passed\n",
    "stderr": ""
  }
}
```

### Message Kinds

| Kind | Direction | Description |
|------|-----------|-------------|
| `PairRequest` | IDE → Drone | Initiate pairing |
| `PairAccepted` | Drone → IDE | Pairing confirmed |
| `PairRejected` | Drone → IDE | Pairing denied |
| `Heartbeat` | Both | Keepalive ping |
| `Chat` | Both | Text message |
| `TaskRequest` | IDE → Drone | Delegate a task |
| `TaskProgress` | Drone → IDE | Progress update (0-100%) |
| `TaskComplete` | Drone → IDE | Task finished with result |
| `TaskFailed` | Drone → IDE | Task failed with error |
| `FileTransferStart` | IDE → Drone | Begin file transfer |
| `FileTransferChunk` | IDE → Drone | File data chunk (base64) |
| `FileTransferComplete` | IDE → Drone | All chunks sent |
| `StatusRequest` | IDE → Drone | Request current status |
| `StatusResponse` | Drone → IDE | Status information |

### Capabilities

Drones advertise their capabilities:

| Capability | Description |
|------------|-------------|
| `file_execution` | Can execute received files |
| `test_runner` | Can run test suites |
| `build_system` | Can build projects |
| `screen_capture` | Can capture screen images |
| `gui_automation` | Can interact with GUI elements |
| `network_monitor` | Can monitor network traffic |
| `general` | General-purpose execution |

### Deployment Instructions

When a file transfer completes, the drone can execute instructions. The
instruction format is line-based:

```
# Comment lines start with #
run {file} --test                    # Execute the file with args
copy {file} /opt/apps/latest.exe     # Copy to another location
notify Application deployed          # Log a notification
```

The `{file}` placeholder is replaced with the actual file path.

## Implementation Guidelines

### Minimal Requirements

To implement a Velocity Drone in any language, you need:

1. **HTTP server** — listen on a port, handle GET/POST requests
2. **JSON parsing** — serialize/deserialize JSON
3. **File I/O** — receive and save files
4. **Process execution** — run shell commands (for tasks and deployments)
5. **Base64** — encode/decode file chunks

### Reference Implementations

- **Python**: `velocity-workspace/drone/velocity_drone.py` (stdlib only)
- **Rust**: Built into V.E.L.O.C.I.T.Y. IDE peer server

### Porting Checklist

1. Implement all endpoints from this spec
2. Generate a unique drone ID on first run
3. Persist drone identity across restarts
4. Handle file transfers with chunk tracking
5. Execute deployment instructions after transfer completion
6. Run tasks asynchronously and report progress
7. Advertise capabilities in health/identity responses
8. Send periodic heartbeats to connected IDEs
