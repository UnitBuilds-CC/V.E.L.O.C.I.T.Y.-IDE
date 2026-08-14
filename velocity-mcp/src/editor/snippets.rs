//! Snippet system — user-defined and built-in code templates with tab stops.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A code snippet with tab stop placeholders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub prefix: String,
    pub name: String,
    pub body: Vec<String>,
    pub description: Option<String>,
    pub scope: Option<String>,
}

impl Snippet {
    /// Expand the snippet body into insertable text, resolving placeholders.
    /// Supports: $N, ${N:default}, ${N|choice1,choice2|}, $TM_FILENAME, $TM_FILENAME_BASE,
    /// $CLIPBOARD, ${N/regex/format/} transforms.
    pub fn expand(&self, variables: &HashMap<String, String>) -> String {
        let raw = self.body.join("\n");
        let mut result = raw.clone();

        // Resolve environment/workspace variables first
        for (key, val) in variables {
            let pattern = format!("${{{}}}", key);
            result = result.replace(&pattern, val);
        }

        // Resolve choice placeholders: ${N|opt1,opt2,opt3|}
        for i in 0..10 {
            let prefix = format!("${{{i}|");
            while let Some(start) = result.find(&prefix) {
                let after = &result[start + prefix.len()..];
                if let Some(end) = after.find("|}") {
                    let choices = &after[..end];
                    let first_choice = choices.split(',').next().unwrap_or("").trim();
                    let full = format!("{}{}|}}", prefix, choices);
                    result = result.replacen(&full, first_choice, 1);
                } else {
                    break;
                }
            }
        }

        // Resolve transform placeholders: ${N/regex/format/flags}
        for i in 0..10 {
            let prefix = format!("${{{i}/");
            while let Some(start) = result.find(&prefix) {
                let after = &result[start + prefix.len()..];
                // Find the three / delimiters
                let parts: Vec<&str> = after.splitn(4, '/').collect();
                if parts.len() >= 3 {
                    let _regex_pattern = parts[0];
                    let format_str = parts[1];
                    let full_end =
                        prefix.len() + parts[0].len() + 1 + parts[1].len() + 1 + parts[2].len();
                    let full = &result[start..start + full_end + 1]; // +1 for closing }
                                                                     // Simple transform: just use the format string as replacement
                    result = result.replacen(full, format_str, 1);
                } else {
                    break;
                }
            }
        }

        // Resolve ${N:default} placeholders by keeping the default text
        for i in 0..10 {
            let placeholder = format!("${{{i}:");
            while let Some(start) = result.find(&placeholder) {
                let after = &result[start + placeholder.len()..];
                if let Some(end) = find_matching_brace(after) {
                    let default = &after[..end];
                    let full = format!("{}{}}}", placeholder, default);
                    result = result.replacen(&full, default, 1);
                } else {
                    break;
                }
            }
            // Remove bare tab stops $N
            result = result.replace(&format!("${i}"), "");
        }
        result = result.replace("$0", "");
        result
    }

    /// Get tab stop positions in the expanded text (for cursor navigation).
    pub fn tab_stops(&self) -> Vec<TabStop> {
        let raw = self.body.join("\n");
        let mut stops = Vec::new();
        let mut offset = 0;

        // Find $1, $2, etc.
        for (i, ch) in raw.chars().enumerate() {
            if ch == '$' {
                let rest = &raw[i + 1..];
                if let Some(digit) = rest.chars().next() {
                    if digit.is_ascii_digit() {
                        let idx = digit as u32 - '0' as u32;
                        stops.push(TabStop {
                            index: idx as usize,
                            offset: i - offset,
                            length: 0,
                        });
                        offset += 2; // remove $N from final length accounting
                    }
                }
            }
        }

        stops.sort_by_key(|s| s.index);
        stops
    }
}

/// Find the index of the matching closing brace, accounting for nesting.
fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 1i32;
    for (i, ch) in s.chars().enumerate() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// A tab stop in an expanded snippet.
#[derive(Debug, Clone)]
pub struct TabStop {
    pub index: usize,
    pub offset: usize,
    pub length: usize,
}

/// Snippet collection for a language.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnippetCollection {
    pub snippets: Vec<Snippet>,
}

impl SnippetCollection {
    /// Load snippets from a JSON file.
    pub fn load_from_file(path: &Path) -> Self {
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        // VS Code-style snippet format: { "name": { "prefix": "...", "body": [...] } }
        let Ok(map) = serde_json::from_str::<HashMap<String, Snippet>>(&content) else {
            return Self::default();
        };
        Self {
            snippets: map.into_values().collect(),
        }
    }

    /// Get snippets matching a prefix.
    pub fn matching(&self, prefix: &str) -> Vec<&Snippet> {
        let lower = prefix.to_lowercase();
        self.snippets
            .iter()
            .filter(|s| s.prefix.to_lowercase().starts_with(&lower))
            .collect()
    }

    /// Built-in Rust snippets.
    pub fn rust_builtins() -> Self {
        Self {
            snippets: vec![
                Snippet {
                    prefix: "fn".to_string(),
                    name: "Function".to_string(),
                    body: vec![
                        "fn ${1:name}(${2:params}) ${3:-> ReturnType }{".to_string(),
                        "    $0".to_string(),
                        "}".to_string(),
                    ],
                    description: Some("Function definition".to_string()),
                    scope: Some("rust".to_string()),
                },
                Snippet {
                    prefix: "impl".to_string(),
                    name: "Impl block".to_string(),
                    body: vec![
                        "impl ${1:Type} {".to_string(),
                        "    $0".to_string(),
                        "}".to_string(),
                    ],
                    description: Some("Impl block".to_string()),
                    scope: Some("rust".to_string()),
                },
                Snippet {
                    prefix: "test".to_string(),
                    name: "Test function".to_string(),
                    body: vec![
                        "#[test]".to_string(),
                        "fn ${1:test_name}() {".to_string(),
                        "    $0".to_string(),
                        "}".to_string(),
                    ],
                    description: Some("Test function".to_string()),
                    scope: Some("rust".to_string()),
                },
                Snippet {
                    prefix: "match".to_string(),
                    name: "Match expression".to_string(),
                    body: vec![
                        "match ${1:expr} {".to_string(),
                        "    ${2:pattern} => ${3:result},".to_string(),
                        "    $0".to_string(),
                        "}".to_string(),
                    ],
                    description: Some("Match expression".to_string()),
                    scope: Some("rust".to_string()),
                },
                Snippet {
                    prefix: "struct".to_string(),
                    name: "Struct definition".to_string(),
                    body: vec![
                        "struct ${1:Name} {".to_string(),
                        "    pub ${2:field}: ${3:Type},".to_string(),
                        "}".to_string(),
                    ],
                    description: Some("Struct definition".to_string()),
                    scope: Some("rust".to_string()),
                },
                Snippet {
                    prefix: "enum".to_string(),
                    name: "Enum definition".to_string(),
                    body: vec![
                        "enum ${1:Name} {".to_string(),
                        "    ${2:Variant},".to_string(),
                        "}".to_string(),
                    ],
                    description: Some("Enum definition".to_string()),
                    scope: Some("rust".to_string()),
                },
                Snippet {
                    prefix: "for".to_string(),
                    name: "For loop".to_string(),
                    body: vec![
                        "for ${1:item} in ${2:iter} {".to_string(),
                        "    $0".to_string(),
                        "}".to_string(),
                    ],
                    description: Some("For loop".to_string()),
                    scope: Some("rust".to_string()),
                },
                Snippet {
                    prefix: "if".to_string(),
                    name: "If statement".to_string(),
                    body: vec![
                        "if ${1:condition} {".to_string(),
                        "    $0".to_string(),
                        "}".to_string(),
                    ],
                    description: Some("If statement".to_string()),
                    scope: Some("rust".to_string()),
                },
            ],
        }
    }
}

/// Active snippet editing session (navigating tab stops).
#[derive(Debug, Clone, Default)]
pub struct SnippetSession {
    pub active: bool,
    pub stops: Vec<TabStop>,
    pub current_stop: usize,
    pub insert_offset: usize,
}

impl SnippetSession {
    pub fn start(stops: Vec<TabStop>, insert_offset: usize) -> Self {
        Self {
            active: !stops.is_empty(),
            stops,
            current_stop: 0,
            insert_offset,
        }
    }

    /// Advance to the next tab stop. Returns None if session is complete.
    pub fn next_stop(&mut self) -> Option<usize> {
        self.current_stop += 1;
        if self.current_stop >= self.stops.len() {
            self.active = false;
            None
        } else {
            Some(self.insert_offset + self.stops[self.current_stop].offset)
        }
    }

    /// Go back to previous tab stop.
    pub fn prev_stop(&mut self) -> Option<usize> {
        if self.current_stop > 0 {
            self.current_stop -= 1;
            Some(self.insert_offset + self.stops[self.current_stop].offset)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_expand() {
        let snippet = Snippet {
            prefix: "fn".to_string(),
            name: "Function".to_string(),
            body: vec![
                "fn ${1:name}() {".to_string(),
                "    $0".to_string(),
                "}".to_string(),
            ],
            description: None,
            scope: None,
        };
        let expanded = snippet.expand(&HashMap::new());
        assert!(expanded.contains("fn name()"));
        assert!(!expanded.contains("$"));
    }

    #[test]
    fn snippet_matching() {
        let collection = SnippetCollection::rust_builtins();
        let matches = collection.matching("fn");
        assert!(!matches.is_empty());
        assert_eq!(matches[0].prefix, "fn");
    }

    #[test]
    fn snippet_session() {
        let stops = vec![
            TabStop {
                index: 1,
                offset: 3,
                length: 4,
            },
            TabStop {
                index: 2,
                offset: 10,
                length: 0,
            },
        ];
        let mut session = SnippetSession::start(stops, 100);
        assert!(session.active);
        let next = session.next_stop();
        assert_eq!(next, Some(110));
    }
}
