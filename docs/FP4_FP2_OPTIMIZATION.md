# FP4/FP2 Fused Pipeline Optimization

## Overview

The V.E.L.O.C.I.T.Y. transformer supports multiple weight quantization formats:
- **FP32**: Full precision (baseline)
- **FP4**: 4-bit floating point (2x memory reduction)
- **FP2**: 2-bit floating point (4x memory reduction)

Currently, FP4/FP2 weights use individual GPU GEMV (General Matrix-Vector) dispatches with CPU-bound attention, while FP32 weights use a fully fused Vulkan pipeline. This document outlines the optimization to enable the fused pipeline for FP4/FP2 weights.

## Current State

### FP32 Path (Optimized)
```
GPU Command Buffer:
  ┌─────────────────────────────────────┐
  │ Layer 0: Q_proj → K_proj → V_proj  │
  │          → Attention → Output_proj  │
  │ Layer 1: ...                        │
  │ ...                                 │
  │ Layer N: ...                        │
  └─────────────────────────────────────┘
  Single dispatch, minimal CPU-GPU sync
```

### FP4/FP2 Path (Current)
```
Per-Layer CPU Loop:
  for each layer:
    Q_proj = GPU_GEMV(weight, input) * global_scale  // GPU
    K_proj = GPU_GEMV(weight, input) * global_scale  // GPU
    V_proj = GPU_GEMV(weight, input) * global_scale  // GPU
    Attention = CPU_attention(Q, K, V)               // CPU ← bottleneck
    Output = GPU_GEMV(weight, attention) * global_scale  // GPU
  
  CPU-GPU sync per layer (slow)
```

## Problem

The fused Vulkan pipeline shaders don't account for the `global_scale` factor applied to FP4/FP2 weights. This scaling is currently applied externally in `nda_gemv_gpu_or_cpu()`, which prevents the fused pipeline from being used.

**Impact:**
- FP4/FP2 models run attention on CPU (slow)
- Multiple CPU-GPU synchronizations per forward pass
- ~3-5x slower than FP32 GPU path for large models

## Solution Options

### Option A: Push Constant (Recommended)

Add `global_scale` as a push constant to the GEMV shader.

**Implementation:**
1. Modify `gemv_shader.glsl` to accept `global_scale` push constant
2. Apply scale inside shader: `output = gemv(weight, input) * global_scale`
3. Update `record_gpu_pipeline()` to push scale per-layer
4. Remove external scaling in `nda_gemv_gpu_or_cpu()`

**Pros:**
- Minimal shader changes
- No additional compute passes
- Maintains single-dispatch efficiency

**Cons:**
- Requires shader recompilation
- Push constant limit (typically 128 bytes, but we only need 4 bytes)

**Estimated Effort:** 2-3 days

### Option B: Scale-Multiply Compute Pass

Record a separate scale-multiply compute pass after each FP4 GEMV dispatch.

**Implementation:**
1. Create `scale_multiply_shader.glsl`: `output[i] = input[i] * scale`
2. Modify `record_gpu_pipeline()` to insert scale pass after each GEMV
3. Keep external scaling for individual GEMV path (backward compatibility)

**Pros:**
- Cleaner separation of concerns
- Easier to debug (scale is visible in command buffer)
- Can optimize scale pass independently

**Cons:**
- Additional compute pass per GEMV (overhead)
- More complex command buffer recording
- ~10-15% slower than Option A

**Estimated Effort:** 3-4 days

### Option C: Pre-Scaled Weights

Pre-multiply weights by `global_scale` during model loading.

**Implementation:**
1. Modify weight loading to apply `global_scale` to FP4/FP2 weights
2. Store scaled weights in GPU buffers
3. Remove scaling from GEMV shader and external code

**Pros:**
- Simplest runtime path (no scaling needed)
- Fastest execution

**Cons:**
- Increases memory usage (defeats quantization purpose)
- Loses precision (scale applied once, not per-operation)
- Breaks weight sharing between models

**Estimated Effort:** 1 day (but not recommended)

## Recommended Approach

**Option A (Push Constant)** is recommended because:
1. Minimal performance overhead
2. Maintains memory efficiency
3. Preserves numerical precision
4. Clean implementation

## Implementation Plan

### Phase 1: Shader Modification (Day 1)

1. Update `gemv_shader.glsl`:
```glsl
layout(push_constant) uniform PushConstants {
    uint rows;
    uint cols;
    float global_scale;  // NEW
} pc;

void main() {
    uint row = gl_GlobalInvocationID.x;
    if (row >= pc.rows) return;
    
    float sum = 0.0;
    for (uint col = 0; col < pc.cols; col++) {
        sum += weight[row][col] * input[col];
    }
    output[row] = sum * pc.global_scale;  // Apply scale here
}
```

2. Update Rust shader compilation to include push constant layout

### Phase 2: Pipeline Recording (Day 2)

1. Modify `record_gpu_pipeline()` in `transformer.rs`:
```rust
for layer in &weights.layers {
    // Push global_scale for this layer
    let scale = layer.q_proj.global_scale;
    push_constants.push(scale);
    
    // Record Q_proj GEMV
    record_gemv(&layer.q_proj, scale);
    // ... rest of layer
}
```

2. Update push constant buffer allocation

### Phase 3: Cleanup & Testing (Day 3)

1. Remove external scaling from `nda_gemv_gpu_or_cpu()` for fused path
2. Keep external scaling for individual GEMV path (backward compatibility)
3. Add unit tests for FP4/FP2 fused pipeline
4. Benchmark performance improvement

### Phase 4: Validation (Day 4)

1. Verify numerical accuracy against CPU path
2. Benchmark on multiple GPU architectures
3. Profile CPU-GPU synchronization overhead
4. Document performance characteristics

## Success Criteria

- [ ] FP4/FP2 models use fused GPU pipeline
- [ ] Numerical accuracy within 1e-4 of CPU path
- [ ] Performance within 10% of FP32 GPU path
- [ ] No regression in FP32 path
- [ ] All existing tests pass

## Risk Mitigation

**Risk:** Shader compilation fails on some GPU drivers
- **Mitigation:** Fallback to individual GEMV path if fused pipeline fails to build

**Risk:** Push constant not supported on older GPUs
- **Mitigation:** Check `max_push_constants_size` during pipeline creation, fallback if < 4 bytes

**Risk:** Numerical precision loss
- **Mitigation:** Use FP32 accumulation in shader, validate against CPU path

## References

- `velocity-ide/src/model/transformer.rs:732` — Original TODO
- `velocity-ide/src/model/nda_gemv.rs` — Current GEMV implementation
- `velocity-ide/src/compiler/vulkan_benchmark.rs` — Vulkan pipeline benchmarks
- [Vulkan Push Constants Spec](https://vulkan.org/specs/push-constants)

## Timeline

- **Week 1:** Implementation (Phases 1-3)
- **Week 2:** Testing & validation (Phase 4)
- **Week 3:** Performance tuning & documentation

## Future Work

After completing this optimization:
1. Add FP8 support (8-bit floating point)
2. Implement mixed-precision training
3. Add quantization-aware training pipeline
4. Optimize attention kernel for FP4/FP2
