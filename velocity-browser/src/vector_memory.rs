use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VectorMemoryNode {
    pub id: String,
    pub session_id: String,
    pub url: String,
    pub text: String,
    pub triple_hash: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SiteVectorStore {
    pub nodes: Vec<VectorMemoryNode>,
    pub index: HashMap<u64, usize>,
}

impl SiteVectorStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, session_id: &str, url: &str, text: &str, triple_hash: u64) -> String {
        let id = format!("{}:{}", session_id, self.nodes.len());
        let node = VectorMemoryNode {
            id: id.clone(),
            session_id: session_id.to_string(),
            url: url.to_string(),
            text: text.to_string(),
            triple_hash,
        };
        let idx = self.nodes.len();
        self.nodes.push(node);
        self.index.insert(triple_hash, idx);
        id
    }

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
}
