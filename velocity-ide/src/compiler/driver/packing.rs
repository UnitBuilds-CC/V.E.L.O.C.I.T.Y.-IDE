//! Weight and input packing routines for NDA and uvec4 GPU formats.
//!
//! # Safety Invariants
//!
//! All `unsafe` blocks reinterpret between `&[u8]` and `&[u32]` via `from_raw_parts`.
//! Callers must ensure input byte slices are 4-byte aligned and have lengths that are
//! multiples of 4. All packing functions are internal and called with pre-validated
//! weight buffers from the model loader.

use serde::Serialize;
use std::time::Instant;

// ─── Packing diagnostics ───────────────────────────────────────────────────

/// Report from a packing operation with timing and validation info.
#[derive(Debug, Clone, Serialize)]
pub struct PackingReport {
    pub operation: String,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub elapsed_us: u64,
    pub validation_issues: Vec<String>,
    pub valid: bool,
}

/// Batch packing report for multiple operations.
#[derive(Debug, Clone, Serialize)]
pub struct BatchPackingReport {
    pub operations: usize,
    pub total_input_bytes: usize,
    pub total_output_bytes: usize,
    pub total_elapsed_us: u64,
    pub per_op_avg_us: f64,
    pub all_valid: bool,
    pub issues: Vec<String>,
}

/// Validate that a byte slice is properly aligned and sized for u32 reinterpretation.
pub fn validate_u32_alignment(data: &[u8], ctx: &str) -> Vec<String> {
    let mut issues = Vec::new();
    if data.len() % 4 != 0 {
        issues.push(format!(
            "{ctx}: byte length {} is not a multiple of 4",
            data.len()
        ));
    }
    if (data.as_ptr() as usize) % 4 != 0 {
        issues.push(format!("{ctx}: pointer is not 4-byte aligned"));
    }
    issues
}

/// Validate dimensions for weight packing operations.
pub fn validate_pack_dims(k: usize, n: usize, data_len: usize, ctx: &str) -> Vec<String> {
    let mut issues = Vec::new();
    if k == 0 {
        issues.push(format!("{ctx}: k dimension is zero"));
    }
    if n == 0 {
        issues.push(format!("{ctx}: n dimension is zero"));
    }
    if k % 16 != 0 {
        issues.push(format!(
            "{ctx}: k={k} is not a multiple of 16 (required for uvec4 packing)"
        ));
    }
    let expected_words = (k / 16) * n;
    let actual_words = data_len / 4;
    if actual_words < expected_words {
        issues.push(format!(
            "{ctx}: data has {actual_words} u32 words but need {expected_words} (k={k}, n={n})"
        ));
    }
    issues
}

/// Validate dimensions for NDA weight packing (128-column groups).
pub fn validate_nda_pack_dims(k: usize, n: usize, data_len: usize, ctx: &str) -> Vec<String> {
    let mut issues = Vec::new();
    if k == 0 {
        issues.push(format!("{ctx}: k dimension is zero"));
    }
    if n == 0 {
        issues.push(format!("{ctx}: n dimension is zero"));
    }
    if k % 128 != 0 {
        issues.push(format!(
            "{ctx}: k={k} is not a multiple of 128 (required for NDA packing)"
        ));
    }
    let expected_words = (k / 16) * n;
    let actual_words = data_len / 4;
    if actual_words < expected_words {
        issues.push(format!(
            "{ctx}: data has {actual_words} u32 words but need {expected_words} (k={k}, n={n})"
        ));
    }
    issues
}

/// Pack weights with validation and a diagnostic report.
pub fn pack_weights_uvec4_report(src: &[u8], k: usize, n: usize) -> (Vec<u8>, PackingReport) {
    let start = Instant::now();
    let mut issues = validate_u32_alignment(src, "pack_weights_uvec4 input");
    issues.extend(validate_pack_dims(k, n, src.len(), "pack_weights_uvec4"));
    let valid = issues.is_empty();

    let result = if valid {
        pack_weights_uvec4(src, k, n)
    } else {
        Vec::new()
    };

    let elapsed = start.elapsed().as_micros() as u64;
    let report = PackingReport {
        operation: "pack_weights_uvec4".to_string(),
        input_bytes: src.len(),
        output_bytes: result.len(),
        elapsed_us: elapsed,
        validation_issues: issues,
        valid,
    };
    (result, report)
}

/// Pack NDA weights with validation and a diagnostic report.
pub fn pack_weights_nda_report(
    weights: &[u8], k: usize, n: usize,
) -> ((Vec<u8>, Vec<u8>), PackingReport) {
    let start = Instant::now();
    let mut issues = validate_u32_alignment(weights, "pack_weights_nda input");
    issues.extend(validate_nda_pack_dims(k, n, weights.len(), "pack_weights_nda"));
    let valid = issues.is_empty();

    let result = if valid {
        pack_weights_nda(weights, k, n)
    } else {
        (Vec::new(), Vec::new())
    };

    let elapsed = start.elapsed().as_micros() as u64;
    let out_bytes = result.0.len() + result.1.len();
    let report = PackingReport {
        operation: "pack_weights_nda".to_string(),
        input_bytes: weights.len(),
        output_bytes: out_bytes,
        elapsed_us: elapsed,
        validation_issues: issues,
        valid,
    };
    (result, report)
}

/// Batch pack multiple weight sets, returning results and aggregate report.
pub fn pack_weights_uvec4_batch(
    items: &[(Vec<u8>, usize, usize)],
) -> (Vec<Vec<u8>>, BatchPackingReport) {
    let start = Instant::now();
    let mut results = Vec::with_capacity(items.len());
    let mut total_in = 0usize;
    let mut total_out = 0usize;
    let mut all_issues = Vec::new();

    for (data, k, n) in items {
        let (packed, report) = pack_weights_uvec4_report(data, *k, *n);
        total_in += report.input_bytes;
        total_out += report.output_bytes;
        all_issues.extend(report.validation_issues.iter().cloned());
        results.push(packed);
    }

    let elapsed = start.elapsed().as_micros() as u64;
    let avg = if items.is_empty() { 0.0 } else { elapsed as f64 / items.len() as f64 };

    let report = BatchPackingReport {
        operations: items.len(),
        total_input_bytes: total_in,
        total_output_bytes: total_out,
        total_elapsed_us: elapsed.max(1),
        per_op_avg_us: avg,
        all_valid: all_issues.is_empty(),
        issues: all_issues,
    };
    (results, report)
}

/// Summary statistics about a set of packing operations.
#[derive(Debug, Clone, Serialize)]
pub struct PackingSummary {
    pub total_ops: usize,
    pub valid_ops: usize,
    pub invalid_ops: usize,
    pub compression_ratio: f64,
    pub total_issues: usize,
    pub heaviest_op: Option<String>,
    pub heaviest_op_bytes: usize,
}

impl BatchPackingReport {
    /// Build a compact summary from this batch report.
    pub fn summary(&self) -> PackingSummary {
        let compression = if self.total_input_bytes > 0 {
            self.total_output_bytes as f64 / self.total_input_bytes as f64
        } else {
            0.0
        };
        PackingSummary {
            total_ops: self.operations,
            valid_ops: if self.all_valid { self.operations } else { 0 },
            invalid_ops: if self.all_valid { 0 } else { self.issues.len() },
            compression_ratio: compression,
            total_issues: self.issues.len(),
            heaviest_op: None,
            heaviest_op_bytes: 0,
        }
    }
}

/// Validate that input data is suitable for NDA input packing.
pub fn validate_inputs_nda(data: &[u32], ctx: &str) -> Vec<String> {
    let mut issues = Vec::new();
    if data.is_empty() {
        issues.push(format!("{ctx}: input is empty"));
    }
    if data.len() % 8 != 0 {
        issues.push(format!(
            "{ctx}: length {} is not a multiple of 8 (need 8 words per col-group-of-128)",
            data.len()
        ));
    }
    issues
}

/// Pack NDA inputs with validation and a diagnostic report.
pub fn pack_inputs_nda_report(
    inputs: &[u32],
) -> ((Vec<u32>, Vec<u32>), PackingReport) {
    let start = Instant::now();
    let issues = validate_inputs_nda(inputs, "pack_inputs_nda");
    let valid = issues.is_empty();

    let result = if valid {
        pack_inputs_nda(inputs)
    } else {
        (Vec::new(), Vec::new())
    };

    let elapsed = start.elapsed().as_micros() as u64;
    let out_bytes = (result.0.len() + result.1.len()) * 4;
    let report = PackingReport {
        operation: "pack_inputs_nda".to_string(),
        input_bytes: inputs.len() * 4,
        output_bytes: out_bytes,
        elapsed_us: elapsed,
        validation_issues: issues,
        valid,
    };
    (result, report)
}

pub fn pack_weights_uvec4(src: &[u8], k: usize, n: usize) -> Vec<u8> {
    // SAFETY: `src` is a weight buffer whose length is guaranteed to be a multiple of 4
    // by the model loader. Pointer cast is valid because the buffer is 4-byte aligned.
    let src_u32 = unsafe { std::slice::from_raw_parts(src.as_ptr() as *const u32, src.len() / 4) };
    let num_col_groups = k / 16;
    let num_col_groups_4 = num_col_groups / 4;
    let mut dest = vec![0u32; num_col_groups * n];

    for cg4 in 0..num_col_groups_4 {
        for row in 0..n {
            for offset in 0..4 {
                let cg = cg4 * 4 + offset;
                let src_idx = cg * n + row;
                let dest_idx = cg4 * n * 4 + row * 4 + offset;
                dest[dest_idx] = src_u32[src_idx];
            }
        }
    }

    // SAFETY: `dest` is a Vec<u32>; reinterpreting as bytes is valid. Length is
    // checked via checked_mul to prevent overflow.
    unsafe {
        let bytes_ptr = dest.as_ptr() as *const u8;
        let byte_len = dest.len().checked_mul(4).expect("dest len overflow");
        std::slice::from_raw_parts(bytes_ptr, byte_len).to_vec()
    }
}

#[allow(dead_code)]
pub fn pack_inputs_nda(inputs_ternary_u32: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let num_col_groups = inputs_ternary_u32.len();
    let num_col_groups_128 = num_col_groups / 8;
    let mut active = vec![0u32; num_col_groups_128 * 4];
    let mut pos = vec![0u32; num_col_groups_128 * 4];

    for cg128 in 0..num_col_groups_128 {
        for w in 0..4 {
            let idx_0 = (cg128 * 8) + (w * 2);
            let idx_1 = idx_0 + 1;

            let val_0 = inputs_ternary_u32[idx_0];
            let val_1 = inputs_ternary_u32[idx_1];

            let mut act_w = 0u32;
            let mut pos_w = 0u32;

            for bit in 0..16 {
                let code_0 = (val_0 >> (bit * 2)) & 0x03;
                if code_0 != 0 {
                    act_w |= 1 << (bit * 2);
                    if code_0 == 1 {
                        pos_w |= 1 << (bit * 2);
                    }
                }

                let code_1 = (val_1 >> (bit * 2)) & 0x03;
                if code_1 != 0 {
                    act_w |= 1 << (bit * 2 + 1);
                    if code_1 == 1 {
                        pos_w |= 1 << (bit * 2 + 1);
                    }
                }
            }

            active[cg128 * 4 + w] = act_w;
            pos[cg128 * 4 + w] = pos_w;
        }
    }
    (active, pos)
}

pub fn pack_weights_nda(weights_ternary_bytes: &[u8], k: usize, n: usize) -> (Vec<u8>, Vec<u8>) {
    // SAFETY: `weights_ternary_bytes` length is a multiple of 4 (guaranteed by caller).
    // Pointer cast is valid because the buffer is 4-byte aligned.
    let src_u32 = unsafe {
        std::slice::from_raw_parts(
            weights_ternary_bytes.as_ptr() as *const u32,
            weights_ternary_bytes.len() / 4,
        )
    };

    let num_col_groups_128 = k / 128;
    let mut active = vec![0u32; num_col_groups_128 * n * 4];
    let mut pos = vec![0u32; num_col_groups_128 * n * 4];

    for cg128 in 0..num_col_groups_128 {
        for row in 0..n {
            for w in 0..4 {
                let cg_0 = (cg128 * 8) + (w * 2);
                let cg_1 = cg_0 + 1;

                let src_idx_0 = cg_0 * n + row;
                let src_idx_1 = cg_1 * n + row;

                let val_0 = src_u32[src_idx_0];
                let val_1 = src_u32[src_idx_1];

                let mut act_w = 0u32;
                let mut pos_w = 0u32;

                for bit in 0..16 {
                    let code_0 = (val_0 >> (bit * 2)) & 0x03;
                    if code_0 != 0 {
                        act_w |= 1 << (bit * 2);
                        if code_0 == 1 {
                            pos_w |= 1 << (bit * 2);
                        }
                    }

                    let code_1 = (val_1 >> (bit * 2)) & 0x03;
                    if code_1 != 0 {
                        act_w |= 1 << (bit * 2 + 1);
                        if code_1 == 1 {
                            pos_w |= 1 << (bit * 2 + 1);
                        }
                    }
                }

                let dest_idx = cg128 * n * 4 + row * 4 + w;
                active[dest_idx] = act_w;
                pos[dest_idx] = pos_w;
            }
        }
    }

    // SAFETY: `active` is a Vec<u32>; reinterpreting as bytes is valid. Length checked.
    let act_bytes = unsafe {
        let byte_len = active.len().checked_mul(4).expect("active len overflow");
        std::slice::from_raw_parts(active.as_ptr() as *const u8, byte_len).to_vec()
    };
    // SAFETY: `pos` is a Vec<u32>; reinterpreting as bytes is valid. Length checked.
    let pos_bytes = unsafe {
        let byte_len = pos.len().checked_mul(4).expect("pos len overflow");
        std::slice::from_raw_parts(pos.as_ptr() as *const u8, byte_len).to_vec()
    };
    (act_bytes, pos_bytes)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Validation tests ──────────────────────────────────────────────────

    #[test]
    fn validate_alignment_ok() {
        let data = vec![0u8; 16];
        let issues = validate_u32_alignment(&data, "test");
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn validate_alignment_bad_length() {
        let data = vec![0u8; 5]; // not multiple of 4
        let issues = validate_u32_alignment(&data, "test");
        assert!(!issues.is_empty());
        assert!(issues[0].contains("not a multiple of 4"));
    }

    #[test]
    fn validate_pack_dims_ok() {
        // k=16, n=4: need (16/16)*4 = 4 words = 16 bytes
        let issues = validate_pack_dims(16, 4, 16, "test");
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn validate_pack_dims_zero_k() {
        let issues = validate_pack_dims(0, 4, 16, "test");
        assert!(!issues.is_empty());
        assert!(issues[0].contains("k dimension is zero"));
    }

    #[test]
    fn validate_pack_dims_zero_n() {
        let issues = validate_pack_dims(16, 0, 16, "test");
        assert!(!issues.is_empty());
        assert!(issues[0].contains("n dimension is zero"));
    }

    #[test]
    fn validate_pack_dims_bad_k_multiple() {
        let issues = validate_pack_dims(15, 4, 16, "test");
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| i.contains("not a multiple of 16")));
    }

    #[test]
    fn validate_pack_dims_insufficient_data() {
        // k=16, n=4: need 4 words = 16 bytes, but only give 8
        let issues = validate_pack_dims(16, 4, 8, "test");
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| i.contains("not enough")) || issues.iter().any(|i| i.contains("need")));
    }

    #[test]
    fn validate_nda_pack_dims_ok() {
        // k=128, n=2: need (128/16)*2 = 16 words = 64 bytes
        let issues = validate_nda_pack_dims(128, 2, 64, "test");
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn validate_nda_pack_dims_bad_k() {
        let issues = validate_nda_pack_dims(64, 2, 64, "test");
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| i.contains("not a multiple of 128")));
    }

    // ─── pack_weights_uvec4 tests ──────────────────────────────────────────

    #[test]
    fn pack_weights_uvec4_basic() {
        // k=64, n=1: 4 col groups → 1 group-of-4, 1 row
        // Input: 4 u32 words = 16 bytes, Output: 4 u32 words = 16 bytes
        let input: Vec<u8> = vec![
            0x01, 0x00, 0x00, 0x00,
            0x02, 0x00, 0x00, 0x00,
            0x03, 0x00, 0x00, 0x00,
            0x04, 0x00, 0x00, 0x00,
        ];
        let result = pack_weights_uvec4(&input, 64, 1);
        assert_eq!(result.len(), 16);
        // With single row, tiling preserves order
        assert_eq!(&result, &input);
    }

    #[test]
    fn pack_weights_uvec4_report_valid() {
        let input: Vec<u8> = vec![0; 64]; // 16 u32 words, k=64 n=4
        let (result, report) = pack_weights_uvec4_report(&input, 64, 4);
        assert!(report.valid);
        assert!(report.validation_issues.is_empty());
        assert_eq!(report.input_bytes, 64);
        assert_eq!(report.operation, "pack_weights_uvec4");
        assert!(!result.is_empty());
    }

    #[test]
    fn pack_weights_uvec4_report_invalid() {
        let input: Vec<u8> = vec![0; 5]; // bad alignment
        let (result, report) = pack_weights_uvec4_report(&input, 64, 4);
        assert!(!report.valid);
        assert!(!report.validation_issues.is_empty());
        assert!(result.is_empty()); // should not pack on invalid input
    }

    // ─── pack_weights_nda tests ────────────────────────────────────────────

    #[test]
    fn pack_weights_nda_basic() {
        // k=128, n=1: 8 col groups, 1 row → 8 u32 words = 32 bytes input
        let input = vec![0u8; 32];
        let ((act, pos), report) = pack_weights_nda_report(&input, 128, 1);
        assert!(report.valid);
        // All zeros input → all zeros output
        assert!(act.iter().all(|&b| b == 0));
        assert!(pos.iter().all(|&b| b == 0));
    }

    #[test]
    fn pack_weights_nda_report_invalid() {
        let input = vec![0u8; 10]; // bad size for k=128
        let ((act, pos), report) = pack_weights_nda_report(&input, 128, 1);
        assert!(!report.valid);
        assert!(act.is_empty());
        assert!(pos.is_empty());
    }

    // ─── pack_inputs_nda tests ─────────────────────────────────────────────

    #[test]
    fn pack_inputs_nda_all_zeros() {
        // 8 col groups × 1 = 8 u32 words (minimum for 1 cg128)
        let input = vec![0u32; 8];
        let (active, pos) = pack_inputs_nda(&input);
        assert_eq!(active.len(), 4); // 1 cg128 × 4
        assert_eq!(pos.len(), 4);
        assert!(active.iter().all(|&v| v == 0));
        assert!(pos.iter().all(|&v| v == 0));
    }

    #[test]
    fn pack_inputs_nda_ternary_codes() {
        // Encode: code 01 (positive ternary +1) in first 2-bit pair
        // val_0 = 0x01 means code_0 at bit 0 is 01 → active bit set, pos bit set
        let mut input = vec![0u32; 8];
        input[0] = 0x01; // first 2-bit pair = 01 (positive)
        let (active, pos) = pack_inputs_nda(&input);
        assert_eq!(active.len(), 4);
        // bit 0 of active[0] should be set (code != 0)
        assert_ne!(active[0] & 0x01, 0);
        // bit 0 of pos[0] should be set (code == 1)
        assert_ne!(pos[0] & 0x01, 0);
    }

    #[test]
    fn pack_inputs_nda_negative_code() {
        // code 10 (negative ternary -1) → active set, pos NOT set
        let mut input = vec![0u32; 8];
        input[0] = 0x02; // first 2-bit pair = 10 (negative)
        let (active, pos) = pack_inputs_nda(&input);
        // active should have bit set
        assert_ne!(active[0] & 0x01, 0);
        // pos should NOT have bit set (code != 1)
        assert_eq!(pos[0] & 0x01, 0);
    }

    // ─── Batch packing tests ───────────────────────────────────────────────

    #[test]
    fn batch_pack_uvec4_empty() {
        let items: Vec<(Vec<u8>, usize, usize)> = vec![];
        let (results, report) = pack_weights_uvec4_batch(&items);
        assert!(results.is_empty());
        assert_eq!(report.operations, 0);
        assert!(report.all_valid);
    }

    #[test]
    fn batch_pack_uvec4_multiple() {
        let items = vec![
            (vec![0u8; 64], 64, 4),  // valid
            (vec![0u8; 64], 64, 4),  // valid
        ];
        let (results, report) = pack_weights_uvec4_batch(&items);
        assert_eq!(results.len(), 2);
        assert_eq!(report.operations, 2);
        assert!(report.all_valid);
        assert_eq!(report.total_input_bytes, 128);
    }

    #[test]
    fn batch_pack_uvec4_with_invalid() {
        let items = vec![
            (vec![0u8; 64], 64, 4),  // valid
            (vec![0u8; 5], 64, 4),   // invalid: bad alignment
        ];
        let (results, report) = pack_weights_uvec4_batch(&items);
        assert_eq!(results.len(), 2);
        assert!(!report.all_valid);
        assert!(!report.issues.is_empty());
    }

    // ─── Diagnostic struct tests ───────────────────────────────────────────

    #[test]
    fn packing_report_serializes() {
        let report = PackingReport {
            operation: "pack_weights_uvec4".to_string(),
            input_bytes: 1024,
            output_bytes: 1024,
            elapsed_us: 42,
            validation_issues: vec![],
            valid: true,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"valid\":true"));
        assert!(json.contains("\"input_bytes\":1024"));
        assert!(json.contains("\"operation\":\"pack_weights_uvec4\""));
    }

    #[test]
    fn batch_packing_report_serializes() {
        let report = BatchPackingReport {
            operations: 26,
            total_input_bytes: 1_500_000,
            total_output_bytes: 1_500_000,
            total_elapsed_us: 5000,
            per_op_avg_us: 192.3,
            all_valid: true,
            issues: vec![],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"operations\":26"));
        assert!(json.contains("\"all_valid\":true"));
    }

    // ─── New diagnostic tests ─────────────────────────────────────────────

    #[test]
    fn packing_summary_from_valid_batch() {
        let items = vec![
            (vec![0u8; 64], 64, 4),
            (vec![0u8; 64], 64, 4),
        ];
        let (_, report) = pack_weights_uvec4_batch(&items);
        let summary = report.summary();
        assert_eq!(summary.total_ops, 2);
        assert_eq!(summary.valid_ops, 2);
        assert_eq!(summary.invalid_ops, 0);
        assert_eq!(summary.total_issues, 0);
        assert!(summary.compression_ratio > 0.0);
    }

    #[test]
    fn packing_summary_from_empty_batch() {
        let items: Vec<(Vec<u8>, usize, usize)> = vec![];
        let (_, report) = pack_weights_uvec4_batch(&items);
        let summary = report.summary();
        assert_eq!(summary.total_ops, 0);
        assert_eq!(summary.compression_ratio, 0.0);
    }

    #[test]
    fn validate_inputs_nda_ok() {
        let data = vec![0u32; 8];
        assert!(validate_inputs_nda(&data, "test").is_empty());
    }

    #[test]
    fn validate_inputs_nda_empty() {
        let data: Vec<u32> = vec![];
        let issues = validate_inputs_nda(&data, "test");
        assert!(issues.iter().any(|i| i.contains("empty")));
    }

    #[test]
    fn validate_inputs_nda_bad_length() {
        let data = vec![0u32; 7]; // not multiple of 8
        let issues = validate_inputs_nda(&data, "test");
        assert!(issues.iter().any(|i| i.contains("multiple of 8")));
    }

    #[test]
    fn pack_inputs_nda_report_valid() {
        let input = vec![0u32; 8];
        let ((act, pos), report) = pack_inputs_nda_report(&input);
        assert!(report.valid);
        assert_eq!(report.operation, "pack_inputs_nda");
        assert_eq!(report.input_bytes, 32);
        assert_eq!(act.len(), 4);
        assert_eq!(pos.len(), 4);
    }

    #[test]
    fn pack_inputs_nda_report_invalid() {
        let input = vec![0u32; 3]; // bad length
        let ((act, pos), report) = pack_inputs_nda_report(&input);
        assert!(!report.valid);
        assert!(act.is_empty());
        assert!(pos.is_empty());
    }

    #[test]
    fn packing_summary_serializes() {
        let summary = PackingSummary {
            total_ops: 10,
            valid_ops: 10,
            invalid_ops: 0,
            compression_ratio: 1.0,
            total_issues: 0,
            heaviest_op: Some("layer_0_q_proj".to_string()),
            heaviest_op_bytes: 4096,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("compression_ratio"));
        assert!(json.contains("layer_0_q_proj"));
    }

    // ── validate_u32_alignment: edge cases ────────────────────────────────

    #[test]
    fn alignment_issue_text_contains_context() {
        let data = vec![0u8; 5];
        let issues = validate_u32_alignment(&data, "my_context");
        assert!(issues[0].contains("my_context"));
    }

    #[test]
    fn alignment_length_one() {
        let data = vec![0u8; 1];
        let issues = validate_u32_alignment(&data, "test");
        assert!(issues.iter().any(|i| i.contains("1")));
    }

    #[test]
    fn alignment_length_three() {
        let data = vec![0u8; 3];
        let issues = validate_u32_alignment(&data, "test");
        assert!(!issues.is_empty());
    }

    #[test]
    fn alignment_length_eight_ok() {
        let data = vec![0u8; 8];
        let issues = validate_u32_alignment(&data, "test");
        assert!(issues.is_empty());
    }

    // ── validate_pack_dims: edge cases ────────────────────────────────────

    #[test]
    fn pack_dims_both_zero() {
        let issues = validate_pack_dims(0, 0, 0, "ctx");
        assert!(issues.iter().any(|i| i.contains("k dimension")));
        assert!(issues.iter().any(|i| i.contains("n dimension")));
    }

    #[test]
    fn pack_dims_issue_text_contains_context() {
        let issues = validate_pack_dims(0, 4, 16, "my_fn");
        assert!(issues[0].contains("my_fn"));
    }

    #[test]
    fn pack_dims_exact_data_match() {
        // k=16, n=1: need (16/16)*1 = 1 word = 4 bytes
        let issues = validate_pack_dims(16, 1, 4, "test");
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn pack_dims_excess_data_ok() {
        // k=16, n=1: need 1 word = 4 bytes, but give 100
        let issues = validate_pack_dims(16, 1, 100, "test");
        assert!(issues.is_empty());
    }

    #[test]
    fn pack_dims_k_zero_also_not_multiple() {
        // k=0 triggers "k is zero"; 0%16==0 so no "not multiple" issue
        let issues = validate_pack_dims(0, 4, 16, "test");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("k dimension is zero"));
    }

    #[test]
    fn pack_dims_multiple_issues() {
        // k=0, n=0 → 2 issues; k%16==0 (0%16==0), data check: 0 words < 0 words → false
        let issues = validate_pack_dims(0, 0, 0, "test");
        assert_eq!(issues.len(), 2);
    }

    // ── validate_nda_pack_dims: edge cases ───────────────────────────────

    #[test]
    fn nda_pack_dims_both_zero() {
        let issues = validate_nda_pack_dims(0, 0, 0, "ctx");
        assert!(issues.iter().any(|i| i.contains("k dimension")));
        assert!(issues.iter().any(|i| i.contains("n dimension")));
    }

    #[test]
    fn nda_pack_dims_k_zero_is_multiple_of_128() {
        // 0 % 128 == 0, so no "not a multiple" issue
        let issues = validate_nda_pack_dims(0, 4, 16, "test");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("k dimension is zero"));
    }

    #[test]
    fn nda_pack_dims_issue_text_contains_context() {
        let issues = validate_nda_pack_dims(64, 2, 64, "my_op");
        assert!(issues[0].contains("my_op"));
    }

    #[test]
    fn nda_pack_dims_exact_data_ok() {
        // k=128, n=1: need (128/16)*1 = 8 words = 32 bytes
        let issues = validate_nda_pack_dims(128, 1, 32, "test");
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn nda_pack_dims_excess_data_ok() {
        let issues = validate_nda_pack_dims(128, 1, 1000, "test");
        assert!(issues.is_empty());
    }

    // ── validate_inputs_nda: edge cases ──────────────────────────────────

    #[test]
    fn inputs_nda_empty_also_not_multiple() {
        // Empty → "empty" issue; 0%8==0 so no "multiple" issue
        let issues = validate_inputs_nda(&[], "ctx");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("empty"));
    }

    #[test]
    fn inputs_nda_issue_text_contains_context() {
        let issues = validate_inputs_nda(&[], "my_fn");
        assert!(issues[0].contains("my_fn"));
    }

    #[test]
    fn inputs_nda_length_sixteen_ok() {
        let data = vec![0u32; 16];
        assert!(validate_inputs_nda(&data, "test").is_empty());
    }

    // ── PackingReport: struct derives ────────────────────────────────────

    #[test]
    fn packing_report_clone_is_independent() {
        let report = PackingReport {
            operation: "test".into(), input_bytes: 100, output_bytes: 50,
            elapsed_us: 10, validation_issues: vec![], valid: true,
        };
        let mut cloned = report.clone();
        cloned.input_bytes = 999;
        assert_eq!(report.input_bytes, 100);
    }

    #[test]
    fn packing_report_debug_format() {
        let report = PackingReport {
            operation: "test".into(), input_bytes: 100, output_bytes: 50,
            elapsed_us: 10, validation_issues: vec![], valid: true,
        };
        let debug = format!("{:?}", report);
        assert!(debug.contains("operation"));
        assert!(debug.contains("input_bytes"));
    }

    #[test]
    fn packing_report_json_all_fields() {
        let report = PackingReport {
            operation: "op".into(), input_bytes: 64, output_bytes: 32,
            elapsed_us: 5, validation_issues: vec!["issue1".into()], valid: false,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("operation"));
        assert!(json.contains("input_bytes"));
        assert!(json.contains("output_bytes"));
        assert!(json.contains("elapsed_us"));
        assert!(json.contains("validation_issues"));
        assert!(json.contains("valid"));
    }

    // ── BatchPackingReport: struct derives ───────────────────────────────

    #[test]
    fn batch_report_clone_is_independent() {
        let report = BatchPackingReport {
            operations: 5, total_input_bytes: 500, total_output_bytes: 250,
            total_elapsed_us: 100, per_op_avg_us: 20.0,
            all_valid: true, issues: vec![],
        };
        let mut cloned = report.clone();
        cloned.operations = 999;
        assert_eq!(report.operations, 5);
    }

    #[test]
    fn batch_report_debug_format() {
        let report = BatchPackingReport {
            operations: 3, total_input_bytes: 300, total_output_bytes: 150,
            total_elapsed_us: 50, per_op_avg_us: 16.7,
            all_valid: true, issues: vec![],
        };
        let debug = format!("{:?}", report);
        assert!(debug.contains("operations"));
        assert!(debug.contains("total_input_bytes"));
    }

    #[test]
    fn batch_report_json_all_fields() {
        let report = BatchPackingReport {
            operations: 10, total_input_bytes: 1000, total_output_bytes: 500,
            total_elapsed_us: 200, per_op_avg_us: 20.0,
            all_valid: false, issues: vec!["err".into()],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("operations"));
        assert!(json.contains("total_input_bytes"));
        assert!(json.contains("total_output_bytes"));
        assert!(json.contains("per_op_avg_us"));
        assert!(json.contains("all_valid"));
        assert!(json.contains("issues"));
    }

    // ── BatchPackingReport::summary ──────────────────────────────────────

    #[test]
    fn summary_compression_ratio_calculation() {
        let report = BatchPackingReport {
            operations: 1, total_input_bytes: 1000, total_output_bytes: 250,
            total_elapsed_us: 10, per_op_avg_us: 10.0,
            all_valid: true, issues: vec![],
        };
        let summary = report.summary();
        assert!((summary.compression_ratio - 0.25).abs() < 0.01);
    }

    #[test]
    fn summary_with_issues_shows_invalid() {
        let report = BatchPackingReport {
            operations: 2, total_input_bytes: 100, total_output_bytes: 50,
            total_elapsed_us: 10, per_op_avg_us: 5.0,
            all_valid: false, issues: vec!["err1".into(), "err2".into()],
        };
        let summary = report.summary();
        assert_eq!(summary.valid_ops, 0);
        assert_eq!(summary.invalid_ops, 2);
        assert_eq!(summary.total_issues, 2);
    }

    #[test]
    fn summary_heaviest_op_is_none_by_default() {
        let report = BatchPackingReport {
            operations: 1, total_input_bytes: 100, total_output_bytes: 50,
            total_elapsed_us: 10, per_op_avg_us: 10.0,
            all_valid: true, issues: vec![],
        };
        let summary = report.summary();
        assert!(summary.heaviest_op.is_none());
        assert_eq!(summary.heaviest_op_bytes, 0);
    }

    // ── PackingSummary: struct derives ───────────────────────────────────

    #[test]
    fn packing_summary_clone_is_independent() {
        let summary = PackingSummary {
            total_ops: 5, valid_ops: 5, invalid_ops: 0,
            compression_ratio: 0.5, total_issues: 0,
            heaviest_op: Some("op".into()), heaviest_op_bytes: 100,
        };
        let mut cloned = summary.clone();
        cloned.total_ops = 999;
        assert_eq!(summary.total_ops, 5);
    }

    #[test]
    fn packing_summary_json_all_fields() {
        let summary = PackingSummary {
            total_ops: 10, valid_ops: 8, invalid_ops: 2,
            compression_ratio: 0.75, total_issues: 3,
            heaviest_op: Some("layer_0".into()), heaviest_op_bytes: 2048,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("total_ops"));
        assert!(json.contains("valid_ops"));
        assert!(json.contains("invalid_ops"));
        assert!(json.contains("compression_ratio"));
        assert!(json.contains("total_issues"));
        assert!(json.contains("heaviest_op"));
        assert!(json.contains("heaviest_op_bytes"));
    }

    // ── pack_weights_uvec4_report: detailed checks ───────────────────────

    #[test]
    fn pack_uvec4_report_output_bytes_matches() {
        let input = vec![0u8; 64];
        let (result, report) = pack_weights_uvec4_report(&input, 64, 4);
        assert_eq!(report.output_bytes, result.len());
    }

    #[test]
    fn pack_uvec4_report_invalid_has_zero_output() {
        let input = vec![0u8; 5]; // bad alignment
        let (result, report) = pack_weights_uvec4_report(&input, 64, 4);
        assert_eq!(report.output_bytes, 0);
        assert!(!report.valid);
    }

    // ── pack_weights_nda_report: detailed checks ─────────────────────────

    #[test]
    fn pack_nda_report_output_bytes_calculation() {
        let input = vec![0u8; 32]; // k=128, n=1
        let ((act, pos), report) = pack_weights_nda_report(&input, 128, 1);
        assert_eq!(report.output_bytes, act.len() + pos.len());
    }

    #[test]
    fn pack_nda_report_invalid_has_zero_output() {
        let input = vec![0u8; 10];
        let ((act, pos), report) = pack_weights_nda_report(&input, 128, 1);
        assert_eq!(report.output_bytes, 0);
        assert!(!report.valid);
    }

    // ── pack_inputs_nda_report: detailed checks ──────────────────────────

    #[test]
    fn pack_inputs_nda_report_output_bytes_calculation() {
        let input = vec![0u32; 8];
        let ((act, pos), report) = pack_inputs_nda_report(&input);
        assert_eq!(report.output_bytes, (act.len() + pos.len()) * 4);
    }

    #[test]
    fn pack_inputs_nda_report_invalid_empty() {
        let input: Vec<u32> = vec![];
        let ((act, pos), report) = pack_inputs_nda_report(&input);
        assert!(!report.valid);
        assert!(act.is_empty());
        assert!(pos.is_empty());
    }

    // ── batch pack: single item ──────────────────────────────────────────

    #[test]
    fn batch_pack_single_item() {
        let items = vec![(vec![0u8; 64], 64, 4)];
        let (results, report) = pack_weights_uvec4_batch(&items);
        assert_eq!(results.len(), 1);
        assert_eq!(report.operations, 1);
        assert!(report.all_valid);
        assert_eq!(report.total_input_bytes, 64);
    }
}
