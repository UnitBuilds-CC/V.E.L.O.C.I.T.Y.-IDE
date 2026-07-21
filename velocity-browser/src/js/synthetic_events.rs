use crate::dom::DomTree;

#[derive(Debug, Clone)]
pub struct PointerEvent {
    pub event_type: String, // "click", "mousedown", "pointerdown"
    pub client_x: f32,
    pub client_y: f32,
    pub button: u8,
    pub bubbles: bool,
    pub default_prevented: bool,
    pub propagation_stopped: bool,
}

pub struct SyntheticEventDispatcher;

impl SyntheticEventDispatcher {
    pub fn dispatch_pointer_event(tree: &mut DomTree, target_node_id: usize, mut event: PointerEvent) -> PointerEvent {
        let mut path = Vec::new();
        let mut curr = Some(target_node_id);

        while let Some(id) = curr {
            path.push(id);
            curr = tree.get_node(id).and_then(|n| n.parent);
        }

        // 1. Capturing Phase (Root -> Parent)
        for &node_id in path.iter().rev().skip(1) {
            if event.propagation_stopped { break; }
            let _ = node_id; // Capture phase listeners
        }

        // 2. Target Phase
        if !event.propagation_stopped {
            let _ = target_node_id; // Target listeners
        }

        // 3. Bubbling Phase (Parent -> Root)
        if event.bubbles {
            for &node_id in path.iter().skip(1) {
                if event.propagation_stopped { break; }
                let _ = node_id; // Bubble phase listeners
            }
        }

        event
    }
}
