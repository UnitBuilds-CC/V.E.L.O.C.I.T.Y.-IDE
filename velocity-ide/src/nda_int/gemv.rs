use super::nda_vec::*;
use super::tables::*;
use crate::nda::{NdaMatrix, NDA_VERSION_FP2, NDA_VERSION_FP4};
use rayon::prelude::*;
use serde::Serialize;

/// Metrics from a GEMV operation.
#[derive(Debug, Default, Clone, Serialize)]
pub struct GemvReport {
    /// Number of rows in the matrix.
    pub matrix_rows: usize,
    /// Number of columns in the matrix.
    pub matrix_cols: usize,
    /// Matrix version (FP2, FP4, or quad).
    pub matrix_version: u16,
    /// Number of GEMV operations performed.
    pub operations: usize,
    /// Total elapsed time (microseconds).
    pub elapsed_us: u64,
}

pub fn nda_gemv_nda_to_nda(matrix: &NdaMatrix, x: &NdaVec) -> NdaVec {
    if matrix.version == NDA_VERSION_FP4 {
        debug_assert_eq!(x.len, matrix.cols);

        let mut out_i32 = vec![0i32; matrix.rows];
        let global_scale_log2 = matrix.scale.log2().round() as i8;

        out_i32
            .par_iter_mut()
            .enumerate()
            .for_each(|(row, out_val)| {
                let row_start = row * matrix.cols;
                let mut acc = 0i32;
                let block_size = matrix.block_size;
                let n_blocks = matrix.cols.div_ceil(block_size);

                for block_idx in 0..n_blocks {
                    let q_scale = matrix.q_scales[block_idx] as i32;
                    if q_scale == 0 {
                        continue;
                    }

                    let mut block_acc = 0i32;
                    let start_col = block_idx * block_size;
                    let end_col = (start_col + block_size).min(matrix.cols);

                    for col in start_col..end_col {
                        let w_idx = row_start + col;
                        let byte_idx = w_idx / 2;
                        let nibble_shift = (w_idx % 2) * 4;
                        let byte = matrix.packed_codes[byte_idx];
                        let code = ((byte >> nibble_shift) & 0x0F) as usize;

                        let x_byte = col / 8;
                        let x_bit = col % 8;
                        let xs = (x.sign[x_byte] >> x_bit) & 1;
                        let xe = (x.extra[x_byte] >> x_bit) & 1;
                        let x_code = ((xs << 1) | xe) as usize;

                        block_acc += FP4_PRODUCT_LUT[x_code][code];
                    }

                    acc += block_acc * q_scale;
                }

                *out_val = (acc + 128) >> 8;
            });

        let out_log2 = combine_log2_scales(global_scale_log2.saturating_add(10), x.log2_scale);
        return NdaVec::from_i32_slice(&out_i32, out_log2);
    }

    if matrix.version == NDA_VERSION_FP2 {
        debug_assert_eq!(x.len, matrix.cols);

        let mut out_i32 = vec![0i32; matrix.rows];
        let global_scale_log2 = matrix.scale.log2().round() as i8;

        out_i32
            .par_iter_mut()
            .enumerate()
            .for_each(|(row, out_val)| {
                let row_start = row * matrix.cols;
                let mut acc = 0i32;
                let block_size = matrix.block_size;
                let n_blocks = matrix.cols.div_ceil(block_size);

                for block_idx in 0..n_blocks {
                    let q_scale = matrix.q_scales[block_idx] as i32;
                    if q_scale == 0 {
                        continue;
                    }

                    let mut block_acc = 0i32;
                    let start_col = block_idx * block_size;
                    let end_col = (start_col + block_size).min(matrix.cols);

                    for col in start_col..end_col {
                        let w_idx = row_start + col;
                        let byte_idx = w_idx / 4;
                        let pair_shift = (w_idx % 4) * 2;
                        let byte = matrix.packed_codes[byte_idx];
                        let code = ((byte >> pair_shift) & 0x03) as usize;

                        let x_byte = col / 8;
                        let x_bit = col % 8;
                        let xs = (x.sign[x_byte] >> x_bit) & 1;
                        let xe = (x.extra[x_byte] >> x_bit) & 1;
                        let x_code = ((xs << 1) | xe) as usize;

                        block_acc += FP2_PRODUCT_LUT[x_code][code];
                    }

                    acc += block_acc * q_scale;
                }

                *out_val = (acc + 128) >> 8;
            });

        let out_log2 = combine_log2_scales(global_scale_log2.saturating_add(8), x.log2_scale);
        return NdaVec::from_i32_slice(&out_i32, out_log2);
    }

    // Default legacy v2 quad path
    debug_assert!(
        matrix.is_quad(),
        "nda_gemv_nda_to_nda requires v2 quad matrix"
    );
    debug_assert_eq!(x.len, matrix.cols);

    let stride = matrix.cols.div_ceil(8);
    let mut out_i32 = vec![0i32; matrix.rows];

    let mat_log2 = matrix.scale.log2().round() as i8;

    let mut x_low = [0usize; 2048];
    let mut x_high = [0usize; 2048];
    let limit = stride.min(2048);
    for b in 0..limit {
        let xs = x.sign[b];
        let xe = x.extra[b];
        x_low[b] = ((xs & 0x0F) | ((xe & 0x0F) << 4)) as usize;
        x_high[b] = (((xs >> 4) & 0x0F) | (xe & 0xF0)) as usize;
    }

    out_i32
        .par_iter_mut()
        .enumerate()
        .for_each(|(row, out_val)| {
            let base = row * stride;
            let mut acc = 0i32;

            for b in 0..stride {
                let ws = matrix.sign[base + b];
                let we = matrix.extra[base + b];
                let w_low = ((ws & 0x0F) | ((we & 0x0F) << 4)) as usize;
                let w_high = (((ws >> 4) & 0x0F) | (we & 0xF0)) as usize;

                acc += (DOT_4_LUT[w_low][x_low[b]] + DOT_4_LUT[w_high][x_high[b]]) as i32;
            }

            *out_val = acc;
        });

    let out_log2 = combine_log2_scales(mat_log2, x.log2_scale);
    NdaVec::from_i32_slice(&out_i32, out_log2)
}

pub fn argmax_i32(logits: &[i32]) -> u32 {
    logits
        .iter()
        .enumerate()
        .max_by_key(|(_, &v)| v)
        .map(|(i, _)| i as u32)
        .unwrap_or(0)
}

/// Compute top-K argmax indices from a logits vector.
/// Returns (index, value) pairs sorted by value descending.
pub fn topk_i32(logits: &[i32], k: usize) -> Vec<(u32, i32)> {
    let mut indexed: Vec<(u32, i32)> = logits
        .iter()
        .enumerate()
        .map(|(i, &v)| (i as u32, v))
        .collect();
    indexed.sort_by(|a, b| b.1.cmp(&a.1));
    indexed.truncate(k);
    indexed
}

/// Batch GEMV: process multiple input vectors through the same matrix.
/// Returns one NdaVec per input, plus a report with metrics.
pub fn nda_gemv_batch(
    matrix: &NdaMatrix,
    inputs: &[NdaVec],
) -> (Vec<NdaVec>, GemvReport) {
    let t_start = std::time::Instant::now();
    let results: Vec<NdaVec> = inputs
        .iter()
        .map(|x| nda_gemv_nda_to_nda(matrix, x))
        .collect();
    let elapsed_us = t_start.elapsed().as_micros() as u64;

    let report = GemvReport {
        matrix_rows: matrix.rows,
        matrix_cols: matrix.cols,
        matrix_version: matrix.version,
        operations: inputs.len(),
        elapsed_us,
    };
    (results, report)
}

#[allow(dead_code)]
pub fn lm_head_nda_to_i32(matrix: &NdaMatrix, x: &NdaVec) -> Vec<i32> {
    debug_assert!(matrix.is_quad());
    debug_assert_eq!(x.len, matrix.cols);

    let stride = matrix.cols.div_ceil(8);
    let mut out = vec![0i32; matrix.rows];

    out.par_iter_mut().enumerate().for_each(|(row, val)| {
        let base = row * stride;
        let mut acc = 0i32;

        for b in 0..stride {
            let ws = matrix.sign[base + b];
            let we = matrix.extra[base + b];
            let xs = x.sign[b];
            let xe = x.extra[b];

            let same = !(ws ^ xs);
            let diff = ws ^ xs;
            let w_large = !(ws ^ we);
            let x_large = !(xs ^ xe);

            let pos = same.count_ones()
                + (same & w_large).count_ones()
                + (same & x_large).count_ones()
                + (same & w_large & x_large).count_ones();

            let neg = diff.count_ones()
                + (diff & w_large).count_ones()
                + (diff & x_large).count_ones()
                + (diff & w_large & x_large).count_ones();

            acc += pos as i32 - neg as i32;
        }
        *val = acc;
    });

    out
}

#[allow(dead_code)]
pub fn nda_gemv_nda_to_i32(matrix: &NdaMatrix, x: &NdaVec) -> Vec<i32> {
    lm_head_nda_to_i32(matrix, x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nda::NdaMatrix;

    fn make_quad_matrix(rows: usize, cols: usize) -> NdaMatrix {
        let bitmap_bytes = rows * cols.div_ceil(8);
        NdaMatrix::new_quad(
            rows,
            cols,
            1.0,
            vec![0xAA; bitmap_bytes],
            vec![0x55; bitmap_bytes],
        )
    }

    #[test]
    fn argmax_i32_basic() {
        assert_eq!(argmax_i32(&[1, 5, 3, 2]), 1);
        assert_eq!(argmax_i32(&[-1, -5, -3]), 0);
        assert_eq!(argmax_i32(&[42]), 0);
    }

    #[test]
    fn argmax_i32_empty() {
        assert_eq!(argmax_i32(&[]), 0);
    }

    #[test]
    fn topk_i32_basic() {
        let logits = vec![10, 50, 30, 20, 40];
        let top = topk_i32(&logits, 3);
        assert_eq!(top.len(), 3);
        assert_eq!(top[0], (1, 50)); // highest
        assert_eq!(top[1], (4, 40)); // second
        assert_eq!(top[2], (2, 30)); // third
    }

    #[test]
    fn topk_i32_k_larger_than_input() {
        let logits = vec![1, 2, 3];
        let top = topk_i32(&logits, 10);
        assert_eq!(top.len(), 3);
    }

    #[test]
    fn gemv_report_default() {
        let r = GemvReport::default();
        assert_eq!(r.matrix_rows, 0);
        assert_eq!(r.operations, 0);
    }

    #[test]
    fn gemv_report_serializable() {
        let r = GemvReport {
            matrix_rows: 128,
            matrix_cols: 896,
            matrix_version: 2,
            operations: 5,
            elapsed_us: 1000,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"matrix_rows\":128"));
        assert!(json.contains("\"operations\":5"));
    }

    #[test]
    fn batch_gemv_basic() {
        let mat = make_quad_matrix(16, 64);
        let inputs: Vec<NdaVec> = (0..3)
            .map(|_| NdaVec::from_f32_slice(&vec![1.0; 64]))
            .collect();
        let (results, report) = nda_gemv_batch(&mat, &inputs);
        assert_eq!(results.len(), 3);
        assert_eq!(report.operations, 3);
        assert_eq!(report.matrix_rows, 16);
        assert_eq!(report.matrix_cols, 64);
        for r in &results {
            assert_eq!(r.len, 16);
        }
    }

    #[test]
    fn batch_gemv_empty() {
        let mat = make_quad_matrix(8, 8);
        let inputs: Vec<NdaVec> = vec![];
        let (results, report) = nda_gemv_batch(&mat, &inputs);
        assert!(results.is_empty());
        assert_eq!(report.operations, 0);
    }
}
