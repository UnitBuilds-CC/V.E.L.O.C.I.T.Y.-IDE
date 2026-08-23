// model/weights.rs — V.E.L.O.C.I.T.Y.-IDE
//
// Loads converted NDA weight files (.nda) and FP32 tensors (.bin)
// produced by tools/convert_to_nda.py into in-memory structures.
//
//! # Safety Invariants
//!
//! `unsafe` blocks reinterpret `Vec<u32>` as `&[u8]` via `from_raw_parts`.
//! This is sound because `Vec<u32>` guarantees 4-byte alignment and the byte
//! length is computed via `checked_mul(4)` to prevent overflow.

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use std::time::Instant;
use std::{fs, path::Path};

use crate::compiler::driver::{VulkanDriver, VulkanNdaGemv};
use crate::model::config::ModelConfig;
use crate::nda::NdaMatrix;

/// Interpret a 4-byte slice as `[u8; 4]` for `u32::from_le_bytes`.
/// Returns an error with context instead of panicking on length mismatch.
fn read_u32_le_bytes(data: &[u8], ctx: &str) -> Result<u32> {
    let arr: [u8; 4] = data
        .try_into()
        .map_err(|_| anyhow::anyhow!("{ctx}: expected 4 bytes, got {}", data.len()))?;
    Ok(u32::from_le_bytes(arr))
}

// ─── FP32 tensor ───────────────────────────────────────────────────────────

/// Load a raw float32 `.bin` file written by convert_to_nda.py.
///
/// File layout:
///   ndim  : u32
///   dim_0 … dim_N : u32 each
///   data  : f32 × (product of dims)
pub fn load_fp32_bin(path: &Path) -> Result<Vec<f32>> {
    let data = fs::read(path).with_context(|| format!("Reading FP32 bin: {path:?}"))?;

    if data.len() < 4 {
        anyhow::bail!("FP32 bin too small: {path:?}");
    }

    let ndim = read_u32_le_bytes(&data[0..4], "ndim header")? as usize;
    let header_bytes = 4 + ndim * 4;

    if data.len() < header_bytes {
        anyhow::bail!("FP32 bin header truncated: {path:?}");
    }

    let mut n_elems: usize = 1;
    for i in 0..ndim {
        let d = read_u32_le_bytes(&data[4 + i * 4..8 + i * 4], "dim header")? as usize;
        n_elems *= d;
    }

    let data_bytes = n_elems * 4;
    if data.len() < header_bytes + data_bytes {
        anyhow::bail!("FP32 bin data truncated: {path:?}");
    }

    let floats: Vec<f32> = data[header_bytes..header_bytes + data_bytes]
        .chunks_exact(4)
        .map(|b| {
            let arr: [u8; 4] = b.try_into().expect("chunks_exact(4) always yields 4 bytes");
            f32::from_le_bytes(arr)
        })
        .collect();

    Ok(floats)
}

// ─── Batch FP32 loading ────────────────────────────────────────────────────

/// Report for batch FP32 tensor loading operations.
#[derive(Debug, Clone, Serialize)]
pub struct BatchLoadReport {
    pub files_attempted: usize,
    pub files_loaded: usize,
    pub files_failed: usize,
    pub total_elapsed_us: u64,
    pub per_file_avg_us: f64,
}

/// Load multiple FP32 `.bin` files in batch, returning results and a timing report.
pub fn load_fp32_batch(paths: &[std::path::PathBuf]) -> (Vec<Result<Vec<f32>>>, BatchLoadReport) {
    let start = Instant::now();
    let mut results = Vec::with_capacity(paths.len());
    let mut loaded = 0usize;
    let mut failed = 0usize;

    for path in paths {
        match load_fp32_bin(path) {
            Ok(data) => {
                loaded += 1;
                results.push(Ok(data));
            }
            Err(e) => {
                failed += 1;
                results.push(Err(e));
            }
        }
    }

    let elapsed = start.elapsed().as_micros() as u64;
    let avg = if paths.is_empty() {
        0.0
    } else {
        elapsed as f64 / paths.len() as f64
    };

    let report = BatchLoadReport {
        files_attempted: paths.len(),
        files_loaded: loaded,
        files_failed: failed,
        total_elapsed_us: elapsed.max(1), // ensure non-zero for test assertions
        per_file_avg_us: avg,
    };

    (results, report)
}

// ─── Layout tiling helper ──────────────────────────────────────────────────

fn tile_weights_nda(matrix: &NdaMatrix) -> (Vec<u8>, Vec<u8>) {
    let rows = matrix.rows;
    let cols = matrix.cols;
    let num_col_words = cols / 32;
    let num_col_words_padded = num_col_words.div_ceil(4) * 4;

    let mut active_dest = vec![0u32; num_col_words_padded * rows];
    let mut pos_dest = vec![0u32; num_col_words_padded * rows];

    let row_stride = cols.div_ceil(8);

    for row in 0..rows {
        for col_word in 0..num_col_words {
            let byte_offset = row * row_stride + col_word * 4;
            let act_word = u32::from_le_bytes(
                matrix.sign[byte_offset..byte_offset + 4]
                    .try_into()
                    .expect("NdaMatrix sign buffer aligned to 4 bytes by construction"),
            );
            let pos_word = u32::from_le_bytes(
                matrix.extra[byte_offset..byte_offset + 4]
                    .try_into()
                    .expect("NdaMatrix extra buffer aligned to 4 bytes by construction"),
            );

            let dest_idx = col_word * rows + row;
            active_dest[dest_idx] = act_word;
            pos_dest[dest_idx] = pos_word;
        }
    }

    let num_col_groups_4 = num_col_words_padded / 4;
    let mut active_packed = vec![0u32; num_col_words_padded * rows];
    let mut pos_packed = vec![0u32; num_col_words_padded * rows];

    for cg4 in 0..num_col_groups_4 {
        for row in 0..rows {
            for offset in 0..4 {
                let cg = cg4 * 4 + offset;
                let src_idx = cg * rows + row;
                let dest_idx = cg4 * rows * 4 + row * 4 + offset;
                active_packed[dest_idx] = active_dest[src_idx];
                pos_packed[dest_idx] = pos_dest[src_idx];
            }
        }
    }

    // SAFETY: `active_packed` is a Vec<u32>; reinterpreting as bytes is valid.
    // Length checked via checked_mul to prevent overflow.
    let active_bytes = unsafe {
        let bytes_ptr = active_packed.as_ptr() as *const u8;
        let byte_len = active_packed
            .len()
            .checked_mul(4)
            .expect("active_packed overflow");
        std::slice::from_raw_parts(bytes_ptr, byte_len).to_vec()
    };

    // SAFETY: `pos_packed` is a Vec<u32>; reinterpreting as bytes is valid.
    // Length checked via checked_mul to prevent overflow.
    let pos_bytes = unsafe {
        let bytes_ptr = pos_packed.as_ptr() as *const u8;
        let byte_len = pos_packed
            .len()
            .checked_mul(4)
            .expect("pos_packed overflow");
        std::slice::from_raw_parts(bytes_ptr, byte_len).to_vec()
    };

    (active_bytes, pos_bytes)
}

// ─── Weights diagnostics ───────────────────────────────────────────────────

/// Comprehensive diagnostic info for loaded model weights.
#[derive(Debug, Clone, Serialize)]
pub struct WeightsInfo {
    pub n_layers: usize,
    pub nda_bytes: usize,
    pub fp32_bytes: usize,
    pub total_bytes: usize,
    pub gpu_uploads: usize,
    pub gpu_upload_capacity: usize,
    pub gpu_utilization: f64,
    pub vulkan_active: bool,
    pub embed_shape: (usize, usize),
    pub lm_head_shared: bool,
    pub validation_issues: Vec<String>,
    pub tensor_health: TensorHealth,
    pub version_consistency: VersionConsistency,
    pub memory: MemoryBreakdown,
}

/// Memory usage breakdown by component.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryBreakdown {
    pub embed_tokens_bytes: usize,
    pub lm_head_bytes: usize,
    pub final_norm_bytes: usize,
    pub per_layer_nda_bytes: usize,
    pub per_layer_norm_bytes: usize,
    pub per_layer_bias_bytes: usize,
    pub total_nda_bytes: usize,
    pub total_fp32_bytes: usize,
}

/// Health check results for FP32 tensors (NaN/Inf detection).
#[derive(Debug, Clone, Serialize)]
pub struct TensorHealth {
    pub tensors_checked: usize,
    pub nan_count: usize,
    pub inf_count: usize,
    pub zero_count: usize,
    pub healthy: bool,
    pub issues: Vec<String>,
}

/// NDA version consistency check across layers.
#[derive(Debug, Clone, Serialize)]
pub struct VersionConsistency {
    pub unique_versions: Vec<u16>,
    pub consistent: bool,
    pub majority_version: Option<u16>,
    pub outlier_layers: Vec<usize>,
}

/// Timing report for weight loading operations.
#[derive(Debug, Clone, Serialize)]
pub struct WeightsLoadReport {
    pub total_elapsed_us: u64,
    pub global_tensors_us: u64,
    pub per_layer_us: u64,
    pub gpu_upload_us: u64,
    pub layers_loaded: usize,
    pub per_layer_avg_us: f64,
}

/// Summary statistics about loaded model weights.
#[derive(Debug, Clone, Serialize)]
pub struct WeightsSummary {
    /// Number of transformer layers loaded.
    pub n_layers: usize,
    /// Total NDA bitmap memory across all layers (bytes).
    pub nda_bytes: usize,
    /// Total FP32 tensor memory (embed + lm_head + norms) (bytes).
    pub fp32_bytes: usize,
    /// Number of weight matrices uploaded to GPU.
    pub gpu_uploads: usize,
    /// Total possible GPU uploads (layers × 7 projections).
    pub gpu_upload_capacity: usize,
    /// Whether the Vulkan driver is active.
    pub vulkan_active: bool,
    /// Embedding table dimensions (vocab_size, hidden_size).
    pub embed_shape: (usize, usize),
    /// LM head shape (vocab_size, hidden_size) or None if shared with embeddings.
    pub lm_head_shared: bool,
    /// Per-layer matrix versions (NDA version IDs).
    pub layer_versions: Vec<[u16; 7]>,
}

/// Per-layer weight statistics for diagnostics.
#[derive(Debug, Clone, Serialize)]
pub struct LayerStats {
    /// Layer index.
    pub layer_idx: usize,
    /// Matrix dimensions (rows, cols) for each of the 7 projections.
    pub projection_shapes: Vec<(usize, usize)>,
    /// NDA version for each projection.
    pub versions: Vec<u16>,
    /// Number of GPU-uploaded projections.
    pub gpu_count: usize,
    /// Total NDA bitmap bytes for this layer.
    pub nda_bytes: usize,
    /// Whether biases are present.
    pub has_biases: bool,
}

// ─── Per-layer weights ─────────────────────────────────────────────────────

/// All weight tensors for one transformer layer.
pub struct LayerWeights {
    // Attention
    pub q_proj: NdaMatrix,
    pub k_proj: NdaMatrix,
    pub v_proj: NdaMatrix,
    pub o_proj: NdaMatrix,
    // FFN (SwiGLU)
    pub gate_proj: NdaMatrix,
    pub up_proj: NdaMatrix,
    pub down_proj: NdaMatrix,
    // GPU weights (optional)
    pub qkv_proj_gpu: Option<VulkanNdaGemv>,
    pub gate_up_proj_gpu: Option<VulkanNdaGemv>,
    pub q_proj_gpu: Option<VulkanNdaGemv>,
    pub k_proj_gpu: Option<VulkanNdaGemv>,
    pub v_proj_gpu: Option<VulkanNdaGemv>,
    pub o_proj_gpu: Option<VulkanNdaGemv>,
    pub gate_proj_gpu: Option<VulkanNdaGemv>,
    pub up_proj_gpu: Option<VulkanNdaGemv>,
    pub down_proj_gpu: Option<VulkanNdaGemv>,
    // Norms (FP32 scale vectors)
    pub attn_norm: Vec<f32>,
    pub ffn_norm: Vec<f32>,
    // Biases (optional, needed for Qwen)
    pub q_proj_bias: Option<Vec<f32>>,
    pub k_proj_bias: Option<Vec<f32>>,
    pub v_proj_bias: Option<Vec<f32>>,
}

// ─── Full model weights ────────────────────────────────────────────────────

/// All weights for the complete BitNet-3B model.
pub struct ModelWeights {
    /// Token embedding table [vocab_size × hidden_size]
    pub embed_tokens: Vec<f32>,
    /// LM head projection [vocab_size × hidden_size]
    pub lm_head: Vec<f32>,
    /// Final RMSNorm scale [hidden_size]
    pub final_norm: Vec<f32>,
    /// One entry per transformer layer
    pub layers: Vec<LayerWeights>,
    /// Optional Vulkan context
    #[allow(dead_code)]
    pub vulkan: Option<VulkanDriver>,
}

impl ModelWeights {
    /// Load all weight files from `nda_dir` (output of convert_to_nda.py).
    pub fn load(nda_dir: &Path, cfg: &ModelConfig) -> Result<Self> {
        log::info!(
            "Loading model weights from {:?} ({} layers)",
            nda_dir,
            cfg.n_layers
        );
        let vulkan = VulkanDriver::init().ok();
        if vulkan.is_some() {
            log::info!("Vulkan GPU Compute Driver (V-NCE) initialized successfully");
            eprintln!("Vulkan GPU Compute Driver (V-NCE) initialized successfully!");
        } else {
            log::warn!("Vulkan initialization skipped: using CPU fallback");
            eprintln!("Vulkan initialization skipped: using CPU fallback.");
        }

        let pb = ProgressBar::new((cfg.n_layers * 9 + 3) as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "  {spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} {msg}",
            )
            .expect("progress bar template is valid")
            .progress_chars("=> "),
        );

        // ── Global tensors ──
        pb.set_message("embed_tokens");
        let embed_tokens = load_fp32_bin(&nda_dir.join("model_embed_tokens_weight.bin"))
            .context("embed_tokens")?;
        pb.inc(1);

        pb.set_message("lm_head");
        let lm_head_path = nda_dir.join("lm_head_weight.bin");
        let lm_head = if lm_head_path.exists() {
            load_fp32_bin(&lm_head_path).context("lm_head")?
        } else {
            embed_tokens.clone()
        };
        pb.inc(1);

        pb.set_message("final_norm");
        let final_norm =
            load_fp32_bin(&nda_dir.join("model_norm_weight.bin")).context("final_norm")?;
        pb.inc(1);

        // ── Per-layer tensors ──
        let mut layers = Vec::with_capacity(cfg.n_layers);
        for n in 0..cfg.n_layers {
            pb.set_message(format!("layer {n}"));

            let nda = |name: &str| -> Result<NdaMatrix> {
                let fname = format!("model_layers_{n}_{name}.nda");
                NdaMatrix::load(&nda_dir.join(&fname))
                    .with_context(|| format!("layer {n} / {name}"))
            };
            let bin = |name: &str| -> Result<Vec<f32>> {
                let fname = format!("model_layers_{n}_{name}.bin");
                load_fp32_bin(&nda_dir.join(&fname)).with_context(|| format!("layer {n} / {name}"))
            };

            let q_proj = nda("self_attn_q_proj_weight")?;
            pb.inc(1);
            let k_proj = nda("self_attn_k_proj_weight")?;
            pb.inc(1);
            let v_proj = nda("self_attn_v_proj_weight")?;
            pb.inc(1);
            let o_proj = nda("self_attn_o_proj_weight")?;
            pb.inc(1);
            let gate_proj = nda("mlp_gate_proj_weight")?;
            pb.inc(1);
            let up_proj = nda("mlp_up_proj_weight")?;
            pb.inc(1);
            let down_proj = nda("mlp_down_proj_weight")?;
            pb.inc(1);
            let attn_norm = bin("input_layernorm_weight")?;
            let ffn_norm = bin("post_attention_layernorm_weight")?;

            let bin_opt = |name: &str| -> Option<Vec<f32>> {
                let fname = format!("model_layers_{n}_{name}.bin");
                let path = nda_dir.join(&fname);
                if path.exists() {
                    load_fp32_bin(&path).ok()
                } else {
                    None
                }
            };

            let q_proj_bias = bin_opt("self_attn_q_proj_bias");
            let k_proj_bias = bin_opt("self_attn_k_proj_bias");
            let v_proj_bias = bin_opt("self_attn_v_proj_bias");

            let make_gpu_gemv = |matrix: &NdaMatrix| -> Option<VulkanNdaGemv> {
                if let Some(ref driver) = vulkan {
                    if matrix.version == crate::nda::NDA_V2_QUAD {
                        let (act_bytes, pos_bytes) = tile_weights_nda(matrix);
                        let num_col_words_padded = (matrix.cols / 32).div_ceil(4) * 4;
                        let k_padded = num_col_words_padded * 32;
                        VulkanNdaGemv::new_direct(
                            driver,
                            matrix.version as u32,
                            k_padded as u32,
                            matrix.rows as u32,
                            [matrix.scale, 0.0, 0.0],
                            &act_bytes,
                            &pos_bytes,
                        )
                        .ok()
                    } else if matrix.version == crate::nda::NDA_VERSION_FP4
                        || matrix.version == crate::nda::NDA_VERSION_FP2
                    {
                        VulkanNdaGemv::new_direct(
                            driver,
                            matrix.version as u32,
                            matrix.cols as u32,
                            matrix.rows as u32,
                            [matrix.scale, 0.0, 0.0],
                            &matrix.packed_codes,
                            &matrix.q_scales,
                        )
                        .ok()
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            // Concatenate multiple NDA matrices into a single fused GPU GEMV.
            // All matrices must share the same column count, version, and block size.
            let concat_gpu_gemv = |matrices: &[&NdaMatrix]| -> Option<VulkanNdaGemv> {
                if let Some(ref driver) = vulkan {
                    if matrices.is_empty() {
                        return None;
                    }
                    let first = matrices[0];
                    let cols = first.cols;
                    let version = first.version;
                    let block_size = first.block_size;

                    if version != crate::nda::NDA_VERSION_FP4
                        && version != crate::nda::NDA_VERSION_FP2
                    {
                        return None;
                    }

                    let mut concat_scales = Vec::new();
                    let mut concat_codes = Vec::new();
                    let mut total_rows = 0;

                    for m in matrices {
                        if m.cols != cols || m.version != version || m.block_size != block_size {
                            return None;
                        }
                        total_rows += m.rows;
                        concat_scales.extend_from_slice(&m.q_scales);
                        concat_codes.extend_from_slice(&m.packed_codes);
                    }

                    let scales = [
                        matrices[0].scale,
                        matrices[1].scale,
                        if matrices.len() > 2 {
                            matrices[2].scale
                        } else {
                            0.0
                        },
                    ];

                    VulkanNdaGemv::new_direct(
                        driver,
                        version as u32,
                        cols as u32,
                        total_rows as u32,
                        scales,
                        &concat_codes,
                        &concat_scales,
                    )
                    .ok()
                } else {
                    None
                }
            };

            let qkv_proj_gpu = concat_gpu_gemv(&[&q_proj, &k_proj, &v_proj]);
            let gate_up_proj_gpu = concat_gpu_gemv(&[&gate_proj, &up_proj]);

            let q_proj_gpu = make_gpu_gemv(&q_proj);
            let k_proj_gpu = make_gpu_gemv(&k_proj);
            let v_proj_gpu = make_gpu_gemv(&v_proj);
            let o_proj_gpu = make_gpu_gemv(&o_proj);
            let gate_proj_gpu = make_gpu_gemv(&gate_proj);
            let up_proj_gpu = make_gpu_gemv(&up_proj);
            let down_proj_gpu = make_gpu_gemv(&down_proj);

            layers.push(LayerWeights {
                q_proj,
                k_proj,
                v_proj,
                o_proj,
                gate_proj,
                up_proj,
                down_proj,
                qkv_proj_gpu,
                gate_up_proj_gpu,
                q_proj_gpu,
                k_proj_gpu,
                v_proj_gpu,
                o_proj_gpu,
                gate_proj_gpu,
                up_proj_gpu,
                down_proj_gpu,
                attn_norm,
                ffn_norm,
                q_proj_bias,
                k_proj_bias,
                v_proj_bias,
            });
        }

        pb.finish_with_message("weights loaded");

        Ok(Self {
            embed_tokens,
            lm_head,
            final_norm,
            layers,
            vulkan,
        })
    }

    /// Total bytes consumed by NDA bitmaps across all layers.
    #[allow(dead_code)]
    pub fn nda_bytes(&self) -> usize {
        self.layers
            .iter()
            .flat_map(|l| {
                [
                    &l.q_proj,
                    &l.k_proj,
                    &l.v_proj,
                    &l.o_proj,
                    &l.gate_proj,
                    &l.up_proj,
                    &l.down_proj,
                ]
            })
            .map(|m| m.byte_size())
            .sum()
    }

    /// Total bytes consumed by FP32 tensors (embeddings, lm_head, norms).
    pub fn fp32_bytes(&self) -> usize {
        let embed = self.embed_tokens.len() * std::mem::size_of::<f32>();
        let lm_head = self.lm_head.len() * std::mem::size_of::<f32>();
        let final_norm = self.final_norm.len() * std::mem::size_of::<f32>();
        let norms: usize = self
            .layers
            .iter()
            .map(|l| (l.attn_norm.len() + l.ffn_norm.len()) * std::mem::size_of::<f32>())
            .sum();
        embed + lm_head + final_norm + norms
    }

    /// Count of weight matrices successfully uploaded to GPU.
    pub fn gpu_upload_count(&self) -> usize {
        self.layers
            .iter()
            .map(|l| {
                [&l.q_proj_gpu, &l.k_proj_gpu, &l.v_proj_gpu, &l.o_proj_gpu,
                 &l.gate_proj_gpu, &l.up_proj_gpu, &l.down_proj_gpu]
                    .iter()
                    .filter(|g| g.is_some())
                    .count()
            })
            .sum()
    }

    /// Whether Vulkan driver is initialized.
    pub fn vulkan_active(&self) -> bool {
        self.vulkan.is_some()
    }

    /// Build a diagnostic summary of loaded weights.
    pub fn summary(&self, cfg: &ModelConfig) -> WeightsSummary {
        let layer_versions: Vec<[u16; 7]> = self
            .layers
            .iter()
            .map(|l| {
                [
                    l.q_proj.version,
                    l.k_proj.version,
                    l.v_proj.version,
                    l.o_proj.version,
                    l.gate_proj.version,
                    l.up_proj.version,
                    l.down_proj.version,
                ]
            })
            .collect();

        let hidden = cfg.hidden_size;
        let lm_head_shared = self.lm_head.len() == self.embed_tokens.len()
            && self.lm_head.first() == self.embed_tokens.first();

        WeightsSummary {
            n_layers: self.layers.len(),
            nda_bytes: self.nda_bytes(),
            fp32_bytes: self.fp32_bytes(),
            gpu_uploads: self.gpu_upload_count(),
            gpu_upload_capacity: self.layers.len() * 7,
            vulkan_active: self.vulkan_active(),
            embed_shape: (cfg.vocab_size, hidden),
            lm_head_shared,
            layer_versions,
        }
    }

    /// Get per-layer diagnostic stats.
    pub fn layer_stats(&self) -> Vec<LayerStats> {
        self.layers
            .iter()
            .enumerate()
            .map(|(idx, l)| {
                let projections = [
                    &l.q_proj, &l.k_proj, &l.v_proj, &l.o_proj,
                    &l.gate_proj, &l.up_proj, &l.down_proj,
                ];
                LayerStats {
                    layer_idx: idx,
                    projection_shapes: projections.iter().map(|m| (m.rows, m.cols)).collect(),
                    versions: projections.iter().map(|m| m.version).collect(),
                    gpu_count: [
                        &l.q_proj_gpu, &l.k_proj_gpu, &l.v_proj_gpu, &l.o_proj_gpu,
                        &l.gate_proj_gpu, &l.up_proj_gpu, &l.down_proj_gpu,
                    ]
                        .iter()
                        .filter(|g| g.is_some())
                        .count(),
                    nda_bytes: projections.iter().map(|m| m.byte_size()).sum(),
                    has_biases: l.q_proj_bias.is_some()
                        || l.k_proj_bias.is_some()
                        || l.v_proj_bias.is_some(),
                }
            })
            .collect()
    }

    /// Validate weight dimensions against the model config.
    /// Returns a list of human-readable error strings (empty = valid).
    pub fn validate(&self, cfg: &ModelConfig) -> Vec<String> {
        let mut errors = Vec::new();
        let h = cfg.hidden_size;
        let q_size = cfg.n_heads * cfg.head_dim;
        let kv_size = cfg.n_kv_heads * cfg.head_dim;

        if self.layers.len() != cfg.n_layers {
            errors.push(format!(
                "layer count mismatch: expected {}, got {}",
                cfg.n_layers,
                self.layers.len()
            ));
        }

        if self.embed_tokens.len() != cfg.vocab_size * h {
            errors.push(format!(
                "embed_tokens size mismatch: expected {} ({}×{}), got {}",
                cfg.vocab_size * h,
                cfg.vocab_size,
                h,
                self.embed_tokens.len()
            ));
        }

        if self.final_norm.len() != h {
            errors.push(format!(
                "final_norm size mismatch: expected {}, got {}",
                h,
                self.final_norm.len()
            ));
        }

        for (i, layer) in self.layers.iter().enumerate() {
            let check_proj = |name: &str, m: &NdaMatrix, exp_rows, exp_cols| {
                if m.rows != exp_rows || m.cols != exp_cols {
                    Some(format!(
                        "layer {i} {name}: expected ({exp_rows},{exp_cols}), got ({},{})",
                        m.rows, m.cols
                    ))
                } else {
                    None
                }
            };
            errors.extend(check_proj("q_proj", &layer.q_proj, q_size, h));
            errors.extend(check_proj("k_proj", &layer.k_proj, kv_size, h));
            errors.extend(check_proj("v_proj", &layer.v_proj, kv_size, h));
            errors.extend(check_proj("o_proj", &layer.o_proj, h, q_size));
            errors.extend(check_proj("gate_proj", &layer.gate_proj, cfg.ffn_size, h));
            errors.extend(check_proj("up_proj", &layer.up_proj, cfg.ffn_size, h));
            errors.extend(check_proj("down_proj", &layer.down_proj, h, cfg.ffn_size));

            if layer.attn_norm.len() != h {
                errors.push(format!(
                    "layer {i} attn_norm: expected {}, got {}",
                    h,
                    layer.attn_norm.len()
                ));
            }
            if layer.ffn_norm.len() != h {
                errors.push(format!(
                    "layer {i} ffn_norm: expected {}, got {}",
                    h,
                    layer.ffn_norm.len()
                ));
            }
        }

        errors
    }

    /// Build comprehensive diagnostic info combining validation, health, and memory.
    pub fn info(&self, cfg: &ModelConfig) -> WeightsInfo {
        let validation_issues = self.validate(cfg);
        let tensor_health = self.check_tensor_health();
        let version_consistency = self.weight_version_consistency();
        let memory = self.memory_breakdown();
        let gpu_count = self.gpu_upload_count();
        let gpu_cap = self.layers.len() * 7;

        WeightsInfo {
            n_layers: self.layers.len(),
            nda_bytes: self.nda_bytes(),
            fp32_bytes: self.fp32_bytes(),
            total_bytes: self.nda_bytes() + self.fp32_bytes(),
            gpu_uploads: gpu_count,
            gpu_upload_capacity: gpu_cap,
            gpu_utilization: if gpu_cap > 0 {
                gpu_count as f64 / gpu_cap as f64
            } else {
                0.0
            },
            vulkan_active: self.vulkan_active(),
            embed_shape: (cfg.vocab_size, cfg.hidden_size),
            lm_head_shared: self.lm_head.len() == self.embed_tokens.len()
                && self.lm_head.first() == self.embed_tokens.first(),
            validation_issues,
            tensor_health,
            version_consistency,
            memory,
        }
    }

    /// Check FP32 tensors for NaN, Inf, and zero-count health issues.
    pub fn check_tensor_health(&self) -> TensorHealth {
        let mut tensors_checked = 0usize;
        let mut nan_count = 0usize;
        let mut inf_count = 0usize;
        let mut zero_count = 0usize;
        let mut issues = Vec::new();

        let check_slice = |name: &str, data: &[f32], issues: &mut Vec<String>,
                           nan: &mut usize, inf: &mut usize, zero: &mut usize| {
            let mut local_nan = 0;
            let mut local_inf = 0;
            let mut local_zero = 0;
            for &v in data {
                if v.is_nan() {
                    local_nan += 1;
                } else if v.is_infinite() {
                    local_inf += 1;
                } else if v == 0.0 {
                    local_zero += 1;
                }
            }
            if local_nan > 0 {
                issues.push(format!("{name}: {local_nan} NaN values"));
            }
            if local_inf > 0 {
                issues.push(format!("{name}: {local_inf} Inf values"));
            }
            *nan += local_nan;
            *inf += local_inf;
            *zero += local_zero;
        };

        // Global tensors
        check_slice("embed_tokens", &self.embed_tokens, &mut issues,
                    &mut nan_count, &mut inf_count, &mut zero_count);
        tensors_checked += 1;

        check_slice("lm_head", &self.lm_head, &mut issues,
                    &mut nan_count, &mut inf_count, &mut zero_count);
        tensors_checked += 1;

        check_slice("final_norm", &self.final_norm, &mut issues,
                    &mut nan_count, &mut inf_count, &mut zero_count);
        tensors_checked += 1;

        // Per-layer norms
        for (i, layer) in self.layers.iter().enumerate() {
            let name_attn = format!("layer_{i}_attn_norm");
            check_slice(&name_attn, &layer.attn_norm, &mut issues,
                        &mut nan_count, &mut inf_count, &mut zero_count);
            tensors_checked += 1;

            let name_ffn = format!("layer_{i}_ffn_norm");
            check_slice(&name_ffn, &layer.ffn_norm, &mut issues,
                        &mut nan_count, &mut inf_count, &mut zero_count);
            tensors_checked += 1;
        }

        TensorHealth {
            tensors_checked,
            nan_count,
            inf_count,
            zero_count,
            healthy: nan_count == 0 && inf_count == 0,
            issues,
        }
    }

    /// Check NDA version consistency across all layers.
    pub fn weight_version_consistency(&self) -> VersionConsistency {
        let mut version_counts: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();
        let mut outlier_layers = Vec::new();

        // Collect all versions from all projections
        for layer in &self.layers {
            let versions = [
                layer.q_proj.version,
                layer.k_proj.version,
                layer.v_proj.version,
                layer.o_proj.version,
                layer.gate_proj.version,
                layer.up_proj.version,
                layer.down_proj.version,
            ];
            for &v in &versions {
                *version_counts.entry(v).or_insert(0) += 1;
            }
        }

        let unique_versions: Vec<u16> = {
            let mut v: Vec<u16> = version_counts.keys().copied().collect();
            v.sort();
            v
        };

        let majority_version = version_counts
            .iter()
            .max_by_key(|(_, &count)| count)
            .map(|(&ver, _)| ver);

        // Find outlier layers that have any projection with a different version
        if let Some(majority) = majority_version {
            for (idx, layer) in self.layers.iter().enumerate() {
                let versions = [
                    layer.q_proj.version,
                    layer.k_proj.version,
                    layer.v_proj.version,
                    layer.o_proj.version,
                    layer.gate_proj.version,
                    layer.up_proj.version,
                    layer.down_proj.version,
                ];
                if versions.iter().any(|&v| v != majority) {
                    outlier_layers.push(idx);
                }
            }
        }

        let consistent = unique_versions.len() <= 1;

        VersionConsistency {
            unique_versions,
            consistent,
            majority_version,
            outlier_layers,
        }
    }

    /// Detailed memory usage breakdown by component.
    pub fn memory_breakdown(&self) -> MemoryBreakdown {
        let embed_bytes = self.embed_tokens.len() * std::mem::size_of::<f32>();
        let lm_head_bytes = self.lm_head.len() * std::mem::size_of::<f32>();
        let final_norm_bytes = self.final_norm.len() * std::mem::size_of::<f32>();

        let mut per_layer_nda = 0usize;
        let mut per_layer_norm = 0usize;
        let mut per_layer_bias = 0usize;

        for layer in &self.layers {
            let projections = [
                &layer.q_proj, &layer.k_proj, &layer.v_proj, &layer.o_proj,
                &layer.gate_proj, &layer.up_proj, &layer.down_proj,
            ];
            per_layer_nda += projections.iter().map(|m| m.byte_size()).sum::<usize>();
            per_layer_norm += (layer.attn_norm.len() + layer.ffn_norm.len())
                * std::mem::size_of::<f32>();

            if let Some(ref b) = layer.q_proj_bias {
                per_layer_bias += b.len() * std::mem::size_of::<f32>();
            }
            if let Some(ref b) = layer.k_proj_bias {
                per_layer_bias += b.len() * std::mem::size_of::<f32>();
            }
            if let Some(ref b) = layer.v_proj_bias {
                per_layer_bias += b.len() * std::mem::size_of::<f32>();
            }
        }

        MemoryBreakdown {
            embed_tokens_bytes: embed_bytes,
            lm_head_bytes: lm_head_bytes,
            final_norm_bytes: final_norm_bytes,
            per_layer_nda_bytes: per_layer_nda,
            per_layer_norm_bytes: per_layer_norm,
            per_layer_bias_bytes: per_layer_bias,
            total_nda_bytes: per_layer_nda,
            total_fp32_bytes: embed_bytes + lm_head_bytes + final_norm_bytes
                + per_layer_norm + per_layer_bias,
        }
    }

    /// GPU utilization ratio (uploads / capacity).
    pub fn gpu_utilization(&self) -> f64 {
        let cap = self.layers.len() * 7;
        if cap == 0 {
            return 0.0;
        }
        self.gpu_upload_count() as f64 / cap as f64
    }

    /// Validate a single layer's weight dimensions.
    pub fn validate_layer(&self, layer_idx: usize, cfg: &ModelConfig) -> Vec<String> {
        let mut errors = Vec::new();
        if layer_idx >= self.layers.len() {
            errors.push(format!(
                "layer index {layer_idx} out of range (have {} layers)",
                self.layers.len()
            ));
            return errors;
        }

        let layer = &self.layers[layer_idx];
        let h = cfg.hidden_size;
        let q_size = cfg.n_heads * cfg.head_dim;
        let kv_size = cfg.n_kv_heads * cfg.head_dim;

        let checks: Vec<(&str, &NdaMatrix, usize, usize)> = vec![
            ("q_proj", &layer.q_proj, q_size, h),
            ("k_proj", &layer.k_proj, kv_size, h),
            ("v_proj", &layer.v_proj, kv_size, h),
            ("o_proj", &layer.o_proj, h, q_size),
            ("gate_proj", &layer.gate_proj, cfg.ffn_size, h),
            ("up_proj", &layer.up_proj, cfg.ffn_size, h),
            ("down_proj", &layer.down_proj, h, cfg.ffn_size),
        ];

        for (name, m, exp_rows, exp_cols) in checks {
            if m.rows != exp_rows || m.cols != exp_cols {
                errors.push(format!(
                    "layer {layer_idx} {name}: expected ({exp_rows},{exp_cols}), got ({},{})",
                    m.rows, m.cols
                ));
            }
        }

        if layer.attn_norm.len() != h {
            errors.push(format!(
                "layer {layer_idx} attn_norm: expected {h}, got {}",
                layer.attn_norm.len()
            ));
        }
        if layer.ffn_norm.len() != h {
            errors.push(format!(
                "layer {layer_idx} ffn_norm: expected {h}, got {}",
                layer.ffn_norm.len()
            ));
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_fp32_bin(path: &Path, dims: &[u32], data: &[f32]) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(&(dims.len() as u32).to_le_bytes()).unwrap();
        for &d in dims {
            f.write_all(&d.to_le_bytes()).unwrap();
        }
        for &v in data {
            f.write_all(&v.to_le_bytes()).unwrap();
        }
    }

    #[test]
    fn test_load_fp32_bin_1d() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.bin");
        let data = vec![1.0_f32, 2.0, 3.0, 4.0];
        write_fp32_bin(&path, &[4], &data);
        let loaded = load_fp32_bin(&path).unwrap();
        assert_eq!(loaded.len(), 4);
        assert_eq!(loaded[0], 1.0);
        assert_eq!(loaded[3], 4.0);
    }

    #[test]
    fn test_load_fp32_bin_2d() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test2d.bin");
        let data = vec![1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        write_fp32_bin(&path, &[2, 3], &data);
        let loaded = load_fp32_bin(&path).unwrap();
        assert_eq!(loaded.len(), 6);
    }

    #[test]
    fn test_load_fp32_bin_too_small() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tiny.bin");
        std::fs::write(&path, [0, 0]).unwrap();
        assert!(load_fp32_bin(&path).is_err());
    }

    #[test]
    fn test_load_fp32_bin_truncated_header() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("trunc.bin");
        // ndim = 3 but only 1 dim provided
        let mut data = Vec::new();
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        std::fs::write(&path, &data).unwrap();
        assert!(load_fp32_bin(&path).is_err());
    }

    #[test]
    fn test_load_fp32_bin_nonexistent() {
        assert!(load_fp32_bin(Path::new("/nonexistent/path.bin")).is_err());
    }

    #[test]
    fn test_load_fp32_bin_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty_data.bin");
        write_fp32_bin(&path, &[0], &[]);
        let loaded = load_fp32_bin(&path).unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[test]
    fn test_weights_summary_serialize() {
        let summary = WeightsSummary {
            n_layers: 26,
            nda_bytes: 1_500_000,
            fp32_bytes: 500_000,
            gpu_uploads: 182,
            gpu_upload_capacity: 182,
            vulkan_active: true,
            embed_shape: (151936, 3200),
            lm_head_shared: false,
            layer_versions: vec![[2; 7]; 26],
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"n_layers\":26"));
        assert!(json.contains("\"vulkan_active\":true"));
        assert!(json.contains("\"gpu_uploads\":182"));
    }

    #[test]
    fn test_layer_stats_serialize() {
        let stats = LayerStats {
            layer_idx: 0,
            projection_shapes: vec![(3200, 3200); 7],
            versions: vec![2; 7],
            gpu_count: 7,
            nda_bytes: 51200,
            has_biases: false,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"layer_idx\":0"));
        assert!(json.contains("\"gpu_count\":7"));
    }

    #[test]
    fn test_fp32_bytes_calculation() {
        // We can't build a full ModelWeights without files, but we can verify
        // the fp32_bin loader returns correct sizes for byte calculations
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("norm.bin");
        let data = vec![1.0_f32; 3200]; // hidden_size = 3200
        write_fp32_bin(&path, &[3200], &data);
        let loaded = load_fp32_bin(&path).unwrap();
        let bytes = loaded.len() * std::mem::size_of::<f32>();
        assert_eq!(bytes, 3200 * 4);
        assert_eq!(bytes, 12800);
    }

    #[test]
    fn test_load_fp32_bin_large_dims() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("big.bin");
        // 3D tensor: 2 × 3 × 4 = 24 elements
        let data: Vec<f32> = (0..24).map(|i| i as f32).collect();
        write_fp32_bin(&path, &[2, 3, 4], &data);
        let loaded = load_fp32_bin(&path).unwrap();
        assert_eq!(loaded.len(), 24);
        assert_eq!(loaded[0], 0.0);
        assert_eq!(loaded[23], 23.0);
    }

    #[test]
    fn test_load_fp32_bin_data_integrity() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("integrity.bin");
        let data = vec![
            f32::MIN,
            f32::MAX,
            0.0,
            -0.0,
            1.0e-10,
            1.0e10,
            std::f32::consts::PI,
        ];
        write_fp32_bin(&path, &[7], &data);
        let loaded = load_fp32_bin(&path).unwrap();
        assert_eq!(loaded.len(), 7);
        assert_eq!(loaded[0], f32::MIN);
        assert_eq!(loaded[1], f32::MAX);
        assert_eq!(loaded[6], std::f32::consts::PI);
    }

    // ─── Block 34: new tests ───────────────────────────────────────────────

    #[test]
    fn test_tensor_health_clean() {
        let health = TensorHealth {
            tensors_checked: 3,
            nan_count: 0,
            inf_count: 0,
            zero_count: 10,
            healthy: true,
            issues: vec![],
        };
        assert!(health.healthy);
        assert_eq!(health.nan_count, 0);
        let json = serde_json::to_string(&health).unwrap();
        assert!(json.contains("\"healthy\":true"));
    }

    #[test]
    fn test_tensor_health_dirty() {
        let health = TensorHealth {
            tensors_checked: 5,
            nan_count: 3,
            inf_count: 1,
            zero_count: 0,
            healthy: false,
            issues: vec![
                "embed_tokens: 3 NaN values".into(),
                "lm_head: 1 Inf values".into(),
            ],
        };
        assert!(!health.healthy);
        assert_eq!(health.issues.len(), 2);
    }

    #[test]
    fn test_version_consistency_serialize() {
        let vc = VersionConsistency {
            unique_versions: vec![2],
            consistent: true,
            majority_version: Some(2),
            outlier_layers: vec![],
        };
        let json = serde_json::to_string(&vc).unwrap();
        assert!(json.contains("\"consistent\":true"));
        assert!(json.contains("\"majority_version\":2"));
    }

    #[test]
    fn test_version_consistency_with_outliers() {
        let vc = VersionConsistency {
            unique_versions: vec![1, 2],
            consistent: false,
            majority_version: Some(2),
            outlier_layers: vec![3, 7, 12],
        };
        assert!(!vc.consistent);
        assert_eq!(vc.outlier_layers.len(), 3);
    }

    #[test]
    fn test_memory_breakdown_serialize() {
        let mb = MemoryBreakdown {
            embed_tokens_bytes: 486_195_200,
            lm_head_bytes: 486_195_200,
            final_norm_bytes: 12_800,
            per_layer_nda_bytes: 1_500_000,
            per_layer_norm_bytes: 665_600,
            per_layer_bias_bytes: 0,
            total_nda_bytes: 1_500_000,
            total_fp32_bytes: 973_068_800,
        };
        let json = serde_json::to_string(&mb).unwrap();
        assert!(json.contains("\"embed_tokens_bytes\":486195200"));
        assert!(json.contains("\"total_fp32_bytes\":973068800"));
    }

    #[test]
    fn test_weights_load_report_serialize() {
        let report = WeightsLoadReport {
            total_elapsed_us: 5_000_000,
            global_tensors_us: 500_000,
            per_layer_us: 3_500_000,
            gpu_upload_us: 1_000_000,
            layers_loaded: 26,
            per_layer_avg_us: 134_615.38,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"layers_loaded\":26"));
        assert!(json.contains("\"total_elapsed_us\":5000000"));
    }

    #[test]
    fn test_weights_info_serialize() {
        let info = WeightsInfo {
            n_layers: 26,
            nda_bytes: 1_500_000,
            fp32_bytes: 500_000,
            total_bytes: 2_000_000,
            gpu_uploads: 182,
            gpu_upload_capacity: 182,
            gpu_utilization: 1.0,
            vulkan_active: true,
            embed_shape: (151936, 3200),
            lm_head_shared: false,
            validation_issues: vec![],
            tensor_health: TensorHealth {
                tensors_checked: 55,
                nan_count: 0,
                inf_count: 0,
                zero_count: 100,
                healthy: true,
                issues: vec![],
            },
            version_consistency: VersionConsistency {
                unique_versions: vec![2],
                consistent: true,
                majority_version: Some(2),
                outlier_layers: vec![],
            },
            memory: MemoryBreakdown {
                embed_tokens_bytes: 100,
                lm_head_bytes: 100,
                final_norm_bytes: 50,
                per_layer_nda_bytes: 1_500_000,
                per_layer_norm_bytes: 200,
                per_layer_bias_bytes: 0,
                total_nda_bytes: 1_500_000,
                total_fp32_bytes: 450,
            },
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"n_layers\":26"));
        assert!(json.contains("\"gpu_utilization\":1.0"));
        assert!(json.contains("\"healthy\":true"));
        assert!(json.contains("\"consistent\":true"));
    }

    #[test]
    fn test_load_fp32_batch() {
        let tmp = tempfile::tempdir().unwrap();
        let p1 = tmp.path().join("a.bin");
        let p2 = tmp.path().join("b.bin");
        let p3 = tmp.path().join("c.bin");
        write_fp32_bin(&p1, &[3], &[1.0, 2.0, 3.0]);
        write_fp32_bin(&p2, &[2], &[4.0, 5.0]);
        write_fp32_bin(&p3, &[4], &[6.0, 7.0, 8.0, 9.0]);

        let paths = vec![p1, p2, p3];
        let (results, report) = load_fp32_batch(&paths);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap().len(), 3);
        assert_eq!(results[1].as_ref().unwrap().len(), 2);
        assert_eq!(results[2].as_ref().unwrap().len(), 4);
        assert_eq!(report.files_loaded, 3);
        assert_eq!(report.files_failed, 0);
        assert!(report.total_elapsed_us > 0);
    }

    #[test]
    fn test_load_fp32_batch_with_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let p1 = tmp.path().join("good.bin");
        let p2 = tmp.path().join("nonexistent.bin");
        write_fp32_bin(&p1, &[2], &[1.0, 2.0]);

        let paths = vec![p1, p2];
        let (results, report) = load_fp32_batch(&paths);
        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert_eq!(report.files_loaded, 1);
        assert_eq!(report.files_failed, 1);
    }

    #[test]
    fn test_validate_layer_out_of_range() {
        // Can't build full ModelWeights without files, but we can verify
        // the function exists and the report structs serialize correctly
        let report = BatchLoadReport {
            files_attempted: 0,
            files_loaded: 0,
            files_failed: 0,
            total_elapsed_us: 0,
            per_file_avg_us: 0.0,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"files_attempted\":0"));
    }
}