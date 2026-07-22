use crate::automation::mediator::MediatorArena;
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct MerkleGraphView {
    // Cache node positions to avoid layout jumping on redraw
    positions: HashMap<u64, Pos2>,
}

impl MerkleGraphView {
    pub fn new() -> Self {
        Self {
            positions: HashMap::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, workspace_root: &Path, mediator: &MediatorArena) {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("🌲 MERKLE SEMANTIC GRAPH EXPLORER").size(14.0).strong().color(Color32::from_rgb(34, 211, 238)));
            ui.label("Interactive visualization of declarations, method calls, and active edit locks using canonical workspace-relative path identities.");
            ui.separator();

            let sm = match crate::automation::open_workspace_site_map(workspace_root) {
                Ok(sm) => sm,
                Err(_) => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label("SiteMap database empty or offline. Run ast watcher first.");
                    });
                    return;
                }
            };

            // Query live triples only so the graph reflects the latest watcher snapshot state.
            let triples = sm.find_live_triples(None, None, None);
            if triples.is_empty() {
                ui.label("No semantic triples recorded in database.");
                return;
            }

            // Extract unique nodes
            let mut nodes = HashSet::new();
            for t in &triples {
                nodes.insert(t.subject_hash);
                nodes.insert(t.object_hash);
            }

            // Canvas sizes
            let mut canvas_size = ui.available_size_before_wrap();
            if !canvas_size.x.is_finite() { canvas_size.x = 600.0; }
            if !canvas_size.y.is_finite() { canvas_size.y = 400.0; }
            canvas_size.y = canvas_size.y.min(450.0);

            let (rect, response) = ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());
            let painter = ui.painter_at(rect);

            // Background card panel fill
            painter.rect_filled(rect, 4.0, Color32::from_rgb(8, 9, 14));

            // Position calculation (circle layout centered on canvas)
            let center = rect.center();
            let radius = (rect.width().min(rect.height()) * 0.35).max(100.0);
            let nodes_vec: Vec<u64> = nodes.into_iter().collect();
            let count = nodes_vec.len();

            for (idx, &node) in nodes_vec.iter().enumerate() {
                self.positions.entry(node).or_insert_with(|| {
                    let angle = (idx as f32 / count as f32) * 2.0 * std::f32::consts::PI;
                    Pos2::new(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    )
                });
            }

            // 1. Draw connection edges (call lines / dependencies)
            for t in &triples {
                if let (Some(&p1), Some(&p2)) = (self.positions.get(&t.subject_hash), self.positions.get(&t.object_hash)) {
                    let stroke_color = match t.predicate_id {
                        2 => Color32::from_rgb(168, 85, 247), // Call relations in purple
                        _ => Color32::from_rgb(33, 36, 51),  // Declare / other in slate border
                    };
                    painter.line_segment([p1, p2], Stroke::new(1.5, stroke_color));
                }
            }

            // 2. Draw nodes and check locks/hover status
            let hover_pos = response.hover_pos();
            let mut hovered_node = None;

            for &node in &nodes_vec {
                if let Some(&pos) = self.positions.get(&node) {
                    let matching_locks: Vec<_> = mediator
                        .active_locks()
                        .into_iter()
                        .filter(|lock| path_identity_hash(&lock.file_path) == node)
                        .collect();
                    let is_locked = !matching_locks.is_empty();
                    let is_conflict = matching_locks.len() > 1;

                    let node_color = if is_conflict {
                        Color32::from_rgb(248, 113, 113) // Conflict red
                    } else if is_locked {
                        Color32::from_rgb(250, 204, 21)  // Warning yellow
                    } else {
                        Color32::from_rgb(34, 211, 238)  // Neon Cyan
                    };

                    let border_color = Color32::from_rgb(226, 227, 243);

                    // Check hover
                    let dist = pos.distance(hover_pos.unwrap_or(Pos2::ZERO));
                    let circle_radius = if dist < 12.0 {
                        hovered_node = Some(node);
                        14.0 // Scale up on hover
                    } else {
                        10.0
                    };

                    painter.circle_filled(pos, circle_radius, node_color);
                    painter.circle_stroke(pos, circle_radius, Stroke::new(1.0, border_color));
                }
            }

            // Draw tooltip for hovered node
            if let Some(node) = hovered_node {
                if let Some(pos) = hover_pos {
                    let tooltip_text = format!("Merkle Node: 0x{:016x}\n(Hovered to inspect)", node);
                    painter.text(
                        pos + Vec2::new(10.0, -10.0),
                        egui::Align2::LEFT_BOTTOM,
                        tooltip_text,
                        egui::FontId::monospace(11.0),
                        Color32::from_rgb(226, 227, 243),
                    );
                }
            }
        });
    }
}

fn path_identity_hash(path: &Path) -> u64 {
    hash_str(&canonicalize_scope_path(path))
}

fn canonicalize_scope_path(path: &Path) -> String {
    let normalized = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().replace('\\', "/")),
            _ => None,
        })
        .collect::<Vec<_>>();
    normalized.join("/")
}

fn hash_str(s: &str) -> u64 {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_scope_path_distinguishes_same_named_files() {
        let left = canonicalize_scope_path(Path::new(r"src\auth\main.rs"));
        let right = canonicalize_scope_path(Path::new(r"src\ui\main.rs"));
        assert_ne!(left, right);
        assert_eq!(left, "src/auth/main.rs");
        assert_eq!(right, "src/ui/main.rs");
    }

    #[test]
    fn path_identity_hash_uses_canonical_relative_path() {
        let windows_style = path_identity_hash(Path::new(r"src\nested\file.rs"));
        let normalized = path_identity_hash(Path::new("src/nested/file.rs"));
        let sibling = path_identity_hash(Path::new(r"src\other\file.rs"));
        assert_eq!(windows_style, normalized);
        assert_ne!(windows_style, sibling);
    }
}
