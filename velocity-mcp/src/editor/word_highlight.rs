//! Word highlighting — visually marks all occurrences of the word under the
//! caret with a subtle background. Pure functions with no egui dependency.

/// Characters considered part of a "word" (alphanumeric + underscore).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Find the word boundaries (start, end char-index, end-exclusive) for the word
/// at `caret`. Returns `None` if the caret is not on or adjacent to a word.
pub fn word_at_caret(text: &str, caret: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n == 0 {
        return None;
    }

    // Determine which character the caret "belongs to".
    // If caret is at a word char → use it.
    // If caret is between two chars → prefer the one before.
    let idx = if caret < n && is_word_char(chars[caret]) {
        caret
    } else if caret > 0 && is_word_char(chars[caret - 1]) {
        caret - 1
    } else {
        return None;
    };

    // Expand to full word boundaries.
    let start = {
        let mut s = idx;
        while s > 0 && is_word_char(chars[s - 1]) {
            s -= 1;
        }
        s
    };
    let end = {
        let mut e = idx;
        while e < n && is_word_char(chars[e]) {
            e += 1;
        }
        e
    };

    Some((start, end))
}

/// Find all whole-word occurrences of the word under the caret.
/// Returns sorted, non-overlapping `(start, end)` char-index pairs.
pub fn find_word_occurrences(text: &str, caret: usize) -> Vec<(usize, usize)> {
    let (ws, we) = match word_at_caret(text, caret) {
        Some(r) => r,
        None => return Vec::new(),
    };
    let chars: Vec<char> = text.chars().collect();
    let word: String = chars[ws..we].iter().collect();
    if word.is_empty() {
        return Vec::new();
    }
    let word_lower = word.to_lowercase();
    let mut ranges = Vec::new();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if !is_word_char(chars[i]) {
            i += 1;
            continue;
        }
        // Start of a token — find its end.
        let tok_start = i;
        while i < n && is_word_char(chars[i]) {
            i += 1;
        }
        let tok: String = chars[tok_start..i].iter().collect();
        if tok.to_lowercase() == word_lower {
            ranges.push((tok_start, i));
        }
    }
    ranges
}

/// Compute a hash of the highlight ranges so the galley cache can bust when they
/// change. Uses FNV-1a.
pub fn highlight_hash(ranges: &[(usize, usize)]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (s, e) in ranges {
        h = h
            .wrapping_mul(0x1000_0000_0000_01b3)
            .wrapping_add(*s as u64);
        h = h
            .wrapping_mul(0x1000_0000_0000_01b3)
            .wrapping_add(*e as u64);
    }
    h
}

/// Check whether any char in `[pos, pos + len)` falls inside a highlight range.
/// Returns Some(true) if fully covered, Some(false) if partially, None if not at all.
/// Callers that want per-character splitting will use `char_highlighted`.
pub fn span_overlaps(pos: usize, len: usize, ranges: &[(usize, usize)]) -> bool {
    for &(rs, re) in ranges {
        if rs < pos + len && re > pos {
            return true;
        }
    }
    false
}

/// Check if a single char index is inside any highlight range.
pub fn char_highlighted(idx: usize, ranges: &[(usize, usize)]) -> bool {
    // Binary search: ranges are sorted by start.
    let pos = ranges.binary_search_by(|&(rs, re)| {
        if re <= idx {
            std::cmp::Ordering::Less
        } else if rs > idx {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    pos.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_at_caret_simple() {
        let text = "fn hello() { }";
        // 'h' is at char index 3. Caret at 5 (inside "hello").
        assert_eq!(word_at_caret(text, 5), Some((3, 8)));
    }

    #[test]
    fn word_at_caret_on_boundary() {
        let text = "abc def";
        // Caret at index 3 (space) → takes the word before: "abc" [0,3)
        assert_eq!(word_at_caret(text, 3), Some((0, 3)));
        // Caret at index 4 (d) → "def" [4,7)
        assert_eq!(word_at_caret(text, 4), Some((4, 7)));
    }

    #[test]
    fn word_at_caret_non_word() {
        let text = "(){}";
        // Caret at index 0 → '(' is not a word char
        assert_eq!(word_at_caret(text, 0), None);
    }

    #[test]
    fn find_all_occurrences() {
        let text = "foo bar foo baz foo";
        // caret at index 1 (inside first "foo")
        let ranges = find_word_occurrences(text, 1);
        assert_eq!(ranges, vec![(0, 3), (8, 11), (16, 19)]);
    }

    #[test]
    fn case_insensitive_matching() {
        let text = "Foo BAR foo bar FOO";
        let ranges = find_word_occurrences(text, 1); // "Foo" → matches "Foo", "foo", "FOO"
        assert_eq!(ranges, vec![(0, 3), (8, 11), (16, 19)]);
    }

    #[test]
    fn no_word_returns_empty() {
        let _text = "123";
        // '1' is alphanumeric so it IS a word char — let's use a symbol
        let text = "()()";
        assert!(find_word_occurrences(text, 0).is_empty());
    }

    #[test]
    fn char_highlighted_works() {
        let ranges = vec![(0, 3), (8, 11)];
        assert!(char_highlighted(0, &ranges));
        assert!(char_highlighted(2, &ranges));
        assert!(!char_highlighted(3, &ranges));
        assert!(char_highlighted(8, &ranges));
        assert!(!char_highlighted(11, &ranges));
    }

    #[test]
    fn span_overlaps_works() {
        let ranges = vec![(3, 8)];
        assert!(span_overlaps(0, 5, &ranges)); // [0,5) overlaps [3,8)
        assert!(!span_overlaps(0, 3, &ranges)); // [0,3) does not overlap [3,8)
        assert!(span_overlaps(5, 2, &ranges)); // [5,7) fully within [3,8)
    }

    #[test]
    fn highlight_hash_stable() {
        let a = vec![(0, 3), (8, 11)];
        let b = vec![(0, 3), (8, 11)];
        assert_eq!(highlight_hash(&a), highlight_hash(&b));
        let c = vec![(0, 3), (9, 12)];
        assert_ne!(highlight_hash(&a), highlight_hash(&c));
    }
}
