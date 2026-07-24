# Agentic Browser Subsystem

The `agentic` module within `velocity-browser` (`velocity-browser/src/agentic/`) equips the browser engine with native AI awareness, Accessible Object Model (AOM) extraction, action evaluation, and self-reflection loops.

---

## 🔑 Key Components

### 1. Accessible Object Model (AOM) (`aom_tree.rs`)
Standard DOM trees are overly verbose for LLM prompt context windows. The AOM extractor filters non-visual/non-interactive DOM elements, producing a compact semantic tree:

- **Token Optimization**: Trims redundant style tags, script elements, and zero-size divs.
- **Interactive Element Indexing**: Assigns numerical IDs (`[1]`, `[2]`, etc.) to clickable buttons, inputs, links, and forms.
- **Semantic Labels**: Extracts ARIA roles, placeholder text, field titles, and bounding client rects.

### 2. Action Predictor (`action_predictor.rs`)
- Analyzes current AOM state and user goal instructions to predict next interaction targets.
- Ranks candidate elements based on spatial position, text similarity, and role relevance.

### 3. Reflection Engine (`reflection.rs` & `adaptive_confidence.rs`)
- Evaluates DOM mutations following an action (e.g. form submission, button click).
- Calculates an **Outcome Score** (`outcome_scorer.rs`) based on URL change, DOM structure change, alert modals, or error text presence.
- Adjusts confidence dynamically (`adaptive_confidence.rs`) to trigger automated retries or alternative action paths if an interaction fails.

### 4. Provider Scorer (`provider_scorer.rs`)
- Tracks performance and latency metrics for upstream LLM providers during autonomous browser sessions.
