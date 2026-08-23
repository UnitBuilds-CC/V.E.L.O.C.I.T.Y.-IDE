# CAPTCHA Solver Engine

_Modular CAPTCHA detection, fingerprinting, and solving system with template replay, shape matching, rule engine, and LLM fallback._

---

## Overview

The `velocity-browser/src/engine/captcha/` subsystem is a 14-file modular CAPTCHA solving engine designed for agent-driven browser automation. It uses a fingerprint-first strategy: attempt to solve challenges via visual fingerprinting and learned templates before spending any LLM tokens.

---

## Module Structure

```
engine/captcha/
├── mod.rs                # Re-exports all public types
├── challenge.rs          # ChallengeDescriptor, ChallengeFeatures, SolveAttempt, SolveState
├── orchestrator.rs       # CaptchaOrchestrator: 9-step solve loop (751 lines)
├── visual_fingerprint.rs # VisualFingerprinter: pixel → ChallengeArchetype + hash
├── fingerprint.rs        # ProviderFingerprinter: identify hCaptcha/reCAPTCHA/etc.
├── observer.rs           # ChallengeObserver: DOM → ChallengeSnapshot with grid layout
├── state_machine.rs      # ChallengeStateMachine: multi-step FSM for challenge solving
├── template_store.rs     # TemplateStore: learned solution cache keyed by visual hash
├── rule_engine.rs        # RuleEngine: deterministic solver with LLM fallback
├── spline.rs             # SplineExtractor: contour extraction, rotation/scale-invariant shapes
├── spline_library.rs     # SplineLibrary: online learning store (signature → object class)
├── shape_match.rs        # ShapeMatcher: fuzzy shape matching
├── shadow_match.rs       # ShadowMatcher: Azure-style shadow/silhouette matching
└── temporal.rs           # TemporalMonitor: frame-differencing for animated challenges
```

---

## Solve Loop (9 Steps)

The `CaptchaOrchestrator` implements a cost-escalation strategy:

```
1. Rasterize challenge region → PixelBuffer
2. Fingerprint pixels → VisualFingerprint (free, ~microseconds)
3. Template lookup by hash:
   ├── HIT + confidence > 0.8 → replay stored sequence (ZERO tokens)
   ├── HIT + low confidence  → use as hint, verify via DOM
   └── MISS → continue to step 4
4. DOM observation → ChallengeSnapshot (grid layout, interactive elements)
5. Provider fingerprint → ChallengeDescriptor (hCaptcha/recaptcha/etc.)
6. State machine init from archetype
7. Execute actions via StealthHumanBehavior
8. On success: store template keyed by visual_hash
9. On failure: decrement confidence, try alternative archetype
```

### SolveResult Variants

```rust
pub enum SolveResult {
    TemplateReplay { visual_hash: u64, confidence: f32 },  // Zero tokens
    NativeSolve { clicked_cells: Vec<usize>, target: String },  // Zero tokens
    FullSolve { descriptor: ChallengeDescriptor, steps: u32 },  // Tokens spent
    Failed { reason: String },
    NoChallenge,
}
```

---

## Key Components

### VisualFingerprinter

Converts a pixel buffer into a perceptual hash + archetype classification:
- Computes a compact visual signature from the challenge image
- Classifies into `ChallengeArchetype` (grid-click, slider, checkbox, image-select)
- Enables O(1) template cache lookup

### ProviderFingerprinter

Identifies the CAPTCHA provider (hCaptcha, reCAPTCHA v2/v3, Cloudflare Turnstile, etc.) from DOM structure and script attributes.

### ChallengeObserver

Extracts a structured `ChallengeSnapshot` from the live DOM:
- Grid layout detection (rows × cols)
- Interactive element positions
- Cell boundaries and labels
- Target description text

### ChallengeStateMachine

Multi-step finite state machine for solving:
- States: `Detecting → Classifying → Selecting → Verifying → Solved/Failed`
- Transitions driven by DOM observations and user-action simulation
- Handles retry logic and alternative archetype fallback

### TemplateStore

Persistent cache of solved challenges:
- Keyed by visual hash (perceptual fingerprint)
- Stores the sequence of clicks/actions that solved it
- Confidence score that decays on failure
- Enables zero-token replay for recurring challenge types

### RuleEngine

Deterministic solver evaluated before LLM fallback:
- Rule-based: "if grid has N images matching target, click them"
- Uses `ObservedCell` from the observer + `SolveContext`
- Produces `SolveAction` (click coordinates or skip)
- Falls back to LLM only when rules can't determine a solution

### SplineExtractor + SplineLibrary

Shape recognition for image-based challenges:
- Contour extraction from rasterized regions
- Rotation and scale-invariant shape signatures (radial bins)
- Online learning: classified shapes stored in `SplineLibrary`
- Enables "click all traffic lights" by matching learned shape signatures

### ShadowMatcher

Azure-style shadow/silhouette matching:
- Compares 2D transforms of candidate shapes against target silhouettes
- Uses `Transform2D` for rotation, scaling, translation
- Handles partial occlusion and overlapping objects

### TemporalMonitor

Frame-differencing monitor for animated/transient challenges:
- Tracks `ChangedRegion` across frames
- Detects when challenge content stabilizes (safe to solve)
- Identifies animated elements that should be ignored

---

## Key Design Decisions

- **Fingerprint-first**: Always try free/cheap methods before spending LLM tokens
- **Zero-token solve paths**: Template replay and native shape matching cost nothing
- **Online learning**: SplineLibrary grows from LLM-classified shapes, reducing future LLM dependency
- **Provider-agnostic**: Fingerprinter identifies the provider, then provider-specific strategies apply
- **Modular**: Each solver component is independent — can be tested and replaced individually

---

## See Also

- [Agentic Browser Subsystem](agentic_browser.md) — AOM tree extraction, action predictor
- [Browser Engine & Networking](../architecture/velocity_browser.md) — Engine capabilities, session management
- [JS Interpreter & Runtime](js_interpreter_runtime.md) — DOM bridge for in-page CAPTCHA interaction
