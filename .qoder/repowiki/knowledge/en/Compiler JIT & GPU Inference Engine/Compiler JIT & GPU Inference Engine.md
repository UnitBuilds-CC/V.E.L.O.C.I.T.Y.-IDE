# Compiler JIT & GPU Inference Engine

## Classification
- **Category**: Core Runtime
- **Files**: velocity-ide/src/compiler/ (39 files: driver/ 12, nda_jit/ 9, shaders/ 18)
- **Criticality**: Critical — local LLM inference backbone

## Summary

GPU-accelerated transformer inference engine using Vulkan compute shaders (18 SPIR-V modules), a two-tier JIT compiler (closure dispatch + x86-64 machine code), and model drivers for BitNet (1-bit ternary) and Qwen architectures.

## Architecture

```
compiler/
├── driver/         # Vulkan GPU pipeline: init → model → execution
│   ├── vulkan_init.rs        # VulkanDriver: Instance → Device → Queue
│   ├── model_pipeline.rs     # VulkanModelPipeline: shaders + buffers + descriptors
│   ├── pipeline_execution.rs # Frame-by-frame inference
│   ├── nda_bitnet_layer.rs   # NDA BitNet (1-bit ternary) GPU layer
│   ├── nda_gemv.rs           # NDA-format GEMV
│   └── qwen_layer.rs         # Qwen model layer
├── nda_jit/        # x86-64 JIT for NDA operations
│   ├── compiler.rs           # AST → closure or machine code
│   ├── x86_emitter.rs        # Byte-level x86-64 encoding
│   ├── optimizer.rs          # Dead-code elimination, constant folding
│   └── exec_page.rs          # RWX memory page for JIT code
└── shaders/        # 18 SPIR-V compute shaders
    ├── nda.rs, act_nda.rs, act_bitnet.rs, act_qwen.rs
    ├── attn_contig.rs, attn_ndakv.rs, attn_softmax.rs
    ├── rope.rs, rms_norm.rs, swiglu.rs, residual_add.rs, bias_add.rs
    ├── kv_write.rs, fp2.rs, fp4.rs, int4.rs, ternary.rs
```

## Key Types

- `VulkanDriver` — GPU instance, device, compute queue, shared input buffer
- `VulkanModelPipeline` — All shader modules + descriptor sets + per-layer buffers
- `VulkanNdaBitNetLayer` — NDA BitNet layer with GPU compute pipelines
- `X86Emitter` — Hand-rolled x86-64 instruction encoder
- `JitProgram` / `JitFn` / `JitVal` — JIT compiled program types

## Key Design Decisions

- Vulkan over CUDA for cross-vendor GPU support (AMD, NVIDIA, Intel)
- SPIR-V shaders embedded as `&[u32]` — zero file I/O at startup
- NDA quaternary encoding for 2-bit weight representation
- Tier-2 JIT only on x86-64; portable fallback on other architectures
- `ash` crate for type-safe Vulkan bindings
