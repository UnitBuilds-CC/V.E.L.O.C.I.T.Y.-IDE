//! Cross-device peer collaboration panel.
//!
//! Shows connected peers, allows adding new peers, chat messaging,
//! file transfer initiation, task delegation, and peer server control.

use eframe::egui;
use egui::RichText;

use crate::agent::peer_link::{PeerCapability, TaskStatus, TransferDirection};
use crate::agent::peer_server::{PeerServer, PeerServerConfig};
use crate::editor::app::velocity_app::struct_def::VelocityApp;

impl VelocityApp {
    /// Render the cross-device peer collaboration panel.
    pub fn render_peer_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let peer_count = self.peer_manager.peers.len();
        let online = self.peer_manager.online_peers().len();
        let active_xfers = self.peer_manager.transfers.len();
        let active_tasks = self.peer_manager.active_tasks().len();

        Self::tier3_header(
            ui,
            "Peers",
            &format!(
                "{} peer(s) \u{00b7} {} online \u{00b7} {} transfer(s) \u{00b7} {} task(s)",
                peer_count, online, active_xfers, active_tasks
            ),
            palette.accent,
            palette.text_muted,
        );

        if !self.peer_status.is_empty() {
            ui.label(
                RichText::new(&self.peer_status)
                    .size(9.0)
                    .color(palette.text_muted),
            );
        }

        egui::ScrollArea::vertical()
            .id_salt("peer_panel_scroll")
            .show(ui, |ui| {
                // ── Server Control ──────────────────────────────────────────
                ui.add_space(4.0);
                ui.label(
                    RichText::new("PEER SERVER")
                        .small()
                        .strong()
                        .color(palette.accent),
                );

                let server_running = self.peer_server_running;
                ui.horizontal(|ui| {
                    let status_text = if server_running {
                        RichText::new(format!("Listening on :{}", self.peer_port))
                            .size(9.0)
                            .color(palette.success)
                    } else {
                        RichText::new("Stopped").size(9.0).color(palette.text_muted)
                    };
                    ui.label(status_text);

                    if server_running {
                        if ui
                            .small_button(RichText::new("Stop").size(9.0).color(palette.error))
                            .clicked()
                        {
                            self.peer_server_running = false;
                            self.peer_status = "Peer server stopped".into();
                        }
                    } else {
                        if ui
                            .small_button(RichText::new("Start").size(9.0).color(palette.success))
                            .clicked()
                        {
                            self.start_peer_server();
                        }
                    }

                    ui.label(RichText::new("Port:").size(9.0).color(palette.text_muted));
                    let port_edit = ui.add(
                        egui::TextEdit::singleline(&mut self.peer_port.to_string())
                            .desired_width(60.0)
                            .font(egui::TextStyle::Monospace),
                    );
                    if port_edit.changed() {
                        // Parse is handled below via the text buffer
                    }
                });

                ui.add_space(2.0);
                if let Some(id) = &self.peer_manager.local_identity {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("ID:").size(9.0).color(palette.text_muted));
                        ui.label(
                            RichText::new(&id.id)
                                .size(9.0)
                                .monospace()
                                .color(palette.text_muted),
                        );
                    });
                }

                // ── Add Peer ────────────────────────────────────────────────
                ui.add_space(8.0);
                ui.label(
                    RichText::new("ADD PEER")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Host:").size(9.0));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.peer_add_host)
                            .desired_width(120.0)
                            .hint_text("192.168.1.50")
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.label(RichText::new("Port:").size(9.0));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.peer_add_port)
                            .desired_width(50.0)
                            .hint_text("9191")
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.label(RichText::new("Name:").size(9.0));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.peer_add_name)
                            .desired_width(80.0)
                            .hint_text("Remote PC"),
                    );
                    if ui.button(RichText::new("Connect").size(9.0)).clicked() {
                        self.connect_new_peer();
                    }
                });

                // ── Peer List ───────────────────────────────────────────────
                ui.add_space(8.0);
                ui.label(
                    RichText::new("CONNECTED PEERS")
                        .small()
                        .strong()
                        .color(palette.accent),
                );

                let peers: Vec<crate::agent::peer_link::PeerIdentity> = self
                    .peer_manager
                    .list_peers()
                    .into_iter()
                    .cloned()
                    .collect();
                if peers.is_empty() {
                    ui.label(
                        RichText::new("No peers connected. Add a peer above to begin.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }

                for peer in &peers {
                    let status_color = if peer.online {
                        palette.success
                    } else {
                        palette.text_muted
                    };
                    let status_dot = if peer.online { "\u{25cf}" } else { "\u{25cb}" };

                    ui.horizontal(|ui| {
                        ui.label(RichText::new(status_dot).size(10.0).color(status_color));
                        ui.label(RichText::new(&peer.name).size(10.0).strong());
                        ui.label(
                            RichText::new(format!("{}:{}", peer.host, peer.port))
                                .size(9.0)
                                .monospace()
                                .color(palette.text_muted),
                        );
                        if let Some(env) = &peer.environment {
                            ui.label(RichText::new(env).size(8.0).color(palette.text_muted));
                        }
                    });

                    // Capabilities
                    if !peer.capabilities.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("caps:").size(8.0).color(palette.text_muted));
                            for cap in &peer.capabilities {
                                ui.label(
                                    RichText::new(cap.label()).size(8.0).color(palette.accent),
                                );
                            }
                        });
                    }

                    // Actions for this peer
                    ui.horizontal(|ui| {
                        if ui.small_button(RichText::new("Chat").size(8.0)).clicked() {
                            self.peer_chat_selected = Some(peer.id.clone());
                        }
                        if ui
                            .small_button(RichText::new("Health Check").size(8.0))
                            .clicked()
                        {
                            let host = peer.host.clone();
                            let port = peer.port;
                            match crate::agent::peer_server::peer_health_check(&host, port) {
                                Ok(_) => {
                                    self.peer_status = format!("{} is healthy", peer.name);
                                    if let Some(p) = self.peer_manager.peers.get_mut(&peer.id) {
                                        p.online = true;
                                    }
                                }
                                Err(e) => {
                                    self.peer_status = format!("{} unreachable: {}", peer.name, e);
                                    if let Some(p) = self.peer_manager.peers.get_mut(&peer.id) {
                                        p.online = false;
                                    }
                                }
                            }
                        }
                        if ui
                            .small_button(RichText::new("Remove").size(8.0).color(palette.error))
                            .clicked()
                        {
                            self.peer_manager.remove_peer(&peer.id);
                            self.peer_status = format!("Removed peer {}", peer.name);
                        }
                    });

                    ui.add_space(4.0);
                }

                // ── Peer Chat ───────────────────────────────────────────────
                ui.add_space(8.0);
                ui.label(
                    RichText::new("PEER CHAT")
                        .small()
                        .strong()
                        .color(palette.accent),
                );

                if let Some(selected_id) = &self.peer_chat_selected {
                    let selected_name = self
                        .peer_manager
                        .get_peer(selected_id)
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| selected_id.clone());

                    ui.label(
                        RichText::new(format!("Chatting with: {}", selected_name))
                            .size(9.0)
                            .color(palette.text_muted),
                    );

                    // Show recent messages with this peer
                    let msgs: Vec<_> = self
                        .peer_manager
                        .inbox
                        .iter()
                        .chain(self.peer_manager.outbox.iter())
                        .filter(|m| &m.to == selected_id || &m.from == selected_id)
                        .collect();

                    for msg in msgs.iter().take(20) {
                        let direction = if &msg.from == selected_id {
                            "\u{2190}"
                        } else {
                            "\u{2192}"
                        };
                        ui.label(
                            RichText::new(format!(
                                "{} [{}] {}",
                                direction, msg.timestamp, msg.payload
                            ))
                            .size(8.0)
                            .monospace()
                            .color(palette.text_muted),
                        );
                    }

                    ui.horizontal(|ui| {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.peer_chat_message)
                                .desired_width(200.0)
                                .hint_text("Type a message..."),
                        );
                        if ui.button("Send").clicked()
                            || (response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            if !self.peer_chat_message.is_empty() {
                                self.peer_manager.send_message(
                                    selected_id,
                                    crate::agent::peer_link::PeerMessageKind::Chat,
                                    serde_json::json!({ "text": self.peer_chat_message.clone() }),
                                );
                                self.peer_chat_message.clear();
                            }
                        }
                    });
                } else {
                    ui.label(
                        RichText::new("Select a peer to chat with.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }

                // ── Active Transfers ────────────────────────────────────────
                ui.add_space(8.0);
                ui.label(
                    RichText::new("ACTIVE TRANSFERS")
                        .small()
                        .strong()
                        .color(palette.accent),
                );

                let transfers: Vec<_> = self.peer_manager.transfers.values().collect();
                if transfers.is_empty() {
                    ui.label(
                        RichText::new("No active transfers.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
                for xfer in &transfers {
                    let dir_label = match xfer.direction {
                        TransferDirection::Outgoing => "\u{2191} OUT",
                        TransferDirection::Incoming => "\u{2193} IN",
                    };
                    let progress = if xfer.total_chunks > 0 {
                        xfer.chunks_received as f32 / xfer.total_chunks as f32
                    } else if xfer.total_size > 0 {
                        0.0
                    } else {
                        0.0
                    };

                    ui.horizontal(|ui| {
                        ui.label(RichText::new(dir_label).size(9.0).strong());
                        ui.label(RichText::new(&xfer.filename).size(9.0).monospace());
                        ui.label(
                            RichText::new(format!("{:.0}%", progress * 100.0))
                                .size(9.0)
                                .color(if xfer.complete {
                                    palette.success
                                } else {
                                    palette.warning
                                }),
                        );
                        if xfer.complete {
                            ui.label(RichText::new("\u{2713}").size(9.0).color(palette.success));
                        }
                    });
                }

                // ── Delegated Tasks ─────────────────────────────────────────
                ui.add_space(8.0);
                ui.label(
                    RichText::new("DELEGATED TASKS")
                        .small()
                        .strong()
                        .color(palette.accent),
                );

                let tasks: Vec<_> = self.peer_manager.tasks.values().collect();
                if tasks.is_empty() {
                    ui.label(
                        RichText::new("No active tasks.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
                for task in &tasks {
                    let status_color = match task.status {
                        TaskStatus::Pending => palette.text_muted,
                        TaskStatus::Running => palette.warning,
                        TaskStatus::Completed => palette.success,
                        TaskStatus::Failed => palette.error,
                        TaskStatus::Cancelled => palette.text_muted,
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&task.prompt).size(9.0).strong());
                        ui.label(
                            RichText::new(task.status.label())
                                .size(8.0)
                                .color(status_color),
                        );
                        ui.label(
                            RichText::new(format!("{:.0}%", task.progress))
                                .size(8.0)
                                .color(palette.text_muted),
                        );
                    });
                    if let Some(err) = &task.error {
                        ui.label(
                            RichText::new(format!("Error: {}", err))
                                .size(8.0)
                                .color(palette.error),
                        );
                    }
                }
            });
    }

    /// Start the peer API server on a background thread.
    pub fn start_peer_server(&mut self) {
        if self.peer_server_running {
            self.peer_status = "Peer server already running".into();
            return;
        }

        let port = self.peer_port;
        let config = PeerServerConfig {
            port,
            ..PeerServerConfig::default()
        };
        let server = PeerServer::new(config);
        let _running_flag = server.running_flag();

        // Clone the peer manager snapshot for the server thread.
        let peer_snapshot = self.peer_manager.clone();

        let handle = std::thread::Builder::new()
            .name("peer-api-server".into())
            .spawn(move || {
                if let Err(e) = server.start(&peer_snapshot) {
                    eprintln!("Peer server error: {}", e);
                }
            });

        match handle {
            Ok(_) => {
                self.peer_server_running = true;
                self.peer_manager.set_listen_port(port);
                self.peer_status = format!("Peer server started on port {}", port);
            }
            Err(e) => {
                self.peer_status = format!("Failed to start peer server: {}", e);
            }
        }
    }

    /// Connect to a new peer using the input fields.
    fn connect_new_peer(&mut self) {
        let host = self.peer_add_host.trim().to_string();
        let port: u16 = self.peer_add_port.trim().parse().unwrap_or(9191);
        let name = if self.peer_add_name.trim().is_empty() {
            format!("peer-{}", host)
        } else {
            self.peer_add_name.trim().to_string()
        };

        if host.is_empty() {
            self.peer_status = "Host is required".into();
            return;
        }

        // Try a health check first to verify reachability.
        match crate::agent::peer_server::peer_health_check(&host, port) {
            Ok(identity) => {
                let peer_name = identity
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&name)
                    .to_string();
                let peer_id = identity
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&host)
                    .to_string();

                let peer = crate::agent::peer_link::PeerIdentity {
                    id: peer_id.clone(),
                    name: peer_name.clone(),
                    host: host.clone(),
                    port,
                    auth_secret_handle: None,
                    first_seen: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    last_seen: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    online: true,
                    capabilities: vec![PeerCapability::General],
                    environment: identity
                        .get("environment")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                };
                self.peer_manager.add_peer(peer);

                // Send a pairing request.
                let _ = crate::agent::peer_server::request_pairing(
                    &host,
                    port,
                    self.peer_manager
                        .local_identity
                        .as_ref()
                        .map(|id| id.name.as_str())
                        .unwrap_or("velocity"),
                );

                self.peer_status = format!("Connected to {} at {}:{}", peer_name, host, port);
            }
            Err(e) => {
                // Add peer anyway (may come online later).
                let peer_id = format!("peer-{}", host.replace('.', "-"));
                let peer = crate::agent::peer_link::PeerIdentity {
                    id: peer_id.clone(),
                    name: name.clone(),
                    host: host.clone(),
                    port,
                    auth_secret_handle: None,
                    first_seen: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    last_seen: 0,
                    online: false,
                    capabilities: vec![PeerCapability::General],
                    environment: None,
                };
                self.peer_manager.add_peer(peer);
                self.peer_status = format!("Added {} (unreachable: {}). Will retry.", name, e);
            }
        }

        // Clear input fields.
        self.peer_add_host.clear();
        self.peer_add_port.clear();
        self.peer_add_name.clear();
    }
}
