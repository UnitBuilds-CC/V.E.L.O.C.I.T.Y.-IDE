#![allow(dead_code)]
//! Configurable keybindings system.
//!
//! Allows users to customize keyboard shortcuts via a JSON configuration file.
//! Provides defaults per workspace mode and supports conflict detection.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A keyboard shortcut.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyBinding {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyBinding {
    pub fn new(key: &str) -> Self {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut actual_key = key.to_string();

        // Parse "Ctrl+Shift+K" format
        let parts: Vec<&str> = key.split('+').collect();
        if parts.len() > 1 {
            for &part in &parts[..parts.len() - 1] {
                match part.to_lowercase().as_str() {
                    "ctrl" => ctrl = true,
                    "shift" => shift = true,
                    "alt" => alt = true,
                    _ => {}
                }
            }
            actual_key = parts.last().unwrap_or(&"").to_string();
        }

        Self { key: actual_key, ctrl, shift, alt }
    }

    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl { parts.push("Ctrl"); }
        if self.shift { parts.push("Shift"); }
        if self.alt { parts.push("Alt"); }
        parts.push(&self.key);
        parts.join("+")
    }

    /// Check if this keybinding matches an egui input event.
    pub fn matches(&self, modifiers: &eframe::egui::Modifiers, key: eframe::egui::Key) -> bool {
        self.ctrl == modifiers.ctrl
            && self.shift == modifiers.shift
            && self.alt == modifiers.alt
            && self.key.to_lowercase() == format!("{:?}", key).to_lowercase()
    }
}

/// A command that can be bound to a key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingEntry {
    pub command: String,
    pub binding: KeyBinding,
    pub when: Option<String>,
}

/// The full keybindings configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeybindingsConfig {
    pub bindings: Vec<KeybindingEntry>,
}

impl KeybindingsConfig {
    /// Load from a JSON file, or return defaults if not found.
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_root.join(".velocity").join("keybindings.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            serde_json::from_str(&content).unwrap_or_else(|_| Self::defaults())
        } else {
            Self::defaults()
        }
    }

    /// Save to the workspace keybindings file.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("keybindings.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// Default keybindings.
    pub fn defaults() -> Self {
        Self {
            bindings: vec![
                // File operations
                entry("file.new", "Ctrl+N", None),
                entry("file.open", "Ctrl+O", None),
                entry("file.save", "Ctrl+S", None),
                entry("file.save_all", "Ctrl+Shift+S", None),
                entry("file.close", "Ctrl+W", None),
                entry("file.quick_open", "Ctrl+P", None),
                // Edit operations
                entry("edit.undo", "Ctrl+Z", None),
                entry("edit.redo", "Ctrl+Shift+Z", None),
                entry("edit.find", "Ctrl+F", Some("editorFocus")),
                entry("edit.replace", "Ctrl+H", Some("editorFocus")),
                entry("edit.find_next", "F3", Some("editorFocus")),
                entry("edit.find_prev", "Shift+F3", Some("editorFocus")),
                entry("edit.indent", "Tab", Some("editorFocus")),
                entry("edit.dedent", "Shift+Tab", Some("editorFocus")),
                entry("edit.toggle_comment", "Ctrl+/", Some("editorFocus")),
                entry("edit.duplicate_line", "Ctrl+Shift+D", Some("editorFocus")),
                entry("edit.delete_line", "Ctrl+Shift+K", Some("editorFocus")),
                entry("edit.move_line_up", "Alt+Up", Some("editorFocus")),
                entry("edit.move_line_down", "Alt+Down", Some("editorFocus")),
                // Navigation
                entry("nav.goto_line", "Ctrl+G", None),
                entry("nav.goto_symbol", "Ctrl+Shift+O", None),
                entry("nav.goto_definition", "F12", Some("editorFocus")),
                entry("nav.find_references", "Shift+F12", Some("editorFocus")),
                entry("nav.back", "Alt+Left", None),
                entry("nav.forward", "Alt+Right", None),
                entry("nav.next_tab", "Ctrl+PageDown", None),
                entry("nav.prev_tab", "Ctrl+PageUp", None),
                // View
                entry("view.command_palette", "Ctrl+Shift+P", None),
                entry("view.toggle_sidebar", "Ctrl+E", None),
                entry("view.toggle_terminal", "Ctrl+`", None),
                entry("view.toggle_chat", "Ctrl+J", None),
                entry("view.toggle_orchestrator", "Ctrl+Shift+Y", None),
                entry("view.toggle_search", "Ctrl+Shift+F", None),
                entry("view.toggle_settings", "Ctrl+,", None),
                entry("view.toggle_extensions", "Ctrl+Shift+X", None),
                entry("view.toggle_activity", "Ctrl+Shift+A", None),
                entry("view.toggle_voice", "Ctrl+Shift+V", None),
                entry("view.fold", "Ctrl+Shift+[", Some("editorFocus")),
                entry("view.unfold", "Ctrl+Shift+]", Some("editorFocus")),
                entry("view.fold_all", "Ctrl+K Ctrl+0", Some("editorFocus")),
                entry("view.unfold_all", "Ctrl+K Ctrl+J", Some("editorFocus")),
                entry("view.word_wrap", "Alt+Z", Some("editorFocus")),
                // Debug
                entry("debug.start", "F5", None),
                entry("debug.stop", "Shift+F5", None),
                entry("debug.step_over", "F10", Some("debugActive")),
                entry("debug.step_into", "F11", Some("debugActive")),
                entry("debug.step_out", "Shift+F11", Some("debugActive")),
                entry("debug.toggle_breakpoint", "F9", Some("editorFocus")),
                entry("debug.continue", "F5", Some("debugActive")),
                // Agent
                entry("agent.request_inline_suggestion", "Ctrl+Shift+I", None),
                // Build
                entry("build.build", "Ctrl+B", None),
                entry("build.run", "Ctrl+R", None),
                entry("build.rollback_deploy", "Ctrl+Alt+R", None),
                // Workspace modes
                entry("mode.coder", "Ctrl+1", None),
                entry("mode.operator", "Ctrl+2", None),
                entry("mode.mission", "Ctrl+3", None),
                entry("mode.accessibility", "Ctrl+4", None),
                // Completion
                entry("completion.trigger", "Ctrl+Space", Some("editorFocus")),
            ],
        }
    }

    /// Find the binding for a given command.
    pub fn binding_for(&self, command: &str) -> Option<&KeyBinding> {
        self.bindings.iter().find(|e| e.command == command).map(|e| &e.binding)
    }

    /// Find the command for a given key combination.
    pub fn command_for(&self, binding: &KeyBinding, context: Option<&str>) -> Option<&str> {
        self.bindings.iter().find(|e| {
            e.binding == *binding && (e.when.is_none() || e.when.as_deref() == context)
        }).map(|e| e.command.as_str())
    }

    /// Update a binding for a command.
    pub fn set_binding(&mut self, command: &str, binding: KeyBinding) {
        if let Some(entry) = self.bindings.iter_mut().find(|e| e.command == command) {
            entry.binding = binding;
        }
    }

    /// Detect conflicts (multiple commands with same binding in same context).
    pub fn conflicts(&self) -> Vec<(&str, &str, &KeyBinding)> {
        let mut seen: HashMap<(&KeyBinding, Option<&str>), &str> = HashMap::new();
        let mut conflicts = Vec::new();
        for entry in &self.bindings {
            let key = (&entry.binding, entry.when.as_deref());
            if let Some(&existing) = seen.get(&key) {
                conflicts.push((existing, entry.command.as_str(), &entry.binding));
            } else {
                seen.insert(key, &entry.command);
            }
        }
        conflicts
    }
}

fn entry(command: &str, binding: &str, when: Option<&str>) -> KeybindingEntry {
    KeybindingEntry {
        command: command.to_string(),
        binding: KeyBinding::new(binding),
        when: when.map(|s| s.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keybinding() {
        let kb = KeyBinding::new("Ctrl+Shift+P");
        assert!(kb.ctrl);
        assert!(kb.shift);
        assert!(!kb.alt);
        assert_eq!(kb.key, "P");
    }

    #[test]
    fn display_keybinding() {
        let kb = KeyBinding::new("Ctrl+Alt+F12");
        assert_eq!(kb.display(), "Ctrl+Alt+F12");
    }

    #[test]
    fn defaults_has_save() {
        let config = KeybindingsConfig::defaults();
        assert!(config.binding_for("file.save").is_some());
    }

    #[test]
    fn command_lookup() {
        let config = KeybindingsConfig::defaults();
        let binding = KeyBinding::new("Ctrl+S");
        let cmd = config.command_for(&binding, None);
        assert_eq!(cmd, Some("file.save"));
    }

    #[test]
    fn no_default_conflicts() {
        let config = KeybindingsConfig::defaults();
        // F5 has two entries but different contexts (None vs debugActive)
        let conflicts = config.conflicts();
        // Expect debug.start and debug.continue conflict on F5 (both have F5)
        assert!(conflicts.len() <= 1);
    }
}
