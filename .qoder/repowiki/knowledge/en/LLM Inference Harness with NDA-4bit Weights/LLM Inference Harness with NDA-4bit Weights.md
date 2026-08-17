# LLM Inference Harness with NDA-4bit Weights

## Classification
- **Category**: Model Inference
- **Files**: velocity-ide/src/model/ (5 files), velocity-ide/src/pipeline_bridge.rs, tokenizer.rs, pipeline_nda.rs
- **Criticality**: High — enables built-in LLM without external dependencies

## Summary

The LLM inference harness loads Qwen 2.5 Coder 0.5B weights in NDA-4bit (FP4) format and runs autoregressive generation with zero-allocation forward passes. The dual-path engine routes text input through the transformer to produce a hidden_state[896] that conditions NDA program generation — giving the IDE a "reasonably smart model strapped in" at extraordinary speeds.

## Model Configuration

`ModelConfig::qwen_coder_05b()` — exact Qwen 2.5 Coder 0.5B architecture:

| Parameter | Value |
|-----------|-------|
| Layers | 24 |
| Hidden size | 896 |
| FFN size | 4864 |
| Q heads | 14 |
| KV heads | 2 (GQA) |
| Head dim | 64 |
| Vocab size | 151,936 |
| Max seq len | 2048 |
| Positional encoding | ALiBi (bit-shift, zero multiplication) |
| EOS token | 151,645 |

## Weight Format (NDA-4bit / FP4)

- `NDA_VERSION_FP4 = 3`: Each weight is 4 bits (sign + 3-bit exponent in E1M0 blockwise logarithmic format)
- Block-quantized with shared `q_scales` per block
- `VulkanNdaGemv`: GPU GEMV kernel with `scales: [f32; 3]` for fused dispatch
- `concat_gpu_gemv()`: Concatenates Q‖K‖V and gate‖up into single fused GEMV objects — halves GPU dispatch calls

## Zero-Allocation Forward Pass

- `forward_one()` returns `&[f32]` from pre-allocated `TransformerScratch` (eliminates 2 heap allocations per token)
- `lm_head()` writes in-place to `&mut [f32]` via `par_iter_mut()` with unsafe pointer math
- `logits` buffer pre-allocated to `vocab_size` in scratch

## Dual-Path Engine

- **Path 1 (Text)**: Natural language → transformer → hidden_state[896]
- **Path 2 (NDA)**: Hidden state conditions NDA program generation with Merkle-verified output
- `DualPathEngine` lazy-loads Path 1 on first text request to save RAM
- Routing: imperative verbs → NDA mode; questions → text mode

## Tokenizer

BPE tokenizer (`tokenizer.rs`) for Qwen's 151,936-token vocabulary. Encode/decode for autoregressive generation.

## GPU Acceleration

- Vulkan compute shaders: FP4 GEMV, RMS norm, RoPE, SwiGLU, attention (contiguous + NDA-KV), KV-write, residual add
- `VulkanModelPipeline`: Orchestrates GPU execution for full transformer forward pass
- Fused GEMV weights reduce memory allocations and dispatch calls
- Deferred: GPU LM head (needs GEMV_FP32 shader), fused dispatch (needs updated push-constant shaders)
