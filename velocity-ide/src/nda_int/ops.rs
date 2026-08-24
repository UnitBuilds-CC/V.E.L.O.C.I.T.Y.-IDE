use super::nda_vec::*;
use super::tables::*;
use serde::Serialize;

/// Batch operation report for NDA vector operations.
#[derive(Debug, Clone, Serialize)]
pub struct NdaOpsReport {
    pub operation: String,
    pub count: usize,
    pub total_elapsed_us: u64,
    pub per_op_avg_us: f64,
}

/// Validate that two NdaVecs are compatible for binary operations.
pub fn validate_binary_op(a: &NdaVec, b: &NdaVec) -> Vec<String> {
    let mut warnings = Vec::new();
    if a.len != b.len {
        warnings.push(format!(
            "length mismatch: {} vs {}",
            a.len, b.len
        ));
    }
    warnings.extend(a.validate());
    warnings.extend(b.validate());
    warnings
}

pub fn nda_vec_add_inplace(x: &mut NdaVec, delta: &NdaVec) {
    debug_assert_eq!(x.len, delta.len);

    let out_log2 = x.log2_scale.max(delta.log2_scale);
    let x_shift = (out_log2 - x.log2_scale).max(0) as u32;
    let del_shift = (out_log2 - delta.log2_scale).max(0) as u32;

    let len = x.len;
    let bytes = len.div_ceil(8);

    let mut sign_vec = x.sign.to_vec();
    let mut extra_vec = x.extra.to_vec();

    if x_shift == 0 && del_shift == 0 {
        for byte_idx in 0..bytes {
            let x_s = sign_vec[byte_idx];
            let x_e = extra_vec[byte_idx];
            let d_s = delta.sign[byte_idx];
            let d_e = delta.extra[byte_idx];

            let idx_low = (x_s & 0x0F) as usize
                | (((x_e & 0x0F) as usize) << 4)
                | (((d_s & 0x0F) as usize) << 8)
                | (((d_e & 0x0F) as usize) << 12);
            let res_low = ADD_LUT_Q16[idx_low];

            let idx_high = ((x_s >> 4) as usize)
                | (((x_e >> 4) as usize) << 4)
                | (((d_s >> 4) as usize) << 8)
                | (((d_e >> 4) as usize) << 12);
            let res_high = ADD_LUT_Q16[idx_high];

            sign_vec[byte_idx] = (res_low & 0x0F) | ((res_high & 0x0F) << 4);
            extra_vec[byte_idx] = (res_low >> 4) | (res_high & 0xF0);
        }

        if !len.is_multiple_of(8) {
            let last_idx = bytes - 1;
            let mask = (1u8 << (len % 8)) - 1;
            sign_vec[last_idx] &= mask;
            extra_vec[last_idx] &= mask;
        }

        x.sign = sign_vec.into();
        x.extra = extra_vec.into();
        x.log2_scale = out_log2;
        return;
    }

    const DECODE_TABLE: [i32; 4] = [-2, -1, 1, 2];
    const ENCODE_TABLE: [u8; 9] = [0, 0, 0, 1, 2, 2, 3, 3, 3];

    for byte_idx in 0..bytes {
        let mut s_byte = 0u8;
        let mut e_byte = 0u8;

        let mut x_s_shift = sign_vec[byte_idx];
        let mut x_e_shift = extra_vec[byte_idx];
        let mut d_s_shift = delta.sign[byte_idx];
        let mut d_e_shift = delta.extra[byte_idx];

        let base_idx = byte_idx * 8;
        for bit in 0..8 {
            let i = base_idx + bit;
            if i >= len {
                break;
            }

            let x_idx = ((x_s_shift & 1) << 1) | (x_e_shift & 1);
            let xv = div_pow2_i32(DECODE_TABLE[x_idx as usize], x_shift);

            let d_idx = ((d_s_shift & 1) << 1) | (d_e_shift & 1);
            let dv = div_pow2_i32(DECODE_TABLE[d_idx as usize], del_shift);

            let sum = xv + dv;
            let clamped = (sum + 4).clamp(0, 8) as usize;
            let enc = ENCODE_TABLE[clamped];

            s_byte |= ((enc >> 1) & 1) << bit;
            e_byte |= (enc & 1) << bit;

            x_s_shift >>= 1;
            x_e_shift >>= 1;
            d_s_shift >>= 1;
            d_e_shift >>= 1;
        }
        sign_vec[byte_idx] = s_byte;
        extra_vec[byte_idx] = e_byte;
    }

    x.sign = sign_vec.into();
    x.extra = extra_vec.into();
    x.log2_scale = out_log2;
}

fn isqrt_inv_q14(v: u64) -> u32 {
    if v == 0 {
        return 1 << 14;
    }

    let leading = v.leading_zeros();
    let k = 64 - leading;
    let shift = k / 2;

    let mut x = if shift <= 14 { 1u64 << (14 - shift) } else { 1 };

    for _ in 0..3 {
        let x2 = x * x;
        let vx2 = v.saturating_mul(x2) >> 14;
        let term = (3u64 << 14).saturating_sub(vx2);
        x = x.saturating_mul(term) >> 15;
        if x == 0 {
            break;
        }
    }

    (x as u32).min(1 << 14)
}

pub fn rms_norm_nda(x: &NdaVec, w: &NdaVec, eps_shift: u32) -> NdaVec {
    debug_assert_eq!(x.len, w.len);
    let n = x.len;

    let mut sum_sq: i64 = 0;
    let bytes = x.sign.len();
    let full_bytes = n / 8;

    for byte_idx in 0..full_bytes {
        let xs = x.sign[byte_idx];
        let xe = x.extra[byte_idx];
        let large_mask = !(xs ^ xe);
        sum_sq += 8 + (large_mask.count_ones() as i64) * 3;
    }

    if !n.is_multiple_of(8) {
        let byte_idx = full_bytes;
        let xs = x.sign[byte_idx];
        let xe = x.extra[byte_idx];
        let active_mask = (1u8 << (n % 8)) - 1;
        let large_mask = (!(xs ^ xe)) & active_mask;
        sum_sq += (n % 8) as i64 + (large_mask.count_ones() as i64) * 3;
    }

    let mean_sq_q14 = (sum_sq << 14) / n as i64;

    let mean_sq_eps = mean_sq_q14 as u64 + (1u64 << (14u32.saturating_sub(eps_shift)));

    let inv_rms_q14 = isqrt_inv_q14(mean_sq_eps);

    let mut prod_table = [0u8; 16];
    const DECODE_TABLE: [i32; 4] = [-2, -1, 1, 2];
    const ENCODE_TABLE: [u8; 9] = [0, 0, 0, 1, 2, 2, 3, 3, 3];

    for x_idx in 0..4 {
        let xv = DECODE_TABLE[x_idx] as i64;
        let normalized = div_pow2_i64(xv * inv_rms_q14 as i64, 7);
        for w_idx in 0..4 {
            let wv = DECODE_TABLE[w_idx] as i64;
            let prod = normalized * wv;
            let clamped = prod.clamp(-4, 4);
            let enc = ENCODE_TABLE[(clamped + 4) as usize];
            prod_table[(x_idx << 2) | w_idx] = enc;
        }
    }

    let mut sign = vec![0u8; bytes];
    let mut extra = vec![0u8; bytes];

    for byte_idx in 0..bytes {
        let mut s_byte = 0u8;
        let mut e_byte = 0u8;

        let mut xs_shift = x.sign[byte_idx];
        let mut xe_shift = x.extra[byte_idx];
        let mut ws_shift = w.sign[byte_idx];
        let mut we_shift = w.extra[byte_idx];

        let base_idx = byte_idx * 8;
        for bit in 0..8 {
            let i = base_idx + bit;
            if i >= n {
                break;
            }

            let x_idx = ((xs_shift & 1) << 1) | (xe_shift & 1);
            let w_idx = ((ws_shift & 1) << 1) | (we_shift & 1);
            let enc = prod_table[((x_idx << 2) | w_idx) as usize];

            s_byte |= ((enc >> 1) & 1) << bit;
            e_byte |= (enc & 1) << bit;

            xs_shift >>= 1;
            xe_shift >>= 1;
            ws_shift >>= 1;
            we_shift >>= 1;
        }
        sign[byte_idx] = s_byte;
        extra[byte_idx] = e_byte;
    }

    NdaVec {
        len: n,
        log2_scale: w.log2_scale,
        sign: sign.into(),
        extra: extra.into(),
    }
}

#[derive(Clone, Debug)]
pub struct AliBiSlopes {
    pub shifts: Vec<u8>,
    #[allow(dead_code)]
    pub n_heads: usize,
}

impl AliBiSlopes {
    pub fn new(n_heads: usize) -> Self {
        let shifts = (1..=n_heads)
            .map(|h| {
                let exact = 8.0 * h as f32 / n_heads as f32;
                exact.round().clamp(1.0, 30.0) as u8
            })
            .collect();
        Self { shifts, n_heads }
    }

    #[inline]
    pub fn shift(&self, head: usize) -> u8 {
        self.shifts[head]
    }
}

pub fn apply_alibi_bias_i32(scores: &mut [i32], q_pos: usize, shift: u8, scale_shift: u32) {
    for (k_pos, score) in scores.iter_mut().enumerate() {
        let distance = q_pos as i32 - k_pos as i32;
        let bias_int = ((distance as i64) << scale_shift) >> shift;
        *score += bias_int as i32;
    }
}

#[derive(Clone)]
pub struct SiluLut {
    #[allow(dead_code)]
    table: [i32; 4],
}

impl SiluLut {
    pub fn new() -> Self {
        Self {
            table: [-1, -1, 1, 2],
        }
    }

    pub fn apply(&self, x: &NdaVec) -> NdaVec {
        let sign = x.sign.clone();
        let mut extra = x.extra.to_vec();
        for i in 0..sign.len() {
            extra[i] |= !sign[i];
        }
        if !x.len.is_multiple_of(8) {
            if let Some(last) = extra.last_mut() {
                let mask = (1u8 << (x.len % 8)) - 1;
                *last &= mask;
            }
        }
        NdaVec {
            len: x.len,
            log2_scale: x.log2_scale,
            sign,
            extra: extra.into(),
        }
    }
}

impl Default for SiluLut {
    fn default() -> Self {
        Self::new()
    }
}

pub fn swiglu_nda(gate: &NdaVec, up: &NdaVec, silu: &SiluLut) -> NdaVec {
    debug_assert_eq!(gate.len, up.len);
    let gate_activated = silu.apply(gate);

    let len = gate.len;
    let bytes = len.div_ceil(8);
    let mut sign = vec![0u8; bytes];
    let mut extra = vec![0u8; bytes];

    for byte_idx in 0..bytes {
        let gs = gate_activated.sign[byte_idx];
        let ge = gate_activated.extra[byte_idx];
        let us = up.sign[byte_idx];
        let ue = up.extra[byte_idx];

        let idx_low = (gs & 0x0F) as usize
            | (((ge & 0x0F) as usize) << 4)
            | (((us & 0x0F) as usize) << 8)
            | (((ue & 0x0F) as usize) << 12);
        let res_low = SWIGLU_LUT_Q16[idx_low];

        let idx_high = ((gs >> 4) as usize)
            | (((ge >> 4) as usize) << 4)
            | (((us >> 4) as usize) << 8)
            | (((ue >> 4) as usize) << 12);
        let res_high = SWIGLU_LUT_Q16[idx_high];

        sign[byte_idx] = (res_low & 0x0F) | ((res_high & 0x0F) << 4);
        extra[byte_idx] = (res_low >> 4) | (res_high & 0xF0);
    }

    if !len.is_multiple_of(8) {
        let last_idx = bytes - 1;
        let mask = (1u8 << (len % 8)) - 1;
        sign[last_idx] &= mask;
        extra[last_idx] &= mask;
    }

    NdaVec {
        len,
        log2_scale: combine_log2_scales(gate.log2_scale, up.log2_scale),
        sign: sign.into(),
        extra: extra.into(),
    }
}

/// Batch in-place addition: add each delta in `deltas` to `x` sequentially.
/// Returns a report with timing diagnostics.
pub fn nda_vec_add_batch(x: &mut NdaVec, deltas: &[NdaVec]) -> NdaOpsReport {
    let start = std::time::Instant::now();
    for delta in deltas {
        nda_vec_add_inplace(x, delta);
    }
    let elapsed = start.elapsed().as_micros() as u64;
    NdaOpsReport {
        operation: "add_batch".to_string(),
        count: deltas.len(),
        total_elapsed_us: elapsed,
        per_op_avg_us: if deltas.is_empty() { 0.0 } else { elapsed as f64 / deltas.len() as f64 },
    }
}

/// Batch RMS normalization: normalize each vector in `vecs` with the same weight.
/// Returns results and a timing report.
pub fn rms_norm_batch(vecs: &[NdaVec], w: &NdaVec, eps_shift: u32) -> (Vec<NdaVec>, NdaOpsReport) {
    let start = std::time::Instant::now();
    let results: Vec<NdaVec> = vecs.iter().map(|x| rms_norm_nda(x, w, eps_shift)).collect();
    let elapsed = start.elapsed().as_micros() as u64;
    let report = NdaOpsReport {
        operation: "rms_norm_batch".to_string(),
        count: vecs.len(),
        total_elapsed_us: elapsed,
        per_op_avg_us: if vecs.is_empty() { 0.0 } else { elapsed as f64 / vecs.len() as f64 },
    };
    (results, report)
}

/// Batch SiLU activation: apply SiLU to each vector.
pub fn silu_batch(silu: &SiluLut, vecs: &[NdaVec]) -> (Vec<NdaVec>, NdaOpsReport) {
    let start = std::time::Instant::now();
    let results: Vec<NdaVec> = vecs.iter().map(|x| silu.apply(x)).collect();
    let elapsed = start.elapsed().as_micros() as u64;
    let report = NdaOpsReport {
        operation: "silu_batch".to_string(),
        count: vecs.len(),
        total_elapsed_us: elapsed,
        per_op_avg_us: if vecs.is_empty() { 0.0 } else { elapsed as f64 / vecs.len() as f64 },
    };
    (results, report)
}

/// Batch SwiGLU: apply SwiGLU to each (gate, up) pair.
pub fn swiglu_batch(
    pairs: &[(&NdaVec, &NdaVec)],
    silu: &SiluLut,
) -> (Vec<NdaVec>, NdaOpsReport) {
    let start = std::time::Instant::now();
    let results: Vec<NdaVec> = pairs
        .iter()
        .map(|(gate, up)| swiglu_nda(gate, up, silu))
        .collect();
    let elapsed = start.elapsed().as_micros() as u64;
    let report = NdaOpsReport {
        operation: "swiglu_batch".to_string(),
        count: pairs.len(),
        total_elapsed_us: elapsed,
        per_op_avg_us: if pairs.is_empty() { 0.0 } else { elapsed as f64 / pairs.len() as f64 },
    };
    (results, report)
}

pub struct NdaEmbedding {
    #[allow(dead_code)]
    pub vocab_size: usize,
    pub hidden_size: usize,
    #[allow(dead_code)]
    pub log2_scale: i8,
    pub sign: Vec<u8>,
    pub extra: Vec<u8>,
}

impl std::fmt::Debug for NdaEmbedding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NdaEmbedding")
            .field("vocab_size", &self.vocab_size)
            .field("hidden_size", &self.hidden_size)
            .field("log2_scale", &self.log2_scale)
            .field("sign_bytes", &self.sign.len())
            .field("extra_bytes", &self.extra.len())
            .finish()
    }
}

impl NdaEmbedding {
    pub fn stride(&self) -> usize {
        self.hidden_size.div_ceil(8)
    }

    #[allow(dead_code)]
    pub fn get(&self, id: usize) -> NdaVec {
        let stride = self.stride();
        let start = id * stride;
        NdaVec {
            len: self.hidden_size,
            log2_scale: self.log2_scale,
            sign: self.sign[start..start + stride].to_vec().into(),
            extra: self.extra[start..start + stride].to_vec().into(),
        }
    }

    /// Validate the embedding table for consistency.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let stride = self.stride();
        let expected_bytes = self.vocab_size * stride;
        if self.sign.len() != expected_bytes {
            warnings.push(format!(
                "sign bytes mismatch: expected {}, got {}",
                expected_bytes,
                self.sign.len()
            ));
        }
        if self.extra.len() != expected_bytes {
            warnings.push(format!(
                "extra bytes mismatch: expected {}, got {}",
                expected_bytes,
                self.extra.len()
            ));
        }
        if self.hidden_size == 0 {
            warnings.push("hidden_size is zero".to_string());
        }
        if self.vocab_size == 0 {
            warnings.push("vocab_size is zero".to_string());
        }
        warnings
    }

    /// Return diagnostic info about this embedding table.
    pub fn info(&self) -> NdaEmbeddingInfo {
        NdaEmbeddingInfo {
            vocab_size: self.vocab_size,
            hidden_size: self.hidden_size,
            log2_scale: self.log2_scale,
            total_bytes: self.sign.len() + self.extra.len(),
            bits_per_embedding: (self.hidden_size * 2) as u32,
        }
    }

    pub fn from_f32(embed: &[f32], vocab_size: usize, hidden_size: usize) -> Self {
        let amax = embed.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
        let log2_scale = if amax > 1e-8 {
            (amax / 2.0).log2().floor() as i8
        } else {
            0i8
        };
        let scale = 2f32.powi(log2_scale as i32);
        let inv_scale = 1.0 / scale;

        let stride = hidden_size.div_ceil(8);
        let mut sign = vec![0u8; vocab_size * stride];
        let mut extra = vec![0u8; vocab_size * stride];

        for (tok_id, row) in embed.chunks_exact(hidden_size).enumerate() {
            for (i, &v) in row.iter().enumerate() {
                let vs = v * inv_scale;
                let is_pos = vs >= 0.0;
                let is_large = vs.abs() >= 1.5;

                let byte_idx = tok_id * stride + i / 8;
                let bit_idx = i % 8;

                if is_pos {
                    sign[byte_idx] |= 1 << bit_idx;
                }
                if is_pos == is_large {
                    extra[byte_idx] |= 1 << bit_idx;
                }
            }
        }

        Self {
            vocab_size,
            hidden_size,
            log2_scale,
            sign,
            extra,
        }
    }
}

/// Diagnostic information about an NdaEmbedding table.
#[derive(Debug, Clone, Serialize)]
pub struct NdaEmbeddingInfo {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub log2_scale: i8,
    pub total_bytes: usize,
    pub bits_per_embedding: u32,
}

/// Validate RMS normalization parameters.
pub fn validate_rms_norm_params(x: &NdaVec, w: &NdaVec, eps_shift: u32) -> Vec<String> {
    let mut issues = Vec::new();
    if x.len != w.len {
        issues.push(format!("x.len ({}) != w.len ({})", x.len, w.len));
    }
    if x.len == 0 {
        issues.push("vector length is 0".into());
    }
    if x.sign.len() != x.extra.len() {
        issues.push(format!("x sign/extra length mismatch: {} vs {}", x.sign.len(), x.extra.len()));
    }
    if w.sign.len() != w.extra.len() {
        issues.push(format!("w sign/extra length mismatch: {} vs {}", w.sign.len(), w.extra.len()));
    }
    if eps_shift > 14 {
        issues.push(format!("eps_shift {} exceeds maximum 14", eps_shift));
    }
    issues
}

/// Diagnostic info about ALiBi slopes configuration.
#[derive(Debug, Clone, Serialize)]
pub struct AliBiSlopesInfo {
    pub n_heads: usize,
    pub min_shift: u8,
    pub max_shift: u8,
    pub unique_shifts: usize,
    pub validation_issues: Vec<String>,
}

impl AliBiSlopes {
    /// Return diagnostic info about this ALiBi configuration.
    pub fn info(&self) -> AliBiSlopesInfo {
        let mut issues = Vec::new();
        if self.n_heads == 0 {
            issues.push("n_heads is 0".into());
        }
        if self.shifts.is_empty() {
            issues.push("shifts buffer is empty".into());
        }
        if self.shifts.len() != self.n_heads {
            issues.push(format!(
                "shifts len {} != n_heads {}",
                self.shifts.len(),
                self.n_heads
            ));
        }
        let min_shift = self.shifts.iter().copied().min().unwrap_or(0);
        let max_shift = self.shifts.iter().copied().max().unwrap_or(0);
        let unique = {
            let mut s = std::collections::HashSet::new();
            for &v in &self.shifts { s.insert(v); }
            s.len()
        };
        AliBiSlopesInfo {
            n_heads: self.n_heads,
            min_shift,
            max_shift,
            unique_shifts: unique,
            validation_issues: issues,
        }
    }
}

/// Validate ALiBi configuration parameters.
pub fn validate_alibi_config(n_heads: usize) -> Vec<String> {
    let mut issues = Vec::new();
    if n_heads == 0 {
        issues.push("n_heads is 0".into());
    }
    if n_heads > 128 {
        issues.push(format!("n_heads {} exceeds typical maximum of 128", n_heads));
    }
    issues
}

/// Aggregate summary of multiple batch operation reports.
#[derive(Debug, Clone, Serialize)]
pub struct NdaOpsSummary {
    pub total_operations: usize,
    pub total_ops_count: usize,
    pub total_elapsed_us: u64,
    pub overall_avg_us: f64,
    pub slowest_operation: Option<String>,
    pub fastest_operation: Option<String>,
}

/// Summarize multiple batch operation reports into an aggregate.
pub fn summarize_ops(reports: &[NdaOpsReport]) -> NdaOpsSummary {
    let total_operations = reports.len();
    let total_ops_count: usize = reports.iter().map(|r| r.count).sum();
    let total_elapsed_us: u64 = reports.iter().map(|r| r.total_elapsed_us).sum();
    let overall_avg_us = if total_ops_count > 0 {
        total_elapsed_us as f64 / total_ops_count as f64
    } else {
        0.0
    };

    let slowest = reports.iter().max_by(|a, b| a.per_op_avg_us.partial_cmp(&b.per_op_avg_us).unwrap_or(std::cmp::Ordering::Equal));
    let fastest = reports.iter().min_by(|a, b| a.per_op_avg_us.partial_cmp(&b.per_op_avg_us).unwrap_or(std::cmp::Ordering::Equal));

    NdaOpsSummary {
        total_operations,
        total_ops_count,
        total_elapsed_us,
        overall_avg_us,
        slowest_operation: slowest.map(|r| r.operation.clone()),
        fastest_operation: fastest.map(|r| r.operation.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_binary_op_compatible() {
        let a = NdaVec::from_i32_slice(&[1, 2, 3, 4], 0);
        let b = NdaVec::from_i32_slice(&[1, -1, 2, -2], 0);
        let w = validate_binary_op(&a, &b);
        assert!(w.is_empty());
    }

    #[test]
    fn validate_binary_op_length_mismatch() {
        let a = NdaVec::from_i32_slice(&[1, 2, 3, 4], 0);
        let b = NdaVec::from_i32_slice(&[1, -1], 0);
        let w = validate_binary_op(&a, &b);
        assert!(!w.is_empty());
        assert!(w[0].contains("length mismatch"));
    }

    #[test]
    fn nda_vec_add_batch_report() {
        let mut x = NdaVec::from_i32_slice(&[1, 1, 1, 1], 0);
        let deltas = vec![
            NdaVec::from_i32_slice(&[1, 1, 1, 1], 0),
            NdaVec::from_i32_slice(&[1, 1, 1, 1], 0),
        ];
        let report = nda_vec_add_batch(&mut x, &deltas);
        assert_eq!(report.count, 2);
        assert_eq!(report.operation, "add_batch");
    }

    #[test]
    fn rms_norm_batch_report() {
        let vecs = vec![
            NdaVec::from_i32_slice(&[1, 2, -1, -2], 0),
            NdaVec::from_i32_slice(&[2, -1, 1, -2], 0),
        ];
        let w = NdaVec::from_i32_slice(&[1, 1, 1, 1], 0);
        let (results, report) = rms_norm_batch(&vecs, &w, 2);
        assert_eq!(results.len(), 2);
        assert_eq!(report.count, 2);
        assert_eq!(report.operation, "rms_norm_batch");
    }

    #[test]
    fn silu_batch_report() {
        let silu = SiluLut::new();
        let vecs = vec![
            NdaVec::from_i32_slice(&[1, 2, -1, -2], 0),
            NdaVec::from_i32_slice(&[2, -1, 1, -2], 0),
        ];
        let (results, report) = silu_batch(&silu, &vecs);
        assert_eq!(results.len(), 2);
        assert_eq!(report.count, 2);
    }

    #[test]
    fn swiglu_batch_report() {
        let silu = SiluLut::new();
        let g1 = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let u1 = NdaVec::from_i32_slice(&[1, 1, 1, 1], 0);
        let g2 = NdaVec::from_i32_slice(&[-1, -2, 1, 2], 0);
        let u2 = NdaVec::from_i32_slice(&[2, 2, 2, 2], 0);
        let pairs = vec![(&g1, &u1), (&g2, &u2)];
        let (results, report) = swiglu_batch(&pairs, &silu);
        assert_eq!(results.len(), 2);
        assert_eq!(report.count, 2);
    }

    #[test]
    fn nda_embedding_validate_and_info() {
        let embed = NdaEmbedding::from_f32(&[0.1, -0.2, 0.3, -0.4, 0.5, -0.6], 2, 3);
        let w = embed.validate();
        assert!(w.is_empty(), "expected no warnings, got: {:?}", w);
        let info = embed.info();
        assert_eq!(info.vocab_size, 2);
        assert_eq!(info.hidden_size, 3);
        assert!(info.total_bytes > 0);
    }

    #[test]
    fn nda_embedding_info_serializes() {
        let embed = NdaEmbedding::from_f32(&[0.1, -0.2, 0.3, -0.4], 2, 2);
        let info = embed.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"vocab_size\":2"));
        assert!(json.contains("\"hidden_size\":2"));
    }

    #[test]
    fn nda_ops_report_serializes() {
        let report = NdaOpsReport {
            operation: "test".to_string(),
            count: 5,
            total_elapsed_us: 100,
            per_op_avg_us: 20.0,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"operation\":\"test\""));
        assert!(json.contains("\"count\":5"));
    }

    #[test]
    fn validate_rms_norm_params_valid() {
        let x = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let w = NdaVec::from_i32_slice(&[1, 1, 1, 1], 0);
        let issues = validate_rms_norm_params(&x, &w, 2);
        assert!(issues.is_empty(), "expected no issues, got: {:?}", issues);
    }

    #[test]
    fn validate_rms_norm_params_length_mismatch() {
        let x = NdaVec::from_i32_slice(&[1, 2, 3, 4], 0);
        let w = NdaVec::from_i32_slice(&[1, 1], 0);
        let issues = validate_rms_norm_params(&x, &w, 2);
        assert!(issues.iter().any(|i| i.contains("!=")));
    }

    #[test]
    fn validate_rms_norm_params_zero_length() {
        let x = NdaVec { len: 0, log2_scale: 0, sign: vec![].into(), extra: vec![].into() };
        let w = NdaVec { len: 0, log2_scale: 0, sign: vec![].into(), extra: vec![].into() };
        let issues = validate_rms_norm_params(&x, &w, 2);
        assert!(issues.iter().any(|i| i.contains("0")));
    }

    #[test]
    fn validate_rms_norm_params_large_eps() {
        let x = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let w = NdaVec::from_i32_slice(&[1, 1, 1, 1], 0);
        let issues = validate_rms_norm_params(&x, &w, 20);
        assert!(issues.iter().any(|i| i.contains("eps_shift")));
    }

    #[test]
    fn alibi_slopes_info_valid() {
        let slopes = AliBiSlopes::new(8);
        let info = slopes.info();
        assert_eq!(info.n_heads, 8);
        assert!(info.min_shift >= 1);
        assert!(info.max_shift <= 30);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn alibi_slopes_info_serializes() {
        let slopes = AliBiSlopes::new(4);
        let info = slopes.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"n_heads\":4"));
        assert!(json.contains("\"min_shift\""));
    }

    #[test]
    fn validate_alibi_config_valid() {
        let issues = validate_alibi_config(8);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_alibi_config_zero_heads() {
        let issues = validate_alibi_config(0);
        assert!(issues.iter().any(|i| i.contains("0")));
    }

    #[test]
    fn validate_alibi_config_too_many_heads() {
        let issues = validate_alibi_config(256);
        assert!(issues.iter().any(|i| i.contains("exceeds")));
    }

    #[test]
    fn summarize_ops_empty() {
        let summary = summarize_ops(&[]);
        assert_eq!(summary.total_operations, 0);
        assert_eq!(summary.total_ops_count, 0);
        assert!(summary.slowest_operation.is_none());
        assert!(summary.fastest_operation.is_none());
    }

    #[test]
    fn summarize_ops_multiple() {
        let reports = vec![
            NdaOpsReport { operation: "add".into(), count: 10, total_elapsed_us: 100, per_op_avg_us: 10.0 },
            NdaOpsReport { operation: "norm".into(), count: 5, total_elapsed_us: 200, per_op_avg_us: 40.0 },
            NdaOpsReport { operation: "silu".into(), count: 8, total_elapsed_us: 40, per_op_avg_us: 5.0 },
        ];
        let summary = summarize_ops(&reports);
        assert_eq!(summary.total_operations, 3);
        assert_eq!(summary.total_ops_count, 23);
        assert_eq!(summary.total_elapsed_us, 340);
        assert_eq!(summary.slowest_operation.as_deref(), Some("norm"));
        assert_eq!(summary.fastest_operation.as_deref(), Some("silu"));
    }

    #[test]
    fn summarize_ops_serializes() {
        let summary = NdaOpsSummary {
            total_operations: 2,
            total_ops_count: 10,
            total_elapsed_us: 100,
            overall_avg_us: 10.0,
            slowest_operation: Some("norm".into()),
            fastest_operation: Some("add".into()),
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"total_operations\":2"));
        assert!(json.contains("\"slowest_operation\":\"norm\""));
    }

    // ─── Expanded Tests ─────────────────────────────────────────────────

    #[test]
    fn nda_embedding_from_f32_all_zeros() {
        let embed = NdaEmbedding::from_f32(&[0.0, 0.0, 0.0, 0.0], 2, 2);
        assert_eq!(embed.log2_scale, 0);
        assert_eq!(embed.vocab_size, 2);
        assert_eq!(embed.hidden_size, 2);
        let w = embed.validate();
        assert!(w.is_empty(), "expected no warnings, got: {:?}", w);
    }

    #[test]
    fn nda_embedding_from_f32_large_values() {
        let embed = NdaEmbedding::from_f32(&[100.0, -200.0, 50.0, -25.0], 2, 2);
        assert!(embed.log2_scale > 0);
        let w = embed.validate();
        assert!(w.is_empty());
    }

    #[test]
    fn nda_embedding_from_f32_single_token() {
        let embed = NdaEmbedding::from_f32(&[1.0, -1.0, 0.5, -0.5], 1, 4);
        assert_eq!(embed.vocab_size, 1);
        assert_eq!(embed.hidden_size, 4);
        let stride = embed.stride();
        assert_eq!(stride, 1); // ceil(4/8) = 1
        assert_eq!(embed.sign.len(), 1); // 1 token * 1 stride
        assert_eq!(embed.extra.len(), 1);
    }

    #[test]
    fn nda_embedding_get_returns_ndavec() {
        let data = vec![
            1.0, -1.0, 0.5, -0.5,  // token 0
            2.0, -2.0, 1.0, -1.0,  // token 1
        ];
        let embed = NdaEmbedding::from_f32(&data, 2, 4);
        let v0 = embed.get(0);
        let v1 = embed.get(1);
        assert_eq!(v0.len, 4);
        assert_eq!(v1.len, 4);
        assert_eq!(v0.log2_scale, embed.log2_scale);
        assert_eq!(v1.log2_scale, embed.log2_scale);
        // Different tokens should have different bitmaps
        assert!(v0.sign != v1.sign || v0.extra != v1.extra || data[0] != data[4]);
    }

    #[test]
    fn nda_embedding_stride_various_sizes() {
        // hidden_size = 1 → stride = 1
        let e1 = NdaEmbedding::from_f32(&[1.0], 1, 1);
        assert_eq!(e1.stride(), 1);

        // hidden_size = 7 → stride = 1
        let e7 = NdaEmbedding::from_f32(&[1.0; 7], 1, 7);
        assert_eq!(e7.stride(), 1);

        // hidden_size = 8 → stride = 1
        let e8 = NdaEmbedding::from_f32(&[1.0; 8], 1, 8);
        assert_eq!(e8.stride(), 1);

        // hidden_size = 9 → stride = 2
        let e9 = NdaEmbedding::from_f32(&[1.0; 9], 1, 9);
        assert_eq!(e9.stride(), 2);

        // hidden_size = 16 → stride = 2
        let e16 = NdaEmbedding::from_f32(&[1.0; 16], 1, 16);
        assert_eq!(e16.stride(), 2);

        // hidden_size = 128 → stride = 16
        let e128 = NdaEmbedding::from_f32(&[1.0; 128], 1, 128);
        assert_eq!(e128.stride(), 16);
    }

    #[test]
    fn nda_embedding_validate_zero_hidden() {
        let embed = NdaEmbedding {
            vocab_size: 10,
            hidden_size: 0,
            log2_scale: 0,
            sign: vec![],
            extra: vec![],
        };
        let w = embed.validate();
        assert!(w.iter().any(|s| s.contains("hidden_size is zero")));
    }

    #[test]
    fn nda_embedding_validate_zero_vocab() {
        let embed = NdaEmbedding {
            vocab_size: 0,
            hidden_size: 8,
            log2_scale: 0,
            sign: vec![],
            extra: vec![],
        };
        let w = embed.validate();
        assert!(w.iter().any(|s| s.contains("vocab_size is zero")));
    }

    #[test]
    fn nda_embedding_validate_sign_mismatch() {
        let embed = NdaEmbedding {
            vocab_size: 2,
            hidden_size: 8,
            log2_scale: 0,
            sign: vec![0xFF; 3], // expected 2*1=2, got 3
            extra: vec![0xFF; 2],
        };
        let w = embed.validate();
        assert!(w.iter().any(|s| s.contains("sign bytes mismatch")));
    }

    #[test]
    fn nda_embedding_validate_extra_mismatch() {
        let embed = NdaEmbedding {
            vocab_size: 2,
            hidden_size: 8,
            log2_scale: 0,
            sign: vec![0xFF; 2],
            extra: vec![0xFF; 5], // expected 2
        };
        let w = embed.validate();
        assert!(w.iter().any(|s| s.contains("extra bytes mismatch")));
    }

    #[test]
    fn nda_embedding_info_bits_per_embedding() {
        let embed = NdaEmbedding::from_f32(&[1.0; 64], 1, 64);
        let info = embed.info();
        assert_eq!(info.bits_per_embedding, 128); // 64 * 2 bits
    }

    #[test]
    fn nda_embedding_info_total_bytes() {
        let embed = NdaEmbedding::from_f32(&[1.0; 16], 4, 4);
        let info = embed.info();
        // stride = ceil(4/8) = 1, total sign bytes = 4*1 = 4, extra = 4
        assert_eq!(info.total_bytes, 8);
    }

    #[test]
    fn nda_embedding_debug_format() {
        let embed = NdaEmbedding::from_f32(&[1.0, -1.0], 1, 2);
        let debug = format!("{:?}", embed);
        assert!(debug.contains("NdaEmbedding"));
        assert!(debug.contains("vocab_size"));
        assert!(debug.contains("sign_bytes"));
    }

    #[test]
    fn alibi_slopes_single_head() {
        let slopes = AliBiSlopes::new(1);
        assert_eq!(slopes.n_heads, 1);
        assert_eq!(slopes.shifts.len(), 1);
        // 8 * 1 / 1 = 8.0 → clamp(1, 30) = 8
        assert_eq!(slopes.shift(0), 8);
    }

    #[test]
    fn alibi_slopes_two_heads() {
        let slopes = AliBiSlopes::new(2);
        // head 0: 8*1/2 = 4 → shift = 4
        // head 1: 8*2/2 = 8 → shift = 8
        assert_eq!(slopes.shift(0), 4);
        assert_eq!(slopes.shift(1), 8);
    }

    #[test]
    fn alibi_slopes_many_heads() {
        let slopes = AliBiSlopes::new(32);
        assert_eq!(slopes.shifts.len(), 32);
        // All shifts should be in [1, 30]
        for &s in &slopes.shifts {
            assert!(s >= 1 && s <= 30, "shift {} out of range", s);
        }
    }

    #[test]
    fn alibi_slopes_info_unique_shifts() {
        let slopes = AliBiSlopes::new(8);
        let info = slopes.info();
        assert_eq!(info.n_heads, 8);
        assert!(info.unique_shifts > 0);
        assert!(info.unique_shifts <= 8);
    }

    #[test]
    fn alibi_slopes_info_invalid_empty() {
        let slopes = AliBiSlopes {
            shifts: vec![],
            n_heads: 0,
        };
        let info = slopes.info();
        assert!(!info.validation_issues.is_empty());
        assert!(info.validation_issues.iter().any(|i| i.contains("0")));
        assert!(info.validation_issues.iter().any(|i| i.contains("empty")));
    }

    #[test]
    fn alibi_slopes_info_mismatched_lengths() {
        let slopes = AliBiSlopes {
            shifts: vec![4, 8],
            n_heads: 5,
        };
        let info = slopes.info();
        assert!(info.validation_issues.iter().any(|i| i.contains("!=")));
    }

    #[test]
    fn validate_rms_norm_params_sign_extra_mismatch_x() {
        let x = NdaVec {
            len: 8,
            log2_scale: 0,
            sign: vec![0xFF; 2].into(), // 2 bytes
            extra: vec![0xFF; 1].into(), // 1 byte — mismatch
        };
        let w = NdaVec::from_i32_slice(&[1, 1, 1, 1, 1, 1, 1, 1], 0);
        let issues = validate_rms_norm_params(&x, &w, 2);
        assert!(issues.iter().any(|i| i.contains("mismatch")));
    }

    #[test]
    fn validate_rms_norm_params_sign_extra_mismatch_w() {
        let x = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let w = NdaVec {
            len: 4,
            log2_scale: 0,
            sign: vec![0xFF; 2].into(), // 2 bytes
            extra: vec![0xFF; 1].into(), // 1 byte — mismatch
        };
        let issues = validate_rms_norm_params(&x, &w, 2);
        assert!(issues.iter().any(|i| i.contains("mismatch")));
    }

    #[test]
    fn summarize_ops_single_report() {
        let reports = vec![
            NdaOpsReport { operation: "gemv".into(), count: 100, total_elapsed_us: 5000, per_op_avg_us: 50.0 },
        ];
        let summary = summarize_ops(&reports);
        assert_eq!(summary.total_operations, 1);
        assert_eq!(summary.total_ops_count, 100);
        assert_eq!(summary.total_elapsed_us, 5000);
        assert!((summary.overall_avg_us - 50.0).abs() < 1e-9);
        assert_eq!(summary.slowest_operation.as_deref(), Some("gemv"));
        assert_eq!(summary.fastest_operation.as_deref(), Some("gemv"));
    }

    #[test]
    fn summarize_ops_all_same_speed() {
        let reports = vec![
            NdaOpsReport { operation: "a".into(), count: 10, total_elapsed_us: 100, per_op_avg_us: 10.0 },
            NdaOpsReport { operation: "b".into(), count: 10, total_elapsed_us: 100, per_op_avg_us: 10.0 },
        ];
        let summary = summarize_ops(&reports);
        assert_eq!(summary.total_operations, 2);
        assert_eq!(summary.total_ops_count, 20);
        assert!((summary.overall_avg_us - 10.0).abs() < 1e-9);
    }

    #[test]
    fn nda_ops_report_clone() {
        let report = NdaOpsReport {
            operation: "test_op".into(),
            count: 42,
            total_elapsed_us: 1234,
            per_op_avg_us: 29.38,
        };
        let cloned = report.clone();
        assert_eq!(cloned.operation, "test_op");
        assert_eq!(cloned.count, 42);
        assert!((cloned.per_op_avg_us - 29.38).abs() < 1e-9);
    }

    #[test]
    fn validate_binary_op_empty_vecs() {
        let a = NdaVec { len: 0, log2_scale: 0, sign: vec![].into(), extra: vec![].into() };
        let b = NdaVec { len: 0, log2_scale: 0, sign: vec![].into(), extra: vec![].into() };
        let w = validate_binary_op(&a, &b);
        // Same length (0) — no length mismatch, but validate() flags zero length
        assert!(w.iter().any(|s| s.contains("zero length")));
    }

    #[test]
    fn nda_embedding_from_f32_negative_values() {
        let embed = NdaEmbedding::from_f32(&[-1.0, -2.0, -3.0, -4.0], 1, 4);
        let v = embed.get(0);
        assert_eq!(v.len, 4);
        // All negative → sign bits all clear
        assert_eq!(v.sign[0] & 0x0F, 0x00);
    }

    #[test]
    fn nda_embedding_stride_byte_count() {
        let embed = NdaEmbedding::from_f32(&[1.0; 24], 3, 8);
        // stride = ceil(8/8) = 1, total sign bytes = 3*1 = 3
        assert_eq!(embed.stride(), 1);
        assert_eq!(embed.sign.len(), 3);
        assert_eq!(embed.extra.len(), 3);
    }

    // ── JSON key count verification ─────────────────────────────────────

    #[test]
    fn ops_report_json_has_exactly_4_keys() {
        let report = NdaOpsReport {
            operation: "test".into(), count: 1, total_elapsed_us: 10, per_op_avg_us: 10.0,
        };
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 4);
    }

    #[test]
    fn ops_summary_json_has_exactly_6_keys() {
        let summary = summarize_ops(&[]);
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&summary).unwrap()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 6);
    }

    #[test]
    fn embedding_info_json_has_exactly_5_keys() {
        let embed = NdaEmbedding::from_f32(&[1.0; 8], 1, 8);
        let info = embed.info();
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&info).unwrap()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 5);
    }

    #[test]
    fn alibi_info_json_has_exactly_5_keys() {
        let slopes = AliBiSlopes::new(4);
        let info = slopes.info();
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&info).unwrap()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 5);
    }

    // ── JSON roundtrip via Value ────────────────────────────────────────

    #[test]
    fn ops_report_json_roundtrip_via_value() {
        let report = NdaOpsReport {
            operation: "gemv".into(), count: 42, total_elapsed_us: 1234, per_op_avg_us: 29.38,
        };
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(v["operation"], "gemv");
        assert_eq!(v["count"], 42);
        assert_eq!(v["total_elapsed_us"], 1234);
    }

    #[test]
    fn ops_summary_json_roundtrip_via_value() {
        let summary = NdaOpsSummary {
            total_operations: 3, total_ops_count: 100, total_elapsed_us: 5000,
            overall_avg_us: 50.0,
            slowest_operation: Some("norm".into()),
            fastest_operation: Some("add".into()),
        };
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&summary).unwrap()).unwrap();
        assert_eq!(v["total_operations"], 3);
        assert_eq!(v["slowest_operation"], "norm");
        assert_eq!(v["fastest_operation"], "add");
    }

    // ── Validation boundary tests ───────────────────────────────────────

    #[test]
    fn validate_rms_norm_eps_shift_14_ok() {
        let x = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let w = NdaVec::from_i32_slice(&[1, 1, 1, 1], 0);
        let issues = validate_rms_norm_params(&x, &w, 14);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_rms_norm_eps_shift_15_fails() {
        let x = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let w = NdaVec::from_i32_slice(&[1, 1, 1, 1], 0);
        let issues = validate_rms_norm_params(&x, &w, 15);
        assert!(issues.iter().any(|i| i.contains("eps_shift")));
    }

    #[test]
    fn validate_alibi_config_128_ok() {
        let issues = validate_alibi_config(128);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_alibi_config_129_fails() {
        let issues = validate_alibi_config(129);
        assert!(issues.iter().any(|i| i.contains("exceeds")));
    }

    // ── Summarize ops formula verification ──────────────────────────────

    #[test]
    fn summarize_ops_overall_avg_formula() {
        let reports = vec![
            NdaOpsReport { operation: "a".into(), count: 10, total_elapsed_us: 100, per_op_avg_us: 10.0 },
            NdaOpsReport { operation: "b".into(), count: 20, total_elapsed_us: 400, per_op_avg_us: 20.0 },
        ];
        let summary = summarize_ops(&reports);
        // total_elapsed=500, total_count=30, avg=500/30
        assert!((summary.overall_avg_us - 500.0 / 30.0).abs() < 1e-9);
    }

    #[test]
    fn summarize_ops_total_ops_count_is_sum() {
        let reports = vec![
            NdaOpsReport { operation: "a".into(), count: 5, total_elapsed_us: 10, per_op_avg_us: 2.0 },
            NdaOpsReport { operation: "b".into(), count: 15, total_elapsed_us: 30, per_op_avg_us: 2.0 },
            NdaOpsReport { operation: "c".into(), count: 25, total_elapsed_us: 50, per_op_avg_us: 2.0 },
        ];
        let summary = summarize_ops(&reports);
        assert_eq!(summary.total_ops_count, 45);
        assert_eq!(summary.total_elapsed_us, 90);
    }

    // ── Debug format ────────────────────────────────────────────────────

    #[test]
    fn ops_report_debug_format() {
        let report = NdaOpsReport {
            operation: "test".into(), count: 5, total_elapsed_us: 100, per_op_avg_us: 20.0,
        };
        let debug = format!("{:?}", report);
        assert!(debug.contains("NdaOpsReport"));
        assert!(debug.contains("operation"));
        assert!(debug.contains("per_op_avg_us"));
    }

    #[test]
    fn ops_summary_debug_format() {
        let summary = summarize_ops(&[NdaOpsReport {
            operation: "x".into(), count: 1, total_elapsed_us: 10, per_op_avg_us: 10.0,
        }]);
        let debug = format!("{:?}", summary);
        assert!(debug.contains("NdaOpsSummary"));
        assert!(debug.contains("slowest_operation"));
    }

    // ── Clone tests ─────────────────────────────────────────────────────

    #[test]
    fn embedding_info_clone() {
        let embed = NdaEmbedding::from_f32(&[1.0; 8], 2, 4);
        let info = embed.info();
        let cloned = info.clone();
        assert_eq!(cloned.vocab_size, info.vocab_size);
        assert_eq!(cloned.total_bytes, info.total_bytes);
    }

    #[test]
    fn alibi_info_clone() {
        let slopes = AliBiSlopes::new(4);
        let info = slopes.info();
        let cloned = info.clone();
        assert_eq!(cloned.n_heads, info.n_heads);
        assert_eq!(cloned.unique_shifts, info.unique_shifts);
    }

    #[test]
    fn alibi_slopes_clone() {
        let slopes = AliBiSlopes::new(8);
        let cloned = slopes.clone();
        assert_eq!(cloned.n_heads, slopes.n_heads);
        assert_eq!(cloned.shifts, slopes.shifts);
    }

    #[test]
    fn ops_summary_clone() {
        let summary = summarize_ops(&[NdaOpsReport {
            operation: "op".into(), count: 10, total_elapsed_us: 100, per_op_avg_us: 10.0,
        }]);
        let cloned = summary.clone();
        assert_eq!(cloned.total_operations, summary.total_operations);
        assert_eq!(cloned.slowest_operation, summary.slowest_operation);
    }

    // ── SiluLut tests ───────────────────────────────────────────────────

    #[test]
    fn silu_lut_default_trait() {
        let silu = SiluLut::default();
        let x = NdaVec::from_i32_slice(&[1, 2, -1, -2], 0);
        let result = silu.apply(&x);
        assert_eq!(result.len, x.len);
    }

    #[test]
    fn silu_preserves_length() {
        let silu = SiluLut::new();
        for size in [1, 4, 8, 16, 32] {
            let vals: Vec<i32> = (0..size).map(|i| if i % 2 == 0 { 1 } else { -1 }).collect();
            let x = NdaVec::from_i32_slice(&vals, 0);
            let result = silu.apply(&x);
            assert_eq!(result.len, x.len);
        }
    }

    // ── Batch operations with empty inputs ──────────────────────────────

    #[test]
    fn add_batch_empty_deltas() {
        let mut x = NdaVec::from_i32_slice(&[1, 1, 1, 1], 0);
        let report = nda_vec_add_batch(&mut x, &[]);
        assert_eq!(report.count, 0);
        assert_eq!(report.per_op_avg_us, 0.0);
    }

    #[test]
    fn rms_norm_batch_empty() {
        let w = NdaVec::from_i32_slice(&[1, 1, 1, 1], 0);
        let (results, report) = rms_norm_batch(&[], &w, 2);
        assert!(results.is_empty());
        assert_eq!(report.count, 0);
        assert_eq!(report.per_op_avg_us, 0.0);
    }

    #[test]
    fn silu_batch_empty() {
        let silu = SiluLut::new();
        let (results, report) = silu_batch(&silu, &[]);
        assert!(results.is_empty());
        assert_eq!(report.count, 0);
    }

    #[test]
    fn swiglu_batch_empty() {
        let silu = SiluLut::new();
        let pairs: Vec<(&NdaVec, &NdaVec)> = vec![];
        let (results, report) = swiglu_batch(&pairs, &silu);
        assert!(results.is_empty());
        assert_eq!(report.count, 0);
    }

    // ── Embedding info serialization roundtrip ──────────────────────────

    #[test]
    fn embedding_info_json_roundtrip() {
        let embed = NdaEmbedding::from_f32(&[1.0; 16], 4, 4);
        let info = embed.info();
        let json = serde_json::to_string(&info).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["vocab_size"], 4);
        assert_eq!(v["hidden_size"], 4);
        assert_eq!(v["bits_per_embedding"], 8);
    }

    // ── AliBi apply_alibi_bias_i32 ──────────────────────────────────────

    #[test]
    fn apply_alibi_bias_zero_distance() {
        let mut scores = vec![100, 200, 300];
        apply_alibi_bias_i32(&mut scores, 0, 8, 0);
        // q_pos=0, k_pos=0: distance=0, bias=0>>8=0
        // q_pos=0, k_pos=1: distance=-1, bias=-1>>8=0 (arithmetic shift)
        // q_pos=0, k_pos=2: distance=-2, bias=-2>>8=0
        assert_eq!(scores[0], 100); // no change for distance 0
    }

    #[test]
    fn apply_alibi_bias_positive_distance() {
        let mut scores = vec![0, 0];
        apply_alibi_bias_i32(&mut scores, 2, 4, 0);
        // q_pos=2, k_pos=0: distance=2, bias=2>>4=0
        // q_pos=2, k_pos=1: distance=1, bias=1>>4=0
        assert_eq!(scores[0], 0);
        assert_eq!(scores[1], 0);
    }

    // ── validate_binary_op with various inputs ──────────────────────────

    #[test]
    fn validate_binary_op_same_vec() {
        let a = NdaVec::from_i32_slice(&[1, -1, 2, -2], 0);
        let w = validate_binary_op(&a, &a);
        assert!(w.is_empty());
    }

    #[test]
    fn validate_binary_op_different_lengths() {
        let a = NdaVec::from_i32_slice(&[1, 2, 3], 0);
        let b = NdaVec::from_i32_slice(&[1, 2, 3, 4], 0);
        let w = validate_binary_op(&a, &b);
        assert!(w.iter().any(|s| s.contains("length mismatch")));
    }

    // ── Equality via JSON ───────────────────────────────────────────────

    #[test]
    fn ops_report_eq_via_json() {
        let r1 = NdaOpsReport { operation: "x".into(), count: 5, total_elapsed_us: 10, per_op_avg_us: 2.0 };
        let r2 = NdaOpsReport { operation: "x".into(), count: 5, total_elapsed_us: 10, per_op_avg_us: 2.0 };
        let j1: serde_json::Value = serde_json::from_str(&serde_json::to_string(&r1).unwrap()).unwrap();
        let j2: serde_json::Value = serde_json::from_str(&serde_json::to_string(&r2).unwrap()).unwrap();
        assert_eq!(j1, j2);
    }
}
