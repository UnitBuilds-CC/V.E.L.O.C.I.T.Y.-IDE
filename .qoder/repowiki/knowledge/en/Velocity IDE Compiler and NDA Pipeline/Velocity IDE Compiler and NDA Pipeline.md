# Velocity IDE Compiler and NDA Pipeline

## Classification
- **Category**: Primary Crate
- **Files**: ~78 source files
- **Criticality**: High — compiler and model inference

## Summary

`velocity-ide` contains the NDA compiler pipeline (lexer → parser → lowering → JIT → sandbox), GPU shader modules for model inference, transformer model definitions, NDA interpreter, RDF triple store with Merkle verification, and automated wiki generator.

## Module Breakdown

| Module | Files | Purpose |
|--------|-------|---------|
| `compiler/` | 45 | NDA lexer/parser, JIT, shaders, driver |
| `site_map/` | 7 | RDF triple store, Merkle verification |
| `model/` | 5 | Transformer config, weights, inference |
| `nda_int/` | 5 | NDA interpreter (ops, tables, GEMV) |
| `wiki/` | 4 | Automated documentation generator |
| `sandbox/` | 3 | JIT sandbox, scope validator |

## Compiler Pipeline

1. `nda_lexer.rs` → Token stream
2. `nda_parser.rs` → AST
3. `rust_to_nda.rs` → NDA IR
4. `nda_jit/` → Native x86 code
5. `sandbox/` → Sandboxed validation

## GPU Shaders (16 files)

BitNet, Qwen, and NDA activation shaders for GPU-accelerated transformer inference via Vulkan.
