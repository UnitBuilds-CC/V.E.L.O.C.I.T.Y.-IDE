# Velocity IDE Compiler and NDA Pipeline

## Classification
- **Category**: Primary Crate
- **Files**: ~77 source files
- **Criticality**: High — compiler, model inference, and dual-path engine

## Summary

`velocity-ide` contains the NDA compiler pipeline (lexer → parser → lowering → JIT → sandbox), GPU shader modules for model inference, transformer model definitions (Qwen 2.5 Coder 0.5B with NDA-4bit weights), the dual-path engine (text ↔ NDA bridge), BPE tokenizer, NDA interpreter, RDF triple store with Merkle verification, and automated wiki generator.

## Module Breakdown

| Module | Files | Purpose |
|--------|-------|---------|
| `compiler/` | 45 | NDA lexer/parser, JIT, shaders, driver |
| `site_map/` | 7 | RDF triple store, Merkle verification |
| `model/` | 5 | Transformer config, weights, zero-alloc inference |
| `nda_int/` | 5 | NDA interpreter (ops, tables, GEMV) |
| `wiki/` | 4 | Automated documentation generator |
| `sandbox/` | 3 | JIT sandbox, scope validator |
| Top-level | 8 | Dual-path engine, tokenizer, pipeline bridge, NDA format, safety |

## Compiler Pipeline

1. `nda_lexer.rs` → Token stream
2. `nda_parser.rs` → AST
3. `rust_to_nda.rs` → NDA IR
4. `nda_jit/` → Native x86 code
5. `sandbox/` → Sandboxed validation

## Model Inference

- `model/config.rs`: Qwen 2.5 Coder 0.5B (24 layers, hidden=896, GQA 14/2, ALiBi) and BitNet 3B configs
- `model/weights.rs`: NDA-4bit (FP4) weight loading from `.nda` files, fused GEMV creation (Q‖K‖V, gate‖up)
- `model/transformer.rs`: Zero-alloc forward pass, in-place LM head, autoregressive generation with temp/top-p

## Dual-Path Engine

- `pipeline_bridge.rs`: `DualPathEngine` routes between Path 1 (text) and Path 2 (NDA)
- `pipeline_nda.rs`: NDA-native pipeline with Merkle-verified output
- `tokenizer.rs`: BPE tokenizer (encode/decode) for Qwen vocabulary (151,936 tokens)

## GPU Shaders (18 files)

BitNet, Qwen, NDA activation, FP4/FP2 low-precision, attention (contiguous + NDA-KV), KV-write, RMS norm, RoPE, SwiGLU, residual add, bias add, int4, ternary, and core NDA shaders for GPU-accelerated transformer inference via Vulkan.

## Model Driver (12 files)

Orchestrates GPU execution for transformer inference. Includes fused GEMV weights (Q‖K‖V concatenated), `scales: [f32; 3]` per-matrix scale infrastructure, and zero-allocation forward pass via pre-allocated scratch buffers.
