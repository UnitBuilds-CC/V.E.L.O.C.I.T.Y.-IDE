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

        Self {
            key: actual_key,
            ctrl,
            shift,
            alt,
        }
    }

    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// Check if this keybinding matches an egui input event.
    pub fn matches(&self, modifiers: &eframe::egui::Modifiers, key: eframe::egui::Key) -> bool {
        self.ctrl == modifiers.ctrl
            && self.shift == modifiers.shift
            && self.alt == modifiers.alt
            && self.key_matches(key)
    }

    /// Build a binding from a live egui key event plus modifier state. The key
    /// half is the Debug name (`"A"`, `"F2"`, `"ArrowUp"`), which the
    /// alias-aware [`Self::key_matches`] accepts alongside the symbol spellings
    /// a person types into `keybindings.json` (`"Up"`, `","`, `"1"`).
    pub fn from_egui(key: eframe::egui::Key, modifiers: &eframe::egui::Modifiers) -> Self {
        Self {
            key: format!("{:?}", key),
            ctrl: modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }
    }

    /// Whether this binding's key field names the given egui key, accepting
    /// the Debug name and the common symbol/word aliases, case-insensitively.
    fn key_matches(&self, key: eframe::egui::Key) -> bool {
        let want = self.key.to_lowercase();
        if want == format!("{:?}", key).to_lowercase() {
            return true;
        }
        key_aliases(key)
            .into_iter()
            .any(|a| a.to_lowercase() == want)
    }
}

/// Symbol/word aliases accepted in `keybindings.json` for keys whose Debug
/// name differs from what a person would naturally type (arrows, digits,
/// punctuation). Returned spellings are compared case-insensitively.
fn key_aliases(key: eframe::egui::Key) -> Vec<&'static str> {
    use eframe::egui::Key::*;
    let aliases: &[&str] = match key {
        ArrowUp => &["up"],
        ArrowDown => &["down"],
        ArrowLeft => &["left"],
        ArrowRight => &["right"],
        Backtick => &["`", "grave"],
        Comma => &[","],
        Period => &["."],
        Slash => &["/"],
        Backslash => &["backslash_key"],
        Minus => &["-"],
        Equals => &["=", "plus"],
        Enter => &["return", "cr"],
        Escape => &["esc"],
        PageUp => &["prior"],
        PageDown => &["next"],
        Insert => &["ins"],
        Delete => &["del"],
        Num0 => &["0"],
        Num1 => &["1"],
        Num2 => &["2"],
        Num3 => &["3"],
        Num4 => &["4"],
        Num5 => &["5"],
        Num6 => &["6"],
        Num7 => &["7"],
        Num8 => &["8"],
        Num9 => &["9"],
        _ => &[],
    };
    aliases.to_vec()
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
                entry("edit.next_change", "Ctrl+Alt+J", None),
                entry("edit.prev_change", "Ctrl+Alt+K", None),
                entry("edit.next_problem", "F8", None),
                entry("edit.prev_problem", "Shift+F8", None),
                entry(
                    "editor.select_expand",
                    "Shift+Alt+Right",
                    Some("editorFocus"),
                ),
                entry(
                    "editor.select_shrink",
                    "Shift+Alt+Left",
                    Some("editorFocus"),
                ),
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
                // NOTE: code folding (`view.fold*`) is intentionally NOT
                // advertised. The tested `code_folding` engine is ready, but
                // the editor renders the whole document as one editable TextEdit
                // whose cursor/undo state is offset-indexed into the full text;
                // collapsing lines from that body would desynchronize them, so
                // visual folding needs a per-line editor rewrite. We refuse to
                // ship a shortcut that only moves a gutter marker while the
                // lines stay put — a dead promise. The `every_default_keybinding_
                // is_dispatchable` guard keeps the advertised set honest.
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
                // LSP refactoring engine (default chords also hardcoded; a user
                // may rebind these to remap the command onto a different key).
                entry("editor.rename_symbol", "F2", None),
                entry("editor.code_actions", "Alt+Enter", None),
                entry("editor.format_document", "Shift+Alt+F", None),
                entry("editor.toggle_bookmark", "Ctrl+Shift+B", None),
                // Build
                entry("build.build", "Ctrl+B", None),
                entry("build.run", "Ctrl+R", None),
                entry("build.rollback_deploy", "Ctrl+Alt+R", None),
                entry("build.show_nodes", "Ctrl+Alt+B", None),
                // Git
                entry("git.switch_branch", "Ctrl+Shift+G", None),
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
        self.bindings
            .iter()
            .find(|e| e.command == command)
            .map(|e| &e.binding)
    }

    /// Find the command for a given key combination.
    pub fn command_for(&self, binding: &KeyBinding, context: Option<&str>) -> Option<&str> {
        self.bindings
            .iter()
            .find(|e| e.binding == *binding && (e.when.is_none() || e.when.as_deref() == context))
            .map(|e| e.command.as_str())
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

    /// Whether a chord is a *user customization*: this config maps the exact
    /// global-scope (no `when`) chord to a command the built-in defaults do not
    /// map there identically. Stock chords deliberately return `None` so they
    /// stay on the hardcoded dispatch chain — the intercept therefore fires
    /// only for bindings a person actually added or rebound in the config file,
    /// and a fresh/default install changes no editor behavior at all.
    pub fn custom_command_for(&self, binding: &KeyBinding) -> Option<String> {
        let cmd = self.command_for(binding, None)?;
        if default_bindings()
            .command_for(binding, None)
            .is_some_and(|d| d == cmd)
        {
            None
        } else {
            Some(cmd.to_string())
        }
    }
}

/// The built-in default bindings as a lazily-built singleton, used to tell a
/// user's customizations apart from the stock mappings.
pub fn default_bindings() -> &'static KeybindingsConfig {
    static DEFAULTS: std::sync::LazyLock<KeybindingsConfig> =
        std::sync::LazyLock::new(KeybindingsConfig::defaults);
    &DEFAULTS
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

    #[test]
    fn alias_key_matches_live_egui_key() {
        use eframe::egui::{Key, Modifiers};
        // Word/symbol spellings a person types resolve to the Debug-named key.
        assert!(KeyBinding::new("Up").matches(&Modifiers::NONE, Key::ArrowUp));
        assert!(KeyBinding::new("Ctrl+1").matches(&Modifiers::CTRL, Key::Num1));
        assert!(KeyBinding::new("Shift+F12").matches(&Modifiers::SHIFT, Key::F12));
        assert!(KeyBinding::new("Esc").matches(&Modifiers::NONE, Key::Escape));
        assert!(KeyBinding::new("Ctrl+`").matches(&Modifiers::CTRL, Key::Backtick));
        // Wrong modifier state must not match, even with the right key.
        assert!(!KeyBinding::new("Ctrl+1").matches(&Modifiers::NONE, Key::Num1));
    }

    #[test]
    fn from_egui_round_trips_into_a_matching_binding() {
        use eframe::egui::{Key, Modifiers};
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        let chord = KeyBinding::from_egui(Key::S, &ctrl_shift);
        // The live event reconstructs the same Ctrl+Shift+S a config would hold.
        assert!(KeyBinding::new("Ctrl+Shift+S").matches(&ctrl_shift, Key::S));
        assert!(chord.matches(&ctrl_shift, Key::S));
    }

    #[test]
    fn stock_chord_is_not_a_customization() {
        let cfg = KeybindingsConfig::defaults();
        // Ctrl+S maps to its default command, so the intercept ignores it.
        assert_eq!(cfg.custom_command_for(&KeyBinding::new("Ctrl+S")), None);
    }

    #[test]
    fn rebound_chord_is_reported_as_a_customization() {
        let mut cfg = KeybindingsConfig::defaults();
        // Move file.save off Ctrl+S and onto Ctrl+K (a chord nothing else uses).
        cfg.set_binding("file.save", KeyBinding::new("Ctrl+K"));
        assert_eq!(
            cfg.custom_command_for(&KeyBinding::new("Ctrl+K"))
                .as_deref(),
            Some("file.save")
        );
        // The abandoned default chord no longer maps to anything here.
        assert_eq!(cfg.custom_command_for(&KeyBinding::new("Ctrl+S")), None);
    }

    #[test]
    fn added_chord_is_reported_as_a_customization() {
        let mut cfg = KeybindingsConfig::defaults();
        cfg.bindings.push(entry("nav.back", "Ctrl+Alt+G", None));
        assert_eq!(
            cfg.custom_command_for(&KeyBinding::new("Ctrl+Alt+G"))
                .as_deref(),
            Some("nav.back")
        );
    }
}
