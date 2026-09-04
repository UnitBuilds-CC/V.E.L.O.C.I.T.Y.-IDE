//! Captcha orchestrator — the coordinator with fingerprint-first fast path.
//!
//! Solve loop:
//! 1. Rasterize challenge region -> PixelBuffer
//! 2. Fingerprint pixels -> VisualFingerprint (free, ~microseconds)
//! 3. Template lookup by hash:
//!    - HIT with confidence > 0.8 -> replay stored sequence (zero tokens)
//!    - HIT with low confidence -> use as hint, still verify via DOM
//!    - MISS -> continue to step 4
//! 4. DOM observation -> ChallengeSnapshot
//! 5. Provider fingerprint -> ChallengeDescriptor
//! 6. State machine init from archetype
//! 7. Execute actions via StealthHumanBehavior
//! 8. On success: store template keyed by visual_hash
//! 9. On failure: decrement confidence, try alternative archetype

use crate::dom::DomTree;
use crate::engine::PixelBuffer;
use crate::nda::NdaTriple;

use super::challenge::{ChallengeDescriptor, ChallengeFeatures};
use super::fingerprint::ProviderFingerprinter;
use super::observer::{ChallengeObserver, GridLayout};
use super::rule_engine::{ObservedCell, RuleEngine, SolveAction, SolveContext};
use super::shadow_match::{ShadowMatch, ShadowMatcher};
use super::spline::SplineExtractor;
use super::spline_library::SplineLibrary;
use super::state_machine::ChallengeStateMachine;
use super::template_store::{SolveTemplate, TemplateStore};
use super::temporal::TemporalMonitor;
use super::visual_fingerprint::{ChallengeArchetype, VisualFingerprint, VisualFingerprinter};

/// Result of a solve attempt.
#[derive(Debug, Clone)]
pub enum SolveResult {
    /// Solved via template replay (zero tokens).
    TemplateReplay { visual_hash: u64, confidence: f32 },
    /// Solved natively by learned shape rules + transient detection (zero tokens).
    NativeSolve {
        clicked_cells: Vec<usize>,
        target: String,
    },
    /// Solved via full analysis (tokens spent).
    FullSolve {
        descriptor: ChallengeDescriptor,
        steps: u32,
    },
    /// Failed to solve.
    Failed { reason: String },
    /// No challenge detected.
    NoChallenge,
}

/// An active challenge being solved.
#[derive(Debug)]
pub struct ActiveChallenge {
    pub descriptor: ChallengeDescriptor,
    pub archetype: ChallengeArchetype,
    pub state_machine: ChallengeStateMachine,
    pub visual_fingerprint: VisualFingerprint,
}

/// The captcha orchestrator — coordinates detection, fingerprinting, template
/// lookup, and solve execution.
pub struct CaptchaOrchestrator {
    pub template_store: TemplateStore,
    pub fingerprinter: VisualFingerprinter,
    pub provider_fingerprinter: ProviderFingerprinter,
    pub active_challenge: Option<ActiveChallenge>,
    /// Learned shape → object-class store, grown from LLM classifications.
    pub spline_library: SplineLibrary,
    /// Deterministic solver evaluated before falling back to the LLM.
    pub rule_engine: RuleEngine,
    /// Contour/shape extractor shared by the native paths.
    pub extractor: SplineExtractor,
    /// Silhouette matcher for Azure-style shadow challenges.
    pub shadow_matcher: ShadowMatcher,
    /// Rolling frame monitor for transient/animated challenges.
    pub temporal: TemporalMonitor,
}

impl Default for CaptchaOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptchaOrchestrator {
    pub fn new() -> Self {
        Self {
            template_store: TemplateStore::new(),
            fingerprinter: VisualFingerprinter::new(),
            provider_fingerprinter: ProviderFingerprinter::new(),
            active_challenge: None,
            spline_library: SplineLibrary::new(),
            rule_engine: RuleEngine::with_defaults(),
            extractor: SplineExtractor::new(),
            shadow_matcher: ShadowMatcher::new(),
            temporal: TemporalMonitor::new(16),
        }
    }

    /// Main entry point: attempt to solve a captcha challenge.
    ///
    /// Returns the solve result and any NDA triples for observability.
    pub fn solve(
        &mut self,
        tree: &DomTree,
        buffer: &PixelBuffer,
        challenge_region: (usize, usize, usize, usize),
        session_id: &str,
    ) -> (SolveResult, Vec<NdaTriple>) {
        let mut nda = Vec::new();

        // Step 1-2: Fingerprint the pixels (free)
        let fp = self.fingerprinter.fingerprint(buffer, challenge_region);
        nda.push(NdaTriple::new(
            session_id,
            250,
            &format!("captcha_fingerprint:hash={:016x}", fp.hash),
        ));

        // Step 3: Template lookup (zero-token fast path)
        let template_hit = self
            .template_store
            .lookup(fp.hash)
            .map(|t| (t.is_reliable(), t.confidence));
        if let Some((is_reliable, confidence)) = template_hit {
            if is_reliable {
                // Fast path: replay stored solution
                nda.push(NdaTriple::new(
                    session_id,
                    251,
                    "captcha_strategy:template_replay",
                ));
                self.template_store.record_outcome(fp.hash, true);
                return (
                    SolveResult::TemplateReplay {
                        visual_hash: fp.hash,
                        confidence,
                    },
                    nda,
                );
            }
            // Low confidence template — use as hint but still verify
            nda.push(NdaTriple::new(
                session_id,
                251,
                "captcha_strategy:template_hint",
            ));
        }

        // Step 4: DOM observation
        let snapshot = ChallengeObserver::observe(tree);
        let snapshot = match snapshot {
            Some(s) => s,
            None => {
                nda.push(NdaTriple::new(
                    session_id,
                    260,
                    "captcha_result:no_challenge",
                ));
                return (SolveResult::NoChallenge, nda);
            }
        };

        // Step 5: Provider fingerprint
        let (provider, _score) = self
            .provider_fingerprinter
            .identify_from_snapshot(&snapshot)
            .unwrap_or_else(|| ("unknown".to_string(), 0.0));

        // Build features from snapshot
        let features = ChallengeFeatures {
            grid: snapshot.grid_layout.as_ref().map(|g| (g.rows, g.cols)),
            interactive_elements: snapshot.interactive_elements.len() as u8,
            iframe_depth: snapshot.iframe_boundaries.len() as u8,
            round_count: 1,
            markers: snapshot.structural_markers.clone(),
        };

        let descriptor = self
            .provider_fingerprinter
            .build_descriptor(&provider, fp.hash, features);

        nda.push(NdaTriple::new(
            session_id,
            252,
            &format!(
                "captcha_provider:{}:{}",
                descriptor.provider, descriptor.variant
            ),
        ));

        // Step 5b: Native rule-based fast path (zero tokens).
        //
        // If the target's shape has been learned, or a transient cell has been
        // detected across observed frames, solve by rule without the LLM. This
        // requires a detected grid to slice the region into cells.
        if let Some(grid) = snapshot.grid_layout.as_ref() {
            let target = snapshot
                .instruction_text
                .as_deref()
                .and_then(parse_target_class);
            let ctx = self.build_solve_context(buffer, challenge_region, grid, target.as_deref());
            let actions = self.rule_engine.evaluate(&ctx);
            if !matches!(actions.as_slice(), [SolveAction::DeferToLlm]) {
                let clicked_cells: Vec<usize> = actions
                    .iter()
                    .filter_map(|a| match a {
                        SolveAction::ClickCell { index } => Some(*index),
                        SolveAction::DeferToLlm => None,
                    })
                    .collect();
                nda.push(NdaTriple::new(
                    session_id,
                    255,
                    "captcha_strategy:native_rules",
                ));
                nda.push(NdaTriple::new(
                    session_id,
                    256,
                    &format!("captcha_native_clicks:{}", clicked_cells.len()),
                ));
                nda.push(NdaTriple::new(
                    session_id,
                    260,
                    "captcha_result:native_solved",
                ));
                return (
                    SolveResult::NativeSolve {
                        clicked_cells,
                        target: target.unwrap_or_default(),
                    },
                    nda,
                );
            }
        }

        // Step 6: Classify archetype and init state machine
        let archetype = VisualFingerprinter::classify_archetype(&fp);
        let mut state_machine =
            ChallengeStateMachine::from_archetype(descriptor.clone(), &archetype);

        nda.push(NdaTriple::new(
            session_id,
            253,
            &format!("captcha_archetype:{:?}", archetype),
        ));

        // Step 7: Execute solve sequence
        let max_steps = 10;
        let mut solved = false;

        for _ in 0..max_steps {
            if state_machine.is_terminal() {
                solved = state_machine.is_solved();
                break;
            }

            let actions = state_machine.available_actions();
            if actions.is_empty() {
                break;
            }

            // Pick the highest-confidence action
            let best = actions.iter().max_by(|a, b| {
                a.confidence
                    .partial_cmp(&b.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            if let Some(transition) = best {
                let action = transition.action.clone();
                state_machine.execute_transition(&action);
            } else {
                break;
            }
        }

        // Step 8-9: Record outcome
        if solved {
            let sequence = state_machine.solve_sequence();
            let template = SolveTemplate::new(fp.hash, descriptor.clone(), sequence);
            self.template_store.store(template);
            self.template_store.record_outcome(fp.hash, true);

            nda.push(NdaTriple::new(session_id, 260, "captcha_result:solved"));
            (
                SolveResult::FullSolve {
                    descriptor,
                    steps: state_machine.step_count,
                },
                nda,
            )
        } else {
            // Record failure if we had a template hint
            let had_template = self.template_store.lookup(fp.hash).is_some();
            if had_template {
                self.template_store.record_outcome(fp.hash, false);
            }

            nda.push(NdaTriple::new(session_id, 260, "captcha_result:failed"));
            (
                SolveResult::Failed {
                    reason: format!(
                        "Could not solve {} ({:?}) after {} steps",
                        descriptor.provider, archetype, state_machine.step_count
                    ),
                },
                nda,
            )
        }
    }

    /// Check if a challenge is present in the DOM (quick check without pixel analysis).
    pub fn detect_challenge(tree: &DomTree) -> bool {
        ChallengeObserver::observe(tree).is_some()
    }

    /// Get statistics about the template store: `(total, reliable)`.
    pub fn store_stats(&self) -> (usize, usize) {
        (
            self.template_store.len(),
            self.template_store.reliable_count(),
        )
    }

    /// Export NDA triples for the current session's captcha activity.
    pub fn export_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        triples.push(NdaTriple::new(
            session_id,
            254,
            &format!("captcha_templates:{}", self.template_store.len()),
        ));
        triples
    }

    /// Teach the library that a pixel region depicts `class`. Called after the
    /// LLM classifies a tile, so future identical shapes solve natively.
    pub fn learn_tile(
        &mut self,
        class: &str,
        buffer: &PixelBuffer,
        region: (usize, usize, usize, usize),
    ) {
        self.spline_library.learn_from_region(class, buffer, region);
    }

    /// Feed one animation frame into the temporal monitor. Call repeatedly
    /// across the observation window (~15s) to detect which cell changes.
    pub fn observe_frame(
        &mut self,
        buffer: &PixelBuffer,
        region: (usize, usize, usize, usize),
        rows: usize,
        cols: usize,
        timestamp_ms: u64,
    ) {
        self.temporal
            .capture(buffer, region, rows, cols, timestamp_ms);
    }

    /// The cell that changed most across observed frames (e.g. the tile that
    /// flips or the letter that changes), if any.
    pub fn flipping_cell(&self) -> Option<usize> {
        self.temporal.most_changed_cell().map(|r| r.cell_index)
    }

    /// Match a reference object region against candidate shadow regions
    /// (Azure-style "pick the matching silhouette").
    pub fn match_shadow(
        &self,
        buffer: &PixelBuffer,
        reference_region: (usize, usize, usize, usize),
        candidate_regions: &[(usize, usize, usize, usize)],
    ) -> Option<ShadowMatch> {
        self.shadow_matcher
            .best_match_regions(buffer, reference_region, candidate_regions)
    }

    /// Slice the challenge region into a uniform grid, extract + classify each
    /// cell, and fold in any temporal change signal, producing the context the
    /// rule engine reasons over.
    fn build_solve_context(
        &self,
        buffer: &PixelBuffer,
        region: (usize, usize, usize, usize),
        grid: &GridLayout,
        target: Option<&str>,
    ) -> SolveContext {
        let rows = (grid.rows as usize).max(1);
        let cols = (grid.cols as usize).max(1);
        let mut ctx = SolveContext::new();
        if let Some(t) = target {
            ctx = ctx.with_target(t);
        }
        let changed = self.temporal.changed_regions();
        for r in 0..rows {
            for c in 0..cols {
                let idx = r * cols + c;
                let cell_region = cell_region_of(region, rows, cols, r, c);
                let sig = self.extractor.extract_signature(buffer, cell_region);
                let mut cell = ObservedCell::new(idx, r, c);
                if !sig.is_empty() {
                    if let Some((class, conf)) = self.spline_library.classify(&sig) {
                        cell.classification = Some((class, conf));
                    }
                    cell.signature = Some(sig);
                }
                if let Some(cr) = changed.iter().find(|cr| cr.cell_index == idx) {
                    cell.change_magnitude = cr.magnitude;
                }
                ctx.push_cell(cell);
            }
        }
        ctx
    }
}

/// Compute the pixel region of grid cell `(row, col)` by uniform slicing.
fn cell_region_of(
    region: (usize, usize, usize, usize),
    rows: usize,
    cols: usize,
    row: usize,
    col: usize,
) -> (usize, usize, usize, usize) {
    let (rx, ry, rw, rh) = region;
    let cw = (rw / cols).max(1);
    let ch = (rh / rows).max(1);
    (rx + col * cw, ry + row * ch, cw, ch)
}

/// Strip a leading English article ("a ", "an ", "the ") from `s`.
fn strip_article(s: &str) -> &str {
    for art in ["a ", "an ", "the "] {
        if let Some(rest) = s.strip_prefix(art) {
            return rest;
        }
    }
    s
}

/// Extract a target object noun from an instruction such as
/// "Select all images with a bus" → `"bus"`, or "Select all buses" → `"buses"`.
/// Returns `None` when no object phrase is recognized.
fn parse_target_class(instruction: &str) -> Option<String> {
    let lower = instruction.to_lowercase();
    for marker in [" with ", " containing ", " of a ", " of "] {
        if let Some(pos) = lower.find(marker) {
            let rest = lower[pos + marker.len()..]
                .trim()
                .trim_end_matches('.')
                .trim();
            let cleaned = strip_article(rest).trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    if let Some(pos) = lower.find("select all ") {
        let rest = lower[pos + "select all ".len()..]
            .trim()
            .trim_end_matches('.')
            .trim();
        let cleaned = strip_article(rest).trim();
        // Guard against "select all images" being treated as the target.
        if !cleaned.is_empty() && cleaned != "images" && cleaned != "squares" {
            return Some(cleaned.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::{DomNode, NodeType};
    use std::collections::HashMap;

    fn make_node(id: usize, tag: &str, attrs: &[(&str, &str)], children: Vec<usize>) -> DomNode {
        let mut attributes = HashMap::new();
        for (k, v) in attrs {
            attributes.insert(k.to_string(), v.to_string());
        }
        DomNode {
            id,
            node_type: NodeType::Element,
            tag_name: tag.to_string(),
            attributes,
            text_content: String::new(),
            children,
            parent: None,
        }
    }

    fn make_captcha_tree() -> DomTree {
        DomTree::new(vec![
            make_node(0, "div", &[("class", "page")], vec![1]),
            make_node(1, "div", &[("class", "h-captcha")], vec![2]),
            make_node(
                2,
                "iframe",
                &[("src", "https://hcaptcha.com/1/api2/anchor")],
                vec![],
            ),
        ])
    }

    fn make_grid_buffer() -> PixelBuffer {
        let mut buf = PixelBuffer::new(300, 300);
        // Draw a 3x3 grid of dark cells
        for r in 0..3 {
            for c in 0..3 {
                let x = 10 + c * 100;
                let y = 10 + r * 100;
                buf.fill_rect(x, y, 80, 80, 30, 30, 30, 255);
            }
        }
        buf
    }

    #[test]
    fn fast_path_template_hit() {
        let mut orch = CaptchaOrchestrator::new();
        let tree = make_captcha_tree();
        let buf = make_grid_buffer();

        // First solve — full analysis
        let (result1, _) = orch.solve(&tree, &buf, (0, 0, 300, 300), "session1");
        match result1 {
            SolveResult::FullSolve { .. } | SolveResult::Failed { .. } => {}
            _ => panic!("Expected FullSolve or Failed, got {:?}", result1),
        }

        // Get the visual hash for the buffer
        let fp = orch.fingerprinter.fingerprint(&buf, (0, 0, 300, 300));

        // Manually boost the template's confidence for testing
        if let Some(t) = orch.template_store.lookup_mut(fp.hash) {
            t.confidence = 0.95;
            t.success_count = 5;
        }

        // Second solve — should hit template fast path
        let (result2, nda2) = orch.solve(&tree, &buf, (0, 0, 300, 300), "session1");
        match result2 {
            SolveResult::TemplateReplay { confidence, .. } => {
                assert!(confidence > 0.8);
            }
            _ => panic!("Expected TemplateReplay, got {:?}", result2),
        }
        assert!(nda2.iter().any(|t| t.predicate_id == 251));
    }

    #[test]
    fn no_challenge_detected() {
        let mut orch = CaptchaOrchestrator::new();
        let tree = DomTree::new(vec![make_node(
            0,
            "div",
            &[("class", "normal-page")],
            vec![],
        )]);
        let buf = PixelBuffer::new(100, 100);

        let (result, _) = orch.solve(&tree, &buf, (0, 0, 100, 100), "session1");
        assert!(matches!(result, SolveResult::NoChallenge));
    }

    #[test]
    fn full_solve_cycle() {
        let mut orch = CaptchaOrchestrator::new();
        let tree = make_captcha_tree();
        let buf = make_grid_buffer();

        let (result, nda) = orch.solve(&tree, &buf, (0, 0, 300, 300), "session1");

        // Should have produced NDA triples
        assert!(!nda.is_empty());
        assert!(nda.iter().any(|t| t.predicate_id == 250)); // fingerprint

        // Result should be FullSolve or Failed (depending on state machine)
        match result {
            SolveResult::FullSolve { descriptor, steps } => {
                assert_eq!(descriptor.provider, "hcaptcha");
                assert!(steps > 0);
            }
            SolveResult::Failed { reason } => {
                assert!(!reason.is_empty());
            }
            _ => panic!("Unexpected result: {:?}", result),
        }
    }

    #[test]
    fn detect_challenge_quick_check() {
        let tree = make_captcha_tree();
        assert!(CaptchaOrchestrator::detect_challenge(&tree));

        let empty = DomTree::new(vec![]);
        assert!(!CaptchaOrchestrator::detect_challenge(&empty));
    }

    #[test]
    fn nda_export() {
        let orch = CaptchaOrchestrator::new();
        let triples = orch.export_nda("test_session");
        assert!(!triples.is_empty());
        assert!(triples.iter().any(|t| t.predicate_id == 254));
    }

    // --- Native spline-recognition integration ---

    fn make_grid_tree_2x2(instruction: &str) -> DomTree {
        let mut instr = make_node(5, "p", &[("class", "prompt")], vec![]);
        instr.text_content = instruction.to_string();
        DomTree::new(vec![
            make_node(
                0,
                "div",
                &[("class", "captcha-container")],
                vec![1, 2, 3, 4, 5],
            ),
            make_node(1, "div", &[("class", "tile")], vec![]),
            make_node(2, "div", &[("class", "tile")], vec![]),
            make_node(3, "div", &[("class", "tile")], vec![]),
            make_node(4, "div", &[("class", "tile")], vec![]),
            instr,
        ])
    }

    /// A 200x200 buffer (2x2 grid, 100px cells) with a square only in cell 0.
    fn square_in_cell0() -> PixelBuffer {
        let mut buf = PixelBuffer::new(200, 200);
        buf.fill_rect(25, 25, 50, 50, 20, 20, 20, 255);
        buf
    }

    #[test]
    fn native_rule_solve_after_learning() {
        let mut orch = CaptchaOrchestrator::new();
        let tree = make_grid_tree_2x2("Select all images with a bus");
        let buf = square_in_cell0();
        // Teach the top-left cell's shape as a "bus" a few times so confidence
        // clears the rule threshold.
        for _ in 0..3 {
            orch.learn_tile("bus", &buf, (0, 0, 100, 100));
        }
        let (result, nda) = orch.solve(&tree, &buf, (0, 0, 200, 200), "s");
        match result {
            SolveResult::NativeSolve {
                clicked_cells,
                target,
            } => {
                assert_eq!(clicked_cells, vec![0]);
                assert_eq!(target, "bus");
            }
            other => panic!("expected NativeSolve, got {:?}", other),
        }
        assert!(nda.iter().any(|t| t.predicate_id == 255));
    }

    #[test]
    fn unlearned_challenge_does_not_native_solve() {
        let mut orch = CaptchaOrchestrator::new();
        let tree = make_grid_tree_2x2("Select all images with a bus");
        let buf = square_in_cell0();
        // No learning → library empty → rule engine defers to the LLM path.
        let (result, _) = orch.solve(&tree, &buf, (0, 0, 200, 200), "s");
        assert!(
            !matches!(result, SolveResult::NativeSolve { .. }),
            "should not native-solve without learned shapes, got {:?}",
            result
        );
    }

    #[test]
    fn temporal_observe_detects_flipping_cell() {
        let mut orch = CaptchaOrchestrator::new();
        let blank = PixelBuffer::new(200, 200);
        let mut lit = PixelBuffer::new(200, 200);
        // The buffer starts white; darken cell index 2 = (row 1, col 0) in a
        // 2x2 grid → region (0, 100, 100, 100), a large supra-threshold change.
        lit.fill_rect(0, 100, 100, 100, 10, 10, 10, 255);
        orch.observe_frame(&blank, (0, 0, 200, 200), 2, 2, 0);
        orch.observe_frame(&lit, (0, 0, 200, 200), 2, 2, 100);
        orch.observe_frame(&blank, (0, 0, 200, 200), 2, 2, 200);
        assert_eq!(orch.flipping_cell(), Some(2));
    }

    #[test]
    fn shadow_match_entry_picks_matching_candidate() {
        let orch = CaptchaOrchestrator::new();
        let mut buf = PixelBuffer::new(180, 60);
        buf.fill_rect(10, 10, 40, 40, 20, 20, 20, 255); // reference: square
                                                        // candidate 0 (region 60..120): disc
        for y in 0..60 {
            for x in 60..120 {
                let dx = x as i32 - 90;
                let dy = y as i32 - 30;
                if dx * dx + dy * dy <= 20 * 20 {
                    buf.set_pixel(x, y, 20, 20, 20, 255);
                }
            }
        }
        buf.fill_rect(130, 10, 40, 40, 20, 20, 20, 255); // candidate 1: square
        let best = orch
            .match_shadow(&buf, (0, 0, 60, 60), &[(60, 0, 60, 60), (120, 0, 60, 60)])
            .expect("a match");
        assert_eq!(best.index, 1, "score = {}", best.score);
    }

    #[test]
    fn parses_target_from_instructions() {
        assert_eq!(
            parse_target_class("Select all images with a bus").as_deref(),
            Some("bus")
        );
        assert_eq!(
            parse_target_class("Click each image containing a traffic light").as_deref(),
            Some("traffic light")
        );
        assert_eq!(
            parse_target_class("Select all buses").as_deref(),
            Some("buses")
        );
        assert_eq!(parse_target_class("Verify you are human").as_deref(), None);
        assert_eq!(parse_target_class("Select all images").as_deref(), None);
    }

    #[test]
    fn cell_region_slicing_is_uniform() {
        // 2x2 grid over a 200x200 region → 100px cells.
        assert_eq!(
            cell_region_of((0, 0, 200, 200), 2, 2, 0, 0),
            (0, 0, 100, 100)
        );
        assert_eq!(
            cell_region_of((0, 0, 200, 200), 2, 2, 1, 0),
            (0, 100, 100, 100)
        );
        assert_eq!(
            cell_region_of((0, 0, 200, 200), 2, 2, 0, 1),
            (100, 0, 100, 100)
        );
    }
}
