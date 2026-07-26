use crate::dom::DomTree;
use std::collections::HashMap;

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

/// Keyboard event for synthetic dispatch.
#[derive(Debug, Clone)]
pub struct KeyboardEvent {
    pub event_type: String, // "keydown", "keyup", "keypress"
    pub key: String,
    pub code: String,
    pub modifiers: Vec<String>, // "ctrl", "shift", "alt", "meta"
    pub repeat: bool,
    pub bubbles: bool,
    pub default_prevented: bool,
    pub propagation_stopped: bool,
}

/// Event phase during dispatch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EventPhase {
    Capture,
    Target,
    Bubble,
}

/// A registered event listener.
#[derive(Debug, Clone)]
pub struct EventListener {
    pub event_type: String,
    pub callback_id: usize,
    pub capture: bool,
    pub once: bool,
    pub passive: bool,
}

/// Record of a dispatched event invocation.
#[derive(Debug, Clone)]
pub struct DispatchRecord {
    pub node_id: usize,
    pub event_type: String,
    pub callback_id: usize,
    pub phase: EventPhase,
}

/// Synthetic event dispatcher with listener registration and invocation.
pub struct SyntheticEventDispatcher {
    listeners: HashMap<usize, Vec<EventListener>>,
    dispatch_log: Vec<DispatchRecord>,
}

impl Default for SyntheticEventDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntheticEventDispatcher {
    pub fn new() -> Self {
        Self {
            listeners: HashMap::new(),
            dispatch_log: Vec::new(),
        }
    }

    /// Register an event listener on a node.
    pub fn add_event_listener(&mut self, node_id: usize, event_type: &str, callback_id: usize, capture: bool, once: bool, passive: bool) {
        let listeners = self.listeners.entry(node_id).or_insert_with(Vec::new);
        listeners.push(EventListener {
            event_type: event_type.to_string(),
            callback_id,
            capture,
            once,
            passive,
        });
    }

    /// Remove an event listener from a node.
    pub fn remove_event_listener(&mut self, node_id: usize, event_type: &str, callback_id: usize) -> bool {
        if let Some(listeners) = self.listeners.get_mut(&node_id) {
            let before = listeners.len();
            listeners.retain(|l| !(l.event_type == event_type && l.callback_id == callback_id));
            listeners.len() < before
        } else {
            false
        }
    }

    /// Static dispatch for backward compatibility (no listener tracking).
    pub fn dispatch_pointer_event_static(tree: &mut DomTree, target_node_id: usize, event: PointerEvent) -> PointerEvent {
        let mut path = Vec::new();
        let mut curr = Some(target_node_id);

        while let Some(id) = curr {
            path.push(id);
            curr = tree.get_node(id).and_then(|n| n.parent);
        }

        // 1. Capturing Phase (Root -> Parent)
        for &node_id in path.iter().rev().skip(1) {
            if event.propagation_stopped { break; }
            // Capture phase listeners (no-op in static mode)
        }

        // 2. Target Phase
        if !event.propagation_stopped {
            // Target listeners (no-op in static mode)
        }

        // 3. Bubbling Phase (Parent -> Root)
        if event.bubbles {
            for &node_id in path.iter().skip(1) {
                if event.propagation_stopped { break; }
                // Bubble phase listeners (no-op in static mode)
            }
        }

        event
    }

    /// Dispatch a pointer event through the DOM tree with listener tracking.
    pub fn dispatch_pointer_event(&mut self, tree: &mut DomTree, target_node_id: usize, event: PointerEvent) -> PointerEvent {
        let mut path = Vec::new();
        let mut curr = Some(target_node_id);

        while let Some(id) = curr {
            path.push(id);
            curr = tree.get_node(id).and_then(|n| n.parent);
        }

        let mut event = event;

        // 1. Capturing Phase (Root -> Parent)
        for &node_id in path.iter().rev() {
            if node_id == target_node_id { break; } // Stop before target
            if event.propagation_stopped { break; }
            self.invoke_listeners(node_id, &event.event_type, EventPhase::Capture, &mut event.default_prevented);
        }

        // 2. Target Phase
        if !event.propagation_stopped {
            self.invoke_listeners(target_node_id, &event.event_type, EventPhase::Target, &mut event.default_prevented);
        }

        // 3. Bubbling Phase (Parent -> Root)
        if event.bubbles {
            for &node_id in path.iter().skip(1) {
                if event.propagation_stopped { break; }
                self.invoke_listeners(node_id, &event.event_type, EventPhase::Bubble, &mut event.default_prevented);
            }
        }

        event
    }

    /// Dispatch a keyboard event through the DOM tree.
    pub fn dispatch_keyboard_event(&mut self, tree: &mut DomTree, target_node_id: usize, event: KeyboardEvent) -> KeyboardEvent {
        let mut path = Vec::new();
        let mut curr = Some(target_node_id);

        while let Some(id) = curr {
            path.push(id);
            curr = tree.get_node(id).and_then(|n| n.parent);
        }

        let mut event = event;

        // 1. Capturing Phase
        for &node_id in path.iter().rev() {
            if node_id == target_node_id { break; }
            if event.propagation_stopped { break; }
            self.invoke_listeners(node_id, &event.event_type, EventPhase::Capture, &mut event.default_prevented);
        }

        // 2. Target Phase
        if !event.propagation_stopped {
            self.invoke_listeners(target_node_id, &event.event_type, EventPhase::Target, &mut event.default_prevented);
        }

        // 3. Bubbling Phase
        if event.bubbles {
            for &node_id in path.iter().skip(1) {
                if event.propagation_stopped { break; }
                self.invoke_listeners(node_id, &event.event_type, EventPhase::Bubble, &mut event.default_prevented);
            }
        }

        event
    }

    /// Invoke listeners for a node and event type.
    fn invoke_listeners(&mut self, node_id: usize, event_type: &str, phase: EventPhase, default_prevented: &mut bool) {
        let listeners = match self.listeners.get(&node_id) {
            Some(l) => l,
            None => return,
        };

        let mut to_remove = Vec::new();
        for (idx, listener) in listeners.iter().enumerate() {
            if listener.event_type != event_type { continue; }
            let is_capture_match = (phase == EventPhase::Capture && listener.capture) ||
                                   (phase == EventPhase::Target) ||
                                   (phase == EventPhase::Bubble && !listener.capture);
            if !is_capture_match { continue; }

            self.dispatch_log.push(DispatchRecord {
                node_id,
                event_type: event_type.to_string(),
                callback_id: listener.callback_id,
                phase,
            });

            if listener.once {
                to_remove.push(idx);
            }
        }

        // Remove once-listeners (in reverse to maintain indices)
        for idx in to_remove.into_iter().rev() {
            if let Some(listeners) = self.listeners.get_mut(&node_id) {
                listeners.remove(idx);
            }
        }
    }

    /// Prevent default behavior (called by listeners).
    pub fn prevent_default(&self) -> bool {
        true // Marker method for agent inspection
    }

    /// Stop propagation (called by listeners).
    pub fn stop_propagation(&self) -> bool {
        true // Marker method for agent inspection
    }

    /// Get the dispatch log for inspection.
    pub fn dispatch_log(&self) -> &[DispatchRecord] {
        &self.dispatch_log
    }

    /// Clear the dispatch log.
    pub fn clear_dispatch_log(&mut self) {
        self.dispatch_log.clear();
    }

    /// Get listener count for a node.
    pub fn listener_count(&self, node_id: usize) -> usize {
        self.listeners.get(&node_id).map(|l| l.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DomTree;

    fn make_tree() -> DomTree {
        let mut tree = DomTree::new(Vec::new());
        let html = tree.create_element("html"); // id=0
        let body = tree.create_element("body"); // id=1
        let div = tree.create_element("div"); // id=2
        let button = tree.create_element("button"); // id=3
        tree.append_child(html, body);
        tree.append_child(body, div);
        tree.append_child(div, button);
        tree
    }

    #[test]
    fn register_and_invoke_listeners() {
        let mut tree = make_tree();
        let mut dispatcher = SyntheticEventDispatcher::new();

        // Add listeners at different levels
        dispatcher.add_event_listener(0, "click", 100, false, false, false); // bubble on html
        dispatcher.add_event_listener(2, "click", 200, false, false, false); // bubble on div
        dispatcher.add_event_listener(3, "click", 300, false, false, false); // bubble on button

        let event = PointerEvent {
            event_type: "click".to_string(),
            client_x: 10.0,
            client_y: 20.0,
            button: 0,
            bubbles: true,
            default_prevented: false,
            propagation_stopped: false,
        };

        let result = dispatcher.dispatch_pointer_event(&mut tree, 3, event);
        assert!(!result.default_prevented);

        let log = dispatcher.dispatch_log();
        assert_eq!(log.len(), 3); // button(300) -> div(200) -> html(100)
        assert_eq!(log[0].callback_id, 300);
        assert_eq!(log[0].phase, EventPhase::Target);
        assert_eq!(log[1].callback_id, 200);
        assert_eq!(log[1].phase, EventPhase::Bubble);
        assert_eq!(log[2].callback_id, 100);
        assert_eq!(log[2].phase, EventPhase::Bubble);
    }

    #[test]
    fn capture_phase_listeners() {
        let mut tree = make_tree();
        let mut dispatcher = SyntheticEventDispatcher::new();

        dispatcher.add_event_listener(0, "click", 100, true, false, false); // capture on html
        dispatcher.add_event_listener(2, "click", 200, true, false, false); // capture on div
        dispatcher.add_event_listener(3, "click", 300, false, false, false); // bubble on button

        let event = PointerEvent {
            event_type: "click".to_string(),
            client_x: 10.0,
            client_y: 20.0,
            button: 0,
            bubbles: true,
            default_prevented: false,
            propagation_stopped: false,
        };

        dispatcher.dispatch_pointer_event(&mut tree, 3, event);
        let log = dispatcher.dispatch_log();
        assert_eq!(log.len(), 3);
        assert_eq!(log[0].callback_id, 100); // capture on html
        assert_eq!(log[0].phase, EventPhase::Capture);
        assert_eq!(log[1].callback_id, 200); // capture on div
        assert_eq!(log[1].phase, EventPhase::Capture);
        assert_eq!(log[2].callback_id, 300); // target on button
        assert_eq!(log[2].phase, EventPhase::Target);
    }

    #[test]
    fn once_listener_auto_removed() {
        let mut tree = make_tree();
        let mut dispatcher = SyntheticEventDispatcher::new();

        dispatcher.add_event_listener(3, "click", 100, false, true, false); // once=true
        dispatcher.add_event_listener(3, "click", 200, false, false, false); // once=false

        let event = PointerEvent {
            event_type: "click".to_string(),
            client_x: 0.0,
            client_y: 0.0,
            button: 0,
            bubbles: false,
            default_prevented: false,
            propagation_stopped: false,
        };

        dispatcher.dispatch_pointer_event(&mut tree, 3, event.clone());
        assert_eq!(dispatcher.dispatch_log().len(), 2);
        assert_eq!(dispatcher.listener_count(3), 1); // once-listener removed

        dispatcher.clear_dispatch_log();
        dispatcher.dispatch_pointer_event(&mut tree, 3, event);
        assert_eq!(dispatcher.dispatch_log().len(), 1); // only non-once listener fires
    }

    #[test]
    fn keyboard_event_dispatch() {
        let mut tree = make_tree();
        let mut dispatcher = SyntheticEventDispatcher::new();

        dispatcher.add_event_listener(3, "keydown", 100, false, false, false);
        dispatcher.add_event_listener(1, "keydown", 200, false, false, false);

        let event = KeyboardEvent {
            event_type: "keydown".to_string(),
            key: "Enter".to_string(),
            code: "Enter".to_string(),
            modifiers: vec!["shift".to_string()],
            repeat: false,
            bubbles: true,
            default_prevented: false,
            propagation_stopped: false,
        };

        let result = dispatcher.dispatch_keyboard_event(&mut tree, 3, event);
        assert!(!result.default_prevented);
        assert_eq!(dispatcher.dispatch_log().len(), 2);
    }

    #[test]
    fn remove_listener() {
        let mut dispatcher = SyntheticEventDispatcher::new();
        dispatcher.add_event_listener(1, "click", 100, false, false, false);
        dispatcher.add_event_listener(1, "click", 200, false, false, false);
        assert_eq!(dispatcher.listener_count(1), 2);

        let removed = dispatcher.remove_event_listener(1, "click", 100);
        assert!(removed);
        assert_eq!(dispatcher.listener_count(1), 1);

        let removed2 = dispatcher.remove_event_listener(1, "click", 999);
        assert!(!removed2);
    }
}
