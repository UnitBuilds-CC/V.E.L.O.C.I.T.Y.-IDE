//! Unified Knowledge / RAG layer.
//!
//! A persistent, chunked, multi-source retrieval store that any agent can query
//! — the workspace's shared long-term memory over arbitrary content (docs,
//! notes, source files, transcripts) rather than the code-only
//! [`crate::editor::semantic_search::SemanticIndex`].
//!
//! Ranking uses the same TF-IDF + cosine-similarity approach as semantic
//! search, but operates over *chunks* (so a hit points at a passage, not a
//! whole file) and persists to `.velocity/knowledge/store.json` so knowledge
//! survives across sessions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Chunk sizing — passages are bounded by lines and characters so a hit is a
/// readable snippet and the vector space stays discriminative.
const CHUNK_LINES: usize = 25;
const CHUNK_CHARS: usize = 1600;
/// Minimum cosine similarity for a chunk to count as a hit.
const MIN_SCORE: f32 = 0.01;

/// File extensions treated as plain-text extractable. The extractor is a seam:
/// future binary formats (PDF, docx) can plug in without touching callers.
const TEXT_EXTENSIONS: &[&str] = &[
    "md", "txt", "rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "c", "cpp", "h", "hpp", "cs",
    "rb", "toml", "yaml", "yml", "json", "csv", "sql", "sh", "html", "css", "log", "ini", "cfg",
];

/// One indexed passage of a source.
#[derive(Debug, Clone)]
struct Chunk {
    source: String,
    ordinal: usize,
    text: String,
    raw: HashMap<String, usize>,
    terms: HashMap<String, f32>,
    magnitude: f32,
}

/// A ranked retrieval result.
#[derive(Debug, Clone)]
pub struct KnowledgeHit {
    pub source: String,
    pub ordinal: usize,
    pub score: f32,
    pub snippet: String,
}

/// Persisted form: only source/ordinal/text are stored; TF-IDF vectors are
/// recomputed on load so the on-disk format stays small and robust.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistChunk {
    source: String,
    ordinal: usize,
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistStore {
    chunks: Vec<PersistChunk>,
}

/// The workspace knowledge base.
#[derive(Debug, Clone, Default)]
pub struct KnowledgeBase {
    chunks: Vec<Chunk>,
    idf: HashMap<String, f32>,
}

impl KnowledgeBase {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingest raw text under a named source, replacing any previous content for
    /// that source. Returns the number of chunks added.
    pub fn ingest_text(&mut self, source: &str, text: &str) -> usize {
        self.chunks.retain(|c| c.source != source);
        let mut added = 0;
        for (ordinal, chunk_text) in chunk_text(text).into_iter().enumerate() {
            let raw = term_counts(&chunk_text);
            if raw.is_empty() {
                continue;
            }
            self.chunks.push(Chunk {
                source: source.to_string(),
                ordinal,
                text: chunk_text,
                raw,
                terms: HashMap::new(),
                magnitude: 0.0,
            });
            added += 1;
        }
        self.rebuild();
        added
    }

    /// Ingest a single file (if its extension is extractable). Source name is
    /// the path relative to `workspace_root` when possible.
    pub fn ingest_path(&mut self, workspace_root: &Path, path: &Path) -> Result<usize, String> {
        let text = extract_text(path)
            .ok_or_else(|| format!("unsupported or unreadable file: {}", path.display()))?;
        let source = path
            .strip_prefix(workspace_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        Ok(self.ingest_text(&source, &text))
    }

    /// Ingest every extractable file under a directory (recursively), skipping
    /// hidden/build directories. Returns (files_ingested, chunks_added).
    pub fn ingest_dir(&mut self, workspace_root: &Path, dir: &Path) -> (usize, usize) {
        let mut files = Vec::new();
        walk(dir, &mut files);
        let mut ingested = 0;
        let mut chunks = 0;
        for file in files {
            if let Ok(added) = self.ingest_path(workspace_root, &file) {
                ingested += 1;
                chunks += added;
            }
        }
        (ingested, chunks)
    }

    /// Query the store, returning up to `k` ranked passages.
    pub fn search(&self, query: &str, k: usize) -> Vec<KnowledgeHit> {
        if self.chunks.is_empty() || query.trim().is_empty() {
            return Vec::new();
        }
        let q_raw = term_counts(query);
        let mut q_vec: HashMap<String, f32> = HashMap::new();
        let mut q_mag_sq = 0.0f32;
        for (term, count) in &q_raw {
            let tf = 1.0 + (*count as f32).ln();
            let idf = self.idf.get(term).copied().unwrap_or(1.0);
            let w = tf * idf;
            q_vec.insert(term.clone(), w);
            q_mag_sq += w * w;
        }
        let q_mag = q_mag_sq.sqrt().max(1e-10);

        let mut scored: Vec<(usize, f32)> = Vec::new();
        for (idx, chunk) in self.chunks.iter().enumerate() {
            let mut dot = 0.0f32;
            for (term, qw) in &q_vec {
                if let Some(dw) = chunk.terms.get(term) {
                    dot += qw * dw;
                }
            }
            let sim = dot / (q_mag * chunk.magnitude);
            if sim > MIN_SCORE {
                scored.push((idx, sim));
            }
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
            .into_iter()
            .map(|(idx, score)| {
                let c = &self.chunks[idx];
                KnowledgeHit {
                    source: c.source.clone(),
                    ordinal: c.ordinal,
                    score,
                    snippet: snippet(&c.text),
                }
            })
            .collect()
    }

    /// Distinct sources with their chunk counts, sorted by name.
    pub fn sources(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for c in &self.chunks {
            *counts.entry(c.source.as_str()).or_default() += 1;
        }
        let mut out: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(s, n)| (s.to_string(), n))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Remove all chunks for a source. Returns whether anything was removed.
    pub fn remove_source(&mut self, source: &str) -> bool {
        let before = self.chunks.len();
        self.chunks.retain(|c| c.source != source);
        let removed = self.chunks.len() != before;
        if removed {
            self.rebuild();
        }
        removed
    }

    /// Drop all knowledge.
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.idf.clear();
    }

    /// Total number of indexed chunks.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Persist to `.velocity/knowledge/store.json`.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let store = PersistStore {
            chunks: self
                .chunks
                .iter()
                .map(|c| PersistChunk {
                    source: c.source.clone(),
                    ordinal: c.ordinal,
                    text: c.text.clone(),
                })
                .collect(),
        };
        let json = serde_json::to_vec_pretty(&store)
            .map_err(|e| format!("knowledge serialize failed: {e}"))?;
        let dir = workspace_root.join(".velocity").join("knowledge");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create knowledge dir: {e}"))?;
        std::fs::write(dir.join("store.json"), &json)
            .map_err(|e| format!("cannot write knowledge store: {e}"))
    }

    /// Load from `.velocity/knowledge/store.json`, recomputing vectors. A
    /// missing or corrupt store yields an empty base.
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_root
            .join(".velocity")
            .join("knowledge")
            .join("store.json");
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::new();
        };
        let Ok(store) = serde_json::from_slice::<PersistStore>(&bytes) else {
            return Self::new();
        };
        let mut kb = Self::new();
        for pc in store.chunks {
            let raw = term_counts(&pc.text);
            if raw.is_empty() {
                continue;
            }
            kb.chunks.push(Chunk {
                source: pc.source,
                ordinal: pc.ordinal,
                text: pc.text,
                raw,
                terms: HashMap::new(),
                magnitude: 0.0,
            });
        }
        kb.rebuild();
        kb
    }

    /// Recompute IDF and per-chunk TF-IDF vectors after any mutation.
    fn rebuild(&mut self) {
        let total = self.chunks.len();
        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        for c in &self.chunks {
            for term in c.raw.keys() {
                *doc_freq.entry(term.clone()).or_default() += 1;
            }
        }
        self.idf.clear();
        for (term, df) in &doc_freq {
            let idf = ((total as f32) / (*df as f32 + 1.0)).ln() + 1.0;
            self.idf.insert(term.clone(), idf);
        }
        for c in &mut self.chunks {
            let mut terms = HashMap::with_capacity(c.raw.len());
            let mut mag_sq = 0.0f32;
            for (term, count) in &c.raw {
                let tf = 1.0 + (*count as f32).ln();
                let idf = self.idf.get(term).copied().unwrap_or(1.0);
                let w = tf * idf;
                terms.insert(term.clone(), w);
                mag_sq += w * w;
            }
            c.terms = terms;
            c.magnitude = mag_sq.sqrt().max(1e-10);
        }
    }
}

/// Whether a path is extractable as text by its extension.
pub fn is_extractable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| TEXT_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Extract plain text from a file. The pluggable seam for binary formats.
pub fn extract_text(path: &Path) -> Option<String> {
    if !is_extractable(path) {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Split text into readable passages bounded by line and character counts.
fn chunk_text(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur = String::new();
    let mut lines = 0;
    for line in text.lines() {
        cur.push_str(line);
        cur.push('\n');
        lines += 1;
        if lines >= CHUNK_LINES || cur.len() >= CHUNK_CHARS {
            chunks.push(std::mem::take(&mut cur));
            lines = 0;
        }
    }
    if !cur.trim().is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// Tokenize + count terms (lowercased alphanumeric runs of length >= 2).
fn term_counts(content: &str) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut current = String::new();
    let flush = |cur: &mut String, counts: &mut HashMap<String, usize>| {
        if cur.len() >= 2 {
            *counts.entry(std::mem::take(cur)).or_default() += 1;
        } else {
            cur.clear();
        }
    };
    for ch in content.chars() {
        if ch.is_alphanumeric() {
            current.extend(ch.to_lowercase());
        } else {
            flush(&mut current, &mut counts);
        }
    }
    flush(&mut current, &mut counts);
    counts
}

/// A single-line preview of a chunk for display.
fn snippet(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 200 {
        let truncated: String = collapsed.chars().take(199).collect();
        format!("{truncated}…")
    } else {
        collapsed
    }
}

/// Recursively collect extractable files, skipping hidden/build directories.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
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
            walk(&path, out);
        } else if path.is_file() && is_extractable(&path) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_and_search_orders_by_relevance() {
        let mut kb = KnowledgeBase::new();
        kb.ingest_text(
            "auth.md",
            "Authentication uses login tokens and session cookies to verify users.",
        );
        kb.ingest_text(
            "db.md",
            "The database layer manages connection pools and SQL query execution.",
        );
        let hits = kb.search("login token authentication", 5);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].source, "auth.md");
    }

    #[test]
    fn empty_query_and_empty_store_return_empty() {
        let kb = KnowledgeBase::new();
        assert!(kb.search("anything", 5).is_empty());
        let mut kb2 = KnowledgeBase::new();
        kb2.ingest_text("s", "some content here");
        assert!(kb2.search("   ", 5).is_empty());
    }

    #[test]
    fn long_text_produces_multiple_chunks() {
        let mut kb = KnowledgeBase::new();
        let text: String = (0..60)
            .map(|i| format!("line number {i} with distinctive tokens alpha beta gamma\n"))
            .collect();
        let added = kb.ingest_text("big.txt", &text);
        assert!(added >= 2, "expected multiple chunks, got {added}");
        assert_eq!(kb.sources(), vec![("big.txt".to_string(), added)]);
    }

    #[test]
    fn reingest_source_replaces_previous() {
        let mut kb = KnowledgeBase::new();
        kb.ingest_text("note.md", "old content about widgets");
        kb.ingest_text("note.md", "new content about gadgets");
        assert_eq!(kb.sources().len(), 1);
        let hits = kb.search("widgets", 5);
        assert!(hits.is_empty(), "old content should be gone");
        assert!(!kb.search("gadgets", 5).is_empty());
    }

    #[test]
    fn remove_source_and_clear() {
        let mut kb = KnowledgeBase::new();
        kb.ingest_text("a", "alpha content one");
        kb.ingest_text("b", "beta content two");
        assert!(kb.remove_source("a"));
        assert_eq!(kb.sources().len(), 1);
        kb.clear();
        assert!(kb.is_empty());
    }

    #[test]
    fn persistence_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut kb = KnowledgeBase::new();
        kb.ingest_text(
            "doc.md",
            "persistent knowledge about neural networks and training",
        );
        kb.save(tmp.path()).expect("save");

        let loaded = KnowledgeBase::load(tmp.path());
        assert_eq!(loaded.sources().len(), 1);
        let hits = loaded.search("neural networks", 5);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].source, "doc.md");
    }

    #[test]
    fn ingest_file_and_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("readme.md"),
            "project overview and setup instructions",
        )
        .unwrap();
        std::fs::write(root.join("data.bin"), [0u8, 1, 2, 3]).unwrap();

        let mut kb = KnowledgeBase::new();
        let added = kb.ingest_path(root, &root.join("readme.md")).unwrap();
        assert!(added >= 1);
        // Binary file is not extractable.
        assert!(kb.ingest_path(root, &root.join("data.bin")).is_err());

        let mut kb2 = KnowledgeBase::new();
        let (files, chunks) = kb2.ingest_dir(root, root);
        assert_eq!(files, 1);
        assert!(chunks >= 1);
    }
}
