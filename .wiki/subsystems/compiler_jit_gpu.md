# Compiler, JIT & GPU Inference

_Velocity-IDE's GPU-accelerated LLM inference engine: Vulkan compute shaders, NDA JIT compiler, and model driver pipeline._

---

## Overview

The `velocity-ide/src/compiler/` subsystem is a full GPU compute pipeline for transformer model inference. It provides three layers:

1. **Driver** (12 files) — Vulkan GPU initialization, model pipeline construction, per-layer GEMV dispatch, BitNet and Qwen model support
2. **NDA JIT** (9 files) — x86-64 machine-code JIT compiler for NDA (Non-linear Decomposed Attention) operations, with AST optimizer and executable page management
3. **Shaders** (18 files) — Pre-compiled SPIR-V compute shaders for every transformer operation (attention, normalization, quantization, activations)

Total: **39 source files** forming the local inference backbone.

---

## Architecture

```
compiler/
├── mod.rs                    # Module root: driver, nda_jit, nda_lexer, nda_parser, rust_to_nda, shaders
├── driver/
│   ├── mod.rs                # Re-exports
│   ├── vulkan_init.rs        # VulkanDriver: Instance → Device → Queue → shared buffers
│   ├── vulkan_benchmark.rs   # GPU benchmarking & capability detection
│   ├── model_pipeline.rs     # VulkanModelPipeline: all shader modules + descriptor sets + buffers
│   ├── pipeline_execution.rs # Frame-by-frame inference execution
│   ├── layer_gpu_gemvs.rs    # Per-layer GPU GEMV dispatch table
│   ├── bitnet_layer.rs       # BitNet (1-bit ternary) transformer layer
│   ├── nda_bitnet_layer.rs   # NDA-format BitNet layer (GPU buffers + compute dispatch)
│   ├── gemv.rs               # General matrix-vector multiply (CPU reference)
│   ├── nda_gemv.rs           # NDA-format GEMV (quaternary encoding)
│   ├── qwen_layer.rs         # Qwen model transformer layer
│   └── packing.rs            # Data packing for GPU upload
├── nda_jit/
│   ├── mod.rs                # Re-exports
│   ├── compiler.rs           # JIT compiler: NdaNode AST → closure or x86 machine code
│   ├── x86_emitter.rs        # X86Emitter: byte-level x86-64 instruction encoding
│   ├── optimizer.rs          # AST optimizer: dead-code elimination, constant folding
│   ├── exec_page.rs          # ExecPage: RWX memory page for JIT code execution
│   ├── vm_helpers.rs         # Vectorized NDA arithmetic helpers
│   ├── symbolic_loop.rs      # Symbolic loop unrolling for NDA graph traversal
│   ├── types.rs              # JitProgram, JitFn, JitState, JitVal, VarRegistry
│   └── tests.rs              # JIT correctness tests
└── shaders/
    ├── mod.rs                # Exports all *_SPV static byte arrays
    ├── nda.rs                # General NDA compute shader
    ├── act_nda.rs            # NDA activation function
    ├── act_bitnet.rs         # BitNet ternary activation
    ├── act_qwen.rs           # Qwen activation (SwiGLU variant)
    ├── attn_contig.rs        # Contiguous attention
    ├── attn_ndakv.rs         # NDA-format key-value attention
    ├── attn_softmax.rs       # Attention softmax
    ├── rope.rs               # Rotary position embedding
    ├── rms_norm.rs           # RMS normalization
    ├── swiglu.rs             # SwiGLU gate activation
    ├── residual_add.rs       # Residual connection add
    ├── bias_add.rs           # Bias addition
    ├── kv_write.rs           # Key-value cache write
    ├── fp2.rs                # 2-bit floating point quantization
    ├── fp4.rs                # 4-bit floating point quantization
    ├── int4.rs               # 4-bit integer quantization
    └── ternary.rs            # Ternary weight operations
```

---

## Driver Layer

### VulkanDriver (`vulkan_init.rs`, 649 lines)

The entry point for GPU compute. Creates:
- Vulkan `Instance` (API version 1.2) with "V.E.L.O.C.I.T.Y. IDE Engine" app info
- Enumerates physical devices, selects one with a `COMPUTE` queue family
- Creates logical `Device` + `compute_queue`
- Allocates a shared coherent input buffer (CPU-mapped for upload)

```rust
pub struct VulkanDriver {
    pub entry: Entry,
    pub instance: Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub queue_family_index: u32,
    pub compute_queue: vk::Queue,
    pub shared_input_buffer: vk::Buffer,
    pub shared_input_memory: vk::DeviceMemory,
    pub shared_input_ptr: *mut c_void,
}
```

### VulkanModelPipeline (`model_pipeline.rs`, 864 lines)

Constructs the full inference pipeline for a transformer model:
- Loads 7 SPIR-V shader modules (rms_norm, rope, kv_write, attn_softmax, swiglu, residual_add, bias_add)
- Creates pipeline layouts + descriptor set layouts (2-buffer and 3-buffer variants)
- Allocates per-layer buffers: attention norms, FFN norms, Q/K/V biases, KV caches
- Allocates working buffers: x_residual, attn_out, gated
- Allocates descriptor pool + command pool + fence

### NDA BitNet Layer (`nda_bitnet_layer.rs`, 1022 lines)

Specialized 1-bit ternary weight transformer layer using NDA encoding:
- Separate NDA compute pipeline + activation pipeline
- GPU buffers for: inputs (3200-dim active + position), Q/K/V/O projections (3200-dim), gate/up projections (8640-dim), down projection
- Weight buffers: Q/K/V/O active + position weights
- All data packed in NDA quaternary format for GPU-side ternary dot products

### Pipeline Execution (`pipeline_execution.rs`)

Frame-by-frame inference:
1. Upload input embeddings to shared buffer
2. For each transformer layer: dispatch RMS norm → attention (Q/K/V GEMV) → RoPE → KV write → softmax → output GEMV → residual → FFN norm → SwiGLU → residual
3. Final RMS norm → output projection
4. Read back logits

---

## NDA JIT Compiler

### Two-Tier Execution Model

The JIT provides two tiers of execution for NDA graph operations:

- **Tier 1 (Closure Dispatch)**: Interprets `NdaNode` AST as Rust closures — portable, works on all architectures
- **Tier 2 (x86-64 Machine-Code GEMV)**: Compiles hot GEMV operations to native x86-64 machine code at runtime

```rust
pub fn jit_tier_info() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    { "Tier-1 (Closure Dispatch) + Tier-2 (x86-64 Machine-Code GEMV JIT)" }
    #[cfg(not(target_arch = "x86_64"))]
    { "Tier-1 (Closure Dispatch) + Portable Fallback" }
}
```

### X86Emitter (`x86_emitter.rs`, 1154 lines)

Hand-rolled x86-64 instruction encoder:
- Byte-level emission: `push_rbp`, `pop_rbp`, `mov_rbp_rsp`, `ret`, `mov_eax_imm32`
- GEMV-specific: native matrix-vector multiply codegen with SIMD-friendly register allocation
- `compile_scalar_block()`: compiles a scalar NDA computation block to machine code
- `gemv_native()`: calls the compiled GEMV directly
- `asm_gemv_available()`: runtime check for x86-64 target

### ExecPage (`exec_page.rs`)

Manages RWX (read-write-execute) memory pages for JIT code:
- Allocates executable memory via OS APIs
- Copies compiled machine code into the page
- Provides a function pointer cast for calling the JIT code

### JIT Compiler (`compiler.rs`, 873 lines)

Compiles `NdaNode` AST graphs into executable form:
- `compile_interpreter_sequence()`: converts a sequence of NDA operations into a `JitProgram`
- Supports bitwise ops (And, Or, Xor, Shl, Shr, Not) on both i32 and f32 IEEE-754 bit patterns
- NDA vector operations: element-wise ops on `NdaVec` with quaternary encoding
- `MAX_WHILE_ITERATIONS = 10_000_000` safety limit
- Integrates with `optimizer.rs` for AST optimization before compilation

### JIT Types (`types.rs`)

```rust
pub struct JitProgram { /* compiled executable */ }
pub enum JitFn { /* closure or native code */ }
pub enum JitVal { Scalar(f32), Vector(Arc<NdaVec>), ... }
pub struct JitState { /* execution state */ }
pub struct VarRegistry { /* variable bindings */ }
```

---

## Shader Layer

### SPIR-V Compute Shaders

Each shader file exports a `&[u32]` static (e.g., `NDA_SPV`, `RMS_NORM_SPV`) containing pre-compiled SPIR-V bytecode. These are loaded directly into Vulkan shader modules at pipeline creation time.

| Shader | Purpose |
|--------|---------|
| `NDA_SPV` | General NDA compute (row/col scatter-gather) |
| `ACT_NDA_SPV` | NDA activation function |
| `ACT_BITNET_SPV` | BitNet ternary activation |
| `ACT_QWEN_SPV` | Qwen-specific activation |
| `ATTN_CONTIG_SPV` | Contiguous attention computation |
| `ATTN_NDAKV_SPV` | NDA-format key-value attention |
| `ATTN_SOFTMAX_SPV` | Attention softmax (numerically stable) |
| `ROPE_SPV` | Rotary position embedding |
| `RMS_NORM_SPV` | RMS layer normalization |
| `SWIGLU_SPV` | SwiGLU gated activation |
| `RESIDUAL_ADD_SPV` | Residual connection addition |
| `BIAS_ADD_SPV` | Bias vector addition |
| `KV_WRITE_SPV` | Key-value cache write |
| `FP2_SPV` | 2-bit float quantization/dequantization |
| `FP4_SPV` | 4-bit float quantization/dequantization |
| `INT4_SPV` | 4-bit integer quantization/dequantization |
| `TERNARY_SPV` | Ternary weight {-1, 0, 1} operations |

### Quantization Support

The shader layer supports multiple weight quantization formats:
- **FP2** (2-bit): Extreme compression, ~4x smaller than INT4
- **FP4** (4-bit): Balanced compression for edge deployment
- **INT4** (4-bit): Standard quantization format
- **Ternary** (BitNet): {-1, 0, 1} weights — multiply-free inference via additions only

---

## Key Design Decisions

- **Vulkan over CUDA**: Cross-GPU-vendor support (AMD, NVIDIA, Intel) without proprietary runtime dependency
- **SPIR-V embedded as `&[u32]`**: Zero file-I/O at startup, shaders compiled into the binary
- **NDA quaternary encoding**: Weights stored in a 2-bit quaternary representation that maps naturally to ternary/bit-serial GPU operations
- **Tier-2 JIT for x86-64 only**: ARM/other architectures fall back to Tier-1 closure dispatch
- **HKDF-derived per-artifact keys**: NDA model weights are encrypted at rest with domain-separated subkeys
- **`ash` crate**: Type-safe Vulkan bindings, no code generation

---

## Dependencies

- `ash` — Vulkan Rust bindings (instance, device, pipeline, descriptor management)
- `memmap2` — Memory-mapped files (shared memory IPC)
- Internal: `velocity-ide::nda` (NdaMatrix, NdaVec), `velocity-ide::nda_int` (NDA interpreter primitives)
- Internal: `velocity-ide::site_map::verifier` (BitwiseOp, MathOp, CmpOp for JIT)

---

## See Also

- [SiteMap & NDA Binary Compiler](sitemap_nda_compiler.md) — RDF triple store and NDA format
- [LLM Inference Harness](../architecture/velocity_ide.md) — Model loading and provider integration
- [NDA Format & Security Model](nda_security.md) — NDA binary format and encryption
