//! PageRank computation for wiki reading order.
//!
//! Computes importance scores for files and symbols based on the call/import
//! graph, enabling a "Start Here" guided reading path through the codebase.

use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// PageRank scores for all nodes in the graph.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PageRankScores {
    /// Scores keyed by node name (file path or symbol name)
    pub scores: HashMap<String, f64>,
    /// Nodes sorted by score descending (most important first)
    pub ranking: Vec<String>,
    /// Average score (for normalization)
    pub avg_score: f64,
    /// Maximum score (for normalization)
    pub max_score: f64,
}

impl PageRankScores {
    /// Get the score for a node (0.0 if not found).
    pub fn get(&self, node: &str) -> f64 {
        self.scores.get(node).copied().unwrap_or(0.0)
    }

    /// Get the normalized score (0.0 to 1.0) for a node.
    pub fn normalized(&self, node: &str) -> f64 {
        if self.max_score == 0.0 {
            return 0.0;
        }
        self.get(node) / self.max_score
    }

    /// Get the top N most important nodes.
    pub fn top_n(&self, n: usize) -> Vec<(&str, f64)> {
        self.ranking
            .iter()
            .take(n)
            .map(|name| (name.as_str(), self.get(name)))
            .collect()
    }

    /// Check if a node is in the top tier (top 10%).
    pub fn is_top_tier(&self, node: &str) -> bool {
        let rank = self.ranking.iter().position(|n| n == node);
        if let Some(rank) = rank {
            let tier_size = (self.ranking.len() as f64 * 0.1).ceil() as usize;
            rank < tier_size.max(1)
        } else {
            false
        }
    }

    /// Get nodes above a certain score threshold.
    pub fn above_threshold(&self, threshold: f64) -> Vec<(&str, f64)> {
        self.ranking
            .iter()
            .filter_map(|name| {
                let score = self.get(name);
                if score >= threshold {
                    Some((name.as_str(), score))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Generate a "Start Here" reading order for files.
    pub fn reading_order(&self, files: &[&str]) -> Vec<String> {
        let mut ordered: Vec<&str> = files.to_vec();
        ordered.sort_by(|a, b| {
            let score_a = self.get(a);
            let score_b = self.get(b);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ordered.into_iter().map(|s| s.to_string()).collect()
    }
}

/// An edge in the dependency graph.
#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub weight: f64,
}

/// Compute PageRank scores for a set of nodes and edges.
///
/// Uses the standard PageRank algorithm with damping factor.
///
/// # Arguments
/// * `nodes` - All node names (files and symbols)
/// * `edges` - Directed edges (from -> to) with optional weights
/// * `damping` - Damping factor (typically 0.85)
/// * `iterations` - Number of iterations to run
///
/// # Returns
/// PageRankScores with scores for all nodes
pub fn compute_pagerank(
    nodes: &[String],
    edges: &[GraphEdge],
    damping: f64,
    iterations: usize,
) -> PageRankScores {
    if nodes.is_empty() {
        return PageRankScores::default();
    }

    let n = nodes.len();
    let node_indices: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();

    // Build adjacency list (outgoing edges)
    let mut out_edges: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    // Track in-degree for each node
    let mut in_degree: Vec<usize> = vec![0; n];

    for edge in edges {
        if let (Some(&from_idx), Some(&to_idx)) = (
            node_indices.get(edge.from.as_str()),
            node_indices.get(edge.to.as_str()),
        ) {
            if from_idx != to_idx {
                // Avoid self-loops
                out_edges[from_idx].push((to_idx, edge.weight));
                in_degree[to_idx] += 1;
            }
        }
    }

    // Initialize scores uniformly
    let initial_score = 1.0 / n as f64;
    let mut scores: Vec<f64> = vec![initial_score; n];
    let mut new_scores: Vec<f64> = vec![0.0; n];

    // Power iteration
    for _ in 0..iterations {
        // Reset new scores to the teleportation factor
        let teleport = (1.0 - damping) / n as f64;
        new_scores.fill(teleport);

        // Distribute scores along edges
        for (from_idx, out) in out_edges.iter().enumerate() {
            if out.is_empty() {
                // Dangling node: distribute score to all nodes
                let share = damping * scores[from_idx] / n as f64;
                for new_score in new_scores.iter_mut() {
                    *new_score += share;
                }
            } else {
                // Distribute proportionally to edge weights
                let total_weight: f64 = out.iter().map(|(_, w)| w).sum();
                for &(to_idx, weight) in out {
                    let share = damping * scores[from_idx] * weight / total_weight;
                    new_scores[to_idx] += share;
                }
            }
        }

        // Swap scores
        std::mem::swap(&mut scores, &mut new_scores);

        // Check for convergence
        let diff: f64 = scores
            .iter()
            .zip(new_scores.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();

        if diff < 1e-6 {
            break;
        }
    }

    // Build result
    let mut score_map: HashMap<String, f64> = HashMap::new();
    let mut ranking: Vec<(String, f64)> = Vec::new();
    let mut max_score = 0.0f64;
    let mut total_score = 0.0f64;

    for (i, name) in nodes.iter().enumerate() {
        let score = scores[i];
        score_map.insert(name.clone(), score);
        ranking.push((name.clone(), score));
        max_score = max_score.max(score);
        total_score += score;
    }

    // Sort by score descending
    ranking.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    PageRankScores {
        scores: score_map,
        ranking: ranking.into_iter().map(|(name, _)| name).collect(),
        avg_score: total_score / n as f64,
        max_score,
    }
}

/// Build a dependency graph from wiki page relationships.
pub fn build_graph_from_wiki(
    file_pages: &[crate::wiki::WikiPage],
    symbol_pages: &[crate::wiki::WikiPage],
) -> (Vec<String>, Vec<GraphEdge>) {
    let mut nodes: HashSet<String> = HashSet::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    // Add all pages as nodes
    for page in file_pages.iter().chain(symbol_pages.iter()) {
        nodes.insert(page.title.clone());
    }

    // Add edges from relationships
    for page in file_pages.iter().chain(symbol_pages.iter()) {
        for (label, targets) in &page.relationships {
            let weight = match label.as_str() {
                "Calls" => 2.0,            // Call edges are important
                "Defines" => 1.5,          // Definition edges are important
                "Imports" | "Uses" => 1.0, // Import edges are standard
                "Called by" => 1.5,        // Incoming calls are important
                _ => 0.5,                  // Other edges are less important
            };

            for target in targets {
                edges.push(GraphEdge {
                    from: page.title.clone(),
                    to: target.clone(),
                    weight,
                });
            }
        }

        // Add edges from called_by (reverse direction)
        for caller in &page.called_by {
            edges.push(GraphEdge {
                from: caller.clone(),
                to: page.title.clone(),
                weight: 1.5,
            });
        }
    }

    (nodes.into_iter().collect(), edges)
}

/// Compute PageRank for a wiki model and return scores.
pub fn compute_wiki_pagerank(
    file_pages: &[crate::wiki::WikiPage],
    symbol_pages: &[crate::wiki::WikiPage],
) -> PageRankScores {
    let (nodes, edges) = build_graph_from_wiki(file_pages, symbol_pages);
    compute_pagerank(&nodes, &edges, 0.85, 100)
}

/// Generate a reading guide from PageRank scores.
///
/// Returns a list of (page_title, score, reason) tuples in reading order.
pub fn generate_reading_guide(
    scores: &PageRankScores,
    file_pages: &[crate::wiki::WikiPage],
    limit: usize,
) -> Vec<ReadingGuideEntry> {
    let mut entries: Vec<ReadingGuideEntry> = Vec::new();

    // Get top-ranked files
    for (name, score) in scores.top_n(limit * 2) {
        // Find the page to get summary info
        if let Some(page) = file_pages.iter().find(|p| p.title == name) {
            let reason = generate_reading_reason(page, score, scores);
            entries.push(ReadingGuideEntry {
                title: name.to_string(),
                slug: page.slug.clone(),
                score,
                reason,
                kind: page.kind,
            });
        }

        if entries.len() >= limit {
            break;
        }
    }

    entries
}

/// An entry in the reading guide.
#[derive(Clone, Debug, Serialize)]
pub struct ReadingGuideEntry {
    pub title: String,
    pub slug: String,
    pub score: f64,
    pub reason: String,
    pub kind: crate::wiki::WikiPageKind,
}

/// Generate a human-readable reason for why this page is important.
fn generate_reading_reason(
    page: &crate::wiki::WikiPage,
    _score: f64,
    scores: &PageRankScores,
) -> String {
    let mut reasons = Vec::new();

    // Check relationship counts
    let defines_count = page
        .relationships
        .iter()
        .find(|(l, _)| l == "Defines")
        .map(|(_, t)| t.len())
        .unwrap_or(0);

    let calls_count = page
        .relationships
        .iter()
        .find(|(l, _)| l == "Calls")
        .map(|(_, t)| t.len())
        .unwrap_or(0);

    let called_by_count = page.called_by.len();

    if defines_count > 5 {
        reasons.push(format!("defines {} symbols", defines_count));
    } else if defines_count > 0 {
        reasons.push(format!("defines {} symbol(s)", defines_count));
    }

    if called_by_count > 5 {
        reasons.push(format!("referenced by {} other files", called_by_count));
    } else if called_by_count > 0 {
        reasons.push(format!("referenced by {} file(s)", called_by_count));
    }

    if calls_count > 5 {
        reasons.push(format!("uses {} dependencies", calls_count));
    }

    if scores.is_top_tier(&page.title) {
        reasons.push("core architecture component".to_string());
    }

    if reasons.is_empty() {
        reasons.push("important module".to_string());
    }

    reasons.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagerank_simple_graph() {
        let nodes = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let edges = vec![
            GraphEdge {
                from: "A".to_string(),
                to: "B".to_string(),
                weight: 1.0,
            },
            GraphEdge {
                from: "B".to_string(),
                to: "C".to_string(),
                weight: 1.0,
            },
            GraphEdge {
                from: "C".to_string(),
                to: "A".to_string(),
                weight: 1.0,
            },
        ];

        let scores = compute_pagerank(&nodes, &edges, 0.85, 100);

        // In a cycle, all nodes should have similar scores
        assert!((scores.get("A") - scores.get("B")).abs() < 0.01);
        assert!((scores.get("B") - scores.get("C")).abs() < 0.01);
    }

    #[test]
    fn test_pagerank_star_graph() {
        // Star graph: A -> B, A -> C, A -> D
        // A should have lowest score (dangling after distributing),
        // B, C, D should have higher scores
        let nodes = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
        ];
        let edges = vec![
            GraphEdge {
                from: "B".to_string(),
                to: "A".to_string(),
                weight: 1.0,
            },
            GraphEdge {
                from: "C".to_string(),
                to: "A".to_string(),
                weight: 1.0,
            },
            GraphEdge {
                from: "D".to_string(),
                to: "A".to_string(),
                weight: 1.0,
            },
        ];

        let scores = compute_pagerank(&nodes, &edges, 0.85, 100);

        // A receives links from B, C, D so should be highest
        assert!(scores.get("A") > scores.get("B"));
        assert!(scores.get("A") > scores.get("C"));
    }

    #[test]
    fn test_pagerank_top_n() {
        let nodes = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let edges = vec![
            GraphEdge {
                from: "B".to_string(),
                to: "A".to_string(),
                weight: 1.0,
            },
            GraphEdge {
                from: "C".to_string(),
                to: "A".to_string(),
                weight: 1.0,
            },
        ];

        let scores = compute_pagerank(&nodes, &edges, 0.85, 100);
        let top_2 = scores.top_n(2);

        assert_eq!(top_2.len(), 2);
        assert_eq!(top_2[0].0, "A"); // A is most important
    }

    #[test]
    fn test_reading_order() {
        let mut scores = PageRankScores::default();
        scores.scores.insert("file1.rs".to_string(), 0.5);
        scores.scores.insert("file2.rs".to_string(), 0.8);
        scores.scores.insert("file3.rs".to_string(), 0.3);
        scores.ranking = vec![
            "file2.rs".to_string(),
            "file1.rs".to_string(),
            "file3.rs".to_string(),
        ];
        scores.max_score = 0.8;

        let files = vec!["file1.rs", "file2.rs", "file3.rs"];
        let order = scores.reading_order(&files);

        assert_eq!(order[0], "file2.rs");
        assert_eq!(order[1], "file1.rs");
        assert_eq!(order[2], "file3.rs");
    }

    #[test]
    fn test_normalized_score() {
        let mut scores = PageRankScores::default();
        scores.scores.insert("A".to_string(), 0.5);
        scores.scores.insert("B".to_string(), 1.0);
        scores.max_score = 1.0;

        assert!((scores.normalized("A") - 0.5).abs() < 0.01);
        assert!((scores.normalized("B") - 1.0).abs() < 0.01);
    }
}
