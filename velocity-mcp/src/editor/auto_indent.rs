//! Auto-indentation and smart editing helpers.
//!
//! Provides indent-on-Enter, dedent-on-closing-brace, and Tab/Shift+Tab
//! indent/dedent for the current selection or line.

/// Detect the indentation unit used in a file (spaces vs tabs, width).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Spaces(u8),
    Tabs,
}

impl Default for IndentStyle {
    fn default() -> Self {
        Self::Spaces(4)
    }
}

impl IndentStyle {
    /// Detect indent style from file content by sampling leading whitespace.
    pub fn detect(content: &str) -> Self {
        let mut tab_lines = 0u32;
        let mut space_lines = 0u32;
        let mut space_widths = [0u32; 9]; // index = width (1..8)

        for line in content.lines().take(200) {
            if line.starts_with('\t') {
                tab_lines += 1;
            } else if line.starts_with(' ') {
                space_lines += 1;
                let leading = line.len() - line.trim_start_matches(' ').len();
                if (1..=8).contains(&leading) {
                    space_widths[leading] += 1;
                }
            }
        }

        if tab_lines > space_lines {
            return Self::Tabs;
        }

        // Find the most common space indent width (usually 2 or 4)
        let mut best_width = 4u8;
        let mut best_count = 0u32;
        for w in [2u8, 4, 3, 8, 6] {
            let count = space_widths.get(w as usize).copied().unwrap_or(0);
            if count > best_count {
                best_count = count;
                best_width = w;
            }
        }
        Self::Spaces(best_width)
    }

    /// Return the string for one indent level.
    pub fn unit(&self) -> &'static str {
        match self {
            Self::Tabs => "\t",
            Self::Spaces(2) => "  ",
            Self::Spaces(3) => "   ",
            Self::Spaces(4) => "    ",
            Self::Spaces(8) => "        ",
            Self::Spaces(_) => "    ",
        }
    }

    /// Width in columns of one indent level.
    pub fn width(&self) -> usize {
        match self {
            Self::Tabs => 4,
            Self::Spaces(n) => *n as usize,
        }
    }
}

/// Given the line the cursor is on (just pressed Enter), compute the indent
/// for the new line. Handles:
/// - Carrying over current line's indentation
/// - Adding one level after `{`, `(`, `[`, `:` at end
/// - Dedenting after `}`, `)`, `]` at start of next content
pub fn compute_newline_indent(current_line: &str, style: IndentStyle) -> String {
    let leading = leading_whitespace(current_line);
    let trimmed = current_line.trim();

    // Check if line ends with an opening bracket/brace
    let opens = trimmed.ends_with('{')
        || trimmed.ends_with('(')
        || trimmed.ends_with('[')
        || trimmed.ends_with(':');

    if opens {
        format!("{}{}", leading, style.unit())
    } else {
        leading.to_string()
    }
}

/// Compute indent adjustment when a closing brace is typed.
/// Returns the new indentation for the line if it should be dedented.
pub fn compute_closing_dedent(current_line: &str, style: IndentStyle) -> Option<String> {
    let trimmed = current_line.trim();
    if trimmed == "}" || trimmed == ")" || trimmed == "]" {
        let leading = leading_whitespace(current_line);
        let unit_width = style.width();
        if leading.len() >= unit_width {
            return Some(leading[..leading.len() - unit_width].to_string());
        }
    }
    None
}

/// Indent a block of lines by one level.
pub fn indent_lines(text: &str, start_line: usize, end_line: usize, style: IndentStyle) -> String {
    let unit = style.unit();
    let mut result = String::with_capacity(text.len() + (end_line - start_line + 1) * unit.len());
    for (i, line) in text.lines().enumerate() {
        if i >= start_line && i <= end_line && !line.is_empty() {
            result.push_str(unit);
        }
        result.push_str(line);
        result.push('\n');
    }
    // Remove trailing newline if original didn't have one
    if !text.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

/// Dedent a block of lines by one level.
pub fn dedent_lines(text: &str, start_line: usize, end_line: usize, style: IndentStyle) -> String {
    let width = style.width();
    let mut result = String::with_capacity(text.len());
    for (i, line) in text.lines().enumerate() {
        if i >= start_line && i <= end_line {
            let leading_count = line.len() - line.trim_start().len();
            let remove = leading_count.min(width);
            // Only remove spaces/tabs
            let stripped = if line.starts_with('\t') {
                &line[1.min(leading_count)..]
            } else {
                &line[remove..]
            };
            result.push_str(stripped);
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }
    if !text.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

/// Extract leading whitespace from a line.
pub fn leading_whitespace(line: &str) -> &str {
    let trimmed = line.trim_start();
    &line[..line.len() - trimmed.len()]
}

/// Get the line index (0-based) for a given char offset in text.
pub fn line_at_offset(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count()
}

/// Get the start byte offset of a given line (0-based).
pub fn line_start_offset(text: &str, line: usize) -> usize {
    let mut offset = 0;
    for (i, l) in text.split('\n').enumerate() {
        if i == line {
            return offset;
        }
        offset += l.len() + 1;
    }
    offset.min(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_spaces_4() {
        let code = "fn main() {\n    let x = 1;\n    if x {\n        y\n    }\n}";
        assert_eq!(IndentStyle::detect(code), IndentStyle::Spaces(4));
    }

    #[test]
    fn detect_tabs() {
        let code = "fn main() {\n\tlet x = 1;\n\tif x {\n\t\ty\n\t}\n}";
        assert_eq!(IndentStyle::detect(code), IndentStyle::Tabs);
    }

    #[test]
    fn newline_indent_after_brace() {
        let indent = compute_newline_indent("    fn foo() {", IndentStyle::Spaces(4));
        assert_eq!(indent, "        ");
    }

    #[test]
    fn newline_indent_preserves() {
        let indent = compute_newline_indent("    let x = 1;", IndentStyle::Spaces(4));
        assert_eq!(indent, "    ");
    }

    #[test]
    fn closing_dedent() {
        let result = compute_closing_dedent("        }", IndentStyle::Spaces(4));
        assert_eq!(result, Some("    ".to_string()));
    }

    #[test]
    fn indent_block() {
        let text = "a\nb\nc";
        let result = indent_lines(text, 1, 1, IndentStyle::Spaces(4));
        assert_eq!(result, "a\n    b\nc");
    }

    #[test]
    fn dedent_block() {
        let text = "a\n    b\nc";
        let result = dedent_lines(text, 1, 1, IndentStyle::Spaces(4));
        assert_eq!(result, "a\nb\nc");
    }
}
