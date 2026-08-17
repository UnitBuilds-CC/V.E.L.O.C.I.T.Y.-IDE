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
use std::{fs, path::Path};

use crate::compiler::driver::{VulkanDriver, VulkanNdaGemv};
use crate::model::config::ModelConfig;
use crate::nda::NdaMatrix;

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

    let ndim = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let header_bytes = 4 + ndim * 4;

    if data.len() < header_bytes {
        anyhow::bail!("FP32 bin header truncated: {path:?}");
    }

    let mut n_elems: usize = 1;
    for i in 0..ndim {
        let d = u32::from_le_bytes(data[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize;
        n_elems *= d;
    }

    let data_bytes = n_elems * 4;
    if data.len() < header_bytes + data_bytes {
        anyhow::bail!("FP32 bin data truncated: {path:?}");
    }

    let floats: Vec<f32> = data[header_bytes..header_bytes + data_bytes]
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();

    Ok(floats)
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
                    .unwrap(),
            );
            let pos_word = u32::from_le_bytes(
                matrix.extra[byte_offset..byte_offset + 4]
                    .try_into()
                    .unwrap(),
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
            .unwrap()
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
}
