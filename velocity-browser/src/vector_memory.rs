//! Semantic vector memory with cosine similarity search.
//!
//! Stores page state embeddings so the agent can recall past experiences
//! when encountering structurally similar pages. Uses lightweight TF-IDF
//! style embeddings (no external model needed) for fast similarity search.

use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VectorMemoryNode {
    pub id: String,
    pub session_id: String,
    pub url: String,
    pub text: String,
    pub triple_hash: u64,
    /// Sparse TF-IDF embedding vector (term → weight)
    #[serde(default)]
    pub embedding: HashMap<String, f64>,
    /// Tags for categorical filtering
    #[serde(default)]
    pub tags: Vec<String>,
    /// Outcome score from the interaction on this page (0.0..=1.0)
    #[serde(default)]
    pub outcome_score: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SiteVectorStore {
    pub nodes: Vec<VectorMemoryNode>,
    pub index: HashMap<u64, usize>,
    /// IDF (inverse document frequency) table built from all stored documents
    pub idf_table: HashMap<String, f64>,
}

impl SiteVectorStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a node with auto-computed embedding.
    pub fn insert(&mut self, session_id: &str, url: &str, text: &str, triple_hash: u64) -> String {
        let id = format!("{}:{}", session_id, self.nodes.len());
        let embedding = self.compute_tf(text);
        let node = VectorMemoryNode {
            id: id.clone(),
            session_id: session_id.to_string(),
            url: url.to_string(),
            text: text.to_string(),
            triple_hash,
            embedding,
            tags: Vec::new(),
            outcome_score: 0.0,
        };
        let idx = self.nodes.len();
        self.nodes.push(node);
        self.index.insert(triple_hash, idx);
        self.rebuild_idf();
        id
    }

    /// Insert with tags and outcome score.
    pub fn insert_rich(
        &mut self,
        session_id: &str,
        url: &str,
        text: &str,
        triple_hash: u64,
        tags: Vec<String>,
        outcome_score: f64,
    ) -> String {
        let id = format!("{}:{}", session_id, self.nodes.len());
        let embedding = self.compute_tf(text);
        let node = VectorMemoryNode {
            id: id.clone(),
            session_id: session_id.to_string(),
            url: url.to_string(),
            text: text.to_string(),
            triple_hash,
            embedding,
            tags,
            outcome_score,
        };
        let idx = self.nodes.len();
        self.nodes.push(node);
        self.index.insert(triple_hash, idx);
        self.rebuild_idf();
        id
    }

    /// Keyword-based substring search (legacy, fast fallback).
    pub fn search(&self, query: &str, limit: usize) -> Vec<&VectorMemoryNode> {
        let query_lower = query.to_lowercase();
        let mut matches: Vec<_> = self
            .nodes
            .iter()
            .filter(|node| node.text.to_lowercase().contains(&query_lower) || node.url.to_lowercase().contains(&query_lower))
            .collect();
        matches.truncate(limit);
        matches
    }

    /// Semantic search using cosine similarity between TF-IDF vectors.
    /// Returns nodes sorted by relevance (highest similarity first).
    pub fn semantic_search(&self, query: &str, limit: usize) -> Vec<(&VectorMemoryNode, f64)> {
        let query_embedding = self.compute_tfidf(query);
        if query_embedding.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<_> = self.nodes.iter()
            .map(|node| {
                let node_tfidf = self.apply_idf(&node.embedding);
                let sim = cosine_similarity(&query_embedding, &node_tfidf);
                (node, sim)
            })
            .filter(|(_, sim)| *sim > 0.0)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
    }

    /// Find nodes similar to a given node (by embedding).
    pub fn find_similar(&self, node_id: &str, limit: usize) -> Vec<(&VectorMemoryNode, f64)> {
        let source = match self.nodes.iter().find(|n| n.id == node_id) {
            Some(n) => n,
            None => return Vec::new(),
        };
        let source_tfidf = self.apply_idf(&source.embedding);

        let mut scored: Vec<_> = self.nodes.iter()
            .filter(|n| n.id != node_id)
            .map(|node| {
                let node_tfidf = self.apply_idf(&node.embedding);
                let sim = cosine_similarity(&source_tfidf, &node_tfidf);
                (node, sim)
            })
            .filter(|(_, sim)| *sim > 0.0)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
    }

    /// Search by tag.
    pub fn search_by_tag(&self, tag: &str, limit: usize) -> Vec<&VectorMemoryNode> {
        let mut results: Vec<_> = self.nodes.iter()
            .filter(|n| n.tags.iter().any(|t| t == tag))
            .collect();
        results.truncate(limit);
        results
    }

    /// Get nodes with outcome scores above threshold (successful interactions).
    pub fn successful_interactions(&self, threshold: f64) -> Vec<&VectorMemoryNode> {
        self.nodes.iter()
            .filter(|n| n.outcome_score >= threshold)
            .collect()
    }

    /// Compute term frequency (TF) for a text.
    fn compute_tf(&self, text: &str) -> HashMap<String, f64> {
        let tokens = tokenize(text);
        let total = tokens.len() as f64;
        if total == 0.0 {
            return HashMap::new();
        }
        let mut freq: HashMap<String, f64> = HashMap::new();
        for token in &tokens {
            *freq.entry(token.clone()).or_insert(0.0) += 1.0;
        }
        for val in freq.values_mut() {
            *val /= total;
        }
        freq
    }

    /// Compute TF-IDF for a query text using the stored IDF table.
    fn compute_tfidf(&self, text: &str) -> HashMap<String, f64> {
        let tf = self.compute_tf(text);
        self.apply_idf(&tf)
    }

    /// Apply IDF weights to a TF vector.
    fn apply_idf(&self, tf: &HashMap<String, f64>) -> HashMap<String, f64> {
        tf.iter()
            .map(|(term, tf_val)| {
                let idf = self.idf_table.get(term).copied().unwrap_or(1.0);
                (term.clone(), tf_val * idf)
            })
            .collect()
    }

    /// Rebuild the IDF table from all stored documents.
    fn rebuild_idf(&mut self) {
        let n = self.nodes.len() as f64;
        if n == 0.0 {
            return;
        }

        let mut doc_freq: HashMap<String, u32> = HashMap::new();
        for node in &self.nodes {
            for term in node.embedding.keys() {
                *doc_freq.entry(term.clone()).or_insert(0) += 1;
            }
        }

        self.idf_table = doc_freq.into_iter()
            .map(|(term, df)| {
                let idf = (n / (df as f64 + 1.0)).ln() + 1.0;
                (term, idf)
            })
            .collect();
    }
}

/// Compute cosine similarity between two sparse vectors.
fn cosine_similarity(a: &HashMap<String, f64>, b: &HashMap<String, f64>) -> f64 {
    let dot: f64 = a.iter()
        .filter_map(|(k, v)| b.get(k).map(|bv| v * bv))
        .sum();

    let mag_a: f64 = a.values().map(|v| v * v).sum::<f64>().sqrt();
    let mag_b: f64 = b.values().map(|v| v * v).sum::<f64>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        0.0
    } else {
        dot / (mag_a * mag_b)
    }
}

/// Tokenize text into lowercase terms, filtering stopwords and short tokens.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '-')
        .map(|s| s.to_lowercase())
        .filter(|s| s.len() >= 3 && !is_stopword(s))
        .collect()
}

/// Common English stopwords to filter out.
fn is_stopword(word: &str) -> bool {
    matches!(word,
        "the" | "and" | "for" | "are" | "but" | "not" | "you" |
        "all" | "can" | "had" | "her" | "was" | "one" | "our" |
        "out" | "has" | "have" | "been" | "from" | "this" | "that" |
        "with" | "they" | "will" | "each" | "which" | "their" |
        "there" | "what" | "about" | "would" | "make" | "like" |
        "just" | "over" | "such" | "take" | "than" | "them" |
        "very" | "some" | "could" | "into" | "other" | "then" |
        "these" | "also" | "after" | "should" | "well" | "only"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_keyword_search() {
        let mut store = SiteVectorStore::new();
        store.insert("s1", "https://example.com", "login form with email and password", 100);
        store.insert("s1", "https://example.com/dashboard", "dashboard showing user stats", 200);

        let results = store.search("login", 10);
        assert_eq!(results.len(), 1);
        assert!(results[0].text.contains("login"));
    }

    #[test]
    fn semantic_search_finds_related() {
        let mut store = SiteVectorStore::new();
        store.insert("s1", "https://a.com", "user authentication login form email password", 1);
        store.insert("s1", "https://a.com", "product catalog listing items prices", 2);
        store.insert("s1", "https://a.com", "sign in credentials username password auth", 3);

        let results = store.semantic_search("login authentication password", 5);
        assert!(results.len() >= 2);
        // The auth-related pages should rank higher than product catalog
        let top_text = &results[0].0.text;
        assert!(top_text.contains("auth") || top_text.contains("login") || top_text.contains("password"));
    }

    #[test]
    fn cosine_similarity_identical_vectors() {
        let mut a = HashMap::new();
        a.insert("hello".to_string(), 1.0);
        a.insert("world".to_string(), 0.5);
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn cosine_similarity_orthogonal_vectors() {
        let mut a = HashMap::new();
        a.insert("hello".to_string(), 1.0);
        let mut b = HashMap::new();
        b.insert("world".to_string(), 1.0);
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn search_by_tag() {
        let mut store = SiteVectorStore::new();
        store.insert_rich("s1", "https://a.com", "page content", 1, vec!["login".into()], 0.9);
        store.insert_rich("s1", "https://b.com", "other page", 2, vec!["checkout".into()], 0.8);

        let results = store.search_by_tag("login", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://a.com");
    }

    #[test]
    fn successful_interactions_filter() {
        let mut store = SiteVectorStore::new();
        store.insert_rich("s1", "https://a.com", "good page", 1, vec![], 0.9);
        store.insert_rich("s1", "https://b.com", "bad page", 2, vec![], 0.2);

        let results = store.successful_interactions(0.5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://a.com");
    }

    #[test]
    fn tokenize_filters_stopwords() {
        let tokens = tokenize("the quick brown fox jumps over the lazy dog");
        assert!(!tokens.contains(&"the".to_string()));
        assert!(!tokens.contains(&"over".to_string()));
        assert!(tokens.contains(&"quick".to_string()));
        assert!(tokens.contains(&"brown".to_string()));
    }
}
