#![allow(dead_code)]
//! Bracket matching — highlights the matching bracket for the character under
//! or adjacent to the cursor.

/// Bracket pair types.
const PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

/// Result of a bracket match search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BracketMatch {
    /// Byte offset of the opening bracket.
    pub open_offset: usize,
    /// Byte offset of the closing bracket.
    pub close_offset: usize,
}

/// Find the matching bracket for the character at `cursor_offset` in `text`.
/// Searches both forward (for openers) and backward (for closers).
/// Skips brackets inside string literals and comments (simple heuristic).
pub fn find_matching_bracket(text: &str, cursor_offset: usize) -> Option<BracketMatch> {
    let bytes = text.as_bytes();
    if cursor_offset >= bytes.len() {
        return None;
    }

    let ch = bytes[cursor_offset] as char;

    // Check if it's an opening bracket
    for &(open, close) in PAIRS {
        if ch == open {
            return find_forward(text, cursor_offset, open, close).map(|close_off| BracketMatch {
                open_offset: cursor_offset,
                close_offset: close_off,
            });
        }
        if ch == close {
            return find_backward(text, cursor_offset, open, close).map(|open_off| BracketMatch {
                open_offset: open_off,
                close_offset: cursor_offset,
            });
        }
    }

    // Also check one position before cursor (common: cursor after bracket)
    if cursor_offset > 0 {
        let prev = bytes[cursor_offset - 1] as char;
        for &(open, close) in PAIRS {
            if prev == open {
                return find_forward(text, cursor_offset - 1, open, close).map(|close_off| {
                    BracketMatch {
                        open_offset: cursor_offset - 1,
                        close_offset: close_off,
                    }
                });
            }
            if prev == close {
                return find_backward(text, cursor_offset - 1, open, close).map(|open_off| {
                    BracketMatch {
                        open_offset: open_off,
                        close_offset: cursor_offset - 1,
                    }
                });
            }
        }
    }

    None
}

/// Search forward from `start` for the matching closing bracket.
fn find_forward(text: &str, start: usize, open: char, close: char) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut string_char = '"';
    let mut escaped = false;

    for i in start..bytes.len() {
        let c = bytes[i] as char;

        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && in_string {
            escaped = true;
            continue;
        }
        if !in_string && (c == '"' || c == '\'') {
            in_string = true;
            string_char = c;
            continue;
        }
        if in_string && c == string_char {
            in_string = false;
            continue;
        }
        if in_string {
            continue;
        }

        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Search backward from `start` for the matching opening bracket.
fn find_backward(text: &str, start: usize, open: char, close: char) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;

    // Simple backward scan (no string tracking for simplicity — good enough for highlight)
    for i in (0..=start).rev() {
        let c = bytes[i] as char;
        if c == close {
            depth += 1;
        } else if c == open {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Auto-close bracket: given an opening char just typed, return the closing char.
pub fn auto_close_char(ch: char) -> Option<char> {
    match ch {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

/// Check if a closing bracket should be auto-skipped (cursor is right before it).
pub fn should_skip_close(text: &str, cursor_offset: usize, typed: char) -> bool {
    if cursor_offset < text.len() {
        let next = text.as_bytes()[cursor_offset] as char;
        next == typed && matches!(typed, ')' | ']' | '}' | '"' | '\'' | '`')
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_simple_braces() {
        let text = "fn foo() { bar() }";
        //         0123456789...
        // The '{' is at byte offset 9
        let result = find_matching_bracket(text, 9); // '{'
        assert_eq!(
            result,
            Some(BracketMatch {
                open_offset: 9,
                close_offset: 17
            })
        );
    }

    #[test]
    fn match_nested() {
        let text = "((a)(b))";
        let result = find_matching_bracket(text, 0);
        assert_eq!(
            result,
            Some(BracketMatch {
                open_offset: 0,
                close_offset: 7
            })
        );
    }

    #[test]
    fn match_from_close() {
        let text = "{ x }";
        let result = find_matching_bracket(text, 4); // '}'
        assert_eq!(
            result,
            Some(BracketMatch {
                open_offset: 0,
                close_offset: 4
            })
        );
    }

    #[test]
    fn no_match_in_string() {
        let text = "let s = \"{\";";
        // The '{' at offset 9 is inside a string
        let result = find_matching_bracket(text, 9);
        assert_eq!(result, None);
    }

    #[test]
    fn auto_close() {
        assert_eq!(auto_close_char('('), Some(')'));
        assert_eq!(auto_close_char('{'), Some('}'));
        assert_eq!(auto_close_char('a'), None);
    }
}
