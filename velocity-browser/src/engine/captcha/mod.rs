//! Generic CAPTCHA solving module.
//!
//! Replaces the old `captcha_solver.rs` with a modular architecture:
//! - `challenge` — generic challenge descriptor (replaces CaptchaType enum)
//! - `visual_fingerprint` — OCR-based pixel signature extraction
//! - `state_machine` — multi-step challenge FSM
//! - `observer` — DOM observation pipeline
//! - `fingerprint` — provider identification
//! - `template_store` — learned solution cache
//! - `orchestrator` — coordinator with fingerprint-first fast path
//! - `spline` — contour extraction + rotation/scale-invariant shape signatures
//! - `shape_match` — fuzzy shape matching (rotation/scale invariant)
//! - `temporal` — frame-differencing monitor for transient/animated challenges
//! - `spline_library` — online learning store (signature → object class)
//! - `rule_engine` — deterministic solver with LLM fallback
//! - `shadow_match` — Azure-style shadow/silhouette matching

pub mod challenge;
pub mod fingerprint;
pub mod observer;
pub mod orchestrator;
pub mod rule_engine;
pub mod shadow_match;
pub mod shape_match;
pub mod spline;
pub mod spline_library;
pub mod state_machine;
pub mod template_store;
pub mod temporal;
pub mod visual_fingerprint;

// Re-export primary types
pub use challenge::{
    CaptchaPosition, CaptchaType, ChallengeDescriptor, ChallengeFeatures, SolveAttempt, SolveState,
};
pub use fingerprint::{ProviderFingerprinter, ProviderSignature};
pub use observer::{
    ChallengeObserver, ChallengeSnapshot, ElementState, GridLayout, InteractiveElement,
};
pub use orchestrator::{ActiveChallenge, CaptchaOrchestrator, SolveResult};
pub use rule_engine::{
    ObservedCell, RuleCondition, RuleEngine, SolveAction, SolveContext, SolveRule,
};
pub use shadow_match::{ShadowMatch, ShadowMatcher, Transform2D};
pub use shape_match::ShapeMatcher;
pub use spline::{Point2D, ShapeSignature, SplineExtractor, SplineSegment, RADIAL_BINS};
pub use spline_library::{ClassifiedShape, SplineLibrary};
pub use state_machine::{
    ActionKind, ChallengeAction, ChallengeState, ChallengeStateMachine, StateTransition,
};
pub use template_store::{SolveTemplate, TemplateStore};
pub use temporal::{ChangedRegion, FrameSnapshot, TemporalMonitor};
pub use visual_fingerprint::{
    AspectBucket, ChallengeArchetype, VisualFingerprint, VisualFingerprinter,
};

// Backward compatibility: re-export CaptchaSolverEngine from the old module
// so existing code continues to work during migration.
pub use crate::engine::captcha_solver::CaptchaSolverEngine;
