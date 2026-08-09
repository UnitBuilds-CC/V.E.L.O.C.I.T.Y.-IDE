//! End-to-end integration test for cross-device agent collaboration.
//!
//! Simulates a complete workflow: two V.E.L.O.C.I.T.Y. instances discover each
//! other, pair up, transfer files, delegate tasks, exchange messages, and
//! coordinate through the bus — proving the concept works end-to-end.

#[cfg(test)]
mod e2e_tests {
    use crate::agent::coordination::{AgentBroadcast, CoordinationBus};
    use crate::agent::peer_bridge::PeerBridge;
    use crate::agent::peer_link::{
        PeerCapability, PeerIdentity, PeerManager, PeerMessageKind, TaskStatus,
    };
    use std::path::Path;

    /// Simulate the "Agent A on PC 1" instance.
    fn create_agent_a() -> PeerManager {
        let mut mgr = PeerManager::new();
        mgr.init(Path::new("/workspace/pc1"), "Agent-A-PC1");
        mgr.set_listen_port(9191);
        mgr
    }

    /// Simulate the "Agent B on PC 2" instance.
    fn create_agent_b() -> PeerManager {
        let mut mgr = PeerManager::new();
        mgr.init(Path::new("/workspace/pc2"), "Agent-B-PC2");
        mgr.set_listen_port(9192);
        mgr
    }

    /// Register peer identities on both sides (simulating a successful pairing).
    fn establish_pairing(a: &mut PeerManager, b: &mut PeerManager) {
        // Agent A knows about Agent B.
        let peer_b = PeerIdentity {
            id: "peer_b".into(),
            name: "Agent-B-PC2".into(),
            host: "192.168.1.50".into(),
            port: 9192,
            auth_secret_handle: None,
            first_seen: 1000,
            last_seen: 1000,
            online: true,
            capabilities: vec![
                PeerCapability::GuiAutomation,
                PeerCapability::ScreenCapture,
                PeerCapability::TestRunner,
            ],
            environment: Some("windows".into()),
        };
        a.add_peer(peer_b);

        // Agent B knows about Agent A.
        let peer_a = PeerIdentity {
            id: "peer_a".into(),
            name: "Agent-A-PC1".into(),
            host: "192.168.1.100".into(),
            port: 9191,
            auth_secret_handle: None,
            first_seen: 1000,
            last_seen: 1000,
            online: true,
            capabilities: vec![PeerCapability::BuildSystem, PeerCapability::FileExecution],
            environment: Some("windows".into()),
        };
        b.add_peer(peer_a);
    }

    // ──────────────────────────────────────────────────────────────────────
    // E2E Scenario: Remote Desktop App E2E Testing
    //
    // Agent A (PC 1 — developer machine): builds the server binary, sends it
    // to Agent B (PC 2 — remote machine), tells B to start the server, then
    // sends a client binary and tells B to run the client against the server.
    // B reports test results back to A.
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn e2e_remote_desktop_testing_workflow() {
        let mut agent_a = create_agent_a();
        let mut agent_b = create_agent_b();
        establish_pairing(&mut agent_a, &mut agent_b);

        // ── Step 1: Agent A sends the server binary to Agent B ─────────
        let server_binary = b"FAKE_SERVER_BINARY_CONTENT_12345";
        let xfer_id = agent_a
            .initiate_transfer(
                "peer_b",
                "remote_server.exe",
                server_binary,
                Some("run {file}\nnotify Server binary deployed and started"),
            )
            .unwrap();

        assert!(agent_a.transfers.contains_key(&xfer_id));
        // initiate_transfer queues: FileTransferStart + 1 chunk + FileTransferComplete = 3 msgs
        assert_eq!(agent_a.outbox.len(), 3);

        // Simulate the transfer arriving at Agent B.
        agent_b.begin_receive_transfer(
            &xfer_id,
            "peer_a",
            "remote_server.exe",
            server_binary.len() as u64,
            "fake_hash",
            1,
            Some("run {file}\nnotify Server binary deployed and started"),
        );
        assert!(agent_b.receive_chunk(&xfer_id, 0));
        assert!(agent_b.transfers[&xfer_id].complete);

        // Finalize the transfer on Agent B — deploy and execute instructions.
        let deploy_result = agent_b.finalize_transfer(&xfer_id);
        assert!(deploy_result.deployed);
        assert!(deploy_result.execution_output.is_some());
        let exec_output = deploy_result.execution_output.unwrap();
        assert!(exec_output.contains("[notify] Server binary deployed and started"));

        // ── Step 2: Agent A delegates a task to Agent B ────────────────
        let task_id = agent_a
            .delegate_task(
                "peer_b",
                "Start the remote desktop server",
                "Launch remote_server.exe and verify it's listening on port 5900",
                vec!["remote_server.exe".into()],
            )
            .unwrap();

        assert_eq!(agent_a.tasks[&task_id].status, TaskStatus::Pending);

        // Simulate Agent B receiving and executing the task.
        // (In reality this would arrive via the peer server inbox.)
        let task_request_msg = agent_a.outbox.last().unwrap();
        assert_eq!(task_request_msg.kind, PeerMessageKind::TaskRequest);
        assert_eq!(task_request_msg.to, "peer_b");

        // Agent B reports progress.
        agent_a.update_task_progress(&task_id, 50.0);
        assert_eq!(agent_a.tasks[&task_id].progress, 50.0);
        assert_eq!(agent_a.tasks[&task_id].status, TaskStatus::Running);

        // Agent B completes the task.
        agent_a.complete_task(
            &task_id,
            serde_json::json!({
                "status": "running",
                "port": 5900,
                "pid": 12345,
            }),
        );
        assert_eq!(agent_a.tasks[&task_id].status, TaskStatus::Completed);
        assert!(agent_a.tasks[&task_id].result.is_some());

        // ── Step 3: Agent A sends the client binary ────────────────────
        let client_binary = b"FAKE_CLIENT_BINARY_CONTENT_67890";
        let xfer2_id = agent_a
            .initiate_transfer(
                "peer_b",
                "remote_client.exe",
                client_binary,
                Some("run {file} --connect localhost:5900"),
            )
            .unwrap();

        // Agent B receives it.
        agent_b.begin_receive_transfer(
            &xfer2_id,
            "peer_a",
            "remote_client.exe",
            client_binary.len() as u64,
            "fake_hash2",
            1,
            Some("run {file} --connect localhost:5900"),
        );
        agent_b.receive_chunk(&xfer2_id, 0);
        let deploy2 = agent_b.finalize_transfer(&xfer2_id);
        assert!(deploy2.deployed);

        // ── Step 4: Agent B reports E2E test results back to Agent A ───
        agent_b.send_message(
            "peer_a",
            PeerMessageKind::Chat,
            serde_json::json!({
                "text": "E2E test complete: server started on port 5900, client connected successfully. Frame rendering OK, input latency < 50ms."
            }),
        );

        // Verify the message is in Agent B's outbox.
        let report_msg = agent_b.outbox.last().unwrap();
        assert_eq!(report_msg.kind, PeerMessageKind::Chat);
        assert!(report_msg.payload["text"]
            .as_str()
            .unwrap()
            .contains("E2E test complete"));

        // Simulate Agent A receiving the report.
        agent_a.inbox.push(report_msg.clone());

        // Verify Agent A's inbox has the message.
        assert_eq!(agent_a.inbox.len(), 1);
        assert_eq!(agent_a.inbox[0].kind, PeerMessageKind::Chat);
        let report_text = agent_a.inbox[0].payload["text"].as_str().unwrap();
        assert!(report_text.contains("E2E test complete"));
        assert!(report_text.contains("latency < 50ms"));

        // ── Step 5: Verify overall state ───────────────────────────────
        // Agent A: 1 completed task, 2 outgoing transfers.
        let completed: Vec<_> = agent_a
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Completed)
            .collect();
        assert_eq!(completed.len(), 1);
        assert_eq!(agent_a.transfers.len(), 2);

        // Agent B: 2 completed transfers with deployment.
        assert_eq!(agent_b.completed_transfers().len(), 2);

        // Both peers are online.
        assert_eq!(agent_a.online_peers().len(), 1);
        assert_eq!(agent_b.online_peers().len(), 1);
    }

    #[test]
    fn e2e_coordination_bus_bridge() {
        // Test that the PeerBridge correctly routes messages between the
        // peer system and the coordination bus.
        let bus = CoordinationBus::new();
        let mut mgr = create_agent_a();
        let peer = PeerIdentity {
            id: "remote_x".into(),
            name: "Remote-X".into(),
            host: "10.0.0.5".into(),
            port: 9191,
            auth_secret_handle: None,
            first_seen: 0,
            last_seen: crate::agent::peer_link::now_secs(),
            online: true,
            capabilities: vec![PeerCapability::TestRunner],
            environment: Some("linux".into()),
        };
        mgr.add_peer(peer);

        let mut bridge = PeerBridge::new(bus.clone(), mgr);

        // ── Inbound: Remote agent sends a chat message ─────────────────
        bridge
            .peer_manager_mut()
            .inbox
            .push(crate::agent::peer_link::PeerMessage {
                id: "msg_in_1".into(),
                from: "remote_x".into(),
                to: "local".into(),
                kind: PeerMessageKind::Chat,
                payload: serde_json::json!({"text": "Server is ready for testing"}),
                timestamp: 5000,
                acknowledged: false,
            });

        let inbound_count = bridge.pump_inbound();
        assert_eq!(inbound_count, 1);

        // The local bus should have a HelpRequested broadcast.
        let broadcasts = bus.drain();
        assert_eq!(broadcasts.len(), 1);
        match &broadcasts[0] {
            AgentBroadcast::HelpRequested { from, task, .. } => {
                assert!(from.contains("remote_x"));
                assert!(task.contains("Server is ready for testing"));
            }
            _ => panic!("Expected HelpRequested, got {:?}", broadcasts[0]),
        }

        // ── Outbound: Local agent reports progress ─────────────────────
        bus.report_progress("local-agent", 75.0, "running tests");

        let outbound_count = bridge.pump_outbound();
        assert_eq!(outbound_count, 1);

        // The peer manager should have a TaskProgress message queued.
        let outbox = &bridge.peer_manager().outbox;
        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].kind, PeerMessageKind::TaskProgress);
        assert_eq!(outbox[0].to, "remote_x");
        assert_eq!(outbox[0].payload["percent"].as_f64().unwrap(), 75.0);

        // ── Outbound: Local agent delegates to a peer ──────────────────
        bus.request_help("local-agent", "peer:remote_x", "Run the GUI test suite");

        let outbound_count = bridge.pump_outbound();
        assert_eq!(outbound_count, 1);

        let outbox = &bridge.peer_manager().outbox;
        assert_eq!(outbox.len(), 2); // Both progress + task request accumulated
        let last = outbox.last().unwrap();
        assert_eq!(last.kind, PeerMessageKind::TaskRequest);
        assert!(last.payload["task"]
            .as_str()
            .unwrap()
            .contains("GUI test suite"));
    }

    #[test]
    fn e2e_multi_peer_broadcast() {
        // Test that a local event is forwarded to ALL online peers.
        let bus = CoordinationBus::new();
        let mut mgr = PeerManager::new();
        mgr.init(Path::new("/tmp"), "Hub");

        // Add 3 online peers.
        for i in 0..3 {
            let peer = PeerIdentity {
                id: format!("peer_{}", i),
                name: format!("Worker-{}", i),
                host: format!("10.0.0.{}", i + 1),
                port: 9191,
                auth_secret_handle: None,
                first_seen: 0,
                last_seen: crate::agent::peer_link::now_secs(),
                online: true,
                capabilities: vec![PeerCapability::General],
                environment: None,
            };
            mgr.add_peer(peer);
        }

        let mut bridge = PeerBridge::new(bus.clone(), mgr);

        // Local agent finishes a task.
        bus.broadcast(AgentBroadcast::AgentFinished {
            agent_id: "local-builder".into(),
            summary: "Build completed successfully".into(),
        });

        let outbound = bridge.pump_outbound();
        assert_eq!(outbound, 3); // One message per online peer.

        // Verify all 3 peers received the message.
        let outbox = &bridge.peer_manager().outbox;
        assert_eq!(outbox.len(), 3);
        for msg in outbox {
            assert_eq!(msg.kind, PeerMessageKind::TaskComplete);
            assert!(msg.payload["result"]
                .as_str()
                .unwrap()
                .contains("Build completed"));
        }
    }

    #[test]
    fn e2e_file_transfer_with_deployment_pipeline() {
        // Test the full file transfer → deployment → instruction execution pipeline.
        let mut mgr = PeerManager::new();
        mgr.init(Path::new("/tmp/workspace"), "Deployer");

        let peer = PeerIdentity {
            id: "deploy_target".into(),
            name: "Deploy-Target".into(),
            host: "10.0.0.10".into(),
            port: 9191,
            auth_secret_handle: None,
            first_seen: 0,
            last_seen: crate::agent::peer_link::now_secs(),
            online: true,
            capabilities: vec![PeerCapability::FileExecution],
            environment: Some("linux".into()),
        };
        mgr.add_peer(peer);

        // Simulate receiving a multi-chunk file.
        let total_size = 300u64;
        let chunk_size = 100u64;
        let total_chunks = (total_size / chunk_size) as u32;

        mgr.begin_receive_transfer(
            "deploy_1",
            "deploy_target",
            "application.tar.gz",
            total_size,
            "abc123hash",
            total_chunks,
            Some("notify Application deployed\nnotify Ready to start"),
        );

        // Receive all chunks.
        for i in 0..total_chunks {
            assert!(mgr.receive_chunk("deploy_1", i));
        }

        // Transfer should be complete.
        assert!(mgr.transfers["deploy_1"].complete);
        assert_eq!(mgr.transfers["deploy_1"].chunks_received, total_chunks);

        // Finalize — deploy and execute.
        let result = mgr.finalize_transfer("deploy_1");
        assert!(result.deployed);
        assert!(result.dest_path.is_some());
        assert!(result.execution_output.is_some());

        let output = result.execution_output.unwrap();
        assert!(output.contains("[notify] Application deployed"));
        assert!(output.contains("[notify] Ready to start"));
        assert!(result.error.is_none());
    }

    #[test]
    fn e2e_peer_presence_and_heartbeat() {
        // Test that peer presence tracking works correctly.
        let mut mgr = PeerManager::new();
        mgr.init(Path::new("/tmp"), "Monitor");

        // Add peers with different last_seen times.
        let now = crate::agent::peer_link::now_secs();

        let recent = PeerIdentity {
            id: "recent".into(),
            name: "Recent".into(),
            host: "10.0.0.1".into(),
            port: 9191,
            auth_secret_handle: None,
            first_seen: now - 10,
            last_seen: now - 5, // 5 seconds ago
            online: true,
            capabilities: vec![],
            environment: None,
        };

        let stale = PeerIdentity {
            id: "stale".into(),
            name: "Stale".into(),
            host: "10.0.0.2".into(),
            port: 9191,
            auth_secret_handle: None,
            first_seen: now - 600,
            last_seen: now - 400, // 400 seconds ago
            online: true,
            capabilities: vec![],
            environment: None,
        };

        mgr.add_peer(recent);
        mgr.add_peer(stale);

        // Both start as online.
        assert_eq!(mgr.online_peers().len(), 2);

        // Update presence with a 300-second timeout.
        mgr.update_presence(300);

        // Only the recent peer should remain online.
        assert_eq!(mgr.online_peers().len(), 1);
        assert!(mgr.get_peer("recent").unwrap().online);
        assert!(!mgr.get_peer("stale").unwrap().online);
    }

    #[test]
    fn e2e_bidirectional_chat() {
        // Test a full chat exchange between two agents.
        let mut a = create_agent_a();
        let mut b = create_agent_b();
        establish_pairing(&mut a, &mut b);

        // A sends a message to B.
        a.send_message(
            "peer_b",
            PeerMessageKind::Chat,
            serde_json::json!({"text": "Hey B, are you ready?"}),
        );
        assert_eq!(a.outbox.len(), 1);

        // Simulate the message arriving at B.
        b.inbox.push(a.outbox.last().unwrap().clone());
        assert_eq!(b.inbox.len(), 1);
        assert_eq!(
            b.inbox[0].payload["text"].as_str().unwrap(),
            "Hey B, are you ready?"
        );

        // B responds.
        b.send_message(
            "peer_a",
            PeerMessageKind::Chat,
            serde_json::json!({"text": "Yes, ready to test!"}),
        );

        // Simulate the response arriving at A.
        a.inbox.push(b.outbox.last().unwrap().clone());
        assert_eq!(a.inbox.len(), 1);
        assert_eq!(
            a.inbox[0].payload["text"].as_str().unwrap(),
            "Yes, ready to test!"
        );

        // Both agents have clean communication state.
        assert_eq!(a.online_peers().len(), 1);
        assert_eq!(b.online_peers().len(), 1);
    }
}
