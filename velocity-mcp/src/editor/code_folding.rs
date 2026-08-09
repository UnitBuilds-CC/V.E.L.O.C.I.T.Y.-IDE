#![allow(dead_code)]
//! Code folding — detects foldable regions and manages collapsed state.
//!
//! Foldable regions are detected by indentation level and bracket blocks.

use std::collections::HashSet;

/// A foldable region in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRegion {
    /// First line of the fold (0-based). This line remains visible.
    pub start_line: usize,
    /// Last line of the fold (0-based, inclusive). Hidden when collapsed.
    pub end_line: usize,
    /// Nesting depth (for indent-based folds).
    pub depth: usize,
}

/// Manages fold state for a single buffer.
#[derive(Debug, Clone, Default)]
pub struct FoldState {
    /// Set of start_line values that are currently collapsed.
    pub collapsed: HashSet<usize>,
    /// Cached foldable regions. Recomputed when content changes.
    pub regions: Vec<FoldRegion>,
    /// Hash of content when regions were last computed.
    regions_hash: u64,
}

impl FoldState {
    /// Toggle fold at a given line. If the line starts a fold region, collapse/expand it.
    pub fn toggle(&mut self, line: usize) {
        if self.collapsed.contains(&line) {
            self.collapsed.remove(&line);
        } else if self.regions.iter().any(|r| r.start_line == line) {
            self.collapsed.insert(line);
        }
    }

    /// Collapse all foldable regions.
    pub fn collapse_all(&mut self) {
        for region in &self.regions {
            self.collapsed.insert(region.start_line);
        }
    }

    /// Expand all foldable regions.
    pub fn expand_all(&mut self) {
        self.collapsed.clear();
    }

    /// Check if a given line is hidden (inside a collapsed fold).
    pub fn is_line_hidden(&self, line: usize) -> bool {
        for &start in &self.collapsed {
            if let Some(region) = self.regions.iter().find(|r| r.start_line == start) {
                if line > region.start_line && line <= region.end_line {
                    return true;
                }
            }
        }
        false
    }

    /// Check if a line is a fold start.
    pub fn is_fold_start(&self, line: usize) -> bool {
        self.regions.iter().any(|r| r.start_line == line)
    }

    /// Check if a line is collapsed.
    pub fn is_collapsed(&self, line: usize) -> bool {
        self.collapsed.contains(&line)
    }

    /// Recompute fold regions from content. Cheap cache: skips if unchanged.
    pub fn recompute(&mut self, content: &str) {
        let h = super::buffer::fnv1a(content);
        if h == self.regions_hash {
            return;
        }
        self.regions_hash = h;
        self.regions = detect_fold_regions(content);
        // Remove collapsed entries that no longer have valid regions
        self.collapsed
            .retain(|line| self.regions.iter().any(|r| r.start_line == *line));
    }

    /// Get visible line numbers (filtering out hidden lines).
    pub fn visible_lines(&self, total_lines: usize) -> Vec<usize> {
        (0..total_lines)
            .filter(|l| !self.is_line_hidden(*l))
            .collect()
    }

    /// Count of hidden lines for display (e.g., "... 5 lines").
    pub fn hidden_count(&self, start_line: usize) -> usize {
        self.regions
            .iter()
            .find(|r| r.start_line == start_line)
            .map(|r| r.end_line - r.start_line)
            .unwrap_or(0)
    }

    /// Get the list of start_lines that are currently collapsed.
    pub fn collapsed_lines(&self) -> Vec<usize> {
        self.collapsed.iter().copied().collect()
    }
}

/// Detect foldable regions using indentation and bracket analysis.
pub fn detect_fold_regions(content: &str) -> Vec<FoldRegion> {
    let lines: Vec<&str> = content.lines().collect();
    let mut regions = Vec::new();

    if lines.is_empty() {
        return regions;
    }

    // Strategy 1: Bracket-based folding ({...}, where { is at end of line)
    let mut brace_stack: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.ends_with('{') || trimmed.ends_with("(") || trimmed.ends_with("[") {
            brace_stack.push(i);
        }
        if trimmed.starts_with('}')
            || trimmed.starts_with(')')
            || trimmed.starts_with(']')
            || trimmed == "}"
            || trimmed == ")"
            || trimmed == "]"
        {
            if let Some(start) = brace_stack.pop() {
                if i > start + 1 {
                    regions.push(FoldRegion {
                        start_line: start,
                        end_line: i,
                        depth: brace_stack.len(),
                    });
                }
            }
        }
    }

    // Strategy 2: Indentation-based folding (for languages without braces)
    // Only add if we found few brace-based regions
    if regions.len() < 3 {
        let indents: Vec<usize> = lines
            .iter()
            .map(|l| l.len() - l.trim_start().len())
            .collect();

        let mut i = 0;
        while i < lines.len() {
            if lines[i].trim().is_empty() {
                i += 1;
                continue;
            }
            let base_indent = indents[i];
            let mut end = i;
            for j in (i + 1)..lines.len() {
                if lines[j].trim().is_empty() {
                    continue;
                }
                if indents[j] > base_indent {
                    end = j;
                } else {
                    break;
                }
            }
            if end > i + 1 {
                // Avoid duplicating a brace-based region
                let already = regions.iter().any(|r| r.start_line == i);
                if !already {
                    regions.push(FoldRegion {
                        start_line: i,
                        end_line: end,
                        depth: base_indent / 4,
                    });
                }
            }
            i = end + 1;
        }
    }

    regions.sort_by_key(|r| r.start_line);
    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_brace_fold() {
        let code = "fn main() {\n    let x = 1;\n    let y = 2;\n}";
        let regions = detect_fold_regions(code);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start_line, 0);
        assert_eq!(regions[0].end_line, 3);
    }

    #[test]
    fn detect_nested_folds() {
        let code = "fn foo() {\n    if true {\n        x;\n    }\n}";
        let regions = detect_fold_regions(code);
        assert_eq!(regions.len(), 2);
    }

    #[test]
    fn fold_toggle_and_hidden() {
        let code = "fn main() {\n    a;\n    b;\n}";
        let mut state = FoldState::default();
        state.recompute(code);
        assert!(!state.is_line_hidden(1));
        state.toggle(0);
        assert!(state.is_line_hidden(1));
        assert!(state.is_line_hidden(2));
        assert!(state.is_line_hidden(3));
        state.toggle(0);
        assert!(!state.is_line_hidden(1));
    }

    #[test]
    fn visible_lines_filters() {
        let code = "a {\n  b\n  c\n}";
        let mut state = FoldState::default();
        state.recompute(code);
        state.toggle(0);
        let visible = state.visible_lines(4);
        assert_eq!(visible, vec![0]); // only first line visible
    }
}
