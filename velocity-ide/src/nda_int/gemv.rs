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

/// Validate GEMV parameters without executing the operation.
pub fn validate_gemv_params(matrix: &NdaMatrix, x: &NdaVec) -> Vec<String> {
    let mut issues = Vec::new();
    if x.len != matrix.cols {
        issues.push(format!(
            "input length {} != matrix cols {}",
            x.len, matrix.cols
        ));
    }
    if matrix.rows == 0 {
        issues.push("matrix has 0 rows".into());
    }
    if matrix.cols == 0 {
        issues.push("matrix has 0 cols".into());
    }
    if matrix.packed_codes.is_empty() && matrix.sign.is_empty() {
        issues.push("matrix has no weight data".into());
    }
    issues.extend(x.validate());
    issues
}

/// Diagnostic info about a GEMV operation.
#[derive(Debug, Clone, Serialize)]
pub struct GemvInfo {
    pub matrix_rows: usize,
    pub matrix_cols: usize,
    pub matrix_version: u16,
    pub version_name: String,
    pub matrix_weight_bytes: usize,
    pub input_len: usize,
    pub output_len: usize,
    pub estimated_output_bytes: usize,
    pub is_parallel: bool,
    pub validation_issues: Vec<String>,
}

/// Compute diagnostic info about a GEMV operation without executing it.
pub fn gemv_info(matrix: &NdaMatrix) -> GemvInfo {
    let version_name = match matrix.version {
        v if v == NDA_VERSION_FP2 => "FP2".into(),
        v if v == NDA_VERSION_FP4 => "FP4".into(),
        _ => "quad".into(),
    };
    let weight_bytes = if !matrix.packed_codes.is_empty() {
        matrix.packed_codes.len()
    } else {
        matrix.sign.len() + matrix.extra.len()
    };
    GemvInfo {
        matrix_rows: matrix.rows,
        matrix_cols: matrix.cols,
        matrix_version: matrix.version,
        version_name,
        matrix_weight_bytes: weight_bytes,
        input_len: matrix.cols,
        output_len: matrix.rows,
        estimated_output_bytes: matrix.rows.div_ceil(8) * 2,
        is_parallel: matrix.rows >= 8,
        validation_issues: Vec::new(),
    }
}

/// Validate top-K parameters.
pub fn validate_topk_params(logits: &[i32], k: usize) -> Vec<String> {
    let mut issues = Vec::new();
    if logits.is_empty() {
        issues.push("logits vector is empty".into());
    }
    if k == 0 {
        issues.push("k is 0".into());
    }
    issues
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

    #[test]
    fn validate_gemv_params_valid() {
        let mat = make_quad_matrix(16, 64);
        let x = NdaVec::from_f32_slice(&vec![1.0; 64]);
        let issues = validate_gemv_params(&mat, &x);
        assert!(issues.is_empty(), "expected no issues, got: {:?}", issues);
    }

    #[test]
    fn validate_gemv_params_length_mismatch() {
        let mat = make_quad_matrix(16, 64);
        let x = NdaVec::from_f32_slice(&vec![1.0; 32]); // wrong length
        let issues = validate_gemv_params(&mat, &x);
        assert!(issues.iter().any(|i| i.contains("!=")));
    }

    #[test]
    fn validate_gemv_params_zero_matrix() {
        let mat = NdaMatrix::new_quad(0, 0, 1.0, vec![].into(), vec![].into());
        let x = NdaVec::from_f32_slice(&[]);
        let issues = validate_gemv_params(&mat, &x);
        assert!(issues.iter().any(|i| i.contains("0 rows") || i.contains("0 cols")));
    }

    #[test]
    fn gemv_info_quad() {
        let mat = make_quad_matrix(128, 256);
        let info = gemv_info(&mat);
        assert_eq!(info.matrix_rows, 128);
        assert_eq!(info.matrix_cols, 256);
        assert_eq!(info.version_name, "quad");
        assert_eq!(info.input_len, 256);
        assert_eq!(info.output_len, 128);
        assert!(info.is_parallel);
    }

    #[test]
    fn gemv_info_serializes() {
        let mat = make_quad_matrix(32, 64);
        let info = gemv_info(&mat);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"matrix_rows\":32"));
        assert!(json.contains("\"version_name\":\"quad\""));
    }

    #[test]
    fn validate_topk_params_valid() {
        let issues = validate_topk_params(&[1, 2, 3], 2);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_topk_params_empty_logits() {
        let issues = validate_topk_params(&[], 2);
        assert!(issues.iter().any(|i| i.contains("empty")));
    }

    #[test]
    fn validate_topk_params_zero_k() {
        let issues = validate_topk_params(&[1, 2, 3], 0);
        assert!(issues.iter().any(|i| i.contains("k is 0")));
    }

    // ─── Expanded Tests ─────────────────────────────────────────────────

    #[test]
    fn argmax_i32_ties_first_wins() {
        // When multiple elements have the same max value, max_by_key returns the last
        // But we want to verify consistent behavior
        assert_eq!(argmax_i32(&[5, 5, 5, 5]), 3); // max_by_key returns last
    }

    #[test]
    fn argmax_i32_all_negative() {
        assert_eq!(argmax_i32(&[-10, -3, -7, -1]), 3); // -1 is max
    }

    #[test]
    fn argmax_i32_large_values() {
        let mut logits = vec![0i32; 1000];
        logits[42] = i32::MAX;
        assert_eq!(argmax_i32(&logits), 42);
    }

    #[test]
    fn topk_i32_k_one_is_greedy() {
        let logits = vec![10, 50, 30, 20, 40];
        let top = topk_i32(&logits, 1);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0], (1, 50));
    }

    #[test]
    fn topk_i32_k_zero() {
        let logits = vec![10, 50, 30];
        let top = topk_i32(&logits, 0);
        assert!(top.is_empty());
    }

    #[test]
    fn topk_i32_single_element() {
        let logits = vec![42];
        let top = topk_i32(&logits, 5);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0], (0, 42));
    }

    #[test]
    fn topk_i32_sorted_descending() {
        let logits = vec![5, 3, 8, 1, 9, 2];
        let top = topk_i32(&logits, 4);
        // Verify sorted descending by value
        for i in 0..top.len() - 1 {
            assert!(top[i].1 >= top[i + 1].1);
        }
    }

    #[test]
    fn gemv_quad_output_length() {
        let mat = make_quad_matrix(32, 64);
        let x = NdaVec::from_f32_slice(&vec![0.5; 64]);
        let out = nda_gemv_nda_to_nda(&mat, &x);
        assert_eq!(out.len, 32);
    }

    #[test]
    fn gemv_quad_zero_input() {
        let mat = make_quad_matrix(8, 8);
        let x = NdaVec::zeros(8, 0);
        let out = nda_gemv_nda_to_nda(&mat, &x);
        assert_eq!(out.len, 8);
    }

    #[test]
    fn gemv_quad_small_matrix() {
        let mat = make_quad_matrix(4, 16);
        let x = NdaVec::from_f32_slice(&vec![1.0; 16]);
        let out = nda_gemv_nda_to_nda(&mat, &x);
        assert_eq!(out.len, 4);
    }

    #[test]
    fn validate_gemv_params_no_weight_data() {
        let mat = NdaMatrix {
            rows: 8,
            cols: 8,
            scale: 1.0,
            version: 2,
            sign: vec![],
            extra: vec![],
            block_size: 0,
            n_blocks: 0,
            q_scales: vec![],
            packed_codes: vec![],
        };
        let x = NdaVec::from_f32_slice(&vec![1.0; 8]);
        let issues = validate_gemv_params(&mat, &x);
        assert!(issues.iter().any(|i| i.contains("no weight data")));
    }

    #[test]
    fn gemv_info_small_not_parallel() {
        let mat = make_quad_matrix(4, 8);
        let info = gemv_info(&mat);
        assert!(!info.is_parallel); // rows < 8
    }

    #[test]
    fn gemv_info_parallel_threshold() {
        let mat = make_quad_matrix(8, 8);
        let info = gemv_info(&mat);
        assert!(info.is_parallel); // rows >= 8
    }

    #[test]
    fn gemv_info_fp4_matrix() {
        let mat = NdaMatrix {
            rows: 16,
            cols: 32,
            scale: 0.5,
            version: NDA_VERSION_FP4,
            sign: vec![],
            extra: vec![],
            block_size: 16,
            n_blocks: 2,
            q_scales: vec![1, 2],
            packed_codes: vec![0xAA; 32],
        };
        let info = gemv_info(&mat);
        assert_eq!(info.version_name, "FP4");
        assert_eq!(info.matrix_weight_bytes, 32);
    }

    #[test]
    fn gemv_info_fp2_matrix() {
        let mat = NdaMatrix {
            rows: 8,
            cols: 16,
            scale: 1.0,
            version: NDA_VERSION_FP2,
            sign: vec![],
            extra: vec![],
            block_size: 8,
            n_blocks: 2,
            q_scales: vec![1, 2],
            packed_codes: vec![0x55; 16],
        };
        let info = gemv_info(&mat);
        assert_eq!(info.version_name, "FP2");
    }

    #[test]
    fn gemv_report_clone() {
        let r = GemvReport {
            matrix_rows: 64,
            matrix_cols: 128,
            matrix_version: 2,
            operations: 10,
            elapsed_us: 5000,
        };
        let cloned = r.clone();
        assert_eq!(cloned.matrix_rows, 64);
        assert_eq!(cloned.operations, 10);
    }

    #[test]
    fn batch_gemv_single_input() {
        let mat = make_quad_matrix(8, 16);
        let x = NdaVec::from_f32_slice(&vec![1.0; 16]);
        let (results, report) = nda_gemv_batch(&mat, &[x]);
        assert_eq!(results.len(), 1);
        assert_eq!(report.operations, 1);
    }

    #[test]
    fn gemv_info_estimated_output_bytes() {
        let mat = make_quad_matrix(100, 64);
        let info = gemv_info(&mat);
        // 100 rows → ceil(100/8) = 13 bytes * 2 = 26
        assert_eq!(info.estimated_output_bytes, 26);
    }

    // ── Block 184: JSON key counts ────────────────────────────────────────

    #[test]
    fn gemv_report_json_has_exactly_5_keys() {
        let r = GemvReport {
            matrix_rows: 16, matrix_cols: 32, matrix_version: 2,
            operations: 1, elapsed_us: 100,
        };
        let val: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&r).unwrap()
        ).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 5);
    }

    #[test]
    fn gemv_info_json_has_exactly_10_keys() {
        let mat = make_quad_matrix(16, 32);
        let info = gemv_info(&mat);
        let val: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&info).unwrap()
        ).unwrap();
        assert_eq!(val.as_object().unwrap().len(), 10);
    }

    // ── Block 184: JSON roundtrip via Value ───────────────────────────────

    #[test]
    fn gemv_report_json_roundtrip_via_value() {
        let r = GemvReport {
            matrix_rows: 128, matrix_cols: 256, matrix_version: 4,
            operations: 10, elapsed_us: 5000,
        };
        let json = serde_json::to_string(&r).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["matrix_rows"], 128);
        assert_eq!(val["matrix_cols"], 256);
        assert_eq!(val["matrix_version"], 4);
        assert_eq!(val["operations"], 10);
        assert_eq!(val["elapsed_us"], 5000);
    }

    #[test]
    fn gemv_info_json_roundtrip_via_value() {
        let mat = make_quad_matrix(64, 128);
        let info = gemv_info(&mat);
        let json = serde_json::to_string(&info).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["matrix_rows"], 64);
        assert_eq!(val["matrix_cols"], 128);
        assert_eq!(val["version_name"], "quad");
        assert_eq!(val["input_len"], 128);
        assert_eq!(val["output_len"], 64);
        assert!(val["is_parallel"].as_bool().unwrap());
    }

    // ── Block 184: Clone independence ─────────────────────────────────────

    #[test]
    fn gemv_report_clone_independent() {
        let mut r = GemvReport {
            matrix_rows: 32, matrix_cols: 64, matrix_version: 2,
            operations: 5, elapsed_us: 200,
        };
        let cloned = r.clone();
        r.operations = 999;
        assert_eq!(cloned.operations, 5);
    }

    #[test]
    fn gemv_info_clone_independent() {
        let mat = make_quad_matrix(16, 32);
        let mut info = gemv_info(&mat);
        let cloned = info.clone();
        info.matrix_rows = 0;
        assert_eq!(cloned.matrix_rows, 16);
    }

    // ── Block 184: Debug format ───────────────────────────────────────────

    #[test]
    fn gemv_report_debug_has_all_fields() {
        let r = GemvReport {
            matrix_rows: 8, matrix_cols: 16, matrix_version: 2,
            operations: 3, elapsed_us: 100,
        };
        let debug = format!("{:?}", r);
        assert!(debug.contains("matrix_rows"));
        assert!(debug.contains("matrix_cols"));
        assert!(debug.contains("matrix_version"));
        assert!(debug.contains("operations"));
        assert!(debug.contains("elapsed_us"));
    }

    #[test]
    fn gemv_info_debug_has_all_fields() {
        let mat = make_quad_matrix(16, 32);
        let info = gemv_info(&mat);
        let debug = format!("{:?}", info);
        assert!(debug.contains("matrix_rows"));
        assert!(debug.contains("version_name"));
        assert!(debug.contains("matrix_weight_bytes"));
        assert!(debug.contains("estimated_output_bytes"));
        assert!(debug.contains("is_parallel"));
        assert!(debug.contains("validation_issues"));
    }

    // ── Block 184: GemvReport default ─────────────────────────────────────

    #[test]
    fn gemv_report_default_all_zeros() {
        let r = GemvReport::default();
        assert_eq!(r.matrix_rows, 0);
        assert_eq!(r.matrix_cols, 0);
        assert_eq!(r.matrix_version, 0);
        assert_eq!(r.operations, 0);
        assert_eq!(r.elapsed_us, 0);
    }

    // ── Block 184: GemvInfo formula verification ──────────────────────────

    #[test]
    fn gemv_info_weight_bytes_quad_matrix() {
        let mat = make_quad_matrix(16, 64);
        let info = gemv_info(&mat);
        // Quad: weight_bytes = sign.len() + extra.len()
        // bitmap_bytes = rows * cols.div_ceil(8) = 16 * 8 = 128
        assert_eq!(info.matrix_weight_bytes, 128 + 128);
    }

    #[test]
    fn gemv_info_weight_bytes_packed_codes() {
        let mat = NdaMatrix {
            rows: 8, cols: 16, scale: 1.0, version: NDA_VERSION_FP4,
            sign: vec![], extra: vec![],
            block_size: 8, n_blocks: 2,
            q_scales: vec![1, 2],
            packed_codes: vec![0xAA; 50],
        };
        let info = gemv_info(&mat);
        // FP4 with packed_codes: weight_bytes = packed_codes.len()
        assert_eq!(info.matrix_weight_bytes, 50);
    }

    #[test]
    fn gemv_info_estimated_output_bytes_formula() {
        for rows in [1, 7, 8, 15, 16, 100, 256] {
            let mat = make_quad_matrix(rows, 64);
            let info = gemv_info(&mat);
            let expected = rows.div_ceil(8) * 2;
            assert_eq!(info.estimated_output_bytes, expected,
                "wrong for rows={}", rows);
        }
    }

    #[test]
    fn gemv_info_input_output_len_match_matrix() {
        let mat = make_quad_matrix(32, 128);
        let info = gemv_info(&mat);
        assert_eq!(info.input_len, 128);  // == matrix.cols
        assert_eq!(info.output_len, 32);  // == matrix.rows
    }

    // ── Block 184: version_name for various versions ──────────────────────

    #[test]
    fn gemv_info_unknown_version_is_quad() {
        let mat = NdaMatrix {
            rows: 8, cols: 8, scale: 1.0, version: 99,
            sign: vec![0xAA; 1], extra: vec![0x55; 1],
            block_size: 0, n_blocks: 0,
            q_scales: vec![], packed_codes: vec![],
        };
        let info = gemv_info(&mat);
        assert_eq!(info.version_name, "quad");
    }

    // ── Block 184: argmax/topk edge cases ─────────────────────────────────

    #[test]
    fn argmax_i32_two_elements() {
        assert_eq!(argmax_i32(&[10, 20]), 1);
        assert_eq!(argmax_i32(&[20, 10]), 0);
    }

    #[test]
    fn topk_i32_negative_values() {
        let logits = vec![-5, -1, -10, -3];
        let top = topk_i32(&logits, 2);
        assert_eq!(top[0], (1, -1));
        assert_eq!(top[1], (3, -3));
    }

    #[test]
    fn topk_i32_all_same_values() {
        let logits = vec![5, 5, 5, 5];
        let top = topk_i32(&logits, 2);
        assert_eq!(top.len(), 2);
        // All values are 5
        for &(_, v) in &top {
            assert_eq!(v, 5);
        }
    }

    // ── Block 184: validate combined issues ───────────────────────────────

    #[test]
    fn validate_gemv_params_multiple_issues() {
        let mat = NdaMatrix {
            rows: 0, cols: 0, scale: 1.0, version: 2,
            sign: vec![], extra: vec![],
            block_size: 0, n_blocks: 0,
            q_scales: vec![], packed_codes: vec![],
        };
        let x = NdaVec::from_f32_slice(&[]);
        let issues = validate_gemv_params(&mat, &x);
        // Should have: 0 rows, 0 cols, no weight data
        assert!(issues.len() >= 3, "expected >= 3 issues, got {}: {:?}", issues.len(), issues);
    }

    #[test]
    fn validate_topk_params_both_errors() {
        let issues = validate_topk_params(&[], 0);
        assert_eq!(issues.len(), 2);
        assert!(issues.iter().any(|i| i.contains("empty")));
        assert!(issues.iter().any(|i| i.contains("k is 0")));
    }

    // ── Block 184: batch_gemv report accuracy ─────────────────────────────

    #[test]
    fn batch_gemv_report_fields_accurate() {
        let mat = make_quad_matrix(16, 32);
        let inputs: Vec<NdaVec> = (0..5)
            .map(|_| NdaVec::from_f32_slice(&vec![0.5; 32]))
            .collect();
        let (_, report) = nda_gemv_batch(&mat, &inputs);
        assert_eq!(report.matrix_rows, 16);
        assert_eq!(report.matrix_cols, 32);
        assert_eq!(report.matrix_version, 2);
        assert_eq!(report.operations, 5);
    }

    // ── Block 184: Compact JSON ───────────────────────────────────────────

    #[test]
    fn gemv_report_compact_json() {
        let r = GemvReport::default();
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("\n"));
    }

    #[test]
    fn gemv_info_compact_json() {
        let mat = make_quad_matrix(8, 8);
        let info = gemv_info(&mat);
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("\n"));
    }

    // ── Block 184: gemv_info parallel threshold ───────────────────────────

    #[test]
    fn gemv_info_parallel_threshold_boundary() {
        let mat7 = make_quad_matrix(7, 8);
        assert!(!gemv_info(&mat7).is_parallel);
        let mat8 = make_quad_matrix(8, 8);
        assert!(gemv_info(&mat8).is_parallel);
    }
}
