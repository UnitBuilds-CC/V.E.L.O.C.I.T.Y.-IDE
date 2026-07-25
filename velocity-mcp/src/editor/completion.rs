#![allow(dead_code)]
//! Code completion engine — provides suggestions from sitemap symbols,
//! keywords, and local identifiers.

use std::path::Path;
use eframe::egui;
use crate::editor::theme::IdePalette;

/// A single completion item.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    /// Text to insert (may differ from label for snippets).
    pub insert_text: String,
    /// Sort priority (lower = higher in list).
    pub sort_key: u32,
}

/// Kind of completion item (for icon rendering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Function,
    Variable,
    Type,
    Module,
    Field,
    Snippet,
    File,
}

impl CompletionKind {
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Keyword => "K",
            Self::Function => "f",
            Self::Variable => "v",
            Self::Type => "T",
            Self::Module => "M",
            Self::Field => ".",
            Self::Snippet => "S",
            Self::File => "F",
        }
    }
}

/// State for the completion popup.
#[derive(Debug, Clone, Default)]
pub struct CompletionState {
    pub active: bool,
    pub items: Vec<CompletionItem>,
    pub selected: usize,
    pub prefix: String,
    /// Character offset where the prefix starts in the buffer.
    pub prefix_start: usize,
    /// Filtered view indices (into `items`).
    pub filtered: Vec<usize>,
}

impl CompletionState {
    /// Convenience: compute completion items from sitemap symbols matching a prefix.
    pub fn compute_items(
        prefix: &str,
        workspace_symbols: &[crate::editor::search::SymbolEntry],
    ) -> Vec<CompletionItem> {
        from_sitemap_symbols(workspace_symbols, prefix)
    }

    /// Show the completion popup with the given items.
    pub fn show(&mut self, items: Vec<CompletionItem>) {
        self.active = true;
        self.items = items;
        self.selected = 0;
        self.refilter();
    }

    pub fn open(&mut self, prefix: String, prefix_start: usize, items: Vec<CompletionItem>) {
        self.active = true;
        self.prefix = prefix;
        self.prefix_start = prefix_start;
        self.items = items;
        self.selected = 0;
        self.refilter();
    }

    pub fn close(&mut self) {
        self.active = false;
        self.items.clear();
        self.filtered.clear();
    }

    pub fn refilter(&mut self) {
        let prefix_lower = self.prefix.to_lowercase();
        self.filtered = self.items.iter().enumerate()
            .filter(|(_, item)| {
                if prefix_lower.is_empty() {
                    return true;
                }
                fuzzy_match(&item.label.to_lowercase(), &prefix_lower)
            })
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }

    pub fn select_next(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1) % self.filtered.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = if self.selected == 0 {
                self.filtered.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    /// Get the currently selected item.
    pub fn current_item(&self) -> Option<&CompletionItem> {
        self.filtered.get(self.selected).and_then(|&i| self.items.get(i))
    }
}

/// Simple fuzzy match (subsequence).
fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|nc| chars.any(|hc| hc == nc))
}

// ─── Completion Providers ────────────────────────────────────────────────────

/// Language keywords for common languages.
pub fn rust_keywords() -> Vec<CompletionItem> {
    let kws = [
        "fn", "let", "mut", "const", "static", "struct", "enum", "impl", "trait",
        "pub", "use", "mod", "crate", "self", "super", "where", "type", "async",
        "await", "match", "if", "else", "for", "while", "loop", "break", "continue",
        "return", "unsafe", "extern", "dyn", "Box", "Vec", "String", "Option",
        "Result", "Some", "None", "Ok", "Err", "true", "false",
    ];
    kws.iter().map(|kw| CompletionItem {
        label: kw.to_string(),
        kind: CompletionKind::Keyword,
        detail: Some("keyword".to_string()),
        insert_text: kw.to_string(),
        sort_key: 100,
    }).collect()
}

pub fn typescript_keywords() -> Vec<CompletionItem> {
    let kws = [
        "function", "const", "let", "var", "class", "interface", "type", "enum",
        "import", "export", "from", "return", "if", "else", "for", "while",
        "switch", "case", "break", "continue", "try", "catch", "finally",
        "throw", "new", "this", "super", "extends", "implements", "async",
        "await", "yield", "typeof", "instanceof", "void", "null", "undefined",
        "true", "false", "string", "number", "boolean", "any", "never",
    ];
    kws.iter().map(|kw| CompletionItem {
        label: kw.to_string(),
        kind: CompletionKind::Keyword,
        detail: Some("keyword".to_string()),
        insert_text: kw.to_string(),
        sort_key: 100,
    }).collect()
}

/// Extract local identifiers from the current buffer for completion.
pub fn extract_local_identifiers(content: &str, current_word: &str) -> Vec<CompletionItem> {
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();

    for word in content.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if word.len() < 2 || word == current_word || !word.chars().next().map_or(false, |c| c.is_alphabetic() || c == '_') {
            continue;
        }
        if seen.insert(word.to_string()) {
            items.push(CompletionItem {
                label: word.to_string(),
                kind: CompletionKind::Variable,
                detail: None,
                insert_text: word.to_string(),
                sort_key: 50,
            });
        }
    }
    items
}

/// Build completion from sitemap symbols.
pub fn from_sitemap_symbols(symbols: &[crate::editor::search::SymbolEntry], prefix: &str) -> Vec<CompletionItem> {
    let prefix_lower = prefix.to_lowercase();
    symbols.iter()
        .filter(|s| s.name.to_lowercase().starts_with(&prefix_lower) || fuzzy_match(&s.name.to_lowercase(), &prefix_lower))
        .take(50)
        .map(|s| {
            let kind = if s.name.starts_with(|c: char| c.is_uppercase()) {
                CompletionKind::Type
            } else {
                CompletionKind::Function
            };
            CompletionItem {
                label: s.name.clone(),
                kind,
                detail: Some(format!("in {}", s.file)),
                insert_text: s.name.clone(),
                sort_key: 30,
            }
        })
        .collect()
}

/// Get keywords for a file extension.
pub fn keywords_for_extension(ext: &str) -> Vec<CompletionItem> {
    match ext {
        "rs" => rust_keywords(),
        "ts" | "tsx" | "js" | "jsx" => typescript_keywords(),
        _ => Vec::new(),
    }
}

/// Determine the word prefix at a cursor position (for triggering completion).
pub fn word_prefix_at(text: &str, cursor_offset: usize) -> (String, usize) {
    let before = &text[..cursor_offset.min(text.len())];
    let start = before.rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|i| i + 1)
        .unwrap_or(0);
    let prefix = before[start..].to_string();
    (prefix, start)
}

/// Get the file extension from a path.
pub fn extension_from_path(path: Option<&Path>) -> &str {
    path.and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .unwrap_or("txt")
}

/// Render the completion popup below the editor.
pub fn render_completion_popup(
    ui: &mut egui::Ui,
    state: &CompletionState,
    palette: IdePalette,
) {
    if state.filtered.is_empty() {
        return;
    }
    egui::Frame::new()
        .fill(palette.bg_secondary)
        .inner_margin(egui::Margin::same(4))
        .stroke(egui::Stroke::new(1.0, palette.border))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    for (idx, &item_idx) in state.filtered.iter().enumerate().take(15) {
                        if let Some(item) = state.items.get(item_idx) {
                            let is_selected = idx == state.selected;
                            let bg = if is_selected {
                                palette.accent.gamma_multiply(0.15)
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            egui::Frame::new().fill(bg).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.colored_label(palette.accent, item.kind.icon());
                                    ui.label(&item.label);
                                    if let Some(detail) = &item.detail {
                                        ui.colored_label(palette.text_muted, detail);
                                    }
                                });
                            });
                        }
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_prefix() {
        let (prefix, start) = word_prefix_at("let foo_bar = 1;", 11);
        assert_eq!(prefix, "foo_bar");
        assert_eq!(start, 4);
    }

    #[test]
    fn extract_identifiers() {
        let items = extract_local_identifiers("let hello = world; let foo = bar;", "");
        assert!(items.iter().any(|i| i.label == "hello"));
        assert!(items.iter().any(|i| i.label == "world"));
    }

    #[test]
    fn fuzzy_filter() {
        assert!(fuzzy_match("completionitem", "cpi"));
        assert!(!fuzzy_match("hello", "xyz"));
    }

    #[test]
    fn rust_keywords_nonempty() {
        assert!(rust_keywords().len() > 30);
    }
}
