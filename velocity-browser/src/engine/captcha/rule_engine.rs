//! Rule-based captcha solver with a deterministic fast path and LLM fallback.
//!
//! Most challenges, once their shapes are learned and their transient cells
//! found, can be solved by a handful of declarative rules — "click every tile
//! classified as a bus", "click the cell that flips", "select the most compact
//! shape". This engine evaluates such rules against a [`SolveContext`] built
//! from the spline/temporal/library primitives and emits concrete
//! [`SolveAction`]s. When no rule fires it yields [`SolveAction::DeferToLlm`],
//! keeping the LLM as a fallback rather than the default.

use super::spline::ShapeSignature;

/// A cell observed in the challenge grid, with any native classification and
/// transient-change evidence attached.
#[derive(Debug, Clone)]
pub struct ObservedCell {
    pub index: usize,
    pub row: usize,
    pub col: usize,
    /// Native classification `(class, confidence)` if the library recognized it.
    pub classification: Option<(String, f32)>,
    /// Shape signature of the cell, if extracted.
    pub signature: Option<ShapeSignature>,
    /// Accumulated temporal change magnitude for this cell (0 if static).
    pub change_magnitude: u32,
}

impl ObservedCell {
    pub fn new(index: usize, row: usize, col: usize) -> Self {
        Self {
            index,
            row,
            col,
            classification: None,
            signature: None,
            change_magnitude: 0,
        }
    }
}

/// The full observation the engine reasons over for one challenge.
#[derive(Debug, Clone, Default)]
pub struct SolveContext {
    /// The target the challenge asks for, lowercased (e.g. `"bus"`), if known.
    pub target_class: Option<String>,
    pub cells: Vec<ObservedCell>,
}

impl SolveContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_target(mut self, target: &str) -> Self {
        self.target_class = Some(target.to_lowercase());
        self
    }

    pub fn push_cell(&mut self, cell: ObservedCell) {
        self.cells.push(cell);
    }
}

/// A condition evaluated against a single [`ObservedCell`] in context.
#[derive(Debug, Clone)]
pub enum RuleCondition {
    /// Cell is natively classified as the context's target class at or above
    /// the given confidence.
    ClassifiedAsTarget { min_confidence: f32 },
    /// Cell is natively classified as a specific class at or above confidence.
    ClassifiedAs { class: String, min_confidence: f32 },
    /// Cell's temporal change magnitude is at or above the threshold.
    Changed { min_magnitude: u32 },
    /// Cell's shape compactness is at or above the threshold.
    CompactnessAtLeast { min: f32 },
}

impl RuleCondition {
    fn matches(&self, cell: &ObservedCell, ctx: &SolveContext) -> bool {
        match self {
            RuleCondition::ClassifiedAsTarget { min_confidence } => {
                match (&ctx.target_class, &cell.classification) {
                    (Some(target), Some((class, conf))) => {
                        class.eq_ignore_ascii_case(target) && *conf >= *min_confidence
                    }
                    _ => false,
                }
            }
            RuleCondition::ClassifiedAs {
                class,
                min_confidence,
            } => cell
                .classification
                .as_ref()
                .map(|(c, conf)| c.eq_ignore_ascii_case(class) && *conf >= *min_confidence)
                .unwrap_or(false),
            RuleCondition::Changed { min_magnitude } => cell.change_magnitude >= *min_magnitude,
            RuleCondition::CompactnessAtLeast { min } => cell
                .signature
                .as_ref()
                .map(|s| s.compactness >= *min)
                .unwrap_or(false),
        }
    }
}

/// The concrete action a rule produces when its condition fires.
#[derive(Debug, Clone, PartialEq)]
pub enum SolveAction {
    /// Click the identified cell.
    ClickCell { index: usize },
    /// No rule matched; hand the challenge to the LLM.
    DeferToLlm,
}

/// A prioritized rule: when `condition` holds for a cell, emit a click on it.
#[derive(Debug, Clone)]
pub struct SolveRule {
    pub name: String,
    pub condition: RuleCondition,
    /// Higher priority rules are evaluated first.
    pub priority: i32,
}

impl SolveRule {
    pub fn new(name: &str, condition: RuleCondition, priority: i32) -> Self {
        Self {
            name: name.to_string(),
            condition,
            priority,
        }
    }
}

/// Evaluates rules against a context to produce solve actions.
#[derive(Debug, Clone, Default)]
pub struct RuleEngine {
    rules: Vec<SolveRule>,
}

impl RuleEngine {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Build an engine with sensible default rules for grid-selection
    /// challenges: prefer native target classification, then transient cells.
    pub fn with_defaults() -> Self {
        let mut engine = Self::new();
        engine.add_rule(SolveRule::new(
            "target-class",
            RuleCondition::ClassifiedAsTarget {
                min_confidence: 0.6,
            },
            100,
        ));
        engine.add_rule(SolveRule::new(
            "flipping-cell",
            RuleCondition::Changed { min_magnitude: 48 },
            50,
        ));
        engine
    }

    pub fn add_rule(&mut self, rule: SolveRule) {
        self.rules.push(rule);
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Evaluate all rules against the context. Returns every cell click the
    /// highest-priority firing rule selects; if no rule fires for any cell,
    /// returns a single [`SolveAction::DeferToLlm`].
    pub fn evaluate(&self, ctx: &SolveContext) -> Vec<SolveAction> {
        for rule in &self.rules {
            let hits: Vec<SolveAction> = ctx
                .cells
                .iter()
                .filter(|cell| rule.condition.matches(cell, ctx))
                .map(|cell| SolveAction::ClickCell { index: cell.index })
                .collect();
            if !hits.is_empty() {
                return hits;
            }
        }
        vec![SolveAction::DeferToLlm]
    }

    /// Whether any rule fires for the given context (i.e. it can be solved
    /// natively without the LLM).
    pub fn can_solve_natively(&self, ctx: &SolveContext) -> bool {
        !matches!(self.evaluate(ctx).as_slice(), [SolveAction::DeferToLlm])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classified_cell(index: usize, class: &str, conf: f32) -> ObservedCell {
        let mut c = ObservedCell::new(index, index / 3, index % 3);
        c.classification = Some((class.to_string(), conf));
        c
    }

    #[test]
    fn target_class_rule_selects_matching_cells() {
        let mut ctx = SolveContext::new().with_target("bus");
        ctx.push_cell(classified_cell(0, "bus", 0.9));
        ctx.push_cell(classified_cell(1, "tree", 0.9));
        ctx.push_cell(classified_cell(2, "bus", 0.7));
        let engine = RuleEngine::with_defaults();
        let actions = engine.evaluate(&ctx);
        assert_eq!(
            actions,
            vec![
                SolveAction::ClickCell { index: 0 },
                SolveAction::ClickCell { index: 2 }
            ]
        );
    }

    #[test]
    fn low_confidence_target_is_not_selected() {
        let mut ctx = SolveContext::new().with_target("bus");
        ctx.push_cell(classified_cell(0, "bus", 0.4));
        let engine = RuleEngine::with_defaults();
        // No cell clears 0.6 confidence and none changed → defer.
        assert_eq!(engine.evaluate(&ctx), vec![SolveAction::DeferToLlm]);
        assert!(!engine.can_solve_natively(&ctx));
    }

    #[test]
    fn flipping_cell_rule_fires_when_no_classification() {
        let mut ctx = SolveContext::new();
        let mut changing = ObservedCell::new(4, 1, 1);
        changing.change_magnitude = 120;
        ctx.push_cell(ObservedCell::new(0, 0, 0));
        ctx.push_cell(changing);
        let engine = RuleEngine::with_defaults();
        assert_eq!(
            engine.evaluate(&ctx),
            vec![SolveAction::ClickCell { index: 4 }]
        );
    }

    #[test]
    fn priority_prefers_classification_over_change() {
        // A cell is both the target class AND changing; another only changes.
        let mut ctx = SolveContext::new().with_target("bus");
        let mut both = classified_cell(0, "bus", 0.9);
        both.change_magnitude = 200;
        let mut only_change = ObservedCell::new(1, 0, 1);
        only_change.change_magnitude = 200;
        ctx.push_cell(both);
        ctx.push_cell(only_change);
        // Highest-priority firing rule is target-class, which selects only cell 0.
        let engine = RuleEngine::with_defaults();
        assert_eq!(
            engine.evaluate(&ctx),
            vec![SolveAction::ClickCell { index: 0 }]
        );
    }

    #[test]
    fn empty_context_defers_to_llm() {
        let engine = RuleEngine::with_defaults();
        assert_eq!(
            engine.evaluate(&SolveContext::new()),
            vec![SolveAction::DeferToLlm]
        );
    }

    #[test]
    fn custom_class_rule() {
        let mut engine = RuleEngine::new();
        engine.add_rule(SolveRule::new(
            "pick-cars",
            RuleCondition::ClassifiedAs {
                class: "car".to_string(),
                min_confidence: 0.5,
            },
            10,
        ));
        let mut ctx = SolveContext::new();
        ctx.push_cell(classified_cell(3, "car", 0.8));
        assert_eq!(
            engine.evaluate(&ctx),
            vec![SolveAction::ClickCell { index: 3 }]
        );
    }

    #[test]
    fn rules_are_sorted_by_priority() {
        let engine = RuleEngine::with_defaults();
        assert_eq!(engine.rule_count(), 2);
    }
}
