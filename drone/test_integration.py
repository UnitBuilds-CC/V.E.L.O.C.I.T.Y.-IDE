#!/usr/bin/env python3
"""
Integration test: V.E.L.O.C.I.T.Y. IDE ↔ Drone workflow.

This test demonstrates the full cross-device collaboration pipeline:
1. Start a drone on a simulated "remote machine"
2. Discover the drone via health check
3. Pair with the drone
4. Transfer a file to the drone
5. Delegate a task to the drone
6. Monitor task progress and collect results
7. Verify the complete workflow

Run: python test_integration.py
"""

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
from urllib.error import HTTPError

sys.path.insert(0, str(Path(__file__).parent))
from velocity_drone import DroneCore, DroneIdentity, DroneServer


class TestFullIDEDroneWorkflow(unittest.TestCase):
    """Simulates a complete IDE ↔ Drone collaboration scenario."""

    @classmethod
    def setUpClass(cls):
        """Start two drones simulating two remote machines."""
        cls.tmpdir1 = tempfile.mkdtemp(prefix="drone_integ_1_")
        cls.tmpdir2 = tempfile.mkdtemp(prefix="drone_integ_2_")
        cls.port1 = 19200
        cls.port2 = 19201

        # Drone 1: "Build machine"
        id1 = DroneIdentity(
            name="BuildMachine",
            port=cls.port1,
            workspace=Path(cls.tmpdir1),
        )
        id1.capabilities = ["build_system", "file_execution", "test_runner"]
        cls.core1 = DroneCore(id1)
        cls.server1 = DroneServer(cls.core1, port=cls.port1)
        cls.server1.start_background()

        # Drone 2: "Test machine"
        id2 = DroneIdentity(
            name="TestMachine",
            port=cls.port2,
            workspace=Path(cls.tmpdir2),
        )
        id2.capabilities = ["test_runner", "general"]
        cls.core2 = DroneCore(id2)
        cls.server2 = DroneServer(cls.core2, port=cls.port2)
        cls.server2.start_background()

        time.sleep(0.3)

    @classmethod
    def tearDownClass(cls):
        cls.server1.stop()
        cls.server2.stop()

    def _get(self, port, path):
        url = f"http://localhost:{port}{path}"
        with urlopen(url, timeout=5) as resp:
            return json.loads(resp.read())

    def _post(self, port, path, data):
        url = f"http://localhost:{port}{path}"
        body = json.dumps(data).encode("utf-8")
        req = Request(url, data=body, method="POST")
        req.add_header("Content-Type", "application/json")
        with urlopen(req, timeout=5) as resp:
            return json.loads(resp.read())

    def test_full_collaboration_workflow(self):
        """
        Scenario: Developer IDE coordinates build on Drone 1, then
        deploys the built artifact to Drone 2 for testing.

        Steps:
        1. Health check both drones
        2. Pair with both drones
        3. Send a "build script" to Drone 1
        4. Drone 1 "builds" and reports completion
        5. Transfer the "built artifact" from Drone 1 to Drone 2
        6. Delegate test execution to Drone 2
        7. Collect test results
        """

        # ── Step 1: Discover drones via health check ──
        health1 = self._get(self.port1, "/peer/health")
        health2 = self._get(self.port2, "/peer/health")

        self.assertEqual(health1["status"], "ok")
        self.assertEqual(health2["status"], "ok")
        self.assertEqual(health1["name"], "BuildMachine")
        self.assertEqual(health2["name"], "TestMachine")
        self.assertIn("build_system", health1["capabilities"])
        self.assertIn("test_runner", health2["capabilities"])

        # ── Step 2: Pair with both drones ──
        pair1 = self._post(self.port1, "/peer/pair", {
            "peer_id": "ide_developer_1",
            "name": "Developer IDE",
        })
        pair2 = self._post(self.port2, "/peer/pair", {
            "peer_id": "ide_developer_1",
            "name": "Developer IDE",
        })

        self.assertTrue(pair1["accepted"])
        self.assertTrue(pair2["accepted"])

        # ── Step 3: Send build task to Drone 1 ──
        build_task = self._post(self.port1, "/peer/task", {
            "task_id": "build_001",
            "prompt": "Build the project",
            "instructions": "echo BUILD_SUCCESS",
        })
        self.assertTrue(build_task["accepted"])

        # Wait for build to complete.
        for _ in range(20):
            time.sleep(0.3)
            build_status = self._get(self.port1, "/peer/task/build_001/status")
            if build_status["status"] in ("completed", "failed"):
                break

        self.assertEqual(build_status["status"], "completed")
        self.assertIn("BUILD_SUCCESS", build_status["result"]["stdout"])

        # ── Step 4: Transfer "built artifact" to Drone 2 ──
        artifact_content = b"ELF_BINARY_MOCK_CONTENTS_" + b"\x00" * 100
        sha256 = hashlib.sha256(artifact_content).hexdigest()
        chunk = base64.b64encode(artifact_content).decode()

        transfer_start = self._post(self.port2, "/peer/file/start", {
            "transfer_id": "deploy_001",
            "filename": "app_binary",
            "total_size": len(artifact_content),
            "sha256": sha256,
            "total_chunks": 1,
            "instructions": "notify Application deployed to test machine",
        })
        self.assertTrue(transfer_start["accepted"])

        self._post(self.port2, "/peer/file/chunk", {
            "transfer_id": "deploy_001",
            "index": 0,
            "data": chunk,
        })

        deploy_result = self._post(self.port2, "/peer/file/complete", {
            "transfer_id": "deploy_001",
        })
        self.assertTrue(deploy_result["complete"])
        self.assertTrue(deploy_result["verified"])
        self.assertTrue(deploy_result["deploy_result"]["deployed"])

        # ── Step 5: Delegate test execution to Drone 2 ──
        test_task = self._post(self.port2, "/peer/task", {
            "task_id": "test_001",
            "prompt": "Run integration tests on the deployed application",
            "instructions": "echo Running tests... && echo 42 passed, 0 failed",
        })
        self.assertTrue(test_task["accepted"])

        # Wait for tests to complete.
        for _ in range(20):
            time.sleep(0.3)
            test_status = self._get(self.port2, "/peer/task/test_001/status")
            if test_status["status"] in ("completed", "failed"):
                break

        self.assertEqual(test_status["status"], "completed")
        self.assertIn("42 passed, 0 failed", test_status["result"]["stdout"])

        # ── Step 6: Send chat message between drones ──
        self._post(self.port1, "/peer/message", {
            "id": "chat_001",
            "from": "ide_developer_1",
            "kind": "Chat",
            "payload": {"text": "Build complete, deploying to test machine"},
        })

        self._post(self.port2, "/peer/message", {
            "id": "chat_002",
            "from": "ide_developer_1",
            "kind": "Chat",
            "payload": {"text": "Tests passed! Ready for production."},
        })

        # Verify messages were received.
        self.assertEqual(len(self.core1.messages), 1)
        self.assertEqual(len(self.core2.messages), 1)
        self.assertEqual(
            self.core1.messages[0]["payload"]["text"],
            "Build complete, deploying to test machine",
        )
        self.assertEqual(
            self.core2.messages[0]["payload"]["text"],
            "Tests passed! Ready for production.",
        )

    def test_multi_chunk_large_file_transfer(self):
        """Test transferring a larger file in multiple chunks."""
        # Simulate a 10KB file split into 4 chunks.
        file_data = bytes(range(256)) * 40  # 10240 bytes
        sha256 = hashlib.sha256(file_data).hexdigest()

        chunk_size = len(file_data) // 4
        chunks = [
            file_data[i * chunk_size:(i + 1) * chunk_size]
            for i in range(4)
        ]

        # Start transfer.
        self._post(self.port1, "/peer/file/start", {
            "transfer_id": "large_xfer",
            "filename": "large_artifact.bin",
            "total_size": len(file_data),
            "sha256": sha256,
            "total_chunks": 4,
        })

        # Send chunks.
        for i, chunk in enumerate(chunks):
            resp = self._post(self.port1, "/peer/file/chunk", {
                "transfer_id": "large_xfer",
                "index": i,
                "data": base64.b64encode(chunk).decode(),
            })
            self.assertTrue(resp["received"])

        # Complete and verify.
        result = self._post(self.port1, "/peer/file/complete", {
            "transfer_id": "large_xfer",
        })
        self.assertTrue(result["complete"])
        self.assertTrue(result["verified"])

    def test_concurrent_tasks_on_multiple_drones(self):
        """Test running tasks concurrently on both drones."""
        # Send tasks to both drones simultaneously.
        self._post(self.port1, "/peer/task", {
            "task_id": "concurrent_1",
            "prompt": "Task on build machine",
            "instructions": "echo build_task_done",
        })
        self._post(self.port2, "/peer/task", {
            "task_id": "concurrent_2",
            "prompt": "Task on test machine",
            "instructions": "echo test_task_done",
        })

        # Wait for both to complete.
        results = {}
        for _ in range(30):
            time.sleep(0.3)
            try:
                s1 = self._get(self.port1, "/peer/task/concurrent_1/status")
                s2 = self._get(self.port2, "/peer/task/concurrent_2/status")
                if s1["status"] == "completed":
                    results["build"] = s1
                if s2["status"] == "completed":
                    results["test"] = s2
                if "build" in results and "test" in results:
                    break
            except Exception:
                continue

        self.assertIn("build", results)
        self.assertIn("test", results)
        self.assertIn("build_task_done", results["build"]["result"]["stdout"])
        self.assertIn("test_task_done", results["test"]["result"]["stdout"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
