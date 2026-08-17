# Dual-Path Engine Text and NDA Routing

## Classification
- **Category**: Architecture / Pipeline
- **Files**: velocity-ide/src/pipeline_bridge.rs (319 LOC), velocity-ide/src/pipeline_nda.rs, velocity-ide/src/tokenizer.rs
- **Criticality**: High — bridges natural language understanding with deterministic code generation

## Summary

The `DualPathEngine` routes user requests between two execution paths. Path 1 (text) handles natural language via the transformer model, producing a hidden_state[896]. Path 2 (NDA) generates Merkle-verified NDA programs conditioned on that hidden state. The bridge ensures that fuzzy intent from Path 1 is anchored to structurally valid, cryptographically verified NDA output from Path 2.

## Routing Logic (Auto Mode)

- **Imperative creation verbs or code keywords** → NDA mode (Path 2)
- **Questions, explanations** → Text mode (Path 1)
- Both paths share the same model weights on disk but maintain separate runtime state (KV caches, head weights)

## Architecture

```
User request
    │
    ▼
DualPathEngine.route()
    ├── Path 1 (Text): NL → Transformer → hidden_state[896]
    │       └── Can hallucinate (acceptable for fuzziness)
    └── Path 2 (NDA): hidden_state → NDA pipeline → verified nodes
            └── Cannot hallucinate (structurally invalid output rejected at emit)
```

## Key Design Decisions

1. **Lazy loading**: Path 1 is loaded on first text request to save RAM
2. **Shared weights**: Both paths use the same model weights from disk
3. **Separate state**: Each path maintains its own KV cache and runtime state
4. **NDA-KV cache**: Path 2 uses hash-chained Merkle blocks (v2 quad bitmap, 2 bits per element)

## Tokenizer

BPE tokenizer (`tokenizer.rs`) handles encode/decode for the Qwen 2.5 Coder vocabulary (151,936 tokens). Supports both Path 1 text generation and Path 2 intent encoding.
