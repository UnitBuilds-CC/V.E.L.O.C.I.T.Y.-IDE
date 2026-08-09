#!/usr/bin/env python3
"""
Velocity Drone — Lightweight portable agent endpoint for cross-device collaboration.

This is a minimal implementation of the V.E.L.O.C.I.T.Y. peer protocol that can
be deployed on any machine without requiring the full IDE. It uses only Python
standard library modules, making it trivially portable.

Usage:
    python velocity_drone.py                          # Start with defaults
    python velocity_drone.py --port 9191 --name "My Drone"
    python velocity_drone.py --workspace /path/to/workdir

Requirements:
    Python 3.8+ (no external dependencies)

Protocol:
    See DRONE_PROTOCOL.md for the full specification.
"""

import argparse
import base64
import hashlib
import json
import os
import platform
import subprocess
import sys
import threading
import time
import uuid
from http.server import HTTPServer, BaseHTTPRequestHandler
from pathlib import Path
from typing import Any, Dict, List, Optional

# ── Drone Identity ──

DRONE_VERSION = "1.0.0"


def get_environment() -> str:
    """Get a description of the current environment."""
    return f"{platform.system().lower()}-{platform.machine()} ({platform.python_version()})"


def generate_drone_id() -> str:
    """Generate a unique drone ID."""
    return f"drone_{uuid.uuid4().hex[:16]}"


class DroneIdentity:
    """The drone's identity and configuration."""

    def __init__(self, name: str, port: int, workspace: Path):
        self.id = generate_drone_id()
        self.name = name
        self.port = port
        self.workspace = workspace
        self.environment = get_environment()
        self.capabilities = [
            "file_execution",
            "test_runner",
            "build_system",
            "general",
        ]
        self.first_seen = int(time.time())
        self.last_seen = int(time.time())
        self.start_time = int(time.time())

        # Try to load persisted identity.
        self._load()

    def _identity_path(self) -> Path:
        return self.workspace / ".velocity" / "drone_identity.json"

    def _load(self):
        """Load identity from disk if it exists."""
        path = self._identity_path()
        if path.exists():
            try:
                data = json.loads(path.read_text())
                self.id = data.get("id", self.id)
                self.name = data.get("name", self.name)
                self.capabilities = data.get("capabilities", self.capabilities)
                self.first_seen = data.get("first_seen", self.first_seen)
            except (json.JSONDecodeError, OSError):
                pass

    def save(self):
        """Persist identity to disk."""
        path = self._identity_path()
        path.parent.mkdir(parents=True, exist_ok=True)
        data = {
            "id": self.id,
            "name": self.name,
            "port": self.port,
            "capabilities": self.capabilities,
            "first_seen": self.first_seen,
            "environment": self.environment,
        }
        path.write_text(json.dumps(data, indent=2))

    def to_dict(self) -> Dict[str, Any]:
        return {
            "id": self.id,
            "name": self.name,
            "host": "0.0.0.0",
            "port": self.port,
            "version": DRONE_VERSION,
            "environment": self.environment,
            "capabilities": self.capabilities,
            "first_seen": self.first_seen,
            "last_seen": int(time.time()),
            "online": True,
        }


# ── File Transfer Manager ──


class FileTransfer:
    """Tracks an in-progress file transfer."""

    def __init__(
        self,
        transfer_id: str,
        filename: str,
        total_size: int,
        sha256: str,
        total_chunks: int,
        instructions: Optional[str],
        save_dir: Path,
    ):
        self.transfer_id = transfer_id
        self.filename = filename
        self.total_size = total_size
        self.sha256 = sha256
        self.total_chunks = total_chunks
        self.instructions = instructions
        self.save_dir = save_dir
        self.chunks: Dict[int, bytes] = {}
        self.complete = False
        self.started_at = int(time.time())

    @property
    def temp_path(self) -> Path:
        return self.save_dir / f"{self.transfer_id}.partial"

    @property
    def dest_path(self) -> Path:
        return self.save_dir / self.filename

    def receive_chunk(self, index: int, data: bytes) -> bool:
        """Receive a chunk. Returns True if accepted."""
        if 0 <= index < self.total_chunks:
            self.chunks[index] = data
            return True
        return False

    def is_complete(self) -> bool:
        return len(self.chunks) >= self.total_chunks

    def assemble(self) -> bytes:
        """Assemble all chunks into the complete file."""
        parts = []
        for i in range(self.total_chunks):
            if i not in self.chunks:
                raise ValueError(f"Missing chunk {i}")
            parts.append(self.chunks[i])
        return b"".join(parts)

    def verify(self, data: bytes) -> bool:
        """Verify the assembled file against the expected hash."""
        actual = hashlib.sha256(data).hexdigest()
        return actual == self.sha256


# ── Task Manager ──


class Task:
    """A task delegated to the drone."""

    def __init__(
        self,
        task_id: str,
        prompt: str,
        instructions: str,
        attached_files: List[str],
    ):
        self.task_id = task_id
        self.prompt = prompt
        self.instructions = instructions
        self.attached_files = attached_files
        self.status = "pending"  # pending, running, completed, failed
        self.progress = 0.0
        self.result: Optional[Dict[str, Any]] = None
        self.error: Optional[str] = None
        self.created_at = int(time.time())
        self.completed_at: Optional[int] = None

    def execute(self, workspace: Path):
        """Execute the task as a shell command."""
        self.status = "running"
        self.progress = 10.0

        try:
            # The instructions are the command to run.
            cmd = self.instructions
            self.progress = 30.0

            result = subprocess.run(
                cmd,
                shell=True,
                cwd=str(workspace),
                capture_output=True,
                text=True,
                timeout=600,  # 10 minute timeout
            )

            self.progress = 100.0
            self.status = "completed" if result.returncode == 0 else "failed"
            self.result = {
                "exit_code": result.returncode,
                "stdout": result.stdout[:10000],  # Cap output
                "stderr": result.stderr[:5000],
            }
            if result.returncode != 0:
                self.error = f"Exit code {result.returncode}"
            self.completed_at = int(time.time())

        except subprocess.TimeoutExpired:
            self.status = "failed"
            self.error = "Task timed out (600s)"
            self.completed_at = int(time.time())
        except Exception as e:
            self.status = "failed"
            self.error = str(e)
            self.completed_at = int(time.time())

    def to_dict(self) -> Dict[str, Any]:
        return {
            "task_id": self.task_id,
            "prompt": self.prompt,
            "status": self.status,
            "progress": self.progress,
            "result": self.result,
            "error": self.error,
            "created_at": self.created_at,
            "completed_at": self.completed_at,
        }


# ── Drone Core ──


class DroneCore:
    """Core drone logic shared across request handlers."""

    def __init__(self, identity: DroneIdentity):
        self.identity = identity
        self.transfers: Dict[str, FileTransfer] = {}
        self.tasks: Dict[str, Task] = {}
        self.messages: List[Dict[str, Any]] = []
        self.paired_peers: Dict[str, Dict[str, Any]] = {}
        self._lock = threading.Lock()

        # Ensure workspace directories exist.
        drops_dir = identity.workspace / ".velocity" / "drops"
        drops_dir.mkdir(parents=True, exist_ok=True)

    @property
    def drops_dir(self) -> Path:
        return self.identity.workspace / ".velocity" / "drops"

    def handle_pair(self, peer_id: str, name: str) -> Dict[str, Any]:
        with self._lock:
            self.paired_peers[peer_id] = {
                "id": peer_id,
                "name": name,
                "paired_at": int(time.time()),
            }
        return {
            "accepted": True,
            "drone_id": self.identity.id,
            "drone_name": self.identity.name,
        }

    def handle_message(self, msg: Dict[str, Any]) -> Dict[str, Any]:
        with self._lock:
            self.messages.append(msg)
            # Keep only last 200 messages.
            if len(self.messages) > 200:
                self.messages = self.messages[-200:]
        return {"received": True, "message_id": msg.get("id", "")}

    def handle_file_start(self, data: Dict[str, Any]) -> Dict[str, Any]:
        transfer = FileTransfer(
            transfer_id=data["transfer_id"],
            filename=data["filename"],
            total_size=data.get("total_size", 0),
            sha256=data.get("sha256", ""),
            total_chunks=data.get("total_chunks", 1),
            instructions=data.get("instructions"),
            save_dir=self.drops_dir,
        )
        with self._lock:
            self.transfers[transfer.transfer_id] = transfer
        return {
            "accepted": True,
            "transfer_id": transfer.transfer_id,
            "save_path": str(transfer.temp_path),
        }

    def handle_file_chunk(self, data: Dict[str, Any]) -> Dict[str, Any]:
        tid = data["transfer_id"]
        index = data["index"]
        chunk_data = base64.b64decode(data["data"])

        with self._lock:
            transfer = self.transfers.get(tid)
            if not transfer:
                return {"error": f"Unknown transfer {tid}"}
            ok = transfer.receive_chunk(index, chunk_data)

        return {"received": ok, "index": index}

    def handle_file_complete(self, data: Dict[str, Any]) -> Dict[str, Any]:
        tid = data["transfer_id"]

        with self._lock:
            transfer = self.transfers.get(tid)
            if not transfer:
                return {"error": f"Unknown transfer {tid}"}

            if not transfer.is_complete():
                return {
                    "complete": False,
                    "error": f"Missing chunks: {transfer.total_chunks - len(transfer.chunks)}",
                }

            # Assemble the file.
            try:
                file_data = transfer.assemble()
            except ValueError as e:
                return {"complete": False, "error": str(e)}

            # Verify hash.
            verified = transfer.verify(file_data) if transfer.sha256 else True

            # Save to destination.
            transfer.dest_path.parent.mkdir(parents=True, exist_ok=True)
            transfer.dest_path.write_bytes(file_data)

            # Execute deployment instructions.
            deploy_result = {"deployed": True, "dest_path": str(transfer.dest_path)}
            if transfer.instructions:
                exec_output = self._execute_deploy_instructions(
                    transfer.instructions, str(transfer.dest_path)
                )
                deploy_result["execution_output"] = exec_output

            transfer.complete = True
            return {
                "complete": True,
                "verified": verified,
                "deploy_result": deploy_result,
            }

    def handle_task(self, data: Dict[str, Any]) -> Dict[str, Any]:
        task = Task(
            task_id=data.get("task_id", f"task_{int(time.time())}"),
            prompt=data.get("prompt", ""),
            instructions=data.get("instructions", ""),
            attached_files=data.get("attached_files", []),
        )

        with self._lock:
            self.tasks[task.task_id] = task

        # Execute in a background thread.
        thread = threading.Thread(
            target=task.execute,
            args=(self.identity.workspace,),
            daemon=True,
        )
        thread.start()

        return {"accepted": True, "task_id": task.task_id, "status": "pending"}

    def handle_task_status(self, task_id: str) -> tuple:
        """Returns (status_code, response_dict)."""
        with self._lock:
            task = self.tasks.get(task_id)
        if not task:
            return 404, {"error": f"Unknown task {task_id}"}
        return 200, task.to_dict()

    def _execute_deploy_instructions(
        self, instructions: str, file_path: str
    ) -> str:
        """Execute deployment instructions line by line."""
        output = []
        for line in instructions.strip().split("\n"):
            line = line.strip()
            if not line or line.startswith("#"):
                continue

            if line.startswith("run "):
                cmd = line[4:].replace("{file}", file_path)
                output.append(f"[run] {cmd}")
                try:
                    result = subprocess.run(
                        cmd,
                        shell=True,
                        cwd=str(self.identity.workspace),
                        capture_output=True,
                        text=True,
                        timeout=120,
                    )
                    if result.stdout.strip():
                        output.append(f"  stdout: {result.stdout.strip()}")
                    if result.stderr.strip():
                        output.append(f"  stderr: {result.stderr.strip()}")
                    output.append(f"  exit: {result.returncode}")
                except Exception as e:
                    output.append(f"  error: {e}")

            elif line.startswith("copy "):
                dest = line[5:].replace("{file}", file_path)
                output.append(f"[copy] {file_path} -> {dest}")
                try:
                    import shutil
                    shutil.copy2(file_path, dest)
                    output.append(f"  copied successfully")
                except Exception as e:
                    output.append(f"  error: {e}")

            elif line.startswith("notify "):
                msg = line[7:]
                output.append(f"[notify] {msg}")

            else:
                output.append(f"[unknown] {line}")

        return "\n".join(output)


# ── HTTP Handler ──


class DroneHTTPHandler(BaseHTTPRequestHandler):
    """HTTP request handler for the drone API."""

    core: DroneCore  # Set by the server.

    def log_message(self, format, *args):
        """Suppress default logging to stderr."""
        pass

    def _send_json(self, status: int, data: Dict[str, Any]):
        body = json.dumps(data).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _read_body(self) -> Dict[str, Any]:
        length = int(self.headers.get("Content-Length", 0))
        if length == 0:
            return {}
        raw = self.rfile.read(length)
        return json.loads(raw)

    def do_GET(self):
        if self.path == "/peer/health":
            uptime = int(time.time()) - self.core.identity.start_time
            self._send_json(200, {
                "status": "ok",
                "id": self.core.identity.id,
                "name": self.core.identity.name,
                "version": DRONE_VERSION,
                "environment": self.core.identity.environment,
                "uptime_secs": uptime,
                "capabilities": self.core.identity.capabilities,
            })

        elif self.path == "/peer/identity":
            self._send_json(200, self.core.identity.to_dict())

        elif self.path.startswith("/peer/task/") and self.path.endswith("/status"):
            task_id = self.path.split("/")[3]
            status_code, result = self.core.handle_task_status(task_id)
            self._send_json(status_code, result)

        else:
            self._send_json(404, {"error": "Not found"})

    def do_POST(self):
        try:
            data = self._read_body()
        except (json.JSONDecodeError, ValueError):
            self._send_json(400, {"error": "Invalid JSON"})
            return

        if self.path == "/peer/pair":
            result = self.core.handle_pair(
                data.get("peer_id", ""),
                data.get("name", "unknown"),
            )
            self._send_json(200, result)

        elif self.path == "/peer/message":
            result = self.core.handle_message(data)
            self._send_json(200, result)

        elif self.path == "/peer/file/start":
            result = self.core.handle_file_start(data)
            self._send_json(200, result)

        elif self.path == "/peer/file/chunk":
            result = self.core.handle_file_chunk(data)
            self._send_json(200, result)

        elif self.path == "/peer/file/complete":
            result = self.core.handle_file_complete(data)
            self._send_json(200, result)

        elif self.path == "/peer/task":
            result = self.core.handle_task(data)
            self._send_json(200, result)

        else:
            self._send_json(404, {"error": "Not found"})


# ── Server ──


class DroneServer:
    """The drone HTTP server."""

    def __init__(self, core: DroneCore, host: str = "0.0.0.0", port: int = 9191):
        self.core = core
        self.host = host
        self.port = port

        # Create handler class with reference to core.
        handler_class = type(
            "BoundDroneHandler",
            (DroneHTTPHandler,),
            {"core": core},
        )

        self.server = HTTPServer((host, port), handler_class)
        self.server.timeout = 1  # Allow periodic checks for shutdown.

    def start(self):
        """Start serving (blocking)."""
        self.core.identity.save()
        print(f"Velocity Drone '{self.core.identity.name}' listening on {self.host}:{self.port}")
        print(f"  ID: {self.core.identity.id}")
        print(f"  Environment: {self.core.identity.environment}")
        print(f"  Workspace: {self.core.identity.workspace}")
        print(f"  Capabilities: {', '.join(self.core.identity.capabilities)}")
        print("Press Ctrl+C to stop.")
        try:
            self.server.serve_forever()
        except KeyboardInterrupt:
            print("\nShutting down...")
            self.server.shutdown()

    def start_background(self) -> threading.Thread:
        """Start serving in a background thread. Returns the thread."""
        self.core.identity.save()
        thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        thread.start()
        time.sleep(0.2)  # Give server time to bind.
        return thread

    def stop(self):
        """Stop the server."""
        self.server.shutdown()


# ── CLI Entry Point ──


def main():
    parser = argparse.ArgumentParser(
        description="Velocity Drone — Lightweight agent endpoint",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  python velocity_drone.py
  python velocity_drone.py --port 9191 --name "Test Drone"
  python velocity_drone.py --workspace /home/user/drone-workspace
        """,
    )
    parser.add_argument("--port", type=int, default=9191, help="Port to listen on (default: 9191)")
    parser.add_argument("--name", type=str, default=None, help="Drone name (default: hostname)")
    parser.add_argument(
        "--workspace",
        type=str,
        default=None,
        help="Workspace directory (default: .velocity_drone)",
    )
    parser.add_argument(
        "--capabilities",
        type=str,
        nargs="+",
        default=None,
        help="Advertised capabilities",
    )

    args = parser.parse_args()

    # Determine workspace.
    workspace = Path(args.workspace) if args.workspace else Path.cwd() / ".velocity_drone"
    workspace.mkdir(parents=True, exist_ok=True)

    # Determine name.
    name = args.name or platform.node() or "velocity-drone"

    # Create identity.
    identity = DroneIdentity(name=name, port=args.port, workspace=workspace)

    # Override capabilities if specified.
    if args.capabilities:
        identity.capabilities = args.capabilities

    # Create core and server.
    core = DroneCore(identity)
    server = DroneServer(core, port=args.port)

    # Start.
    server.start()


if __name__ == "__main__":
    main()
