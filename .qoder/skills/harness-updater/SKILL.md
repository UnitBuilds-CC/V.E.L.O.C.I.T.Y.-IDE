# Harness Updater

## Description
Maintain and upgrade the LLM inference harness in `velocity-ide/src/`. Covers the transformer model stack (config, weights, inference), GPU shader pipeline (Vulkan GEMV, activation shaders), dual-path engine, tokenizer, and NDA weight format. Use when porting improvements from V.E.L.O.C.I.T.Y.-OS, adding model support, optimizing inference, or converting new model weights to NDA format.

## When to Use
- Porting harness improvements from the OS codebase (`V.E.L.O.C.I.T.Y.-OS-main/`)
- Adding support for a new model architecture or quantization format
- Optimizing inference performance (zero-alloc, fused GEMV, GPU LM head)
- Converting model weights to NDA-4bit (FP4) or NDA-2bit (FP2) format
- Debugging transformer forward pass or GPU shader issues
- Updating NDA weight format versions or block sizes

## Key Files

| File | Role |
|------|------|
| `velocity-ide/src/model/config.rs` | Model architecture configs (Qwen 2.5 Coder 0.5B, BitNet 3B) |
| `velocity-ide/src/model/weights.rs` | NDA weight loading, `concat_gpu_gemv()`, fused projections |
| `velocity-ide/src/model/transformer.rs` | Zero-alloc forward pass, in-place LM head, generation |
| `velocity-ide/src/compiler/driver/nda_gemv.rs` | `VulkanNdaGemv` with `scales: [f32; 3]`, FP4/FP2 dispatch |
| `velocity-ide/src/compiler/driver/layer_gpu_gemvs.rs` | Per-layer fused GEMV orchestration |
| `velocity-ide/src/compiler/shaders/fp4.rs` | FP4 Vulkan compute shader |
| `velocity-ide/src/compiler/shaders/fp2.rs` | FP2 Vulkan compute shader |
| `velocity-ide/src/pipeline_bridge.rs` | Dual-path engine (text ↔ NDA routing) |
| `velocity-ide/src/tokenizer.rs` | BPE tokenizer (Qwen 2.5 Coder vocab) |
| `velocity-ide/src/nda.rs` | NDA format definitions (v1 ternary, v2 quad, v3 FP4, v4 FP2) |

## Upgrade Checklist

When porting from OS to IDE:

1. **Compare Rust-side code** — `diff` the OS and IDE versions of `transformer.rs`, `weights.rs`, `config.rs`
2. **Check shader compatibility** — OS may use different shader variants (SMALL/LARGE, GEMV_FP32) not present in IDE
3. **Verify push constant sizes** — OS may use 24-byte push constants (k, n, scale0, scale1, scale2, fused_flags) vs IDE's 8-byte
4. **Port Rust-only changes first** — Zero-alloc forward pass, fused GEMV weights, in-place LM head don't need shader changes
5. **Defer shader-dependent features** — GPU LM head, fused dispatch need matching shader bytecode
6. **Run `cargo check`** — Verify zero warnings
7. **Run `cargo test`** — Verify all tests pass (especially `fetch_panel_data` tests)
8. **Run `cargo clippy`** — Zero clippy warnings

## NDA Weight Format Reference

| Version | Bits/Weight | Use Case |
|---------|-------------|----------|
| v1 (ternary) | ~1.6 | Legacy models |
| v2 (quad) | 2 | NDA-KV cache |
| v3 (FP4) | 4 | Qwen 2.5 Coder 0.5B weights |
| v4 (FP2) | 2 | Ultra-compact models |

## Target Model: Qwen 2.5 Coder 0.5B

- 24 layers, hidden=896, ffn=4864, GQA (14 Q heads / 2 KV heads)
- ALiBi positional encoding (bit-shift, zero multiplication)
- Vocab: 151,936 tokens
- NDA-4bit quantization: ~250MB total weight size
- Fused GEMV: Q‖K‖V concatenated, gate‖up concatenated
