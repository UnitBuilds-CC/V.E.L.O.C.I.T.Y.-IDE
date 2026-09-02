//! Workflow definition — a named, ordered sequence of steps.
use serde::{Deserialize, Serialize};
use crate::{WorkflowId, Step};

/// A workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: WorkflowId,
    pub name: String,
    pub steps: Vec<Step>,
    pub max_concurrency: usize,
    pub description: Option<String>,
}

impl Workflow {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: WorkflowId::new(),
            name: name.into(),
            steps: Vec::new(),
            max_concurrency: 1,
            description: None,
        }
    }

    pub fn add_step(&mut self, step: Step) {
        self.steps.push(step);
    }

    pub fn with_concurrency(mut self, n: usize) -> Self {
        self.max_concurrency = n;
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Compute the dependency graph: returns adjacency list of step index -> dependent step indices.
    pub fn dependency_graph(&self) -> Vec<Vec<usize>> {
        let mut graph = vec![Vec::new(); self.steps.len()];
        for (i, step) in self.steps.iter().enumerate() {
            for dep_id in &step.input.depends_on {
                if let Some(dep_idx) = self.steps.iter().position(|s| &s.id == dep_id) {
                    graph[dep_idx].push(i);
                }
            }
        }
        graph
    }

    /// Topological sort of steps respecting dependencies.
    /// Returns None if there is a cycle.
    pub fn topological_order(&self) -> Option<Vec<usize>> {
        let n = self.steps.len();
        let mut in_degree = vec![0usize; n];
        let graph = self.dependency_graph();

        for deps in &graph {
            for &dep in deps {
                in_degree[dep] += 1;
            }
        }

        let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut order = Vec::with_capacity(n);

        while let Some(node) = queue.pop() {
            order.push(node);
            for &dep in &graph[node] {
                in_degree[dep] -= 1;
                if in_degree[dep] == 0 {
                    queue.push(dep);
                }
            }
        }

        if order.len() == n { Some(order) } else { None }
    }

    /// Get steps that have no unresolved dependencies given a set of completed step IDs.
    pub fn ready_steps(&self, completed: &[crate::StepId]) -> Vec<(usize, &Step)> {
        self.steps.iter().enumerate().filter(|(_, step)| {
            step.input.depends_on.iter().all(|dep| completed.contains(dep))
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StepKind, StepId};

    #[test]
    fn workflow_add_steps() {
        let mut wf = Workflow::new("test");
        wf.add_step(Step::new("step1", StepKind::Barrier));
        wf.add_step(Step::new("step2", StepKind::Barrier));
        assert_eq!(wf.steps.len(), 2);
    }

    #[test]
    fn topological_order_no_deps() {
        let mut wf = Workflow::new("test");
        wf.add_step(Step::new("a", StepKind::Barrier));
        wf.add_step(Step::new("b", StepKind::Barrier));
        let order = wf.topological_order().unwrap();
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn topological_order_with_deps() {
        let mut wf = Workflow::new("test");
        let s1 = Step::new("first", StepKind::Barrier);
        let s1_id = s1.id.clone();
        wf.add_step(s1);
        wf.add_step(Step::new("second", StepKind::Barrier).depends_on(s1_id));
        let order = wf.topological_order().unwrap();
        assert_eq!(order[0], 0);
        assert_eq!(order[1], 1);
    }

    #[test]
    fn ready_steps_respects_deps() {
        let mut wf = Workflow::new("test");
        let s1 = Step::new("first", StepKind::Barrier);
        let s1_id = s1.id.clone();
        wf.add_step(s1);
        wf.add_step(Step::new("second", StepKind::Barrier).depends_on(s1_id.clone()));
        let ready = wf.ready_steps(&[]);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, 0);
        let ready = wf.ready_steps(&[s1_id]);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, 1);
    }
}
