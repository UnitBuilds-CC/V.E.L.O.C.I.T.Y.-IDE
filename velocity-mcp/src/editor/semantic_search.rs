#![allow(dead_code)]
//! Semantic Code Search: TF-IDF vector-based search that understands code
//! meaning beyond literal string matching. Complements the existing
//! `project_search` with similarity-ranked results.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A document in the semantic index (one per file).
#[derive(Debug, Clone)]
struct Document {
    path: PathBuf,
    terms: HashMap<String, f32>,
    magnitude: f32,
}

/// A semantic search hit with relevance score.
#[derive(Debug, Clone)]
pub struct SemanticHit {
    pub path: PathBuf,
    pub score: f32,
    pub preview: String,
}

/// TF-IDF semantic search index for a workspace.
#[derive(Debug, Clone)]
pub struct SemanticIndex {
    documents: Vec<Document>,
    idf: HashMap<String, f32>,
    total_docs: usize,
}

impl Default for SemanticIndex {
    fn default() -> Self {
        Self {
            documents: Vec::new(),
            idf: HashMap::new(),
            total_docs: 0,
        }
    }
}

impl SemanticIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the index from a workspace root. Indexes all source files.
    pub fn build(workspace_root: &Path) -> Self {
        let mut index = Self::new();
        let files = collect_indexable_files(workspace_root, workspace_root);
        let total = files.len();

        // Compute document-frequency for each term
        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        let mut raw_docs: Vec<(PathBuf, HashMap<String, usize>)> = Vec::new();

        for file_path in &files {
            let Ok(content) = fs::read_to_string(file_path) else {
                continue;
            };
            let tf = compute_term_frequencies(&content);
            for term in tf.keys() {
                *doc_freq.entry(term.clone()).or_default() += 1;
            }
            let rel = file_path
                .strip_prefix(workspace_root)
                .unwrap_or(file_path)
                .to_path_buf();
            raw_docs.push((rel, tf));
        }

        // Compute IDF: log(N / df)
        for (term, df) in &doc_freq {
            let idf_value = ((total as f32) / (*df as f32 + 1.0)).ln() + 1.0;
            index.idf.insert(term.clone(), idf_value);
        }

        // Build TF-IDF vectors
        for (path, tf) in raw_docs {
            let mut terms: HashMap<String, f32> = HashMap::new();
            let mut mag_sq = 0.0f32;
            for (term, count) in &tf {
                let tf_weight = 1.0 + (*count as f32).ln();
                let idf_weight = index.idf.get(term).copied().unwrap_or(1.0);
                let tfidf = tf_weight * idf_weight;
                terms.insert(term.clone(), tfidf);
                mag_sq += tfidf * tfidf;
            }
            let magnitude = mag_sq.sqrt().max(1e-10);
            index.documents.push(Document {
                path,
                terms,
                magnitude,
            });
        }

        index.total_docs = total;
        index
    }

    /// Search the index with a natural-language or code query.
    pub fn search(&self, query: &str, max_results: usize) -> Vec<SemanticHit> {
        if self.documents.is_empty() || query.is_empty() {
            return Vec::new();
        }

        // Build query vector
        let query_tf = compute_term_frequencies(query);
        let mut query_vec: HashMap<String, f32> = HashMap::new();
        let mut query_mag_sq = 0.0f32;
        for (term, count) in &query_tf {
            let tf_weight = 1.0 + (*count as f32).ln();
            let idf_weight = self.idf.get(term).copied().unwrap_or(1.0);
            let tfidf = tf_weight * idf_weight;
            query_vec.insert(term.clone(), tfidf);
            query_mag_sq += tfidf * tfidf;
        }
        let query_mag = query_mag_sq.sqrt().max(1e-10);

        // Compute cosine similarity for each document
        let mut scores: Vec<(usize, f32)> = Vec::new();
        for (idx, doc) in self.documents.iter().enumerate() {
            let mut dot = 0.0f32;
            for (term, q_weight) in &query_vec {
                if let Some(d_weight) = doc.terms.get(term) {
                    dot += q_weight * d_weight;
                }
            }
            let similarity = dot / (query_mag * doc.magnitude);
            if similarity > 0.01 {
                scores.push((idx, similarity));
            }
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(max_results);

        scores
            .into_iter()
            .map(|(idx, score)| {
                let doc = &self.documents[idx];
                // Build preview from top terms
                let mut top_terms: Vec<_> = doc.terms.iter().collect();
                top_terms.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
                let preview: Vec<_> = top_terms.iter().take(5).map(|(t, _)| t.as_str()).collect();
                SemanticHit {
                    path: doc.path.clone(),
                    score,
                    preview: preview.join(", "),
                }
            })
            .collect()
    }

    /// Number of indexed documents.
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    /// Number of unique terms in the vocabulary.
    pub fn vocabulary_size(&self) -> usize {
        self.idf.len()
    }
}

/// Tokenize content into terms with frequency counts.
fn compute_term_frequencies(content: &str) -> HashMap<String, usize> {
    let mut tf: HashMap<String, usize> = HashMap::new();
    for token in tokenize(content) {
        *tf.entry(token).or_default() += 1;
    }
    tf
}

/// Simple tokenizer: splits on non-alphanumeric, lowercases, filters short tokens,
/// and handles camelCase/snake_case splitting.
fn tokenize(content: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in content.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
        } else {
            if !current.is_empty() {
                emit_tokens(&current, &mut tokens);
                current.clear();
            }
        }
    }
    if !current.is_empty() {
        emit_tokens(&current, &mut tokens);
    }
    tokens
}

/// Emit tokens from a word, splitting camelCase and snake_case.
fn emit_tokens(word: &str, tokens: &mut Vec<String>) {
    // Split on underscores
    for part in word.split('_') {
        if part.is_empty() {
            continue;
        }
        // Split camelCase
        let mut current = String::new();
        for ch in part.chars() {
            if ch.is_uppercase() && !current.is_empty() {
                let lower = current.to_lowercase();
                if lower.len() >= 2 {
                    tokens.push(lower);
                }
                current.clear();
            }
            current.push(ch);
        }
        if !current.is_empty() {
            let lower = current.to_lowercase();
            if lower.len() >= 2 {
                tokens.push(lower);
            }
        }
    }
    // Also emit the whole word lowercased
    let whole = word.to_lowercase();
    if whole.len() >= 2 {
        tokens.push(whole);
    }
}

/// Collect all indexable source files from a workspace.
fn collect_indexable_files(root: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_index(root, dir, &mut files);
    files
}

fn walk_index(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.')
            || name == "target"
            || name == "node_modules"
            || name == "__pycache__"
        {
            continue;
        }
        if path.is_dir() {
            walk_index(root, &path, files);
        } else if path.is_file() && is_indexable(&name) {
            files.push(path);
        }
    }
}

/// Check if a file is worth indexing by extension.
fn is_indexable(name: &str) -> bool {
    const EXTENSIONS: &[&str] = &[
        "rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "c", "cpp", "h",
        "hpp", "cs", "rb", "swift", "kt", "scala", "toml", "yaml", "yml",
        "json", "md", "txt", "sql", "sh", "bash", "zsh", "html", "css",
    ];
    name.rsplit('.')
        .next()
        .map(|ext| EXTENSIONS.contains(&ext))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_camel_case() {
        let tokens = tokenize("processUserData");
        assert!(tokens.contains(&"process".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"data".to_string()));
    }

    #[test]
    fn tokenize_snake_case() {
        let tokens = tokenize("process_user_data");
        assert!(tokens.contains(&"process".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"data".to_string()));
    }

    #[test]
    fn tf_idf_basic_search() {
        let index = SemanticIndex {
            documents: vec![
                Document {
                    path: PathBuf::from("auth.rs"),
                    terms: [("auth".into(), 3.0), ("login".into(), 2.0), ("token".into(), 1.5)]
                        .into_iter()
                        .collect(),
                    magnitude: 4.0,
                },
                Document {
                    path: PathBuf::from("db.rs"),
                    terms: [("database".into(), 3.0), ("query".into(), 2.0), ("pool".into(), 1.5)]
                        .into_iter()
                        .collect(),
                    magnitude: 4.0,
                },
            ],
            idf: [
                ("auth".into(), 2.0),
                ("login".into(), 2.0),
                ("token".into(), 1.5),
                ("database".into(), 2.0),
                ("query".into(), 1.5),
                ("pool".into(), 1.5),
            ]
            .into_iter()
            .collect(),
            total_docs: 2,
        };

        let results = index.search("authentication login", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].path, PathBuf::from("auth.rs"));
    }

    #[test]
    fn empty_index_returns_empty() {
        let index = SemanticIndex::new();
        let results = index.search("anything", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn is_indexable_checks() {
        assert!(is_indexable("main.rs"));
        assert!(is_indexable("app.tsx"));
        assert!(!is_indexable("image.png"));
        assert!(!is_indexable("binary.exe"));
    }
}
