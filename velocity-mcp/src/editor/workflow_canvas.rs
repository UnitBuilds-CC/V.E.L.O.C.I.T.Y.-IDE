//! Visual workflow canvas: node-based drag-and-drop workflow designer.
//!
//! Provides a visual graph editor where users can place workflow step nodes,
//! connect them with edges to define execution flow, and see real-time
//! execution status. Each node represents a [`WorkflowStep`] and edges define
//! the execution order (including conditional branching).

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a node on the canvas.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self(format!("node_{ts}"))
    }
}

/// Visual position of a node on the canvas.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NodePosition {
    pub x: f32,
    pub y: f32,
}

impl Default for NodePosition {
    fn default() -> Self {
        Self { x: 100.0, y: 100.0 }
    }
}

/// The kind of visual node, mirroring [`crate::editor::workflow::WorkflowStep`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CanvasNodeKind {
    /// Entry point — where execution begins.
    Start,
    /// Dispatch a prompt to a headless agent.
    AgentTask { prompt: String, team: Option<String> },
    /// Invoke a registered MCP tool.
    Tool { name: String, args: serde_json::Value },
    /// Invoke a connector.
    Connector { id: String, req: serde_json::Value },
    /// Conditional branch — true/false exits.
    Condition { description: String },
    /// Terminal node — marks end of a branch.
    End,
}

impl CanvasNodeKind {
    /// Display label for the node header.
    pub fn label(&self) -> &str {
        match self {
            Self::Start => "START",
            Self::AgentTask { .. } => "AGENT",
            Self::Tool { .. } => "TOOL",
            Self::Connector { .. } => "CONNECTOR",
            Self::Condition { .. } => "CONDITION",
            Self::End => "END",
        }
    }

    /// Accent color for the node border/header.
    pub fn color(&self) -> egui::Color32 {
        match self {
            Self::Start => egui::Color32::from_rgb(76, 175, 80),    // green
            Self::AgentTask { .. } => egui::Color32::from_rgb(33, 150, 243),  // blue
            Self::Tool { .. } => egui::Color32::from_rgb(255, 152, 0),   // orange
            Self::Connector { .. } => egui::Color32::from_rgb(156, 39, 176),  // purple
            Self::Condition { .. } => egui::Color32::from_rgb(255, 87, 34),   // deep orange
            Self::End => egui::Color32::from_rgb(158, 158, 158),   // grey
        }
    }

    /// Short description shown in the node body.
    pub fn description(&self) -> String {
        match self {
            Self::Start => "Execution begins here".to_string(),
            Self::AgentTask { prompt, team } => {
                let t = team.as_deref().unwrap_or("default");
                format!("Team: {t}\n{}", &prompt[..prompt.len().min(60)])
            }
            Self::Tool { name, .. } => format!("tool: {name}"),
            Self::Connector { id, .. } => format!("connector: {id}"),
            Self::Condition { description } => description.clone(),
            Self::End => "Execution ends here".to_string(),
        }
    }
}

/// A node on the visual canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasNode {
    pub id: NodeId,
    pub kind: CanvasNodeKind,
    pub position: NodePosition,
    /// Whether this node is currently selected in the editor.
    #[serde(skip)]
    pub selected: bool,
}

/// An edge connecting two nodes. `from_port` identifies the output port
/// (e.g., "ok", "fail", "true", "false" for conditions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasEdge {
    pub from: NodeId,
    pub from_port: String,
    pub to: NodeId,
}

/// Execution status of a node during a live run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeExecutionStatus {
    Idle,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

/// The full visual workflow canvas state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCanvas {
    pub workflow_id: String,
    pub name: String,
    pub nodes: Vec<CanvasNode>,
    pub edges: Vec<CanvasEdge>,
    /// Pan offset for the canvas viewport.
    #[serde(skip)]
    pub pan: egui::Vec2,
    /// Zoom level (1.0 = 100%).
    #[serde(skip)]
    pub zoom: f32,
}

impl WorkflowCanvas {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        let start_id = NodeId::new();
        let end_id = NodeId::new();
        Self {
            workflow_id: id.into(),
            name: name.into(),
            nodes: vec![
                CanvasNode {
                    id: start_id.clone(),
                    kind: CanvasNodeKind::Start,
                    position: NodePosition { x: 80.0, y: 200.0 },
                    selected: false,
                },
                CanvasNode {
                    id: end_id,
                    kind: CanvasNodeKind::End,
                    position: NodePosition { x: 600.0, y: 200.0 },
                    selected: false,
                },
            ],
            edges: Vec::new(),
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
        }
    }

    /// Add a new node at the given position.
    pub fn add_node(&mut self, kind: CanvasNodeKind, position: NodePosition) -> NodeId {
        let id = NodeId::new();
        self.nodes.push(CanvasNode {
            id: id.clone(),
            kind,
            position,
            selected: false,
        });
        id
    }

    /// Remove a node and all edges referencing it.
    pub fn remove_node(&mut self, id: &NodeId) {
        self.nodes.retain(|n| &n.id != id);
        self.edges.retain(|e| &e.from != id && &e.to != id);
    }

    /// Add an edge between two nodes.
    pub fn add_edge(&mut self, from: NodeId, from_port: &str, to: NodeId) {
        // Prevent duplicate edges.
        if self.edges.iter().any(|e| e.from == from && e.from_port == from_port && e.to == to) {
            return;
        }
        self.edges.push(CanvasEdge {
            from,
            from_port: from_port.to_string(),
            to,
        });
    }

    /// Remove an edge.
    pub fn remove_edge(&mut self, from: &NodeId, from_port: &str, to: &NodeId) {
        self.edges.retain(|e| &e.from != from || e.from_port != from_port || &e.to != to);
    }

    /// Get a node by id.
    pub fn node(&self, id: &NodeId) -> Option<&CanvasNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    /// Get a mutable node by id.
    pub fn node_mut(&mut self, id: &NodeId) -> Option<&mut CanvasNode> {
        self.nodes.iter_mut().find(|n| &n.id == id)
    }

    /// Deselect all nodes.
    pub fn deselect_all(&mut self) {
        for node in &mut self.nodes {
            node.selected = false;
        }
    }

    /// Get the currently selected node, if any.
    pub fn selected_node(&self) -> Option<&CanvasNode> {
        self.nodes.iter().find(|n| n.selected)
    }

    /// Get outgoing edges from a node.
    pub fn outgoing_edges(&self, id: &NodeId) -> Vec<&CanvasEdge> {
        self.edges.iter().filter(|e| &e.from == id).collect()
    }

    /// Topological sort of nodes for execution order.
    /// Returns None if the graph has cycles.
    pub fn execution_order(&self) -> Option<Vec<NodeId>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();

        for node in &self.nodes {
            in_degree.entry(node.id.0.clone()).or_insert(0);
            adj.entry(node.id.0.clone()).or_default();
        }
        for edge in &self.edges {
            *in_degree.entry(edge.to.0.clone()).or_insert(0) += 1;
            adj.entry(edge.from.0.clone()).or_default().push(edge.to.0.clone());
        }

        let mut queue: Vec<String> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(id, _)| id.clone())
            .collect();
        let mut order = Vec::new();

        while let Some(id) = queue.pop() {
            order.push(NodeId(id.clone()));
            if let Some(neighbors) = adj.get(&id) {
                for next in neighbors {
                    if let Some(deg) = in_degree.get_mut(next) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(next.clone());
                        }
                    }
                }
            }
        }

        if order.len() == self.nodes.len() {
            Some(order)
        } else {
            None // Cycle detected
        }
    }

    /// Convert this canvas into a linear [`crate::editor::workflow::Workflow`]
    /// by following the execution order.
    pub fn to_workflow(&self) -> Option<crate::editor::workflow::Workflow> {
        let order = self.execution_order()?;
        let mut wf = crate::editor::workflow::Workflow::new(&self.workflow_id, &self.name);

        for node_id in &order {
            if let Some(node) = self.node(node_id) {
                let step = match &node.kind {
                    CanvasNodeKind::Start | CanvasNodeKind::End => continue,
                    CanvasNodeKind::AgentTask { prompt, team } => {
                        crate::editor::workflow::WorkflowStep::AgentTask {
                            prompt: prompt.clone(),
                            team: team.clone(),
                        }
                    }
                    CanvasNodeKind::Tool { name, args } => {
                        crate::editor::workflow::WorkflowStep::Tool {
                            name: name.clone(),
                            args: args.clone(),
                        }
                    }
                    CanvasNodeKind::Connector { id, req } => {
                        crate::editor::workflow::WorkflowStep::Connector {
                            id: id.clone(),
                            req: req.clone(),
                        }
                    }
                    CanvasNodeKind::Condition { .. } => {
                        crate::editor::workflow::WorkflowStep::Condition {
                            require: crate::editor::workflow::StepOutcome::Ok,
                        }
                    }
                };
                wf.steps.push(step);
            }
        }
        Some(wf)
    }
}

/// Render the visual workflow canvas with drag-and-drop, zoom, and pan.
impl WorkflowCanvas {
    /// Draw the full canvas UI. Returns actions the caller should process.
    pub fn draw(&mut self, ui: &mut egui::Ui, available_size: egui::Vec2) -> CanvasAction {
        let mut action = CanvasAction::None;
        let palette_bg = egui::Color32::from_rgb(30, 30, 30);
        let palette_grid = egui::Color32::from_rgb(45, 45, 45);

        let (response, painter) = ui.allocate_painter(available_size, egui::Sense::click_and_drag());
        let rect = response.rect;

        // Background
        painter.rect_filled(rect, 0.0, palette_bg);

        // Grid dots
        let zoom = self.zoom;
        let pan = self.pan;
        let grid_spacing = 30.0 * zoom;
        let offset_x = pan.x % grid_spacing;
        let offset_y = pan.y % grid_spacing;
        let mut y = rect.min.y + offset_y;
        while y < rect.max.y {
            let mut x = rect.min.x + offset_x;
            while x < rect.max.x {
                painter.circle_filled(egui::pos2(x, y), 1.0, palette_grid);
                x += grid_spacing;
            }
            y += grid_spacing;
        }

        // Pan with middle mouse or Ctrl+drag
        if response.dragged_by(egui::PointerButton::Middle)
            || (response.dragged_by(egui::PointerButton::Primary) && ui.input(|i| i.modifiers.ctrl))
        {
            self.pan += response.drag_delta();
        }

        // Zoom with scroll
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 && response.hovered() {
            self.zoom = (self.zoom + scroll * 0.001).clamp(0.3, 3.0);
        }

        // Capture zoom/pan for drawing functions
        let zoom = self.zoom;
        let pan = self.pan;

        // Draw edges first (behind nodes)
        for edge in &self.edges {
            draw_edge_fn(&painter, &self.nodes, edge, rect, zoom, pan);
        }

        // Draw nodes (immutable pass)
        let hover_pos = response.hover_pos().unwrap_or(egui::pos2(0.0, 0.0));
        let mut node_rects: Vec<(NodeId, egui::Rect)> = Vec::new();
        for node in &self.nodes {
            let node_rect = draw_node_fn(&painter, node, rect, zoom, pan);
            node_rects.push((node.id.clone(), node_rect));
        }

        // Interaction pass (mutable)
        let is_dragging_primary = response.dragged_by(egui::PointerButton::Primary)
            && !ui.input(|i| i.modifiers.ctrl);
        let is_clicked = response.clicked();
        let mut dragging_node: Option<NodeId> = None;

        for (node_id, node_rect) in &node_rects {
            // Check if user is dragging this node
            if is_dragging_primary && node_rect.contains(hover_pos) {
                dragging_node = Some(node_id.clone());
            }
            // Click to select
            if is_clicked && node_rect.contains(hover_pos) {
                self.deselect_all();
                if let Some(n) = self.nodes.iter_mut().find(|n| &n.id == node_id) {
                    n.selected = true;
                }
                action = CanvasAction::NodeSelected(node_id.clone());
            }
        }

        // Apply drag to node
        if let Some(drag_id) = dragging_node {
            let drag_delta = response.drag_delta();
            if let Some(node) = self.nodes.iter_mut().find(|n| n.id == drag_id) {
                node.position.x += drag_delta.x / zoom;
                node.position.y += drag_delta.y / zoom;
            }
        }

        action
    }
}

/// Draw a single node as a standalone function and return its screen rect.
fn draw_node_fn(
    painter: &egui::Painter,
    node: &CanvasNode,
    canvas_rect: egui::Rect,
    zoom: f32,
    pan: egui::Vec2,
) -> egui::Rect {
    let node_width = 160.0 * zoom;
    let node_height = 80.0 * zoom;
    let x = canvas_rect.min.x + (node.position.x * zoom) + pan.x;
    let y = canvas_rect.min.y + (node.position.y * zoom) + pan.y;
    let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(node_width, node_height));

    let header_height = 22.0 * zoom;
    let header_rect = egui::Rect::from_min_size(rect.min, egui::vec2(node_width, header_height));
    let body_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x, rect.min.y + header_height),
        egui::vec2(node_width, node_height - header_height),
    );

    let border_color = node.kind.color();
    let bg_color = if node.selected {
        egui::Color32::from_rgb(50, 50, 60)
    } else {
        egui::Color32::from_rgb(38, 38, 38)
    };

    // Body
    painter.rect_filled(body_rect, 0.0, bg_color);
    painter.rect_stroke(body_rect, 0.0, egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 60)), egui::StrokeKind::Inside);

    // Header
    painter.rect_filled(header_rect, 0.0, border_color);
    let label_size = 10.0 * zoom;
    painter.text(
        header_rect.center(),
        egui::Align2::CENTER_CENTER,
        node.kind.label(),
        egui::FontId::proportional(label_size),
        egui::Color32::WHITE,
    );

    // Description
    let desc = node.kind.description();
    let desc_size = 8.0 * zoom;
    painter.text(
        body_rect.shrink(6.0 * zoom).center(),
        egui::Align2::CENTER_CENTER,
        &desc,
        egui::FontId::proportional(desc_size),
        egui::Color32::from_rgb(180, 180, 180),
    );

    // Output port (right side)
    let port_pos = egui::pos2(rect.max.x, rect.center().y);
    painter.circle_filled(port_pos, 5.0 * zoom, border_color);
    painter.circle_stroke(port_pos, 5.0 * zoom, egui::Stroke::new(1.0, egui::Color32::WHITE));

    // Input port (left side) — not on Start node
    if !matches!(node.kind, CanvasNodeKind::Start) {
        let in_pos = egui::pos2(rect.min.x, rect.center().y);
        painter.circle_filled(in_pos, 5.0 * zoom, border_color);
        painter.circle_stroke(in_pos, 5.0 * zoom, egui::Stroke::new(1.0, egui::Color32::WHITE));
    }

    rect
}

/// Draw an edge between two nodes as a standalone function.
fn draw_edge_fn(
    painter: &egui::Painter,
    nodes: &[CanvasNode],
    edge: &CanvasEdge,
    canvas_rect: egui::Rect,
    zoom: f32,
    pan: egui::Vec2,
) {
    let from_node = match nodes.iter().find(|n| n.id == edge.from) {
        Some(n) => n,
        None => return,
    };
    let to_node = match nodes.iter().find(|n| n.id == edge.to) {
        Some(n) => n,
        None => return,
    };

    let node_width = 160.0 * zoom;
    let node_height = 80.0 * zoom;

    let from_x = canvas_rect.min.x + (from_node.position.x * zoom) + pan.x + node_width;
    let from_y = canvas_rect.min.y + (from_node.position.y * zoom) + pan.y + node_height / 2.0;
    let to_x = canvas_rect.min.x + (to_node.position.x * zoom) + pan.x;
    let to_y = canvas_rect.min.y + (to_node.position.y * zoom) + pan.y + node_height / 2.0;

    let from = egui::pos2(from_x, from_y);
    let to = egui::pos2(to_x, to_y);

    // Bezier curve for smooth edges
    let ctrl_offset = (to_x - from_x).abs().max(50.0) * 0.5;
    let ctrl1 = egui::pos2(from_x + ctrl_offset, from_y);
    let ctrl2 = egui::pos2(to_x - ctrl_offset, to_y);

    let color = from_node.kind.color().linear_multiply(0.7);
    let points = bezier_points(from, ctrl1, ctrl2, to, 20);
    for pair in points.windows(2) {
        painter.line_segment([pair[0], pair[1]], egui::Stroke::new(2.0 * zoom, color));
    }

    // Arrowhead
    let dir = (to - ctrl2).normalized();
    let arrow_size = 8.0 * zoom;
    let left = to - dir * arrow_size + egui::Vec2::new(-dir.y, dir.x) * arrow_size * 0.5;
    let right = to - dir * arrow_size - egui::Vec2::new(-dir.y, dir.x) * arrow_size * 0.5;
    painter.add(egui::Shape::convex_polygon(vec![to, left, right], color, egui::Stroke::NONE));
}

/// Actions produced by canvas interaction.
#[derive(Debug, Clone)]
pub enum CanvasAction {
    None,
    NodeSelected(NodeId),
    NodeDeleted(NodeId),
    EdgeCreated { from: NodeId, to: NodeId },
    RunRequested,
}

/// Compute points along a cubic bezier curve.
fn bezier_points(p0: egui::Pos2, p1: egui::Pos2, p2: egui::Pos2, p3: egui::Pos2, segments: usize) -> Vec<egui::Pos2> {
    (0..=segments)
        .map(|i| {
            let t = i as f32 / segments as f32;
            let mt = 1.0 - t;
            let x = mt * mt * mt * p0.x + 3.0 * mt * mt * t * p1.x + 3.0 * mt * t * t * p2.x + t * t * t * p3.x;
            let y = mt * mt * mt * p0.y + 3.0 * mt * mt * t * p1.y + 3.0 * mt * t * t * p2.y + t * t * t * p3.y;
            egui::pos2(x, y)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_new_has_start_and_end() {
        let canvas = WorkflowCanvas::new("test", "Test Workflow");
        assert_eq!(canvas.nodes.len(), 2);
        assert!(matches!(canvas.nodes[0].kind, CanvasNodeKind::Start));
        assert!(matches!(canvas.nodes[1].kind, CanvasNodeKind::End));
    }

    #[test]
    fn add_and_remove_node() {
        let mut canvas = WorkflowCanvas::new("test", "Test");
        let id = canvas.add_node(
            CanvasNodeKind::Tool { name: "write_file".into(), args: serde_json::json!({}) },
            NodePosition { x: 300.0, y: 200.0 },
        );
        assert_eq!(canvas.nodes.len(), 3);
        canvas.remove_node(&id);
        assert_eq!(canvas.nodes.len(), 2);
    }

    #[test]
    fn add_edge_and_execution_order() {
        let mut canvas = WorkflowCanvas::new("test", "Test");
        let start_id = canvas.nodes[0].id.clone();
        let end_id = canvas.nodes[1].id.clone();
        let mid = canvas.add_node(
            CanvasNodeKind::Tool { name: "read_file".into(), args: serde_json::json!({}) },
            NodePosition { x: 300.0, y: 200.0 },
        );
        canvas.add_edge(start_id.clone(), "ok", mid.clone());
        canvas.add_edge(mid.clone(), "ok", end_id.clone());

        let order = canvas.execution_order().unwrap();
        assert_eq!(order.len(), 3);
        assert_eq!(order[0], start_id);
        assert_eq!(order[2], end_id);
    }

    #[test]
    fn cycle_detected() {
        let mut canvas = WorkflowCanvas::new("test", "Test");
        let a = canvas.nodes[0].id.clone();
        let b = canvas.nodes[1].id.clone();
        canvas.add_edge(a.clone(), "ok", b.clone());
        canvas.add_edge(b.clone(), "ok", a.clone());
        assert!(canvas.execution_order().is_none());
    }

    #[test]
    fn to_workflow_converts_nodes() {
        let mut canvas = WorkflowCanvas::new("wf1", "My Workflow");
        let start_id = canvas.nodes[0].id.clone();
        let end_id = canvas.nodes[1].id.clone();
        let tool_node = canvas.add_node(
            CanvasNodeKind::Tool { name: "write_file".into(), args: serde_json::json!({"f": "a.txt"}) },
            NodePosition { x: 300.0, y: 200.0 },
        );
        canvas.add_edge(start_id, "ok", tool_node.clone());
        canvas.add_edge(tool_node, "ok", end_id);

        let wf = canvas.to_workflow().unwrap();
        assert_eq!(wf.steps.len(), 1);
        assert!(matches!(wf.steps[0], crate::editor::workflow::WorkflowStep::Tool { .. }));
    }
}
