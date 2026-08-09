#!/usr/bin/env python3
"""Tests for the Velocity Drone — verifies all protocol endpoints."""

import base64
import hashlib
import json
import os
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.error import URLError, HTTPError

# Add parent dir to path so we can import the drone.
sys.path.insert(0, str(Path(__file__).parent))
from velocity_drone import DroneCore, DroneIdentity, DroneServer, DRONE_VERSION

# ── Shared server setup ──

_shared_tmpdir = None
_shared_workspace = None
_shared_port = 19191
_shared_core = None
_shared_server = None


def _ensure_server():
    """Start the shared drone server once."""
    global _shared_tmpdir, _shared_workspace, _shared_core, _shared_server
    if _shared_server is not None:
        return

    _shared_tmpdir = tempfile.mkdtemp(prefix="drone_test_")
    _shared_workspace = Path(_shared_tmpdir)

    identity = DroneIdentity(name="TestDrone", port=_shared_port, workspace=_shared_workspace)
    _shared_core = DroneCore(identity)
    _shared_server = DroneServer(_shared_core, port=_shared_port)
    _shared_server.start_background()


def _stop_server():
    """Stop the shared drone server."""
    global _shared_server
    if _shared_server is not None:
        _shared_server.stop()
        _shared_server = None


# ── Helper functions ──

def http_get(path: str) -> dict:
    url = f"http://localhost:{_shared_port}{path}"
    req = Request(url, method="GET")
    with urlopen(req, timeout=5) as resp:
        return json.loads(resp.read())


def http_post(path: str, data: dict) -> dict:
    url = f"http://localhost:{_shared_port}{path}"
    body = json.dumps(data).encode("utf-8")
    req = Request(url, data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    with urlopen(req, timeout=5) as resp:
        return json.loads(resp.read())


# ── Tests ──


class TestHealthEndpoint(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        _ensure_server()

    def test_health_returns_ok(self):
        result = http_get("/peer/health")
        self.assertEqual(result["status"], "ok")
        self.assertIn("id", result)
        self.assertIn("name", result)
        self.assertEqual(result["version"], DRONE_VERSION)
        self.assertIn("capabilities", result)
        self.assertIsInstance(result["capabilities"], list)

    def test_health_has_uptime(self):
        result = http_get("/peer/health")
        self.assertIn("uptime_secs", result)
        self.assertGreaterEqual(result["uptime_secs"], 0)


class TestIdentityEndpoint(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        _ensure_server()

    def test_identity_returns_full_info(self):
        result = http_get("/peer/identity")
        self.assertEqual(result["name"], "TestDrone")
        self.assertIn("id", result)
        self.assertIn("port", result)
        self.assertIn("environment", result)
        self.assertTrue(result["online"])


class TestPairEndpoint(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        _ensure_server()

    def test_pair_accepted(self):
        result = http_post("/peer/pair", {
            "peer_id": "test_ide_1",
            "name": "Test IDE",
        })
        self.assertTrue(result["accepted"])
        self.assertIn("drone_id", result)
        self.assertIn("drone_name", result)

    def test_pair_stored(self):
        http_post("/peer/pair", {"peer_id": "test_ide_2", "name": "IDE 2"})
        self.assertIn("test_ide_2", _shared_core.paired_peers)


class TestMessageEndpoint(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        _ensure_server()

    def test_message_received(self):
        result = http_post("/peer/message", {
            "id": "msg_test_1",
            "from": "ide_1",
            "kind": "Chat",
            "payload": {"text": "Hello drone!"},
        })
        self.assertTrue(result["received"])
        self.assertEqual(result["message_id"], "msg_test_1")

    def test_messages_stored(self):
        initial = len(_shared_core.messages)
        http_post("/peer/message", {
            "id": "msg_test_2",
            "from": "ide_1",
            "kind": "Chat",
            "payload": {"text": "Test"},
        })
        self.assertEqual(len(_shared_core.messages), initial + 1)


class TestFileTransfer(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        _ensure_server()

    def test_full_transfer(self):
        """Test complete file transfer: start -> chunks -> complete."""
        file_content = b"Hello, this is a test file for the drone!"
        sha256 = hashlib.sha256(file_content).hexdigest()
        chunk_data = base64.b64encode(file_content).decode()

        # Start transfer.
        start = http_post("/peer/file/start", {
            "transfer_id": "xfer_test_1",
            "filename": "test_file.txt",
            "total_size": len(file_content),
            "sha256": sha256,
            "total_chunks": 1,
            "instructions": "notify File received successfully",
        })
        self.assertTrue(start["accepted"])

        # Send chunk.
        chunk_resp = http_post("/peer/file/chunk", {
            "transfer_id": "xfer_test_1",
            "index": 0,
            "data": chunk_data,
        })
        self.assertTrue(chunk_resp["received"])

        # Complete.
        complete = http_post("/peer/file/complete", {
            "transfer_id": "xfer_test_1",
        })
        self.assertTrue(complete["complete"])
        self.assertTrue(complete["verified"])
        self.assertIn("deploy_result", complete)
        self.assertTrue(complete["deploy_result"]["deployed"])
        self.assertIn("[notify] File received successfully",
                       complete["deploy_result"]["execution_output"])

    def test_multi_chunk_transfer(self):
        """Test transfer with multiple chunks."""
        content = b"A" * 300 + b"B" * 300 + b"C" * 300
        sha256 = hashlib.sha256(content).hexdigest()

        http_post("/peer/file/start", {
            "transfer_id": "xfer_multi",
            "filename": "multi.bin",
            "total_size": 900,
            "sha256": sha256,
            "total_chunks": 3,
        })

        for i in range(3):
            chunk = content[i * 300:(i + 1) * 300]
            http_post("/peer/file/chunk", {
                "transfer_id": "xfer_multi",
                "index": i,
                "data": base64.b64encode(chunk).decode(),
            })

        result = http_post("/peer/file/complete", {
            "transfer_id": "xfer_multi",
        })
        self.assertTrue(result["complete"])
        self.assertTrue(result["verified"])

    def test_incomplete_transfer_rejected(self):
        """Completing with missing chunks should fail."""
        http_post("/peer/file/start", {
            "transfer_id": "xfer_incomplete",
            "filename": "incomplete.bin",
            "total_size": 300,
            "sha256": "abc",
            "total_chunks": 3,
        })
        # Only send 1 of 3 chunks.
        http_post("/peer/file/chunk", {
            "transfer_id": "xfer_incomplete",
            "index": 0,
            "data": base64.b64encode(b"chunk").decode(),
        })

        result = http_post("/peer/file/complete", {
            "transfer_id": "xfer_incomplete",
        })
        self.assertFalse(result["complete"])


class TestTaskExecution(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        _ensure_server()

    def test_task_execution(self):
        """Test task delegation and execution."""
        result = http_post("/peer/task", {
            "task_id": "task_test_1",
            "prompt": "Echo test",
            "instructions": "echo hello from drone",
        })
        self.assertTrue(result["accepted"])
        self.assertEqual(result["task_id"], "task_test_1")

        # Wait for task to complete.
        status = None
        for _ in range(20):
            time.sleep(0.3)
            status = http_get("/peer/task/task_test_1/status")
            if status["status"] in ("completed", "failed"):
                break

        self.assertIsNotNone(status)
        self.assertEqual(status["status"], "completed")
        self.assertIn("hello from drone", status["result"]["stdout"])
        self.assertEqual(status["result"]["exit_code"], 0)

    def test_task_failure(self):
        """Test task that fails."""
        result = http_post("/peer/task", {
            "task_id": "task_fail",
            "prompt": "Fail test",
            "instructions": "exit 1",
        })
        self.assertTrue(result["accepted"])

        status = None
        for _ in range(20):
            time.sleep(0.3)
            status = http_get("/peer/task/task_fail/status")
            if status["status"] in ("completed", "failed"):
                break

        self.assertIsNotNone(status)
        self.assertEqual(status["status"], "failed")
        self.assertIsNotNone(status["error"])

    def test_unknown_task_status(self):
        """Querying an unknown task returns 404."""
        try:
            http_get("/peer/task/nonexistent/status")
            self.fail("Should have raised HTTPError")
        except HTTPError as e:
            self.assertEqual(e.code, 404)


class TestDroneIdentity(unittest.TestCase):
    def test_identity_creation(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            identity = DroneIdentity("TestName", 9191, Path(tmpdir))
            self.assertEqual(identity.name, "TestName")
            self.assertEqual(identity.port, 9191)
            self.assertTrue(identity.id.startswith("drone_"))
            self.assertIn("file_execution", identity.capabilities)

    def test_identity_persistence(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            ws = Path(tmpdir)
            id1 = DroneIdentity("Original", 9191, ws)
            id1.save()

            # Create a new identity from the same workspace.
            id2 = DroneIdentity("New", 9191, ws)
            self.assertEqual(id2.id, id1.id)  # Should load persisted ID.
            self.assertEqual(id2.name, "Original")  # Should load persisted name.

    def test_to_dict(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            identity = DroneIdentity("DictTest", 9191, Path(tmpdir))
            d = identity.to_dict()
            self.assertIn("id", d)
            self.assertIn("name", d)
            self.assertEqual(d["name"], "DictTest")
            self.assertTrue(d["online"])


class TestDeployInstructions(unittest.TestCase):
    def test_notify_instruction(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            ws = Path(tmpdir)
            identity = DroneIdentity("Test", 9191, ws)
            core = DroneCore(identity)
            output = core._execute_deploy_instructions(
                "notify Hello World", "/tmp/file.txt"
            )
            self.assertIn("[notify] Hello World", output)

    def test_comments_ignored(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            ws = Path(tmpdir)
            identity = DroneIdentity("Test", 9191, ws)
            core = DroneCore(identity)
            output = core._execute_deploy_instructions(
                "# This is a comment\nnotify After comment", "/tmp/file.txt"
            )
            self.assertNotIn("[unknown]", output)
            self.assertIn("[notify] After comment", output)


if __name__ == "__main__":
    try:
        unittest.main(verbosity=2)
    finally:
        _stop_server()
