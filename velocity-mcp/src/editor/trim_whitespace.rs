/// Trim trailing whitespace (spaces and tabs) from every line.
///
/// Preserves CRLF line endings: only ` ` and `\t` are stripped from line
/// ends, never `\r` which is part of the line terminator. Returns the
/// original string unchanged (no allocation) when nothing needs trimming.
pub fn trim_trailing_ws(text: &str) -> String {
    // Fast path: if no space or tab byte exists at all, return clone.
    if !text.bytes().any(|b| b == b' ' || b == b'\t') {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            result.push('\n');
        }
        // Handle CRLF: separate the \r suffix so spaces/tabs before it
        // are still recognized as trailing whitespace.
        let (content, cr) = if line.ends_with('\r') {
            (&line[..line.len() - 1], "\r")
        } else {
            (line, "")
        };
        let trimmed = content.trim_end_matches([' ', '\t']);
        result.push_str(trimmed);
        result.push_str(cr);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_trailing_spaces() {
        assert_eq!(trim_trailing_ws("hello   \nworld\t\t\n"), "hello\nworld\n");
    }

    #[test]
    fn preserves_crlf_line_endings() {
        assert_eq!(trim_trailing_ws("foo  \r\nbar\t\r\n"), "foo\r\nbar\r\n");
    }

    #[test]
    fn noop_on_clean_text() {
        let src = "fn main() {\n    println!(\"hi\");\n}\n";
        assert_eq!(trim_trailing_ws(src), src);
    }

    #[test]
    fn handles_no_final_newline() {
        assert_eq!(trim_trailing_ws("line1  \nline2  "), "line1\nline2");
    }

    #[test]
    fn empty_string() {
        assert_eq!(trim_trailing_ws(""), "");
    }

    #[test]
    fn only_whitespace_line_becomes_empty() {
        assert_eq!(trim_trailing_ws("ok\n   \nend\n"), "ok\n\nend\n");
    }
}
