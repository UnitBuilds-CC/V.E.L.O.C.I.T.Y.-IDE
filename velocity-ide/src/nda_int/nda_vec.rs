#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NdaVec {
    pub len: usize,
    pub log2_scale: i8,
    pub sign: std::sync::Arc<[u8]>,
    pub extra: std::sync::Arc<[u8]>,
}

impl NdaVec {
    #[allow(dead_code)]
    pub fn zeros(len: usize, log2_scale: i8) -> Self {
        let bytes = len.div_ceil(8);
        Self {
            len,
            log2_scale,
            sign: vec![0xFF; bytes].into(),
            extra: vec![0x00; bytes].into(),
        }
    }

    pub fn from_f32_slice(x: &[f32]) -> Self {
        let (sign, extra, scale) = crate::nda::quantize_activations_v2_quad(x);
        let log2_scale = scale.log2().round() as i8;
        Self {
            len: x.len(),
            log2_scale,
            sign: sign.into(),
            extra: extra.into(),
        }
    }

    #[allow(clippy::needless_range_loop)]
    pub fn to_f32_vec(&self) -> Vec<f32> {
        let scale = 2.0f32.powi(self.log2_scale as i32);
        let mut out = vec![0.0f32; self.len];
        for i in 0..self.len {
            out[i] = (self.get_raw(i) as f32) * scale;
        }
        out
    }

    pub fn from_i32_slice(data: &[i32], log2_scale: i8) -> Self {
        let len = data.len();
        let bytes = len.div_ceil(8);
        let mut sign = vec![0u8; bytes];
        let mut extra = vec![0u8; bytes];

        const ENCODE_TABLE: [u8; 9] = [0, 0, 0, 1, 2, 2, 3, 3, 3];

        for (i, &v) in data.iter().enumerate() {
            let clamped = v.clamp(-4, 4);
            let enc = ENCODE_TABLE[(clamped + 4) as usize];

            let byte_idx = i / 8;
            let bit_idx = i % 8;

            sign[byte_idx] |= ((enc >> 1) & 1) << bit_idx;
            extra[byte_idx] |= (enc & 1) << bit_idx;
        }

        Self {
            len,
            log2_scale,
            sign: sign.into(),
            extra: extra.into(),
        }
    }

    #[inline]
    pub fn get_raw(&self, i: usize) -> i32 {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        let mask = 1u8 << bit_idx;
        let is_pos = (self.sign[byte_idx] & mask) != 0;
        let is_large = (self.sign[byte_idx] & mask) == (self.extra[byte_idx] & mask);
        let mag = if is_large { 2i32 } else { 1 };
        if is_pos {
            mag
        } else {
            -mag
        }
    }

    #[inline]
    pub fn bitmap_bytes(&self) -> usize {
        self.len.div_ceil(8)
    }

    /// Count the number of each raw value in the vector: [count(-2), count(-1), count(1), count(2)].
    pub fn value_histogram(&self) -> [usize; 4] {
        let mut hist = [0usize; 4]; // [-2, -1, 1, 2]
        for i in 0..self.len {
            let raw = self.get_raw(i);
            match raw {
                -2 => hist[0] += 1,
                -1 => hist[1] += 1,
                1 => hist[2] += 1,
                2 => hist[3] += 1,
                _ => {}
            }
        }
        hist
    }

    /// Compute the dot product of two NdaVecs as i32 (ignoring scale).
    pub fn dot_raw(&self, other: &NdaVec) -> i32 {
        debug_assert_eq!(self.len, other.len);
        let mut sum = 0i32;
        for i in 0..self.len {
            sum += self.get_raw(i) * other.get_raw(i);
        }
        sum
    }

    /// Hamming distance: number of positions where the sign bitmaps differ.
    pub fn sign_hamming_distance(&self, other: &NdaVec) -> usize {
        debug_assert_eq!(self.len, other.len);
        let bytes = self.bitmap_bytes();
        let mut dist = 0usize;
        for b in 0..bytes {
            let xor = self.sign[b] ^ other.sign[b];
            dist += xor.count_ones() as usize;
        }
        // Adjust for padding bits in the last byte
        let excess = bytes * 8 - self.len;
        if excess > 0 {
            let last_xor = self.sign[bytes - 1] ^ other.sign[bytes - 1];
            let pad_mask = ((1u16 << excess) - 1) as u8;
            dist -= (last_xor & pad_mask).count_ones() as usize;
        }
        dist
    }

    /// Memory footprint in bytes (sign + extra bitmaps).
    pub fn memory_bytes(&self) -> usize {
        self.bitmap_bytes() * 2
    }

    /// Validate the NdaVec for consistency.
    /// Returns a list of warnings (empty = all good).
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.len == 0 {
            warnings.push("NdaVec has zero length".to_string());
        }

        let expected_bytes = self.len.div_ceil(8);
        if self.sign.len() != expected_bytes {
            warnings.push(format!(
                "sign bitmap size mismatch: expected {} bytes, got {}",
                expected_bytes,
                self.sign.len()
            ));
        }
        if self.extra.len() != expected_bytes {
            warnings.push(format!(
                "extra bitmap size mismatch: expected {} bytes, got {}",
                expected_bytes,
                self.extra.len()
            ));
        }

        warnings
    }

    /// Return diagnostic information about this NdaVec.
    pub fn info(&self) -> NdaVecInfo {
        let hist = self.value_histogram();
        NdaVecInfo {
            len: self.len,
            log2_scale: self.log2_scale,
            memory_bytes: self.memory_bytes(),
            bits_per_element: 2.0,
            value_histogram: hist,
            unique_values: hist.iter().filter(|&&c| c > 0).count(),
        }
    }
}

/// Diagnostic information about an NdaVec.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NdaVecInfo {
    pub len: usize,
    pub log2_scale: i8,
    pub memory_bytes: usize,
    pub bits_per_element: f32,
    pub value_histogram: [usize; 4],
    pub unique_values: usize,
}

/// Report on f32 → NdaVec conversion accuracy.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NdaVecConversionReport {
    pub input_len: usize,
    pub output_log2_scale: i8,
    pub memory_bytes: usize,
    pub compression_ratio: f64,
    pub max_abs_error: f64,
    pub mean_abs_error: f64,
    pub validation_issues: Vec<String>,
}

/// Convert an f32 slice to NdaVec and produce a conversion accuracy report.
pub fn from_f32_slice_report(x: &[f32]) -> (NdaVec, NdaVecConversionReport) {
    let nv = NdaVec::from_f32_slice(x);
    let roundtrip = nv.to_f32_vec();

    let mut max_abs_error = 0.0f64;
    let mut sum_abs_error = 0.0f64;
    for (i, &orig) in x.iter().enumerate() {
        let err = (orig as f64 - roundtrip[i] as f64).abs();
        max_abs_error = max_abs_error.max(err);
        sum_abs_error += err;
    }
    let mean_abs_error = if x.is_empty() { 0.0 } else { sum_abs_error / x.len() as f64 };
    let input_bytes = x.len() * 4;
    let compression_ratio = if nv.memory_bytes() > 0 {
        input_bytes as f64 / nv.memory_bytes() as f64
    } else {
        0.0
    };

    let mut issues = Vec::new();
    if x.is_empty() {
        issues.push("input slice is empty".into());
    }
    if max_abs_error > 1.0 {
        issues.push(format!("max abs error {:.4} exceeds 1.0", max_abs_error));
    }

    let report = NdaVecConversionReport {
        input_len: x.len(),
        output_log2_scale: nv.log2_scale,
        memory_bytes: nv.memory_bytes(),
        compression_ratio,
        max_abs_error,
        mean_abs_error,
        validation_issues: issues,
    };
    (nv, report)
}

/// Batch convert multiple f32 slices to NdaVecs with a summary report.
pub fn batch_from_f32(vecs: &[Vec<f32>]) -> (Vec<NdaVec>, NdaVecConversionReport) {
    let mut all_nv = Vec::with_capacity(vecs.len());
    let mut total_max_err = 0.0f64;
    let mut total_sum_err = 0.0f64;
    let mut total_elements = 0usize;

    for v in vecs {
        let (nv, report) = from_f32_slice_report(v);
        total_max_err = total_max_err.max(report.max_abs_error);
        total_sum_err += report.mean_abs_error * v.len() as f64;
        total_elements += v.len();
        all_nv.push(nv);
    }

    let mean_abs_error = if total_elements > 0 {
        total_sum_err / total_elements as f64
    } else {
        0.0
    };

    let total_input_bytes: usize = vecs.iter().map(|v| v.len() * 4).sum();
    let total_output_bytes: usize = all_nv.iter().map(|nv| nv.memory_bytes()).sum();
    let compression_ratio = if total_output_bytes > 0 {
        total_input_bytes as f64 / total_output_bytes as f64
    } else {
        0.0
    };

    let mut issues = Vec::new();
    if vecs.is_empty() {
        issues.push("no vectors to convert".into());
    }

    let report = NdaVecConversionReport {
        input_len: total_elements,
        output_log2_scale: 0,
        memory_bytes: total_output_bytes,
        compression_ratio,
        max_abs_error: total_max_err,
        mean_abs_error,
        validation_issues: issues,
    };
    (all_nv, report)
}

/// Validate that two NdaVecs are compatible for dot product.
pub fn validate_dot_product_params(a: &NdaVec, b: &NdaVec) -> Vec<String> {
    let mut issues = Vec::new();
    if a.len != b.len {
        issues.push(format!("length mismatch: {} vs {}", a.len, b.len));
    }
    if a.len == 0 {
        issues.push("vector length is 0".into());
    }
    issues.extend(a.validate());
    issues.extend(b.validate());
    issues
}

#[inline]
pub fn combine_log2_scales(a: i8, b: i8) -> i8 {
    a.saturating_add(b)
}

#[inline]
pub fn div_pow2_i32(v: i32, shift: u32) -> i32 {
    if shift == 0 {
        v
    } else if shift >= 31 {
        0
    } else {
        v / (1i32 << shift)
    }
}

#[inline]
pub fn div_pow2_i64(v: i64, shift: u32) -> i64 {
    if shift == 0 {
        v
    } else if shift >= 63 {
        0
    } else {
        v / (1i64 << shift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nda_vec_value_histogram() {
        let v = NdaVec::from_i32_slice(&[1, -1, 2, -2, 1, 0], 0);
        let hist = v.value_histogram();
        // -2: 1, -1: 1, 1: 2, 2: 1 (0 encodes to sign=0,extra=0 → raw=-2 or -1)
        assert_eq!(hist.iter().sum::<usize>(), v.len);
    }

    #[test]
    fn nda_vec_dot_raw() {
        let a = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let b = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let dot = a.dot_raw(&b);
        // All same sign → all positive products
        assert!(dot > 0);
    }

    #[test]
    fn nda_vec_dot_raw_orthogonal() {
        let a = NdaVec::from_i32_slice(&[1, 1, 1, 1], 0);
        let b = NdaVec::from_i32_slice(&[-1, -1, -1, -1], 0);
        let dot = a.dot_raw(&b);
        assert!(dot < 0);
    }

    #[test]
    fn nda_vec_sign_hamming_distance() {
        let a = NdaVec::from_i32_slice(&[1, 1, 1, 1, 1, 1, 1, 1], 0);
        let b = NdaVec::from_i32_slice(&[1, 1, 1, 1, -1, -1, -1, -1], 0);
        let dist = a.sign_hamming_distance(&b);
        assert_eq!(dist, 4); // last 4 bits differ
    }

    #[test]
    fn nda_vec_sign_hamming_distance_identical() {
        let a = NdaVec::from_i32_slice(&[1, -1, 2, -2], 0);
        let dist = a.sign_hamming_distance(&a);
        assert_eq!(dist, 0);
    }

    #[test]
    fn nda_vec_memory_bytes() {
        let v = NdaVec::from_i32_slice(&[1, 2, 3, 4, 5, 6, 7, 8], 0);
        assert_eq!(v.memory_bytes(), 2); // 1 byte sign + 1 byte extra
    }

    #[test]
    fn nda_vec_memory_bytes_non_aligned() {
        let v = NdaVec::from_i32_slice(&[1, 2, 3], 0);
        assert_eq!(v.memory_bytes(), 2); // ceil(3/8)=1 byte each
    }

    #[test]
    fn combine_log2_scales_basic() {
        assert_eq!(combine_log2_scales(3, 5), 8);
        assert_eq!(combine_log2_scales(-3, 5), 2);
    }

    #[test]
    fn combine_log2_scales_saturating() {
        assert_eq!(combine_log2_scales(100, 100), i8::MAX);
        assert_eq!(combine_log2_scales(-100, -100), i8::MIN);
    }

    #[test]
    fn div_pow2_i32_basic() {
        assert_eq!(div_pow2_i32(16, 2), 4);
        assert_eq!(div_pow2_i32(16, 0), 16);
        assert_eq!(div_pow2_i32(16, 31), 0);
    }

    #[test]
    fn div_pow2_i64_basic() {
        assert_eq!(div_pow2_i64(1024, 3), 128);
        assert_eq!(div_pow2_i64(1024, 0), 1024);
        assert_eq!(div_pow2_i64(1024, 63), 0);
    }

    #[test]
    fn nda_vec_validate_clean() {
        let v = NdaVec::from_i32_slice(&[1, -1, 2, -2], 0);
        let w = v.validate();
        assert!(w.is_empty(), "expected no warnings, got: {:?}", w);
    }

    #[test]
    fn nda_vec_validate_zero_length() {
        let v = NdaVec {
            len: 0,
            log2_scale: 0,
            sign: vec![].into(),
            extra: vec![].into(),
        };
        let w = v.validate();
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("zero length"));
    }

    #[test]
    fn nda_vec_validate_bitmap_mismatch() {
        let v = NdaVec {
            len: 16, // expects 2 bytes
            log2_scale: 0,
            sign: vec![0xFF; 3].into(), // wrong: 3 bytes
            extra: vec![0x00; 2].into(),
        };
        let w = v.validate();
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("sign bitmap size mismatch"));
    }

    #[test]
    fn nda_vec_info_basic() {
        let v = NdaVec::from_i32_slice(&[1, -1, 2, -2, 1], 3);
        let info = v.info();
        assert_eq!(info.len, 5);
        assert_eq!(info.log2_scale, 3);
        assert_eq!(info.memory_bytes, 2); // ceil(5/8)=1 byte * 2
        assert_eq!(info.bits_per_element, 2.0);
        assert!(info.unique_values > 0);
        assert_eq!(info.value_histogram.iter().sum::<usize>(), 5);
    }

    #[test]
    fn nda_vec_info_serializes() {
        let v = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let info = v.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"len\":4"));
        assert!(json.contains("\"bits_per_element\":2.0"));
    }

    #[test]
    fn from_f32_slice_report_basic() {
        let input = vec![1.0, -1.0, 0.5, -0.5, 2.0, -2.0, 0.0, 0.1];
        let (nv, report) = from_f32_slice_report(&input);
        assert_eq!(report.input_len, 8);
        assert_eq!(nv.len, 8);
        assert!(report.memory_bytes > 0);
        assert!(report.compression_ratio > 1.0);
        assert!(report.validation_issues.is_empty());
    }

    #[test]
    fn from_f32_slice_report_empty() {
        let input: Vec<f32> = vec![];
        let (_, report) = from_f32_slice_report(&input);
        assert_eq!(report.input_len, 0);
        assert!(!report.validation_issues.is_empty());
    }

    #[test]
    fn from_f32_slice_report_serializes() {
        let input = vec![1.0, -1.0, 2.0, -2.0];
        let (_, report) = from_f32_slice_report(&input);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"input_len\":4"));
        assert!(json.contains("\"compression_ratio\""));
    }

    #[test]
    fn batch_from_f32_basic() {
        let vecs = vec![
            vec![1.0, -1.0, 0.5, -0.5],
            vec![2.0, -2.0, 0.0, 0.1],
            vec![-0.3, 0.3, 1.5, -1.5],
        ];
        let (nvs, report) = batch_from_f32(&vecs);
        assert_eq!(nvs.len(), 3);
        assert_eq!(report.input_len, 12);
        assert!(report.compression_ratio > 1.0);
    }

    #[test]
    fn batch_from_f32_empty() {
        let (nvs, report) = batch_from_f32(&[]);
        assert!(nvs.is_empty());
        assert!(!report.validation_issues.is_empty());
    }

    #[test]
    fn validate_dot_product_params_valid() {
        let a = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let b = NdaVec::from_i32_slice(&[1, -1, 2, -2], 0);
        let issues = validate_dot_product_params(&a, &b);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_dot_product_params_length_mismatch() {
        let a = NdaVec::from_i32_slice(&[1, 2, 3, 4], 0);
        let b = NdaVec::from_i32_slice(&[1, -1], 0);
        let issues = validate_dot_product_params(&a, &b);
        assert!(issues.iter().any(|i| i.contains("mismatch")));
    }

    #[test]
    fn validate_dot_product_params_zero_length() {
        let a = NdaVec { len: 0, log2_scale: 0, sign: vec![].into(), extra: vec![].into() };
        let b = NdaVec { len: 0, log2_scale: 0, sign: vec![].into(), extra: vec![].into() };
        let issues = validate_dot_product_params(&a, &b);
        assert!(issues.iter().any(|i| i.contains("0")));
    }
}
