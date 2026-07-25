# Agentic Browser Subsystem

The `agentic/` module within `velocity-browser` (10 files) equips the browser engine with native AI awareness: Accessible Object Model extraction, action prediction, outcome scoring, self-reflection, and spatial vector memory.

---

## Accessible Object Model (AOM)

### Purpose

Standard DOM trees contain excessive noise for LLM consumption — style tags, script elements, zero-size divs, and decorative markup. The AOM extractor filters these into a compact semantic tree optimized for token efficiency.

### Implementation (`aom_tree.rs`)

```rust
pub struct AgenticAomTree {
    pub nodes: Vec<AgenticAomNode>,
    // ...
}
pub struct AgenticAomNode {
    // Interactive element with numerical ID, ARIA role,
    // bounding rect, accessible text
}
```

**Key behaviors**:
- **Token optimization**: Strips non-visual/non-interactive DOM elements
- **Interactive element indexing**: Assigns sequential numerical IDs (`[1]`, `[2]`, ...) to clickable buttons, inputs, links, and forms
- **Semantic label extraction**: Pulls ARIA roles, placeholder text, field titles, and `getBoundingClientRect()` data
- **Compact prompt serialization**: `to_compact_prompt()` produces a minimal text representation suitable for LLM context windows

### AOM Test Coverage

`aom_test.rs` provides dedicated test coverage for AOM extraction correctness.

---

## Action Predictor Engine

### Implementation (`action_predictor.rs`)

```rust
pub struct ActionPredictorEngine { ... }

pub struct PredictedActionTarget {
    // Target element reference, confidence score, action type
}
```

The predictor analyzes the current AOM state alongside user goal instructions to rank candidate interaction targets:

1. **Spatial analysis**: Element position and bounding rect relevance
2. **Text similarity**: Semantic match between goal text and element labels
3. **Role relevance**: ARIA role weighting (buttons > divs for click actions)
4. **Confidence scoring**: `AdaptiveConfidenceScorer` adjusts thresholds dynamically

### Adaptive Confidence (`adaptive_confidence.rs`)

```rust
pub struct AdaptiveConfidenceScorer { ... }  // (exact name may vary)
```

Dynamically adjusts confidence thresholds based on:
- Previous action success/failure rate
- DOM mutation magnitude after each action
- Provider latency and response quality

---

## Outcome Scorer & Reflection

### Outcome Scorer (`outcome_scorer.rs`)

```rust
pub struct OutcomeScorer { ... }  // (exact name may vary)
```

Evaluates DOM mutations following an action to determine success:

| Signal | Weight | Description |
|--------|--------|-------------|
| URL change | High | Navigation occurred |
| DOM structure change | Medium | Content updated |
| Alert modal presence | Negative | Error or warning appeared |
| Error text detection | Negative | Visible error message |
| Form submission success | High | Form accepted |

### Reflection Engine (`reflection.rs`)

The reflection loop closes the agentic cycle:

```
1. Agent takes action (click, type, navigate)
       │
       ▼
2. DOM mutation observed via MutationBatcher
       │
       ▼
3. OutcomeScorer evaluates mutation
       │
       ▼
4. AdaptiveConfidence adjusts thresholds
       │
       ▼
5. If score below threshold → retry with alternative action
   If score above threshold → proceed to next step
```

---

## Provider Scorer

### Implementation (`provider_scorer.rs`)

Tracks performance metrics for upstream LLM providers during autonomous browser sessions:

- **Latency tracking**: Response time per provider
- **Success rate**: Tool call success/failure ratio
- **Quality scoring**: Outcome score correlation per provider
- **Routing decisions**: Prefer providers with better browser-task performance

---

## NDA Encoder

### Implementation (`nda_encoder.rs`)

```rust
pub struct NdaEncoder { ... }
```

Serializes AOM trees, action histories, and reflection results into compact NDA binary format for:
- Efficient storage in `.velocity/agentic/` run data
- Fast round-trip between agent sessions
- Deterministic replay of agentic browser sessions

---

## OCR Text Engine

### Implementation (`ocr_map.rs`)

```rust
pub struct VelocityOcrEngine { ... }
pub struct OcrTextBoundingBox { ... }
```

On-screen text recognition for browser content:
- Extracts text from rendered page regions
- Provides bounding box coordinates for each recognized text element
- Supplements AOM when visual text is not in the DOM (canvas, images with text)

---

## Zero-Allocation NDA Writer

### Implementation (`zero_alloc_writer.rs`)

```rust
pub struct ZeroAllocNdaWriter { ... }
```

Writes NDA binary data without heap allocations:
- Stack-buffer-only serialization
- Critical for hot-path agentic loops where GC pressure must be minimized
- Used by the reflection engine for rapid state encoding

---

## Vector Memory & Site Memory

### Implementation (`vector_memory.rs`)

```rust
pub struct SiteVectorStore { ... }
```

Spatial AOM site memory for persistent browser session knowledge:
- Stores AOM snapshots as spatial vectors
- Enables similarity search across previously visited pages
- Supports agent recall of page structures from prior sessions
- Located at `velocity-browser/src/vector_memory.rs`

---

## Data Flow: Agentic Browser Loop

```
1. Agent receives goal (e.g., "fill out the contact form")
       │
       ▼
2. BrowserSession navigates to target URL
       │
       ▼
3. AOM extracted from DOM (aom_tree.rs)
       │
       ▼
4. ActionPredictorEngine ranks candidate elements
       │
       ▼
5. Highest-confidence action executed (click/type)
       │
       ▼
6. DOM mutation observed → OutcomeScorer evaluates
       │
       ▼
7. Reflection engine decides: proceed or retry
       │
       ▼
8. NdaEncoder serializes state → SiteVectorStore caches
       │
       ▼
9. Loop to step 3 until goal complete
```
