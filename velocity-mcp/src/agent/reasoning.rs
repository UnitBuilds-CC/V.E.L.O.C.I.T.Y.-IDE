//! Advanced reasoning engine: tree-of-thought, planning, and confidence scoring.
//!
//! Provides structured reasoning capabilities for complex problems:
//! - **Tree of Thought**: Explore multiple solution paths in parallel
//! - **Planning**: Decompose tasks into validated multi-step plans
//! - **Confidence Scoring**: Quantify certainty in different approaches
//!
//! The reasoning engine integrates with the existing agent loop to provide
//! deeper analysis before execution, reducing wasted work and improving outcomes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single thought/idea in the reasoning tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thought {
    /// Unique identifier for this thought.
    pub id: String,
    /// The content of this thought.
    pub content: String,
    /// Parent thought ID (None for root thoughts).
    pub parent: Option<String>,
    /// Child thought IDs.
    pub children: Vec<String>,
    /// Confidence score (0.0 to 1.0) for this thought.
    pub confidence: f32,
    /// Evaluation of this thought's viability.
    pub evaluation: ThoughtEvaluation,
    /// Depth in the tree (0 = root).
    pub depth: usize,
    /// Whether this branch has been fully explored.
    pub explored: bool,
}

/// Evaluation of a thought's quality.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ThoughtEvaluation {
    /// Promising — worth exploring further.
    Promising,
    /// Neutral — neither clearly good nor bad.
    Neutral,
    /// Unlikely — probably won't work.
    Unlikely,
    /// Invalid — logically flawed or impossible.
    Invalid,
}

impl ThoughtEvaluation {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Promising => "promising",
            Self::Neutral => "neutral",
            Self::Unlikely => "unlikely",
            Self::Invalid => "invalid",
        }
    }

    pub fn score(&self) -> f32 {
        match self {
            Self::Promising => 0.8,
            Self::Neutral => 0.5,
            Self::Unlikely => 0.2,
            Self::Invalid => 0.0,
        }
    }
}

/// The full reasoning tree for a problem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningTree {
    /// The original problem/question being reasoned about.
    pub problem: String,
    /// All thoughts in the tree, keyed by ID.
    pub thoughts: HashMap<String, Thought>,
    /// Root thought IDs (entry points).
    pub roots: Vec<String>,
    /// Maximum depth to explore.
    pub max_depth: usize,
    /// Counter for generating unique thought IDs.
    next_id: u64,
}

impl ReasoningTree {
    /// Create a new reasoning tree for a problem.
    pub fn new(problem: &str) -> Self {
        Self {
            problem: problem.to_string(),
            thoughts: HashMap::new(),
            roots: Vec::new(),
            max_depth: 5,
            next_id: 1,
        }
    }

    /// Generate a unique thought ID.
    fn gen_id(&mut self) -> String {
        let id = format!("t{}", self.next_id);
        self.next_id += 1;
        id
    }

    /// Add a root thought (top-level approach).
    pub fn add_root(&mut self, content: &str, confidence: f32) -> String {
        let id = self.gen_id();
        let thought = Thought {
            id: id.clone(),
            content: content.to_string(),
            parent: None,
            children: Vec::new(),
            confidence: confidence.clamp(0.0, 1.0),
            evaluation: ThoughtEvaluation::Neutral,
            depth: 0,
            explored: false,
        };
        self.thoughts.insert(id.clone(), thought);
        self.roots.push(id.clone());
        id
    }

    /// Add a child thought branching from a parent.
    pub fn add_child(&mut self, parent_id: &str, content: &str, confidence: f32) -> Option<String> {
        // Check depth limit first (immutable read).
        let parent_depth = self.thoughts.get(parent_id)?.depth;
        if parent_depth + 1 >= self.max_depth {
            return None;
        }
        let depth = parent_depth + 1;

        // Generate ID (needs &mut self for next_id).
        let id = self.gen_id();

        let thought = Thought {
            id: id.clone(),
            content: content.to_string(),
            parent: Some(parent_id.to_string()),
            children: Vec::new(),
            confidence: confidence.clamp(0.0, 1.0),
            evaluation: ThoughtEvaluation::Neutral,
            depth,
            explored: false,
        };
        self.thoughts.insert(id.clone(), thought);
        // Now add child reference to parent.
        if let Some(parent) = self.thoughts.get_mut(parent_id) {
            parent.children.push(id.clone());
        }
        Some(id)
    }

    /// Evaluate a thought (set its evaluation and update confidence).
    pub fn evaluate(&mut self, thought_id: &str, evaluation: ThoughtEvaluation) {
        if let Some(thought) = self.thoughts.get_mut(thought_id) {
            thought.evaluation = evaluation;
            // Blend the evaluation score with the original confidence.
            thought.confidence = (thought.confidence + evaluation.score()) / 2.0;
        }
    }

    /// Mark a thought as fully explored.
    pub fn mark_explored(&mut self, thought_id: &str) {
        if let Some(thought) = self.thoughts.get_mut(thought_id) {
            thought.explored = true;
        }
    }

    /// Get the best path through the tree (highest cumulative confidence).
    pub fn best_path(&self) -> Vec<&Thought> {
        if self.roots.is_empty() {
            return Vec::new();
        }

        // Find the root with highest confidence.
        let best_root = self.roots.iter()
            .map(|id| (id, self.thoughts.get(id).map(|t| t.confidence).unwrap_or(0.0)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, _)| id.clone());

        let Some(root_id) = best_root else { return Vec::new(); };

        // Greedily follow the highest-confidence child at each level.
        let mut path = Vec::new();
        let mut current_id = root_id;
        loop {
            if let Some(thought) = self.thoughts.get(&current_id) {
                path.push(thought);
                // Find best child.
                let best_child = thought.children.iter()
                    .filter_map(|cid| self.thoughts.get(cid).map(|t| (cid, t.confidence)))
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(cid, _)| cid.clone());

                match best_child {
                    Some(child_id) => current_id = child_id,
                    None => break,
                }
            } else {
                break;
            }
        }
        path
    }

    /// Get all unexplored thoughts (candidates for further expansion).
    pub fn unexplored(&self) -> Vec<&Thought> {
        self.thoughts.values()
            .filter(|t| !t.explored && t.depth < self.max_depth)
            .collect()
    }

    /// Overall confidence in the best solution path.
    pub fn overall_confidence(&self) -> f32 {
        let path = self.best_path();
        if path.is_empty() {
            return 0.0;
        }
        // Geometric mean of confidences along the path.
        let product: f32 = path.iter().map(|t| t.confidence).product();
        product.powf(1.0 / path.len() as f32)
    }

    /// Total number of thoughts in the tree.
    pub fn thought_count(&self) -> usize {
        self.thoughts.len()
    }

    /// Number of leaf nodes (no children).
    pub fn leaf_count(&self) -> usize {
        self.thoughts.values().filter(|t| t.children.is_empty()).count()
    }

    /// Summary of the reasoning tree for display.
    pub fn summary(&self) -> ReasoningSummary {
        let total = self.thoughts.len();
        let explored = self.thoughts.values().filter(|t| t.explored).count();
        let promising = self.thoughts.values().filter(|t| t.evaluation == ThoughtEvaluation::Promising).count();
        let invalid = self.thoughts.values().filter(|t| t.evaluation == ThoughtEvaluation::Invalid).count();

        ReasoningSummary {
            problem: self.problem.clone(),
            total_thoughts: total,
            explored,
            promising,
            invalid,
            best_confidence: self.overall_confidence(),
            max_depth_reached: self.thoughts.values().map(|t| t.depth).max().unwrap_or(0),
        }
    }
}

/// Summary statistics of a reasoning session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningSummary {
    pub problem: String,
    pub total_thoughts: usize,
    pub explored: usize,
    pub promising: usize,
    pub invalid: usize,
    pub best_confidence: f32,
    pub max_depth_reached: usize,
}

impl ReasoningSummary {
    /// Format for display.
    pub fn display(&self) -> String {
        format!(
            "Problem: {}\n\
             Thoughts: {} total, {} explored, {} promising, {} invalid\n\
             Best confidence: {:.0}% | Max depth: {}",
            self.problem,
            self.total_thoughts,
            self.explored,
            self.promising,
            self.invalid,
            self.best_confidence * 100.0,
            self.max_depth_reached,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_empty_tree() {
        let tree = ReasoningTree::new("How to optimize performance?");
        assert_eq!(tree.thought_count(), 0);
        assert_eq!(tree.roots.len(), 0);
        assert_eq!(tree.overall_confidence(), 0.0);
    }

    #[test]
    fn add_root_and_children() {
        let mut tree = ReasoningTree::new("Best approach?");
        let r1 = tree.add_root("Approach A: caching", 0.7);
        let r2 = tree.add_root("Approach B: parallelism", 0.8);
        assert_eq!(tree.thought_count(), 2);
        assert_eq!(tree.roots.len(), 2);

        let c1 = tree.add_child(&r1, "Use LRU cache", 0.6).unwrap();
        let c2 = tree.add_child(&r1, "Use write-through cache", 0.5).unwrap();
        assert_eq!(tree.thoughts[&r1].children.len(), 2);
        assert_eq!(tree.thoughts[&c1].depth, 1);
    }

    #[test]
    fn evaluate_and_best_path() {
        let mut tree = ReasoningTree::new("Fix bug");
        let r1 = tree.add_root("Check logs", 0.9);
        let c1 = tree.add_child(&r1, "Found null pointer", 0.8).unwrap();
        tree.add_child(&c1, "Add null check", 0.95).unwrap();

        let r2 = tree.add_root("Rewrite module", 0.3);

        tree.evaluate(&r1, ThoughtEvaluation::Promising);
        tree.evaluate(&c1, ThoughtEvaluation::Promising);

        let path = tree.best_path();
        assert!(path.len() >= 2);
        assert!(path[0].content.contains("logs") || path[0].content.contains("null"));
    }

    #[test]
    fn max_depth_enforced() {
        let mut tree = ReasoningTree::new("test");
        tree.max_depth = 2;
        let r = tree.add_root("root", 0.5);
        let c1 = tree.add_child(&r, "child", 0.5).unwrap();
        let result = tree.add_child(&c1, "grandchild", 0.5);
        assert!(result.is_none()); // depth 2 >= max_depth 2
    }

    #[test]
    fn summary_computation() {
        let mut tree = ReasoningTree::new("How to scale?");
        let r = tree.add_root("Horizontal scaling", 0.7);
        tree.add_child(&r, "Add more nodes", 0.8).unwrap();
        tree.evaluate(&r, ThoughtEvaluation::Promising);
        tree.mark_explored(&r);

        let summary = tree.summary();
        assert_eq!(summary.total_thoughts, 2);
        assert_eq!(summary.explored, 1);
        assert_eq!(summary.promising, 1);
        assert!(summary.best_confidence > 0.0);
    }

    #[test]
    fn unexplored_tracking() {
        let mut tree = ReasoningTree::new("test");
        let r = tree.add_root("root", 0.5);
        let c = tree.add_child(&r, "child", 0.5).unwrap();
        assert_eq!(tree.unexplored().len(), 2);
        tree.mark_explored(&r);
        assert_eq!(tree.unexplored().len(), 1);
    }
}
