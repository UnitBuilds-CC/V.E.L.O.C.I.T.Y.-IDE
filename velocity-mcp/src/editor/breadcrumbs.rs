#![allow(dead_code)]
//! Breadcrumb navigation — shows the file path segments and symbol hierarchy
//! above the editor for quick navigation.

use std::path::{Path, PathBuf};
use eframe::egui;

/// A single breadcrumb segment.
#[derive(Debug, Clone)]
pub struct BreadcrumbSegment {
    pub label: String,
    pub kind: BreadcrumbKind,
    /// For file segments: the path up to this point.
    pub path: Option<PathBuf>,
    /// For symbol segments: the symbol name for go-to.
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreadcrumbKind {
    Root,
    Directory,
    File,
    Symbol,
}

/// Build breadcrumb segments from a file path relative to workspace root.
pub fn build_breadcrumbs(
    workspace_root: &Path,
    file_path: &Path,
    current_symbol: Option<&str>,
) -> Vec<BreadcrumbSegment> {
    let mut segments = Vec::new();

    // Workspace root name
    let root_name = workspace_root.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    segments.push(BreadcrumbSegment {
        label: root_name,
        kind: BreadcrumbKind::Root,
        path: Some(workspace_root.to_path_buf()),
        symbol: None,
    });

    // Relative path segments
    if let Ok(relative) = file_path.strip_prefix(workspace_root) {
        let components: Vec<&std::ffi::OsStr> = relative.iter().collect();
        let mut accumulated = workspace_root.to_path_buf();

        for (i, component) in components.iter().enumerate() {
            accumulated = accumulated.join(component);
            let label = component.to_string_lossy().to_string();
            let is_last = i == components.len() - 1;

            segments.push(BreadcrumbSegment {
                label,
                kind: if is_last { BreadcrumbKind::File } else { BreadcrumbKind::Directory },
                path: Some(accumulated.clone()),
                symbol: None,
            });
        }
    } else {
        // File not under workspace root — just show file name
        let name = file_path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        segments.push(BreadcrumbSegment {
            label: name,
            kind: BreadcrumbKind::File,
            path: Some(file_path.to_path_buf()),
            symbol: None,
        });
    }

    // Current symbol (if known)
    if let Some(sym) = current_symbol {
        segments.push(BreadcrumbSegment {
            label: sym.to_string(),
            kind: BreadcrumbKind::Symbol,
            path: None,
            symbol: Some(sym.to_string()),
        });
    }

    segments
}

/// Render breadcrumbs in egui. Returns the clicked segment (if any).
pub fn render_breadcrumbs(
    ui: &mut egui::Ui,
    segments: &[BreadcrumbSegment],
    palette: &crate::editor::theme::IdePalette,
) -> Option<BreadcrumbAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;

        for (i, segment) in segments.iter().enumerate() {
            if i > 0 {
                ui.colored_label(palette.text_muted, "\u{203A}"); // › separator
            }

            let color = match segment.kind {
                BreadcrumbKind::Root => palette.text_muted,
                BreadcrumbKind::Directory => palette.text_muted,
                BreadcrumbKind::File => palette.text,
                BreadcrumbKind::Symbol => palette.accent,
            };

            let resp = ui.add(egui::Label::new(
                egui::RichText::new(&segment.label).color(color).size(12.0)
            ).sense(egui::Sense::click()));

            if resp.clicked() {
                action = Some(match segment.kind {
                    BreadcrumbKind::Root | BreadcrumbKind::Directory => {
                        BreadcrumbAction::OpenDirectory(segment.path.clone().unwrap_or_default())
                    }
                    BreadcrumbKind::File => {
                        BreadcrumbAction::OpenFile(segment.path.clone().unwrap_or_default())
                    }
                    BreadcrumbKind::Symbol => {
                        BreadcrumbAction::JumpToSymbol(segment.symbol.clone().unwrap_or_default())
                    }
                });
            }
        }
    });

    action
}

/// Actions from breadcrumb clicks.
#[derive(Debug, Clone)]
pub enum BreadcrumbAction {
    OpenDirectory(PathBuf),
    OpenFile(PathBuf),
    JumpToSymbol(String),
}

// ─── Editor scroll-position persistence ──────────────────────────────────────

/// Per-tab editor view state (persisted across tab switches).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EditorViewState {
    /// Vertical scroll offset in pixels.
    pub scroll_y: f32,
    /// Cursor char offset.
    pub cursor_offset: usize,
    /// First visible line (for restoring viewport).
    pub top_line: usize,
    /// Whether word wrap is enabled for this buffer.
    pub word_wrap: bool,
}

/// Map of file path → view state for persistence.
pub type ViewStateMap = std::collections::HashMap<PathBuf, EditorViewState>;

/// Save view states to the workspace preferences directory.
pub fn save_view_states(workspace_root: &Path, states: &ViewStateMap) {
    let path = workspace_root.join(".velocity").join("editor-states.json");
    if let Ok(json) = serde_json::to_string(states) {
        let _ = std::fs::write(path, json);
    }
}

/// Load view states from the workspace preferences directory.
pub fn load_view_states(workspace_root: &Path) -> ViewStateMap {
    let path = workspace_root.join(".velocity").join("editor-states.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

// ─── Word Wrap Toggle ────────────────────────────────────────────────────────

/// Word wrap configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[derive(Default)]
pub enum WordWrapMode {
    #[default]
    Off,
    On,
    /// Wrap at a specific column.
    Column(u16),
}


impl WordWrapMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "No Wrap",
            Self::On => "Word Wrap",
            Self::Column(_) => "Wrap at Column",
        }
    }

    pub fn toggle(&self) -> Self {
        match self {
            Self::Off => Self::On,
            Self::On => Self::Off,
            Self::Column(_) => Self::Off,
        }
    }

    /// Get the max width for egui LayoutJob wrapping.
    pub fn wrap_width(&self, viewport_width: f32) -> f32 {
        match self {
            Self::Off => f32::INFINITY,
            Self::On => viewport_width,
            Self::Column(n) => *n as f32 * 7.8, // approximate char width
        }
    }
}

// ─── Project Creation Wizard ─────────────────────────────────────────────────

/// Template for project creation.
#[derive(Debug, Clone)]
pub struct ProjectTemplate {
    pub name: &'static str,
    pub description: &'static str,
    pub language: &'static str,
    pub init_commands: Vec<&'static str>,
}

/// Get available project templates.
pub fn project_templates() -> Vec<ProjectTemplate> {
    vec![
        ProjectTemplate {
            name: "Rust Binary",
            description: "A new Rust binary crate with Cargo",
            language: "rust",
            init_commands: vec!["cargo init --name {name}"],
        },
        ProjectTemplate {
            name: "Rust Library",
            description: "A new Rust library crate with Cargo",
            language: "rust",
            init_commands: vec!["cargo init --lib --name {name}"],
        },
        ProjectTemplate {
            name: "Node.js (TypeScript)",
            description: "A Node.js project with TypeScript",
            language: "typescript",
            init_commands: vec!["npm init -y", "npm install -D typescript @types/node", "npx tsc --init"],
        },
        ProjectTemplate {
            name: "React App (Vite)",
            description: "React app with Vite and TypeScript",
            language: "typescript",
            init_commands: vec!["npm create vite@latest {name} -- --template react-ts"],
        },
        ProjectTemplate {
            name: "Python",
            description: "A Python project with virtual environment",
            language: "python",
            init_commands: vec!["python -m venv .venv"],
        },
        ProjectTemplate {
            name: "Empty",
            description: "An empty project directory",
            language: "none",
            init_commands: vec![],
        },
    ]
}

/// Project wizard state.
#[derive(Debug, Clone, Default)]
pub struct ProjectWizardState {
    pub visible: bool,
    pub name: String,
    pub location: String,
    pub selected_template: usize,
    pub error: Option<String>,
}

impl ProjectWizardState {
    pub fn open(&mut self) {
        self.visible = true;
        self.name.clear();
        self.error = None;
    }

    pub fn create_project(&mut self) -> Result<PathBuf, String> {
        if self.name.trim().is_empty() {
            return Err("Project name cannot be empty".to_string());
        }
        if self.location.trim().is_empty() {
            return Err("Location cannot be empty".to_string());
        }

        let project_path = PathBuf::from(&self.location).join(&self.name);
        std::fs::create_dir_all(&project_path)
            .map_err(|e| format!("Failed to create directory: {}", e))?;

        let templates = project_templates();
        if let Some(template) = templates.get(self.selected_template) {
            for cmd_template in &template.init_commands {
                let cmd = cmd_template.replace("{name}", &self.name);
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                if let Some((program, args)) = parts.split_first() {
                    let _ = std::process::Command::new(program)
                        .args(args)
                        .current_dir(&project_path)
                        .output();
                }
            }
        }

        // Create .velocity directory
        let _ = std::fs::create_dir_all(project_path.join(".velocity"));

        self.visible = false;
        Ok(project_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breadcrumbs_from_path() {
        let root = Path::new("/workspace/project");
        let file = Path::new("/workspace/project/src/main.rs");
        let segments = build_breadcrumbs(root, file, Some("fn main"));
        assert!(segments.len() >= 4); // root, src, main.rs, fn main
        assert_eq!(segments.last().unwrap().kind, BreadcrumbKind::Symbol);
    }

    #[test]
    fn word_wrap_toggle() {
        let mode = WordWrapMode::Off;
        assert_eq!(mode.toggle(), WordWrapMode::On);
        assert_eq!(WordWrapMode::On.toggle(), WordWrapMode::Off);
    }

    #[test]
    fn project_templates_available() {
        let templates = project_templates();
        assert!(templates.len() >= 5);
    }

    #[test]
    fn word_wrap_width() {
        assert_eq!(WordWrapMode::Off.wrap_width(800.0), f32::INFINITY);
        assert_eq!(WordWrapMode::On.wrap_width(800.0), 800.0);
    }
}
