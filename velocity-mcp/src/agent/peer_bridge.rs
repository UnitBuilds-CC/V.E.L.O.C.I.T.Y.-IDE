//! Bridge between the cross-device peer system and the local coordination bus.
//!
//! Translates [`PeerMessage`]s from remote agents into [`AgentBroadcast`]s on
//! the local bus, and vice versa. This lets remote peers participate in the
//! same coordination loop as local agents — their messages appear as if from
//! a local agent, and local broadcasts are forwarded to interested peers.

use crate::agent::coordination::{AgentBroadcast, CoordinationBus};
use crate::agent::peer_link::{PeerManager, PeerMessage, PeerMessageKind};
use std::path::PathBuf;

/// A bridge that synchronizes the peer messaging layer with the local
/// coordination bus. Call `pump_inbound` each tick to forward remote peer
/// messages as local broadcasts, and `pump_outbound` to forward local
/// broadcasts to remote peers.
pub struct PeerBridge {
    /// The local coordination bus.
    bus: CoordinationBus,
    /// The peer manager (cloned handle).
    peer_mgr: PeerManager,
}

impl PeerBridge {
    /// Create a new bridge connecting the given bus and peer manager.
    pub fn new(bus: CoordinationBus, peer_mgr: PeerManager) -> Self {
        Self { bus, peer_mgr }
    }

    /// Forward incoming peer messages to the local coordination bus.
    ///
    /// Call this each UI tick. Consumes messages from the peer inbox and
    /// translates them into `AgentBroadcast` variants.
    pub fn pump_inbound(&mut self) -> usize {
        let mut count = 0;

        // Drain the inbox by taking ownership of current messages.
        let messages: Vec<PeerMessage> = self.peer_mgr.inbox.drain(..).collect();

        for msg in &messages {
            let broadcast = match &msg.kind {
                PeerMessageKind::Chat => {
                    // Remote agent sent a text message → surface as a help request
                    // so the user sees it in the orchestration activity feed.
                    let text = msg
                        .payload
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(empty)");
                    Some(AgentBroadcast::HelpRequested {
                        from: format!("peer:{}", msg.from),
                        to: "local".to_string(),
                        task: format!("[peer chat] {}", text),
                    })
                }
                PeerMessageKind::TaskRequest => {
                    let prompt = msg
                        .payload
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(no prompt)");
                    Some(AgentBroadcast::HelpRequested {
                        from: format!("peer:{}", msg.from),
                        to: "local".to_string(),
                        task: format!("[peer task] {}", prompt),
                    })
                }
                PeerMessageKind::TaskProgress => {
                    let percent = msg
                        .payload
                        .get("percent")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    let status = msg
                        .payload
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("working");
                    Some(AgentBroadcast::ProgressReported {
                        agent_id: format!("peer:{}", msg.from),
                        percent,
                        status: status.to_string(),
                    })
                }
                PeerMessageKind::TaskComplete => {
                    let summary = msg
                        .payload
                        .get("result")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "task completed".to_string());
                    Some(AgentBroadcast::AgentFinished {
                        agent_id: format!("peer:{}", msg.from),
                        summary,
                    })
                }
                PeerMessageKind::FileTransferComplete => {
                    let filename = msg
                        .payload
                        .get("filename")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    Some(AgentBroadcast::FileClaimed {
                        agent_id: format!("peer:{}", msg.from),
                        path: PathBuf::from(filename),
                    })
                }
                // Heartbeat, pairing, status — no bus action needed.
                PeerMessageKind::Heartbeat
                | PeerMessageKind::PairRequest
                | PeerMessageKind::PairAccepted
                | PeerMessageKind::PairRejected
                | PeerMessageKind::TaskFailed
                | PeerMessageKind::FileTransferStart
                | PeerMessageKind::FileTransferChunk
                | PeerMessageKind::StatusRequest
                | PeerMessageKind::StatusResponse => None,
            };

            if let Some(bcast) = broadcast {
                self.bus.broadcast(bcast);
                count += 1;
            }
        }

        count
    }

    /// Forward local coordination broadcasts to remote peers.
    ///
    /// Call this each UI tick. Drains the bus and sends relevant events
    /// to connected peers.
    pub fn pump_outbound(&mut self) -> usize {
        let broadcasts = self.bus.drain();
        let mut count = 0;

        let online_peers: Vec<String> = self
            .peer_mgr
            .online_peers()
            .into_iter()
            .map(|p| p.id.clone())
            .collect();

        for bcast in &broadcasts {
            match bcast {
                AgentBroadcast::ProgressReported {
                    agent_id,
                    percent,
                    status,
                } => {
                    // Forward progress to all online peers.
                    for peer_id in &online_peers {
                        self.peer_mgr.send_message(
                            peer_id,
                            PeerMessageKind::TaskProgress,
                            serde_json::json!({
                                "agent_id": agent_id,
                                "percent": percent,
                                "status": status,
                            }),
                        );
                        count += 1;
                    }
                }
                AgentBroadcast::AgentFinished { agent_id, summary } => {
                    for peer_id in &online_peers {
                        self.peer_mgr.send_message(
                            peer_id,
                            PeerMessageKind::TaskComplete,
                            serde_json::json!({
                                "agent_id": agent_id,
                                "result": summary,
                            }),
                        );
                        count += 1;
                    }
                }
                AgentBroadcast::FileClaimed { agent_id, path } => {
                    for peer_id in &online_peers {
                        self.peer_mgr.send_message(
                            peer_id,
                            PeerMessageKind::StatusResponse,
                            serde_json::json!({
                                "event": "file_claimed",
                                "agent_id": agent_id,
                                "path": path.to_string_lossy(),
                            }),
                        );
                        count += 1;
                    }
                }
                AgentBroadcast::FileReleased { agent_id, path } => {
                    for peer_id in &online_peers {
                        self.peer_mgr.send_message(
                            peer_id,
                            PeerMessageKind::StatusResponse,
                            serde_json::json!({
                                "event": "file_released",
                                "agent_id": agent_id,
                                "path": path.to_string_lossy(),
                            }),
                        );
                        count += 1;
                    }
                }
                AgentBroadcast::HelpRequested { from, to, task } => {
                    // If the target is a peer, forward it.
                    if let Some(peer_id) = to.strip_prefix("peer:") {
                        self.peer_mgr.send_message(
                            peer_id,
                            PeerMessageKind::TaskRequest,
                            serde_json::json!({
                                "from": from,
                                "task": task,
                            }),
                        );
                        count += 1;
                    }
                }
            }
        }

        count
    }

    /// Get a reference to the underlying coordination bus.
    pub fn bus(&self) -> &CoordinationBus {
        &self.bus
    }

    /// Get a mutable reference to the peer manager.
    pub fn peer_manager_mut(&mut self) -> &mut PeerManager {
        &mut self.peer_mgr
    }

    /// Get a reference to the peer manager.
    pub fn peer_manager(&self) -> &PeerManager {
        &self.peer_mgr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bridge() -> PeerBridge {
        let bus = CoordinationBus::new();
        let mut mgr = PeerManager::new();
        mgr.init(std::path::Path::new("/tmp"), "TestHost");
        PeerBridge::new(bus, mgr)
    }

    #[test]
    fn pump_inbound_chat_becomes_help_request() {
        let mut bridge = make_bridge();

        // Simulate an incoming chat message.
        bridge.peer_manager_mut().inbox.push(PeerMessage {
            id: "msg1".into(),
            from: "remote-1".into(),
            to: "local".into(),
            kind: PeerMessageKind::Chat,
            payload: serde_json::json!({"text": "Hello from remote!"}),
            timestamp: 1000,
            acknowledged: false,
        });

        let forwarded = bridge.pump_inbound();
        assert_eq!(forwarded, 1);

        // The bus should have a HelpRequested broadcast.
        let broadcasts = bridge.bus().drain();
        assert_eq!(broadcasts.len(), 1);
        match &broadcasts[0] {
            AgentBroadcast::HelpRequested { from, task, .. } => {
                assert!(from.contains("remote-1"));
                assert!(task.contains("Hello from remote!"));
            }
            _ => panic!("Expected HelpRequested broadcast"),
        }
    }

    #[test]
    fn pump_inbound_task_progress_becomes_progress() {
        let mut bridge = make_bridge();

        bridge.peer_manager_mut().inbox.push(PeerMessage {
            id: "msg2".into(),
            from: "remote-2".into(),
            to: "local".into(),
            kind: PeerMessageKind::TaskProgress,
            payload: serde_json::json!({"percent": 75.0, "status": "compiling"}),
            timestamp: 2000,
            acknowledged: false,
        });

        let forwarded = bridge.pump_inbound();
        assert_eq!(forwarded, 1);

        let broadcasts = bridge.bus().drain();
        assert_eq!(broadcasts.len(), 1);
        match &broadcasts[0] {
            AgentBroadcast::ProgressReported {
                agent_id,
                percent,
                status,
            } => {
                assert!(agent_id.contains("remote-2"));
                assert_eq!(*percent, 75.0);
                assert_eq!(status, "compiling");
            }
            _ => panic!("Expected ProgressReported broadcast"),
        }
    }

    #[test]
    fn pump_inbound_task_complete_becomes_agent_finished() {
        let mut bridge = make_bridge();

        bridge.peer_manager_mut().inbox.push(PeerMessage {
            id: "msg3".into(),
            from: "remote-3".into(),
            to: "local".into(),
            kind: PeerMessageKind::TaskComplete,
            payload: serde_json::json!({"result": "all tests passed"}),
            timestamp: 3000,
            acknowledged: false,
        });

        let forwarded = bridge.pump_inbound();
        assert_eq!(forwarded, 1);

        let broadcasts = bridge.bus().drain();
        match &broadcasts[0] {
            AgentBroadcast::AgentFinished { agent_id, summary } => {
                assert!(agent_id.contains("remote-3"));
                assert!(summary.contains("all tests passed"));
            }
            _ => panic!("Expected AgentFinished broadcast"),
        }
    }

    #[test]
    fn pump_inbound_heartbeat_ignored() {
        let mut bridge = make_bridge();

        bridge.peer_manager_mut().inbox.push(PeerMessage {
            id: "msg4".into(),
            from: "remote-4".into(),
            to: "local".into(),
            kind: PeerMessageKind::Heartbeat,
            payload: serde_json::json!({}),
            timestamp: 4000,
            acknowledged: false,
        });

        let forwarded = bridge.pump_inbound();
        assert_eq!(forwarded, 0);
    }

    #[test]
    fn pump_outbound_progress_to_peers() {
        let mut bridge = make_bridge();

        // Add a fake online peer.
        let peer = crate::agent::peer_link::PeerIdentity {
            id: "peer-1".into(),
            name: "Remote PC".into(),
            host: "10.0.0.1".into(),
            port: 9191,
            auth_secret_handle: None,
            first_seen: 0,
            last_seen: crate::agent::peer_link::now_secs(),
            online: true,
            capabilities: vec![],
            environment: None,
        };
        bridge.peer_manager_mut().add_peer(peer);

        // Simulate a local progress broadcast.
        bridge.bus().report_progress("local-agent", 50.0, "halfway");

        let forwarded = bridge.pump_outbound();
        assert_eq!(forwarded, 1);

        // The peer manager should have a TaskProgress message in the outbox.
        let outbox = &bridge.peer_manager().outbox;
        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].kind, PeerMessageKind::TaskProgress);
        assert_eq!(outbox[0].to, "peer-1");
    }

    #[test]
    fn pump_outbound_help_to_peer() {
        let mut bridge = make_bridge();

        let peer = crate::agent::peer_link::PeerIdentity {
            id: "peer-2".into(),
            name: "Test PC".into(),
            host: "10.0.0.2".into(),
            port: 9191,
            auth_secret_handle: None,
            first_seen: 0,
            last_seen: crate::agent::peer_link::now_secs(),
            online: true,
            capabilities: vec![],
            environment: None,
        };
        bridge.peer_manager_mut().add_peer(peer);

        // Request help targeting a peer.
        bridge
            .bus()
            .request_help("local-agent", "peer:peer-2", "fix the build");

        let forwarded = bridge.pump_outbound();
        assert_eq!(forwarded, 1);

        let outbox = &bridge.peer_manager().outbox;
        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].kind, PeerMessageKind::TaskRequest);
        assert_eq!(outbox[0].to, "peer-2");
    }

    #[test]
    fn pump_outbound_no_online_peers() {
        let mut bridge = make_bridge();

        // No peers online — broadcasts should be dropped silently.
        bridge
            .bus()
            .report_progress("local-agent", 10.0, "starting");

        let forwarded = bridge.pump_outbound();
        assert_eq!(forwarded, 0);
    }

    #[test]
    fn bidirectional_flow() {
        let mut bridge = make_bridge();

        // Remote sends a chat → becomes a local broadcast.
        bridge.peer_manager_mut().inbox.push(PeerMessage {
            id: "in1".into(),
            from: "remote-A".into(),
            to: "local".into(),
            kind: PeerMessageKind::Chat,
            payload: serde_json::json!({"text": "need help testing"}),
            timestamp: 5000,
            acknowledged: false,
        });

        let in_count = bridge.pump_inbound();
        assert_eq!(in_count, 1);

        // Local agent responds with progress → forwarded to remote.
        bridge
            .bus()
            .report_progress("local-agent", 25.0, "starting tests");

        // Add an online peer for outbound.
        let peer = crate::agent::peer_link::PeerIdentity {
            id: "remote-A".into(),
            name: "Remote A".into(),
            host: "10.0.0.5".into(),
            port: 9191,
            auth_secret_handle: None,
            first_seen: 0,
            last_seen: crate::agent::peer_link::now_secs(),
            online: true,
            capabilities: vec![],
            environment: None,
        };
        bridge.peer_manager_mut().add_peer(peer);

        let out_count = bridge.pump_outbound();
        assert!(out_count >= 1);
    }
}
