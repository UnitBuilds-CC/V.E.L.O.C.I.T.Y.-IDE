// V.E.L.O.C.I.T.Y.-IDE — NDA (Non-linear Decomposed Attention) core types and GEMV kernels
//
// NDA v2 uses pure-additive quaternary {-2, -1, +1, +2} weight encoding:
//   sign bitmap  : 1 bit/elem — 1 = positive weight, 0 = negative weight
//   extra bitmap : 1 bit/elem — encodes magnitude via XNOR rule with sign
//
// Decode rule (NO MULTIPLICATION):
//   large = (sign_bit == extra_bit)        // XNOR condition
//   weight = (sign_bit ? +1 : -1) * (large ? 2 : 1)
//
// Encoding table:
//   sign=0, extra=0  →  −2   acc -= x; acc -= x
//   sign=0, extra=1  →  −1   acc -= x
//   sign=1, extra=0  →  +1   acc += x
//   sign=1, extra=1  →  +2   acc += x; acc += x
//
// Benefits over v1 ternary {-1,0,+1}:
//   • No zero weights: every element contributes
//   • No multiplications: GEMV is pure add/subtract
//   • 8 ops per 32-bit register (INT4 SIMD path)
//   • Same on-disk size: 2 bits per weight
//
// v1 ternary files are still loadable (backward compat via version field).

use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use serde::Serialize;
use std::{fs, path::Path};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Magic bytes: "NDA\0"
pub const NDA_MAGIC: u32 = 0x4E44_4100;
/// v1: ternary {-1, 0, +1} via active+pos bitmaps (legacy)
pub const NDA_V1_TERN: u16 = 1;
/// v2: quad {-2,-1,+1,+2} via sign+extra bitmaps (current)
pub const NDA_V2_QUAD: u16 = 2;
/// v3: 4-bit FP4 E2M1 blockwise logarithmic format
pub const NDA_VERSION_FP4: u16 = 3;
/// v4: 2-bit FP2 E1M0 blockwise logarithmic format
pub const NDA_VERSION_FP2: u16 = 4;

// ─── Data structure ───────────────────────────────────────────────────────────

/// A weight matrix packed as either legacy 1-bit/2-bit bitmaps (v1/v2) or
/// block-wise double-quantized logarithmic representations (v3/v4).
#[derive(Debug, Serialize)]
pub struct NdaMatrix {
    pub rows: usize,
    pub cols: usize,
    /// Per-matrix scale (legacy global scale or v3/v4 global_scale).
    pub scale: f32,
    /// NDA version (1 = ternary, 2 = quad, 3 = FP4, 4 = FP2).
    pub version: u16,

    // v1/v2 fields
    pub sign: Vec<u8>,
    pub extra: Vec<u8>,

    // v3/v4 fields
    #[allow(dead_code)]
    pub block_size: usize,
    #[allow(dead_code)]
    pub n_blocks: usize,
    pub q_scales: Vec<u8>,
    pub packed_codes: Vec<u8>,
}

impl NdaMatrix {
    /// Create a legacy NdaMatrix (v2 quad) for backwards compatibility in tests/benchmarks.
    pub fn new_quad(rows: usize, cols: usize, scale: f32, sign: Vec<u8>, extra: Vec<u8>) -> Self {
        Self {
            rows,
            cols,
            scale,
            version: NDA_V2_QUAD,
            sign,
            extra,
            block_size: 64,
            n_blocks: 0,
            q_scales: Vec::new(),
            packed_codes: Vec::new(),
        }
    }

    /// Load a `.nda` file (v1, v2, v3, or v4).
    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read(path).with_context(|| format!("Reading NDA file: {path:?}"))?;

        if data.len() < 6 {
            bail!("NDA file too small ({} B): {path:?}", data.len());
        }

        let magic = u32::from_le_bytes(data[0..4].try_into().expect("slice is exactly 4 bytes"));
        let version = u16::from_le_bytes(data[4..6].try_into().expect("slice is exactly 2 bytes"));

        if magic != NDA_MAGIC {
            bail!("Invalid NDA magic {magic:#010x} in {path:?}");
        }

        if version == NDA_V1_TERN || version == NDA_V2_QUAD {
            // Legacy header: HDR = 18
            const HDR: usize = 18;
            if data.len() < HDR {
                bail!("NDA file too small ({} B): {path:?}", data.len());
            }
            let rows = u32::from_le_bytes(data[6..10].try_into().expect("slice is exactly 4 bytes"))
                as usize;
            let cols =
                u32::from_le_bytes(data[10..14].try_into().expect("slice is exactly 4 bytes"))
                    as usize;
            let scale =
                f32::from_le_bytes(data[14..18].try_into().expect("slice is exactly 4 bytes"));

            let bitmap_bytes = (rows * cols).div_ceil(8);
            let expected = HDR + 2 * bitmap_bytes;
            if data.len() < expected {
                bail!(
                    "NDA file truncated: expected {expected} B, got {} B: {path:?}",
                    data.len()
                );
            }

            let sign = data[HDR..HDR + bitmap_bytes].to_vec();
            let extra = data[HDR + bitmap_bytes..HDR + 2 * bitmap_bytes].to_vec();

            Ok(Self {
                rows,
                cols,
                scale,
                version,
                sign,
                extra,
                block_size: 64,
                n_blocks: 0,
                q_scales: Vec::new(),
                packed_codes: Vec::new(),
            })
        } else if version == NDA_VERSION_FP4 || version == NDA_VERSION_FP2 {
            // New header: magic(4) + version(2) + rows(2) + cols(4) + block_size(4) + n_blocks(4) + global_scale(4) = 24 bytes
            const HDR: usize = 24;
            if data.len() < HDR {
                bail!("NDA file too small ({} B): {path:?}", data.len());
            }
            let rows = u16::from_le_bytes(data[6..8].try_into().expect("slice is exactly 2 bytes"))
                as usize;
            let cols = u32::from_le_bytes(data[8..12].try_into().expect("slice is exactly 4 bytes"))
                as usize;
            let block_size =
                u32::from_le_bytes(data[12..16].try_into().expect("slice is exactly 4 bytes"))
                    as usize;
            let n_blocks =
                u32::from_le_bytes(data[16..20].try_into().expect("slice is exactly 4 bytes"))
                    as usize;
            let global_scale =
                f32::from_le_bytes(data[20..24].try_into().expect("slice is exactly 4 bytes"));

            let q_scales_bytes = n_blocks;
            let codes_bytes = if version == NDA_VERSION_FP4 {
                (rows * cols).div_ceil(2)
            } else {
                (rows * cols).div_ceil(4)
            };

            let expected = HDR + q_scales_bytes + codes_bytes;
            if data.len() < expected {
                bail!(
                    "NDA file truncated: expected {expected} B, got {} B: {path:?}",
                    data.len()
                );
            }

            let q_scales = data[HDR..HDR + q_scales_bytes].to_vec();
            let packed_codes =
                data[HDR + q_scales_bytes..HDR + q_scales_bytes + codes_bytes].to_vec();

            Ok(Self {
                rows,
                cols,
                scale: global_scale,
                version,
                sign: Vec::new(),
                extra: Vec::new(),
                block_size,
                n_blocks,
                q_scales,
                packed_codes,
            })
        } else {
            bail!("Unsupported NDA version {version} in {path:?}");
        }
    }

    /// True if loaded as v2 quad encoding.
    #[inline]
    pub fn is_quad(&self) -> bool {
        self.version == NDA_V2_QUAD
    }

    /// Fraction of effectively-zero weights (for v1 only; v2 = always 0.0).
    #[allow(dead_code)]
    pub fn sparsity(&self) -> f32 {
        if self.version != NDA_V1_TERN {
            return 0.0;
        }
        // v1: active bitmap = sign bitmap
        let ones: u32 = self.sign.iter().map(|b| b.count_ones()).sum();
        1.0 - (ones as f32) / (self.rows * self.cols) as f32
    }

    /// On-disk byte size.
    #[allow(dead_code)]
    pub fn byte_size(&self) -> usize {
        if self.version == NDA_V1_TERN || self.version == NDA_V2_QUAD {
            18 + self.sign.len() + self.extra.len()
        } else {
            24 + self.q_scales.len() + self.packed_codes.len()
        }
    }

    /// Human-readable version name.
    pub fn version_name(&self) -> &'static str {
        match self.version {
            NDA_V1_TERN => "v1 ternary {-1,0,+1}",
            NDA_V2_QUAD => "v2 quad {-2,-1,+1,+2}",
            NDA_VERSION_FP4 => "v3 FP4 E2M1",
            NDA_VERSION_FP2 => "v4 FP2 E1M0",
            _ => "unknown",
        }
    }

    /// Validate internal consistency. Returns list of error strings (empty = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let n_elems = self.rows * self.cols;

        if self.rows == 0 || self.cols == 0 {
            errors.push(format!("zero dimension: rows={}, cols={}", self.rows, self.cols));
        }

        if self.version == NDA_V1_TERN || self.version == NDA_V2_QUAD {
            let expected_bytes = n_elems.div_ceil(8);
            if self.sign.len() != expected_bytes {
                errors.push(format!(
                    "sign bitmap size mismatch: expected {}, got {}",
                    expected_bytes,
                    self.sign.len()
                ));
            }
            if self.extra.len() != expected_bytes {
                errors.push(format!(
                    "extra bitmap size mismatch: expected {}, got {}",
                    expected_bytes,
                    self.extra.len()
                ));
            }
        } else if self.version == NDA_VERSION_FP4 || self.version == NDA_VERSION_FP2 {
            if self.block_size == 0 {
                errors.push("block_size is zero".to_string());
            }
            let expected_codes = if self.version == NDA_VERSION_FP4 {
                n_elems.div_ceil(2)
            } else {
                n_elems.div_ceil(4)
            };
            if self.packed_codes.len() != expected_codes {
                errors.push(format!(
                    "packed_codes size mismatch: expected {}, got {}",
                    expected_codes,
                    self.packed_codes.len()
                ));
            }
            if self.n_blocks > 0 && self.block_size > 0 {
                let expected_n_blocks = self.cols / self.block_size;
                let expected_q_scales = self.rows * expected_n_blocks;
                if self.q_scales.len() != expected_q_scales {
                    errors.push(format!(
                        "q_scales size mismatch: expected {}, got {}",
                        expected_q_scales,
                        self.q_scales.len()
                    ));
                }
            }
        } else {
            errors.push(format!("unknown version: {}", self.version));
        }

        if self.scale.is_nan() || self.scale.is_infinite() {
            errors.push(format!("invalid scale: {}", self.scale));
        }

        errors
    }

    /// Memory breakdown by component.
    pub fn memory_breakdown(&self) -> NdaMemoryBreakdown {
        let (header, data) = if self.version == NDA_V1_TERN || self.version == NDA_V2_QUAD {
            (18, self.sign.len() + self.extra.len())
        } else {
            (24, self.q_scales.len() + self.packed_codes.len())
        };
        NdaMemoryBreakdown {
            header_bytes: header,
            data_bytes: data,
            total_bytes: header + data,
            bits_per_weight: if self.rows * self.cols > 0 {
                (data as f64 * 8.0) / (self.rows * self.cols) as f64
            } else {
                0.0
            },
        }
    }

    /// Weight value distribution for v2 quad matrices.
    /// Returns counts of {-2, -1, +1, +2}.
    pub fn quad_distribution(&self) -> [usize; 4] {
        if self.version != NDA_V2_QUAD {
            return [0; 4];
        }
        let mut counts = [0usize; 4]; // [-2, -1, +1, +2]
        let n_elems = self.rows * self.cols;
        for i in 0..n_elems {
            let byte_idx = i / 8;
            let bit_idx = i % 8;
            let s = (self.sign[byte_idx] >> bit_idx) & 1;
            let e = (self.extra[byte_idx] >> bit_idx) & 1;
            match (s, e) {
                (0, 0) => counts[0] += 1, // -2
                (0, 1) => counts[1] += 1, // -1
                (1, 0) => counts[2] += 1, // +1
                (1, 1) => counts[3] += 1, // +2
                _ => unreachable!(),
            }
        }
        counts
    }

    /// Save matrix to .nda file format (round-trip with load).
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut data = Vec::new();

        if self.version == NDA_V1_TERN || self.version == NDA_V2_QUAD {
            // Legacy header: magic(4) + version(2) + rows(4) + cols(4) + scale(4) = 18 bytes
            data.extend_from_slice(&NDA_MAGIC.to_le_bytes());
            data.extend_from_slice(&self.version.to_le_bytes());
            data.extend_from_slice(&(self.rows as u32).to_le_bytes());
            data.extend_from_slice(&(self.cols as u32).to_le_bytes());
            data.extend_from_slice(&self.scale.to_le_bytes());
            data.extend_from_slice(&self.sign);
            data.extend_from_slice(&self.extra);
        } else if self.version == NDA_VERSION_FP4 || self.version == NDA_VERSION_FP2 {
            // New header: magic(4) + version(2) + rows(2) + cols(4) + block_size(4) + n_blocks(4) + global_scale(4) = 24 bytes
            data.extend_from_slice(&NDA_MAGIC.to_le_bytes());
            data.extend_from_slice(&self.version.to_le_bytes());
            data.extend_from_slice(&(self.rows as u16).to_le_bytes());
            data.extend_from_slice(&(self.cols as u32).to_le_bytes());
            data.extend_from_slice(&(self.block_size as u32).to_le_bytes());
            data.extend_from_slice(&(self.n_blocks as u32).to_le_bytes());
            data.extend_from_slice(&self.scale.to_le_bytes());
            data.extend_from_slice(&self.q_scales);
            data.extend_from_slice(&self.packed_codes);
        } else {
            bail!("Cannot save unsupported NDA version {}", self.version);
        }

        fs::write(path, &data).with_context(|| format!("Writing NDA file: {path:?}"))?;
        Ok(())
    }
}

/// Memory breakdown for an NDA matrix.
#[derive(Debug, Clone, Serialize)]
pub struct NdaMemoryBreakdown {
    /// Header bytes (18 for v1/v2, 24 for v3/v4).
    pub header_bytes: usize,
    /// Data bytes (bitmaps or packed codes).
    pub data_bytes: usize,
    /// Total bytes (header + data).
    pub total_bytes: usize,
    /// Average bits per weight element.
    pub bits_per_weight: f64,
}

/// Statistics from batch-loading multiple NDA matrices.
#[derive(Debug, Clone, Serialize)]
pub struct NdaBatchLoadReport {
    /// Number of matrices loaded.
    pub count: usize,
    /// Total bytes across all matrices.
    pub total_bytes: usize,
    /// Total weight elements across all matrices.
    pub total_elements: usize,
    /// Version distribution: count of v1, v2, v3, v4 matrices.
    pub version_counts: [usize; 4],
    /// Validation errors found (empty = all valid).
    pub validation_errors: Vec<String>,
    /// Elapsed time to load all matrices (microseconds).
    pub elapsed_us: u64,
}

impl NdaMatrix {
    /// Load multiple .nda files from a directory matching a pattern.
    /// Returns matrices plus a batch load report.
    pub fn load_batch(dir: &Path, pattern: &str) -> Result<(Vec<NdaMatrix>, NdaBatchLoadReport)> {
        use std::time::Instant;
        let t_start = Instant::now();

        let mut matrices = Vec::new();
        let mut version_counts = [0usize; 4];
        let mut validation_errors = Vec::new();
        let mut total_bytes = 0usize;
        let mut total_elements = 0usize;

        if !dir.is_dir() {
            bail!("Not a directory: {dir:?}");
        }

        let mut entries: Vec<_> = fs::read_dir(dir)
            .with_context(|| format!("Reading directory: {dir:?}"))?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.ends_with(".nda") && n.contains(pattern))
                    .unwrap_or(false)
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            match NdaMatrix::load(&path) {
                Ok(m) => {
                    let errors = m.validate();
                    if !errors.is_empty() {
                        for err in &errors {
                            validation_errors.push(format!("{:?}: {}", path, err));
                        }
                    }
                    let v_idx = match m.version {
                        v if v == NDA_V1_TERN => 0,
                        v if v == NDA_V2_QUAD => 1,
                        v if v == NDA_VERSION_FP4 => 2,
                        v if v == NDA_VERSION_FP2 => 3,
                        _ => 1, // default to v2 bucket
                    };
                    version_counts[v_idx] += 1;
                    total_bytes += m.byte_size();
                    total_elements += m.rows * m.cols;
                    matrices.push(m);
                }
                Err(e) => {
                    validation_errors.push(format!("{:?}: load failed: {}", path, e));
                }
            }
        }

        let elapsed_us = t_start.elapsed().as_micros() as u64;
        let report = NdaBatchLoadReport {
            count: matrices.len(),
            total_bytes,
            total_elements,
            version_counts,
            validation_errors,
            elapsed_us,
        };
        Ok((matrices, report))
    }
}

// ─── GEMV kernels ─────────────────────────────────────────────────────────────

// ─── Codebook Grids ───────────────────────────────────────────────────────────

const FP4_GRID_VALUES: [f32; 16] = [
    0.0, 0.25, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 0.0, -0.25, -1.0, -1.5, -2.0, -3.0, -4.0, -6.0,
];

const FP2_GRID_VALUES: [f32; 4] = [0.0, 1.0, 0.0, -1.0];

// ─── GEMV kernels ─────────────────────────────────────────────────────────────

/// Compute `y = W · x` for an NDA matrix (auto-selects v1, v2, v3, or v4 kernel).
///
/// `x.len()` must equal `matrix.cols`.
#[inline]
pub fn nda_gemv(matrix: &NdaMatrix, x: &[f32]) -> Vec<f32> {
    if matrix.version == NDA_VERSION_FP4 {
        nda_gemv_fp4(matrix, x)
    } else if matrix.version == NDA_VERSION_FP2 {
        nda_gemv_fp2(matrix, x)
    } else if matrix.is_quad() {
        let (x_sign, x_extra, act_scale) = quantize_activations_v2_quad(x);
        nda_gemv_v2_quad_quantized(matrix, &x_sign, &x_extra, act_scale)
    } else {
        nda_gemv_v1_tern(matrix, x)
    }
}

/// FP4 (E2M1) blockwise CPU GEMV (optimized with zero inner-loop divisions)
pub fn nda_gemv_fp4(matrix: &NdaMatrix, x: &[f32]) -> Vec<f32> {
    debug_assert_eq!(
        x.len(),
        matrix.cols,
        "nda_gemv_fp4: x.len()={} != cols={}",
        x.len(),
        matrix.cols
    );
    let mut out = vec![0.0_f32; matrix.rows];
    let block_size = matrix.block_size;
    let global_scale = matrix.scale;
    let n_blocks = matrix.cols / block_size;

    out.par_iter_mut().enumerate().for_each(|(row, out_val)| {
        let row_start = row * matrix.cols;
        let mut acc = 0.0_f32;

        for block_idx in 0..n_blocks {
            let q_scale = matrix.q_scales[row * n_blocks + block_idx] as f32;
            let scale = q_scale * global_scale;
            let block_start_col = block_idx * block_size;
            let start_byte = (row_start + block_start_col) / 2;
            let col_pairs = block_size / 2;

            for (byte_idx, pair) in (start_byte..).zip(0..col_pairs) {
                let col0 = block_start_col + pair * 2;
                let byte = matrix.packed_codes[byte_idx];

                let code0 = (byte & 0x0F) as usize;
                let code1 = ((byte >> 4) & 0x0F) as usize;

                acc += x[col0] * FP4_GRID_VALUES[code0] * scale;
                acc += x[col0 + 1] * FP4_GRID_VALUES[code1] * scale;
            }
        }
        *out_val = acc;
    });

    out
}

/// FP2 (E1M0) blockwise CPU GEMV (optimized with zero inner-loop divisions)
pub fn nda_gemv_fp2(matrix: &NdaMatrix, x: &[f32]) -> Vec<f32> {
    debug_assert_eq!(
        x.len(),
        matrix.cols,
        "nda_gemv_fp2: x.len()={} != cols={}",
        x.len(),
        matrix.cols
    );
    let mut out = vec![0.0_f32; matrix.rows];
    let block_size = matrix.block_size;
    let global_scale = matrix.scale;
    let n_blocks = matrix.cols / block_size;

    out.par_iter_mut().enumerate().for_each(|(row, out_val)| {
        let row_start = row * matrix.cols;
        let mut acc = 0.0_f32;

        for block_idx in 0..n_blocks {
            let q_scale = matrix.q_scales[row * n_blocks + block_idx] as f32;
            let scale = q_scale * global_scale;
            let block_start_col = block_idx * block_size;
            let start_byte = (row_start + block_start_col) / 4;
            let col_quads = block_size / 4;

            for (byte_idx, quad) in (start_byte..).zip(0..col_quads) {
                let col0 = block_start_col + quad * 4;
                let byte = matrix.packed_codes[byte_idx];

                let code0 = (byte & 0x03) as usize;
                let code1 = ((byte >> 2) & 0x03) as usize;
                let code2 = ((byte >> 4) & 0x03) as usize;
                let code3 = ((byte >> 6) & 0x03) as usize;

                acc += x[col0] * FP2_GRID_VALUES[code0] * scale;
                acc += x[col0 + 1] * FP2_GRID_VALUES[code1] * scale;
                acc += x[col0 + 2] * FP2_GRID_VALUES[code2] * scale;
                acc += x[col0 + 3] * FP2_GRID_VALUES[code3] * scale;
            }
        }
        *out_val = acc;
    });

    out
}

/// Quantize a float32 activation vector to quaternary {-2, -1, +1, +2} represented as sign + extra bitmaps.
pub fn quantize_activations_v2_quad(x: &[f32]) -> (Vec<u8>, Vec<u8>, f32) {
    let amax = x.iter().map(|&v| v.abs()).fold(0.0_f32, f32::max);
    let scale = if amax < 1e-8 { 1.0 } else { amax / 2.0 };
    let inv_scale = 1.0 / scale;

    let bitmap_bytes = x.len().div_ceil(8);
    let mut sign = vec![0u8; bitmap_bytes];
    let mut extra = vec![0u8; bitmap_bytes];

    for (i, &v) in x.iter().enumerate() {
        let val_scaled = v * inv_scale;
        let is_large = val_scaled.abs() >= 1.5;
        let is_pos = v >= 0.0;

        let sign_bit = if is_pos { 1 } else { 0 };
        let is_large_bit = if is_large { 1 } else { 0 };
        let extra_bit = !(sign_bit ^ is_large_bit) & 1;

        let byte_idx = i / 8;
        let bit_idx = i % 8;

        if sign_bit == 1 {
            sign[byte_idx] |= 1 << bit_idx;
        }
        if extra_bit == 1 {
            extra[byte_idx] |= 1 << bit_idx;
        }
    }

    (sign, extra, scale)
}

/// **v2 quad GEMV with 2-bit activations** — both weights and activations are {-2, -1, +1, +2}.
/// Uses bitwise popcounts to compute the dot products, eliminating all float arithmetic in the loop.
pub fn nda_gemv_v2_quad_quantized(
    matrix: &NdaMatrix,
    x_sign: &[u8],
    x_extra: &[u8],
    act_scale: f32,
) -> Vec<f32> {
    debug_assert!(
        matrix.is_quad(),
        "nda_gemv_v2_quad_quantized requires v2 quad matrix"
    );
    debug_assert_eq!(x_sign.len(), matrix.cols.div_ceil(8));

    let stride = matrix.cols.div_ceil(8);
    let out_scale = matrix.scale * act_scale;
    let mut out = vec![0.0_f32; matrix.rows];

    out.par_iter_mut().enumerate().for_each(|(row, out_val)| {
        let base = row * stride;
        let mut acc = 0_i32;

        for byte_idx in 0..stride {
            let w_sign = matrix.sign[base + byte_idx];
            let w_extra = matrix.extra[base + byte_idx];
            let x_s = x_sign[byte_idx];
            let x_e = x_extra[byte_idx];

            let same_sign = !(w_sign ^ x_s);
            let diff_sign = w_sign ^ x_s;

            let w_large = !(w_sign ^ w_extra);
            let x_large = !(x_s ^ x_e);

            let same_w_large = same_sign & w_large;
            let same_x_large = same_sign & x_large;
            let same_both_large = same_w_large & x_large;

            let diff_w_large = diff_sign & w_large;
            let diff_x_large = diff_sign & x_large;
            let diff_both_large = diff_w_large & x_large;

            let pos_contrib = same_sign.count_ones()
                + same_w_large.count_ones()
                + same_x_large.count_ones()
                + same_both_large.count_ones();

            let neg_contrib = diff_sign.count_ones()
                + diff_w_large.count_ones()
                + diff_x_large.count_ones()
                + diff_both_large.count_ones();

            acc += (pos_contrib as i32) - (neg_contrib as i32);
        }

        *out_val = (acc as f32) * out_scale;
    });

    out
}

/// **v1 ternary GEMV** — {−1, 0, +1}, legacy backward-compat path.
///
/// sign=active bitmap, extra=pos bitmap.
pub fn nda_gemv_v1_tern(matrix: &NdaMatrix, x: &[f32]) -> Vec<f32> {
    debug_assert_eq!(
        x.len(),
        matrix.cols,
        "nda_gemv_v1: x.len()={} != cols={}",
        x.len(),
        matrix.cols
    );

    let stride = matrix.cols.div_ceil(8);
    let scale = matrix.scale;
    let mut out = vec![0.0_f32; matrix.rows];

    out.par_iter_mut().enumerate().for_each(|(row, out_val)| {
        let base = row * stride;
        let mut acc = 0.0_f32;

        for byte_idx in 0..stride {
            let act = matrix.sign[base + byte_idx]; // active bitmap
            if act == 0 {
                continue;
            } // skip all-zero bytes
            let pos_b = matrix.extra[base + byte_idx];
            let bit_start = byte_idx * 8;
            let mut temp = act;

            while temp > 0 {
                let bit = temp.trailing_zeros() as usize;
                let mask = 1_u8 << bit;
                // SAFETY: `bit_start + bit` is within `x.len()` because `byte_idx < stride`
                // and `stride * 8 >= matrix.cols == x.len()`. The bit is set in `temp`,
                // so `bit < 8` and `bit_start + bit < (byte_idx + 1) * 8 <= stride * 8`.
                let xi = unsafe { *x.get_unchecked(bit_start + bit) };
                if pos_b & mask != 0 {
                    acc += xi;
                } else {
                    acc -= xi;
                }
                temp &= temp - 1;
            }
        }

        *out_val = acc * scale;
    });

    out
}

/// Diagnostic snapshot of an NdaMatrix.
#[derive(Debug, Clone, Serialize)]
pub struct NdaMatrixInfo {
    pub rows: usize,
    pub cols: usize,
    pub version: u16,
    pub version_name: String,
    pub scale: f32,
    pub memory: NdaMemoryBreakdown,
    pub validation_issues: usize,
    pub is_quad: bool,
}

/// Timing report for a GEMV operation.
#[derive(Debug, Clone, Serialize)]
pub struct GemvReport {
    pub rows: usize,
    pub cols: usize,
    pub version: u16,
    pub elapsed_us: u64,
    pub output_len: usize,
}

/// Timing report for a batch GEMV operation.
#[derive(Debug, Clone, Serialize)]
pub struct BatchGemvReport {
    pub count: usize,
    pub total_elapsed_us: u64,
    pub per_op_avg_us: f64,
    pub total_rows: usize,
}

impl NdaMatrix {
    /// Return a diagnostic snapshot of this matrix.
    pub fn info(&self) -> NdaMatrixInfo {
        NdaMatrixInfo {
            rows: self.rows,
            cols: self.cols,
            version: self.version,
            version_name: self.version_name().to_string(),
            scale: self.scale,
            memory: self.memory_breakdown(),
            validation_issues: self.validate().len(),
            is_quad: self.is_quad(),
        }
    }
}

/// Batch GEMV: compute `y_i = W · x_i` for multiple input vectors.
/// Returns outputs and a timing report.
pub fn nda_gemv_batch(
    matrix: &NdaMatrix,
    xs: &[Vec<f32>],
) -> (Vec<Vec<f32>>, BatchGemvReport) {
    use std::time::Instant;
    let start = Instant::now();
    let outputs: Vec<Vec<f32>> = xs.iter().map(|x| nda_gemv(matrix, x)).collect();
    let elapsed = start.elapsed().as_micros() as u64;
    let report = BatchGemvReport {
        count: xs.len(),
        total_elapsed_us: elapsed,
        per_op_avg_us: if xs.is_empty() {
            0.0
        } else {
            elapsed as f64 / xs.len() as f64
        },
        total_rows: matrix.rows * xs.len(),
    };
    (outputs, report)
}

/// GEMV with timing diagnostics.
/// Returns the output vector and a report.
pub fn nda_gemv_with_report(matrix: &NdaMatrix, x: &[f32]) -> (Vec<f32>, GemvReport) {
    use std::time::Instant;
    let start = Instant::now();
    let out = nda_gemv(matrix, x);
    let elapsed = start.elapsed().as_micros() as u64;
    let report = GemvReport {
        rows: matrix.rows,
        cols: matrix.cols,
        version: matrix.version,
        elapsed_us: elapsed,
        output_len: out.len(),
    };
    (out, report)
}

/// Batch quantize multiple activation vectors to v2 quad format.
/// Returns (sign_bitmaps, extra_bitmaps, scales) for each input.
pub fn quantize_activations_v2_quad_batch(
    xs: &[Vec<f32>],
) -> Vec<(Vec<u8>, Vec<u8>, f32)> {
    xs.iter()
        .map(|x| quantize_activations_v2_quad(x))
        .collect()
}

/// Batch quantize multiple activation vectors to INT8 format.
pub fn quantize_activations_i8_batch(xs: &[Vec<f32>]) -> Vec<(Vec<i8>, f32)> {
    xs.iter()
        .map(|x| quantize_activations_i8(x))
        .collect()
}

// ─── INT8 activation variant (for GPU prep / future INT4 SIMD) ────────────────

/// Quantize a f32 activation vector to INT8, returning (quantized, scale).
///
/// Dynamic per-token symmetric quantization (like LLM.int8()):
///   scale = max(|x|) / 127.0
///   q[i]  = round(x[i] / scale)  clamped to [-127, 127]
pub fn quantize_activations_i8(x: &[f32]) -> (Vec<i8>, f32) {
    let amax = x.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
    if amax < 1e-8 {
        return (vec![0i8; x.len()], 1.0);
    }
    let act_scale = amax / 127.0;
    let inv_scale = 1.0 / act_scale;
    let q: Vec<i8> = x
        .iter()
        .map(|v| (v * inv_scale).round().clamp(-127.0, 127.0) as i8)
        .collect();
    (q, act_scale)
}

/// INT8-activation GEMV for v2 quad matrices.
///
/// Same pure-add/subtract logic but operates on i8 activations.
/// Accumulator is i32 (no overflow for typical sequence lengths).
/// Returns f32 output after dequantizing: y = (W·q) * (weight_scale * act_scale).
pub fn nda_gemv_v2_i8(matrix: &NdaMatrix, q: &[i8], act_scale: f32) -> Vec<f32> {
    debug_assert!(matrix.is_quad(), "nda_gemv_v2_i8 requires v2 quad matrix");
    debug_assert_eq!(q.len(), matrix.cols);

    let stride = matrix.cols.div_ceil(8);
    let out_scale = matrix.scale * act_scale;
    let mut out = vec![0.0_f32; matrix.rows];

    out.par_iter_mut().enumerate().for_each(|(row, out_val)| {
        let base = row * stride;
        let mut acc = 0_i32;

        for byte_idx in 0..stride {
            let sign_byte = matrix.sign[base + byte_idx];
            let extra_byte = matrix.extra[base + byte_idx];
            let bit_start = byte_idx * 8;
            let bit_end = ((bit_start + 8).min(matrix.cols)) - bit_start;

            for bit in 0..bit_end {
                let s = (sign_byte >> bit) & 1;
                let e = (extra_byte >> bit) & 1;
                // SAFETY: `bit_start + bit` is within `q.len()` because `bit < bit_end`
                // and `bit_start + bit_end <= matrix.cols <= q.len()`.
                let qi = unsafe { *q.get_unchecked(bit_start + bit) } as i32;

                // Step A: +qi or -qi (no multiply — just conditional negate)
                let contrib = if s == 1 { qi } else { -qi };
                acc += contrib;

                // Step B: double if XNOR (magnitude = 2)
                if s == e {
                    acc += contrib;
                }
            }
        }

        // Dequantize: integer accumulator → float
        *out_val = (acc as f32) * out_scale;
    });

    out
}

// ─── Benchmark harness ────────────────────────────────────────────────────────

/// Quick synthetic benchmark for v2 quad GEMV.
pub fn run_nda_benchmark() {
    use rand::Rng;
    use std::time::Instant;

    println!("V.E.L.O.C.I.T.Y.-IDE  NDA v2 GEMV benchmark");
    println!("=============================================");
    println!("Encoding: {{-2,-1,+1,+2}} pure-additive (no multiplications)");
    println!();

    let mut rng = rand::thread_rng();

    for (label, rows, cols) in [
        ("QKV proj  3200\u{00d7}3200", 3200_usize, 3200_usize),
        ("FFN gate  8640\u{00d7}3200", 8640_usize, 3200_usize),
        ("FFN down  3200\u{00d7}8640", 3200_usize, 8640_usize),
        ("LM head  32002\u{00d7}3200", 32002_usize, 3200_usize),
    ] {
        let bitmap_bytes = (rows * cols).div_ceil(8);

        // Synthetic v2 matrix: balanced {-2,-1,+1,+2} distribution (~25% each)
        // sign: random, extra: random → gives roughly equal 4-way split
        let sign: Vec<u8> = (0..bitmap_bytes).map(|_| rng.gen::<u8>()).collect();
        let extra: Vec<u8> = (0..bitmap_bytes).map(|_| rng.gen::<u8>()).collect();

        let matrix = NdaMatrix::new_quad(rows, cols, 1.0, sign, extra);

        let x: Vec<f32> = (0..cols).map(|_| rng.gen_range(-1.0_f32..1.0)).collect();

        // Warm-up
        let _ = nda_gemv(&matrix, &x);

        const ITERS: u32 = 10;
        let t0 = Instant::now();
        for _ in 0..ITERS {
            std::hint::black_box(nda_gemv(&matrix, &x));
        }
        let elapsed = t0.elapsed();
        let ms_per = elapsed.as_secs_f64() * 1000.0 / ITERS as f64;
        let gops = (rows * cols * 2) as f64 / (ms_per * 1e6); // ×2: each weight = up to 2 adds

        println!(
            "  {:26}  {:6.2} ms/call  {:.2} GOps  [v2 quad 2-bit popcount, no mul]",
            label, ms_per, gops
        );

        // Also bench INT8 variant
        let (q, act_scale) = quantize_activations_i8(&x);
        let _ = nda_gemv_v2_i8(&matrix, &q, act_scale);

        let t0 = Instant::now();
        for _ in 0..ITERS {
            std::hint::black_box(nda_gemv_v2_i8(&matrix, &q, act_scale));
        }
        let elapsed_i8 = t0.elapsed();
        let ms_i8 = elapsed_i8.as_secs_f64() * 1000.0 / ITERS as f64;
        println!("  {:26}  {:6.2} ms/call  [v2 INT8 acts]", label, ms_i8);
        println!();
    }
}

/// Report on quantization quality for an activation vector.
#[derive(Debug, Clone, Serialize)]
pub struct NdaQuantizationReport {
    pub input_len: usize,
    pub output_scale: f32,
    pub input_amax: f32,
    pub max_abs_error: f64,
    pub mean_abs_error: f64,
    pub compression_ratio: f64,
    pub validation_issues: Vec<String>,
}

/// Quantize an f32 activation vector and produce a quality report.
pub fn quantize_with_report(x: &[f32]) -> ((Vec<u8>, Vec<u8>, f32), NdaQuantizationReport) {
    let result = quantize_activations_v2_quad(x);
    let (sign, extra, scale) = &result;

    // Reconstruct approximate values for error measurement
    let mut max_abs_error = 0.0f64;
    let mut sum_abs_error = 0.0f64;
    for (i, &orig) in x.iter().enumerate() {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        let mask = 1u8 << bit_idx;
        let is_pos = (sign[byte_idx] & mask) != 0;
        let is_large = (sign[byte_idx] & mask) == (extra[byte_idx] & mask);
        let mag = if is_large { 2.0 } else { 1.0 };
        let reconstructed = if is_pos { mag } else { -mag };
        let approx = reconstructed * (*scale) as f64;
        let err = (orig as f64 - approx).abs();
        max_abs_error = max_abs_error.max(err);
        sum_abs_error += err;
    }
    let mean_abs_error = if x.is_empty() { 0.0 } else { sum_abs_error / x.len() as f64 };

    let input_bytes = x.len() * 4;
    let output_bytes = sign.len() + extra.len();
    let compression_ratio = if output_bytes > 0 {
        input_bytes as f64 / output_bytes as f64
    } else {
        0.0
    };

    let amax = x.iter().map(|&v| v.abs()).fold(0.0_f32, f32::max);

    let mut issues = Vec::new();
    if x.is_empty() {
        issues.push("input is empty".into());
    }
    if *scale < 1e-10 {
        issues.push(format!("scale is near zero: {}", scale));
    }

    let report = NdaQuantizationReport {
        input_len: x.len(),
        output_scale: *scale,
        input_amax: amax,
        max_abs_error,
        mean_abs_error,
        compression_ratio,
        validation_issues: issues,
    };
    (result, report)
}

/// Validate that two matrices are compatible for concatenation or chaining.
pub fn validate_matrix_compatibility(a: &NdaMatrix, b: &NdaMatrix) -> Vec<String> {
    let mut issues = Vec::new();
    if a.cols != b.rows {
        issues.push(format!(
            "dimension mismatch: a.cols ({}) != b.rows ({})",
            a.cols, b.rows
        ));
    }
    if a.version != b.version {
        issues.push(format!(
            "version mismatch: a.version ({}) != b.version ({})",
            a.version, b.version
        ));
    }
    issues
}

/// Aggregate summary of multiple NDA matrices.
#[derive(Debug, Clone, Serialize)]
pub struct NdaMatrixSummary {
    pub matrix_count: usize,
    pub total_rows: usize,
    pub total_cols: usize,
    pub total_memory_bytes: usize,
    pub versions: Vec<u16>,
    pub largest_matrix: Option<String>,
    pub smallest_matrix: Option<String>,
    pub validation_issues: Vec<String>,
}

/// Summarize a collection of NDA matrices.
pub fn summarize_matrices(matrices: &[NdaMatrix]) -> NdaMatrixSummary {
    let total_rows: usize = matrices.iter().map(|m| m.rows).sum();
    let total_cols: usize = matrices.iter().map(|m| m.cols).sum();
    let total_memory: usize = matrices.iter().map(|m| m.memory_breakdown().total_bytes).sum();
    let versions: Vec<u16> = matrices.iter().map(|m| m.version).collect();

    let mut largest: Option<(usize, String)> = None;
    let mut smallest: Option<(usize, String)> = None;
    for (i, m) in matrices.iter().enumerate() {
        let size = m.rows * m.cols;
        let label = format!("matrix[{}] ({}x{})", i, m.rows, m.cols);
        if largest.as_ref().is_none_or(|(s, _)| size > *s) {
            largest = Some((size, label.clone()));
        }
        if smallest.as_ref().is_none_or(|(s, _)| size < *s) {
            smallest = Some((size, label));
        }
    }

    let mut issues = Vec::new();
    if matrices.is_empty() {
        issues.push("no matrices to summarize".into());
    }

    NdaMatrixSummary {
        matrix_count: matrices.len(),
        total_rows,
        total_cols,
        total_memory_bytes: total_memory,
        versions,
        largest_matrix: largest.map(|(_, l)| l),
        smallest_matrix: smallest.map(|(_, l)| l),
        validation_issues: issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_quad_matrix(rows: usize, cols: usize) -> NdaMatrix {
        let bitmap_bytes = (rows * cols).div_ceil(8);
        let sign = vec![0xAA; bitmap_bytes]; // alternating 10101010
        let extra = vec![0x55; bitmap_bytes]; // alternating 01010101
        NdaMatrix::new_quad(rows, cols, 1.0, sign, extra)
    }

    #[test]
    fn test_version_name() {
        let m = make_quad_matrix(8, 8);
        assert_eq!(m.version_name(), "v2 quad {-2,-1,+1,+2}");
    }

    #[test]
    fn test_validate_valid_quad() {
        let m = make_quad_matrix(16, 32);
        let errors = m.validate();
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_validate_bad_sign_size() {
        let mut m = make_quad_matrix(8, 8);
        m.sign.push(0); // corrupt: extra byte
        let errors = m.validate();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.contains("sign bitmap size mismatch")));
    }

    #[test]
    fn test_validate_zero_dimension() {
        let m = NdaMatrix::new_quad(0, 8, 1.0, vec![], vec![]);
        let errors = m.validate();
        assert!(errors.iter().any(|e| e.contains("zero dimension")));
    }

    #[test]
    fn test_validate_nan_scale() {
        let mut m = make_quad_matrix(8, 8);
        m.scale = f32::NAN;
        let errors = m.validate();
        assert!(errors.iter().any(|e| e.contains("invalid scale")));
    }

    #[test]
    fn test_memory_breakdown_quad() {
        let m = make_quad_matrix(64, 64);
        let bd = m.memory_breakdown();
        assert_eq!(bd.header_bytes, 18);
        assert_eq!(bd.data_bytes, 2 * (64_usize * 64).div_ceil(8));
        assert_eq!(bd.total_bytes, 18 + bd.data_bytes);
        assert!((bd.bits_per_weight - 2.0).abs() < 0.1);
    }

    #[test]
    fn test_memory_breakdown_serialize() {
        let bd = NdaMemoryBreakdown {
            header_bytes: 18,
            data_bytes: 1024,
            total_bytes: 1042,
            bits_per_weight: 2.0,
        };
        let json = serde_json::to_string(&bd).unwrap();
        assert!(json.contains("\"bits_per_weight\":2.0"));
    }

    #[test]
    fn test_quad_distribution() {
        // sign=0xAA=10101010, extra=0x55=01010101
        // bit pattern: s=0,e=1 -> -1; s=1,e=0 -> +1
        // So for each byte: 4 bits are (s=0,e=1)=-1, 4 bits are (s=1,e=0)=+1
        let m = make_quad_matrix(8, 8);
        let dist = m.quad_distribution();
        assert_eq!(dist[0], 0); // no -2
        assert_eq!(dist[1], 32); // half are -1
        assert_eq!(dist[2], 32); // half are +1
        assert_eq!(dist[3], 0); // no +2
    }

    #[test]
    fn test_quad_distribution_non_quad_returns_zero() {
        let m = NdaMatrix {
            rows: 4,
            cols: 4,
            scale: 1.0,
            version: NDA_VERSION_FP4,
            sign: vec![],
            extra: vec![],
            block_size: 64,
            n_blocks: 0,
            q_scales: vec![],
            packed_codes: vec![],
        };
        assert_eq!(m.quad_distribution(), [0; 4]);
    }

    #[test]
    fn test_save_load_roundtrip_quad() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.nda");
        let original = make_quad_matrix(32, 64);
        original.save(&path).unwrap();
        let loaded = NdaMatrix::load(&path).unwrap();
        assert_eq!(loaded.rows, original.rows);
        assert_eq!(loaded.cols, original.cols);
        assert_eq!(loaded.version, original.version);
        assert_eq!(loaded.scale, original.scale);
        assert_eq!(loaded.sign, original.sign);
        assert_eq!(loaded.extra, original.extra);
    }

    #[test]
    fn test_save_load_roundtrip_fp4() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test_fp4.nda");
        let rows = 4usize;
        let cols = 64usize;
        let block_size = 64usize;
        let n_blocks_per_row = cols / block_size; // 1
        let total_q_scales = rows * n_blocks_per_row; // 4
        // The file format's n_blocks field = total q_scale entries
        let original = NdaMatrix {
            rows,
            cols,
            scale: 0.5,
            version: NDA_VERSION_FP4,
            sign: vec![],
            extra: vec![],
            block_size,
            n_blocks: total_q_scales, // file format stores total count
            q_scales: vec![128; total_q_scales],
            packed_codes: vec![0xAB; (rows * cols).div_ceil(2)],
        };
        original.save(&path).unwrap();
        let loaded = NdaMatrix::load(&path).unwrap();
        assert_eq!(loaded.rows, 4);
        assert_eq!(loaded.cols, 64);
        assert_eq!(loaded.version, NDA_VERSION_FP4);
        assert_eq!(loaded.block_size, 64);
        assert_eq!(loaded.q_scales, original.q_scales);
        assert_eq!(loaded.packed_codes, original.packed_codes);
    }

    #[test]
    fn test_nda_serialization_json() {
        let m = make_quad_matrix(8, 16);
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"rows\":8"));
        assert!(json.contains("\"cols\":16"));
        assert!(json.contains("\"version\":2"));
    }

    #[test]
    fn test_batch_load_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let (matrices, report) = NdaMatrix::load_batch(tmp.path(), "model").unwrap();
        assert_eq!(matrices.len(), 0);
        assert_eq!(report.count, 0);
        assert_eq!(report.total_bytes, 0);
    }

    #[test]
    fn test_batch_load_with_files() {
        let tmp = tempfile::tempdir().unwrap();
        // Create two .nda files
        let m1 = make_quad_matrix(8, 8);
        let m2 = make_quad_matrix(16, 16);
        m1.save(&tmp.path().join("model_layer_0_q.nda")).unwrap();
        m2.save(&tmp.path().join("model_layer_0_k.nda")).unwrap();
        // Also create a non-matching file
        m1.save(&tmp.path().join("other_file.nda")).unwrap();

        let (matrices, report) = NdaMatrix::load_batch(tmp.path(), "model_layer_0").unwrap();
        assert_eq!(matrices.len(), 2);
        assert_eq!(report.count, 2);
        assert!(report.validation_errors.is_empty());
        assert_eq!(report.version_counts[1], 2); // both v2
    }

    #[test]
    fn test_batch_load_report_serialize() {
        let report = NdaBatchLoadReport {
            count: 10,
            total_bytes: 50000,
            total_elements: 200000,
            version_counts: [0, 8, 2, 0],
            validation_errors: vec![],
            elapsed_us: 5000,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"count\":10"));
        assert!(json.contains("\"total_elements\":200000"));
    }

    #[test]
    fn test_gemv_quad_roundtrip() {
        let m = make_quad_matrix(4, 8);
        let x: Vec<f32> = (0..8).map(|i| i as f32 * 0.1).collect();
        let y = nda_gemv(&m, &x);
        assert_eq!(y.len(), 4);
        // Output should be finite
        for &v in &y {
            assert!(v.is_finite());
        }
    }

    #[test]
    fn test_quantize_activations_v2_quad() {
        let x = vec![0.0, 1.0, -1.0, 2.0, -2.0, 0.5, -0.5, 1.5];
        let (sign, extra, scale) = quantize_activations_v2_quad(&x);
        assert_eq!(sign.len(), 1);
        assert_eq!(extra.len(), 1);
        assert!(scale > 0.0);
    }

    #[test]
    fn test_quantize_i8() {
        let x = vec![0.0, 0.5, -0.5, 1.0, -1.0];
        let (q, scale) = quantize_activations_i8(&x);
        assert_eq!(q.len(), 5);
        assert!(scale > 0.0);
        assert_eq!(q[0], 0); // zero maps to zero
    }

    #[test]
    fn test_quantize_i8_zero_input() {
        let x = vec![0.0; 4];
        let (q, scale) = quantize_activations_i8(&x);
        assert_eq!(q, vec![0i8; 4]);
        assert_eq!(scale, 1.0); // fallback scale for zero input
    }

    #[test]
    fn test_nda_matrix_info() {
        let m = make_quad_matrix(16, 32);
        let info = m.info();
        assert_eq!(info.rows, 16);
        assert_eq!(info.cols, 32);
        assert!(info.is_quad);
        assert_eq!(info.version, 2);
        assert_eq!(info.validation_issues, 0);
        assert!(info.memory.total_bytes > 0);
    }

    #[test]
    fn test_nda_matrix_info_serializes() {
        let m = make_quad_matrix(8, 8);
        let info = m.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"rows\":8"));
        assert!(json.contains("\"is_quad\":true"));
    }

    #[test]
    fn test_gemv_with_report() {
        let m = make_quad_matrix(4, 8);
        let x: Vec<f32> = (0..8).map(|i| i as f32 * 0.1).collect();
        let (out, report) = nda_gemv_with_report(&m, &x);
        assert_eq!(out.len(), 4);
        assert_eq!(report.rows, 4);
        assert_eq!(report.cols, 8);
        assert_eq!(report.version, 2);
        assert_eq!(report.output_len, 4);
    }

    #[test]
    fn test_gemv_batch() {
        let m = make_quad_matrix(4, 8);
        let xs: Vec<Vec<f32>> = (0..3)
            .map(|k| (0..8).map(|i| (i + k) as f32 * 0.1).collect())
            .collect();
        let (outputs, report) = nda_gemv_batch(&m, &xs);
        assert_eq!(outputs.len(), 3);
        for out in &outputs {
            assert_eq!(out.len(), 4);
        }
        assert_eq!(report.count, 3);
        assert_eq!(report.total_rows, 12); // 4 rows * 3 inputs
    }

    #[test]
    fn test_gemv_batch_empty() {
        let m = make_quad_matrix(4, 8);
        let (outputs, report) = nda_gemv_batch(&m, &[]);
        assert_eq!(outputs.len(), 0);
        assert_eq!(report.count, 0);
        assert_eq!(report.per_op_avg_us, 0.0);
    }

    #[test]
    fn test_quantize_batch_v2() {
        let xs = vec![
            vec![0.0, 1.0, -1.0, 2.0],
            vec![0.5, -0.5, 1.5, -1.5],
        ];
        let results = quantize_activations_v2_quad_batch(&xs);
        assert_eq!(results.len(), 2);
        for (sign, extra, scale) in &results {
            assert_eq!(sign.len(), 1);
            assert_eq!(extra.len(), 1);
            assert!(*scale > 0.0);
        }
    }

    #[test]
    fn test_quantize_batch_i8() {
        let xs = vec![
            vec![0.0, 0.5, -0.5, 1.0],
            vec![1.0, -1.0, 0.0, 0.5],
        ];
        let results = quantize_activations_i8_batch(&xs);
        assert_eq!(results.len(), 2);
        for (q, scale) in &results {
            assert_eq!(q.len(), 4);
            assert!(*scale > 0.0);
        }
    }

    #[test]
    fn test_gemv_report_serializes() {
        let report = GemvReport {
            rows: 32,
            cols: 64,
            version: 2,
            elapsed_us: 150,
            output_len: 32,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"rows\":32"));
        assert!(json.contains("\"elapsed_us\":150"));
    }

    #[test]
    fn test_batch_gemv_report_serializes() {
        let report = BatchGemvReport {
            count: 5,
            total_elapsed_us: 1000,
            per_op_avg_us: 200.0,
            total_rows: 160,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"count\":5"));
        assert!(json.contains("\"total_rows\":160"));
    }

    #[test]
    fn quantize_with_report_basic() {
        let input = vec![1.0, -1.0, 0.5, -0.5, 2.0, -2.0, 0.0, 0.1];
        let ((sign, extra, scale), report) = quantize_with_report(&input);
        assert_eq!(report.input_len, 8);
        assert_eq!(sign.len(), 1);
        assert_eq!(extra.len(), 1);
        assert!(report.output_scale > 0.0);
        assert!(report.compression_ratio > 1.0);
        assert!(report.validation_issues.is_empty());
    }

    #[test]
    fn quantize_with_report_empty() {
        let input: Vec<f32> = vec![];
        let (_, report) = quantize_with_report(&input);
        assert_eq!(report.input_len, 0);
        assert!(!report.validation_issues.is_empty());
    }

    #[test]
    fn quantize_with_report_serializes() {
        let input = vec![1.0, -1.0, 2.0, -2.0];
        let (_, report) = quantize_with_report(&input);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"input_len\":4"));
        assert!(json.contains("\"compression_ratio\""));
    }

    #[test]
    fn validate_matrix_compatibility_compatible() {
        let a = make_quad_matrix(16, 64);
        let b = make_quad_matrix(64, 32);
        let issues = validate_matrix_compatibility(&a, &b);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_matrix_compatibility_dimension_mismatch() {
        let a = make_quad_matrix(16, 64);
        let b = make_quad_matrix(32, 16); // b.rows=32 != a.cols=64
        let issues = validate_matrix_compatibility(&a, &b);
        assert!(issues.iter().any(|i| i.contains("dimension mismatch")));
    }

    #[test]
    fn validate_matrix_compatibility_version_mismatch() {
        let a = make_quad_matrix(16, 64);
        let mut b = make_quad_matrix(64, 32);
        b.version = NDA_VERSION_FP4;
        let issues = validate_matrix_compatibility(&a, &b);
        assert!(issues.iter().any(|i| i.contains("version mismatch")));
    }

    #[test]
    fn summarize_matrices_basic() {
        let matrices = vec![
            make_quad_matrix(16, 64),
            make_quad_matrix(32, 128),
            make_quad_matrix(8, 32),
        ];
        let summary = summarize_matrices(&matrices);
        assert_eq!(summary.matrix_count, 3);
        assert_eq!(summary.total_rows, 56);
        assert_eq!(summary.total_cols, 224);
        assert!(summary.total_memory_bytes > 0);
        assert!(summary.largest_matrix.is_some());
        assert!(summary.smallest_matrix.is_some());
    }

    #[test]
    fn summarize_matrices_empty() {
        let summary = summarize_matrices(&[]);
        assert_eq!(summary.matrix_count, 0);
        assert!(!summary.validation_issues.is_empty());
    }

    #[test]
    fn summarize_matrices_serializes() {
        let matrices = vec![make_quad_matrix(8, 8)];
        let summary = summarize_matrices(&matrices);
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"matrix_count\":1"));
        assert!(json.contains("\"total_memory_bytes\""));
    }

    // ─── Block 87: comprehensive tests ─────────────────────────────────────

    // ── version_name coverage ────────────────────────────────────────────────

    #[test]
    fn version_name_all_variants() {
        let m1 = NdaMatrix::new_quad(8, 8, 1.0, vec![0; 8], vec![0; 8]);
        assert_eq!(m1.version_name(), "v2 quad {-2,-1,+1,+2}");

        let m_tern = NdaMatrix {
            rows: 8, cols: 8, scale: 1.0, version: NDA_V1_TERN,
            sign: vec![0; 8], extra: vec![0; 8],
            block_size: 0, n_blocks: 0, q_scales: vec![], packed_codes: vec![],
        };
        assert_eq!(m_tern.version_name(), "v1 ternary {-1,0,+1}");

        let m_fp4 = NdaMatrix {
            rows: 4, cols: 64, scale: 1.0, version: NDA_VERSION_FP4,
            sign: vec![], extra: vec![],
            block_size: 64, n_blocks: 4, q_scales: vec![128; 4], packed_codes: vec![0; 128],
        };
        assert_eq!(m_fp4.version_name(), "v3 FP4 E2M1");

        let m_fp2 = NdaMatrix {
            rows: 4, cols: 64, scale: 1.0, version: NDA_VERSION_FP2,
            sign: vec![], extra: vec![],
            block_size: 64, n_blocks: 4, q_scales: vec![128; 4], packed_codes: vec![0; 64],
        };
        assert_eq!(m_fp2.version_name(), "v4 FP2 E1M0");

        let m_unknown = NdaMatrix {
            rows: 8, cols: 8, scale: 1.0, version: 99,
            sign: vec![0; 8], extra: vec![0; 8],
            block_size: 0, n_blocks: 0, q_scales: vec![], packed_codes: vec![],
        };
        assert_eq!(m_unknown.version_name(), "unknown");
    }

    // ── validate edge cases ──────────────────────────────────────────────────

    #[test]
    fn validate_extra_bitmap_mismatch() {
        let mut m = make_quad_matrix(8, 8);
        m.extra.push(0); // corrupt extra
        let errors = m.validate();
        assert!(errors.iter().any(|e| e.contains("extra bitmap size mismatch")),
            "expected extra bitmap error, got: {:?}", errors);
    }

    #[test]
    fn validate_infinite_scale() {
        let mut m = make_quad_matrix(8, 8);
        m.scale = f32::INFINITY;
        let errors = m.validate();
        assert!(errors.iter().any(|e| e.contains("invalid scale")));
    }

    #[test]
    fn validate_fp4_zero_block_size() {
        let m = NdaMatrix {
            rows: 4, cols: 64, scale: 1.0, version: NDA_VERSION_FP4,
            sign: vec![], extra: vec![],
            block_size: 0, n_blocks: 0,
            q_scales: vec![], packed_codes: vec![0; 4 * 64 / 2],
        };
        let errors = m.validate();
        assert!(errors.iter().any(|e| e.contains("block_size is zero")),
            "expected block_size error, got: {:?}", errors);
    }

    #[test]
    fn validate_fp4_packed_codes_mismatch() {
        let m = NdaMatrix {
            rows: 4, cols: 64, scale: 1.0, version: NDA_VERSION_FP4,
            sign: vec![], extra: vec![],
            block_size: 64, n_blocks: 4,
            q_scales: vec![128; 4],
            packed_codes: vec![0; 10], // wrong: should be 4*64/2=128
        };
        let errors = m.validate();
        assert!(errors.iter().any(|e| e.contains("packed_codes size mismatch")),
            "expected packed_codes error, got: {:?}", errors);
    }

    #[test]
    fn validate_fp4_q_scales_mismatch() {
        let m = NdaMatrix {
            rows: 4, cols: 64, scale: 1.0, version: NDA_VERSION_FP4,
            sign: vec![], extra: vec![],
            block_size: 64, n_blocks: 4,
            q_scales: vec![128; 2], // wrong: should be 4 * (64/64) = 4
            packed_codes: vec![0; 128],
        };
        let errors = m.validate();
        assert!(errors.iter().any(|e| e.contains("q_scales size mismatch")),
            "expected q_scales error, got: {:?}", errors);
    }

    #[test]
    fn validate_fp2_valid() {
        let m = NdaMatrix {
            rows: 4, cols: 64, scale: 1.0, version: NDA_VERSION_FP2,
            sign: vec![], extra: vec![],
            block_size: 64, n_blocks: 4,
            q_scales: vec![128; 4],
            packed_codes: vec![0; 4 * 64 / 4], // FP2: 4 bits per elem
        };
        let errors = m.validate();
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_unknown_version() {
        let mut m = make_quad_matrix(8, 8);
        m.version = 99;
        let errors = m.validate();
        assert!(errors.iter().any(|e| e.contains("unknown version")),
            "expected unknown version error, got: {:?}", errors);
    }

    // ── sparsity ─────────────────────────────────────────────────────────────

    #[test]
    fn sparsity_non_v1_returns_zero() {
        let m = make_quad_matrix(8, 8);
        assert_eq!(m.sparsity(), 0.0);
    }

    #[test]
    fn sparsity_v1_all_active() {
        // v1: sign = active bitmap. All ones = all active = 0% sparse
        let n = 64;
        let m = NdaMatrix {
            rows: 8, cols: 8, scale: 1.0, version: NDA_V1_TERN,
            sign: vec![0xFF; 8], // all active
            extra: vec![0; 8],
            block_size: 0, n_blocks: 0, q_scales: vec![], packed_codes: vec![],
        };
        assert_eq!(m.sparsity(), 0.0, "all-active should be 0% sparse");
    }

    #[test]
    fn sparsity_v1_none_active() {
        let m = NdaMatrix {
            rows: 8, cols: 8, scale: 1.0, version: NDA_V1_TERN,
            sign: vec![0x00; 8], // none active
            extra: vec![0; 8],
            block_size: 0, n_blocks: 0, q_scales: vec![], packed_codes: vec![],
        };
        assert_eq!(m.sparsity(), 1.0, "none-active should be 100% sparse");
    }

    // ── byte_size ────────────────────────────────────────────────────────────

    #[test]
    fn byte_size_fp4() {
        let m = NdaMatrix {
            rows: 4, cols: 64, scale: 1.0, version: NDA_VERSION_FP4,
            sign: vec![], extra: vec![],
            block_size: 64, n_blocks: 4,
            q_scales: vec![128; 4], packed_codes: vec![0; 128],
        };
        // 24 + q_scales.len() + packed_codes.len() = 24 + 4 + 128 = 156
        assert_eq!(m.byte_size(), 156);
    }

    #[test]
    fn byte_size_v1_tern() {
        let m = NdaMatrix {
            rows: 8, cols: 8, scale: 1.0, version: NDA_V1_TERN,
            sign: vec![0xFF; 8], extra: vec![0xAA; 8],
            block_size: 0, n_blocks: 0, q_scales: vec![], packed_codes: vec![],
        };
        assert_eq!(m.byte_size(), 18 + 8 + 8);
    }

    // ── is_quad ──────────────────────────────────────────────────────────────

    #[test]
    fn is_quad_false_for_other_versions() {
        let mut m = make_quad_matrix(8, 8);
        m.version = NDA_V1_TERN;
        assert!(!m.is_quad());
        m.version = NDA_VERSION_FP4;
        assert!(!m.is_quad());
    }

    // ── memory_breakdown edge cases ──────────────────────────────────────────

    #[test]
    fn memory_breakdown_fp4() {
        let m = NdaMatrix {
            rows: 4, cols: 64, scale: 1.0, version: NDA_VERSION_FP4,
            sign: vec![], extra: vec![],
            block_size: 64, n_blocks: 4,
            q_scales: vec![128; 4], packed_codes: vec![0; 128],
        };
        let bd = m.memory_breakdown();
        assert_eq!(bd.header_bytes, 24);
        assert_eq!(bd.data_bytes, 4 + 128);
        assert_eq!(bd.total_bytes, 24 + 132);
    }

    #[test]
    fn memory_breakdown_zero_dimension() {
        let m = NdaMatrix::new_quad(0, 0, 1.0, vec![], vec![]);
        let bd = m.memory_breakdown();
        assert_eq!(bd.bits_per_weight, 0.0);
        assert_eq!(bd.total_bytes, 18);
    }

    // ── quad_distribution patterns ───────────────────────────────────────────

    #[test]
    fn quad_distribution_all_minus_two() {
        // s=0, e=0 → -2 for all bits
        let m = NdaMatrix {
            rows: 1, cols: 8, scale: 1.0, version: NDA_V2_QUAD,
            sign: vec![0x00], extra: vec![0x00],
            block_size: 0, n_blocks: 0, q_scales: vec![], packed_codes: vec![],
        };
        let dist = m.quad_distribution();
        assert_eq!(dist, [8, 0, 0, 0]); // all -2
    }

    #[test]
    fn quad_distribution_all_plus_two() {
        // s=1, e=1 → +2 for all bits
        let m = NdaMatrix {
            rows: 1, cols: 8, scale: 1.0, version: NDA_V2_QUAD,
            sign: vec![0xFF], extra: vec![0xFF],
            block_size: 0, n_blocks: 0, q_scales: vec![], packed_codes: vec![],
        };
        let dist = m.quad_distribution();
        assert_eq!(dist, [0, 0, 0, 8]); // all +2
    }

    #[test]
    fn quad_distribution_mixed() {
        // byte 0x01 = bit0=1, rest=0
        // sign=0x01: bit0 s=1, bits1-7 s=0
        // extra=0x00: all e=0
        // bit0: s=1,e=0 → +1
        // bits1-7: s=0,e=0 → -2
        let m = NdaMatrix {
            rows: 1, cols: 8, scale: 1.0, version: NDA_V2_QUAD,
            sign: vec![0x01], extra: vec![0x00],
            block_size: 0, n_blocks: 0, q_scales: vec![], packed_codes: vec![],
        };
        let dist = m.quad_distribution();
        assert_eq!(dist[0], 7); // -2
        assert_eq!(dist[2], 1); // +1
    }

    // ── save/load edge cases ─────────────────────────────────────────────────

    #[test]
    fn save_load_nonexistent_dir() {
        let m = make_quad_matrix(8, 8);
        let result = m.save(Path::new("/no/such/dir/test.nda"));
        assert!(result.is_err());
    }

    #[test]
    fn load_nonexistent_file() {
        let result = NdaMatrix::load(Path::new("/no/such/file.nda"));
        assert!(result.is_err());
    }

    #[test]
    fn load_corrupt_magic() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("corrupt.nda");
        std::fs::write(&path, &[0xFF, 0xFF, 0xFF, 0xFF]).unwrap();
        let result = NdaMatrix::load(&path);
        assert!(result.is_err());
    }

    #[test]
    fn save_load_roundtrip_preserves_data() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hash_test.nda");
        let original = make_quad_matrix(32, 64);
        original.save(&path).unwrap();
        let loaded = NdaMatrix::load(&path).unwrap();
        assert_eq!(original.rows, loaded.rows);
        assert_eq!(original.cols, loaded.cols);
        assert_eq!(original.scale, loaded.scale);
        assert_eq!(original.version, loaded.version);
        assert_eq!(original.sign, loaded.sign);
        assert_eq!(original.extra, loaded.extra);
    }

    // ── GEMV correctness ─────────────────────────────────────────────────────

    #[test]
    fn gemv_zero_input_zero_output() {
        let m = make_quad_matrix(4, 8);
        let x = vec![0.0; 8];
        let y = nda_gemv(&m, &x);
        assert_eq!(y, vec![0.0; 4]);
    }

    #[test]
    fn gemv_output_length_equals_rows() {
        let m = make_quad_matrix(16, 32);
        let x = vec![1.0; 32];
        let y = nda_gemv(&m, &x);
        assert_eq!(y.len(), 16);
        for &v in &y {
            assert!(v.is_finite());
        }
    }

    #[test]
    fn gemv_v2_i8_basic() {
        let m = make_quad_matrix(4, 8);
        let q = vec![0i8; 8]; // all-zero quantized input
        let y = nda_gemv_v2_i8(&m, &q, 1.0);
        assert_eq!(y.len(), 4);
        assert_eq!(y, vec![0.0; 4]); // zero input → zero output
    }

    #[test]
    fn gemv_v2_i8_nonzero() {
        let m = make_quad_matrix(4, 8);
        let q = vec![10i8; 8];
        let y = nda_gemv_v2_i8(&m, &q, 0.5);
        assert_eq!(y.len(), 4);
        for &v in &y {
            assert!(v.is_finite());
        }
    }

    #[test]
    fn gemv_v2_quad_quantized_basic() {
        let m = make_quad_matrix(4, 8);
        let x_sign = vec![0xAA; 1]; // 8 bits
        let x_extra = vec![0x55; 1];
        let y = nda_gemv_v2_quad_quantized(&m, &x_sign, &x_extra, 1.0);
        assert_eq!(y.len(), 4);
        for &v in &y {
            assert!(v.is_finite());
        }
    }

    // ── quantize edge cases ──────────────────────────────────────────────────

    #[test]
    fn quantize_v2_all_zeros() {
        let x = vec![0.0; 8];
        let (sign, extra, scale) = quantize_activations_v2_quad(&x);
        assert_eq!(sign.len(), 1);
        assert_eq!(extra.len(), 1);
        assert_eq!(scale, 1.0); // fallback for near-zero amax
    }

    #[test]
    fn quantize_v2_large_values() {
        let x = vec![100.0, -100.0, 50.0, -50.0, 0.0, 25.0, -25.0, 75.0];
        let (sign, extra, scale) = quantize_activations_v2_quad(&x);
        assert!(scale > 0.0);
        assert_eq!(sign.len(), 1);
    }

    #[test]
    fn quantize_i8_boundary() {
        let x = vec![127.0_f32, -127.0, 0.0, 1.0, -1.0];
        let (q, scale) = quantize_activations_i8(&x);
        assert_eq!(q.len(), 5);
        assert!(scale > 0.0);
        // 127.0 should quantize to 127
        assert_eq!(q[0], 127);
        assert_eq!(q[1], -127);
    }

    #[test]
    fn quantize_i8_clamping() {
        // Values beyond i8 range should be clamped
        let x = vec![1000.0, -1000.0];
        let (q, scale) = quantize_activations_i8(&x);
        assert!(q[0] <= 127);
        assert!(q[1] >= -127);
    }

    // ── quantize_with_report edge cases ──────────────────────────────────────

    #[test]
    fn quantize_with_report_near_zero_input() {
        // Input with very small values — amax < 1e-8 triggers scale fallback to 1.0
        let x = vec![1e-20, -1e-20, 0.0];
        let ((_, _, scale), report) = quantize_with_report(&x);
        assert_eq!(scale, 1.0); // fallback scale for near-zero amax
        assert_eq!(report.input_len, 3);
        assert!(report.input_amax < 1e-8);
    }

    #[test]
    fn quantize_with_report_quality() {
        let x = vec![1.0, -1.0, 0.5, -0.5, 2.0, -2.0, 0.0, 0.1];
        let (_, report) = quantize_with_report(&x);
        assert_eq!(report.input_len, 8);
        assert!(report.compression_ratio > 1.0);
        assert!(report.mean_abs_error >= 0.0);
        assert!(report.max_abs_error >= 0.0);
    }

    // ── validate_matrix_compatibility extras ─────────────────────────────────

    #[test]
    fn validate_compatibility_both_issues() {
        let a = make_quad_matrix(16, 64);
        let mut b = make_quad_matrix(32, 16); // dim mismatch
        b.version = NDA_VERSION_FP4; // version mismatch
        let issues = validate_matrix_compatibility(&a, &b);
        assert_eq!(issues.len(), 2, "expected 2 issues, got: {:?}", issues);
    }

    // ── summarize_matrices extras ────────────────────────────────────────────

    #[test]
    fn summarize_matrices_single() {
        let matrices = vec![make_quad_matrix(16, 64)];
        let summary = summarize_matrices(&matrices);
        assert_eq!(summary.matrix_count, 1);
        assert_eq!(summary.total_rows, 16);
        assert_eq!(summary.total_cols, 64);
        assert!(summary.largest_matrix.is_some());
        assert!(summary.smallest_matrix.is_some());
        assert_eq!(summary.versions, vec![2]);
    }

    #[test]
    fn summarize_matrices_tracks_versions() {
        let mut matrices = vec![make_quad_matrix(8, 8)];
        let mut m2 = make_quad_matrix(16, 16);
        m2.version = NDA_VERSION_FP4;
        matrices.push(m2);
        let summary = summarize_matrices(&matrices);
        assert_eq!(summary.versions.len(), 2);
        assert!(summary.versions.contains(&2));
        assert!(summary.versions.contains(&NDA_VERSION_FP4));
    }

    // ── NdaMatrix construction consistency ─────────────────────────────────

    #[test]
    fn new_quad_sets_fields_correctly() {
        let sign = vec![0xAA; 8];
        let extra = vec![0x55; 8];
        let m = NdaMatrix::new_quad(8, 8, 2.5, sign.clone(), extra.clone());
        assert_eq!(m.rows, 8);
        assert_eq!(m.cols, 8);
        assert_eq!(m.scale, 2.5);
        assert_eq!(m.version, NDA_V2_QUAD);
        assert_eq!(m.sign, sign);
        assert_eq!(m.extra, extra);
        assert!(m.is_quad());
    }

    // ── batch load with mixed valid/invalid ──────────────────────────────────

    #[test]
    fn batch_load_with_corrupt_file() {
        let tmp = tempfile::tempdir().unwrap();
        let m = make_quad_matrix(8, 8);
        m.save(&tmp.path().join("model_good.nda")).unwrap();
        // Write a corrupt file
        std::fs::write(tmp.path().join("model_bad.nda"), &[0xFF; 4]).unwrap();
        let (matrices, report) = NdaMatrix::load_batch(tmp.path(), "model").unwrap();
        // Should load the good one and report the bad one
        assert_eq!(matrices.len(), 1);
        assert!(!report.validation_errors.is_empty());
    }

    // ── NdaMatrixInfo extras ─────────────────────────────────────────────────

    #[test]
    fn info_fp4_matrix() {
        let m = NdaMatrix {
            rows: 4, cols: 64, scale: 0.5, version: NDA_VERSION_FP4,
            sign: vec![], extra: vec![],
            block_size: 64, n_blocks: 4,
            q_scales: vec![128; 4], packed_codes: vec![0; 128],
        };
        let info = m.info();
        assert_eq!(info.version, 3);
        assert_eq!(info.version_name, "v3 FP4 E2M1");
        assert!(!info.is_quad);
    }

    // ─── Block 97: additional comprehensive tests ──────────────────────────

    // ── nda_gemv_batch ───────────────────────────────────────────────────────

    #[test]
    fn gemv_batch_empty_inputs() {
        let m = NdaMatrix::new_quad(8, 64, 1.0, vec![0xAA; 16], vec![0x55; 16]);
        let (outputs, report) = nda_gemv_batch(&m, &[]);
        assert!(outputs.is_empty());
        assert_eq!(report.count, 0);
        assert_eq!(report.per_op_avg_us, 0.0);
    }

    #[test]
    fn gemv_batch_single_input() {
        // 4x32 quad: stride=4, sign/extra need 4*4=16 bytes
        let m = NdaMatrix::new_quad(4, 32, 1.0, vec![0xAA; 16], vec![0x55; 16]);
        let x = vec![0.5; 32];
        let (outputs, report) = nda_gemv_batch(&m, &[x]);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].len(), 4);
        assert_eq!(report.count, 1);
        assert_eq!(report.total_rows, 4);
    }

    #[test]
    fn gemv_batch_multiple_inputs() {
        let m = NdaMatrix::new_quad(4, 32, 1.0, vec![0xAA; 16], vec![0x55; 16]);
        let xs = vec![vec![0.5; 32], vec![-0.5; 32], vec![0.0; 32]];
        let (outputs, report) = nda_gemv_batch(&m, &xs);
        assert_eq!(outputs.len(), 3);
        assert_eq!(report.count, 3);
        assert_eq!(report.total_rows, 12);
    }

    // ── nda_gemv_with_report ─────────────────────────────────────────────────

    #[test]
    fn gemv_with_report_fields() {
        // 8x64 quad: stride=8, sign/extra need 8*8=64 bytes
        let m = NdaMatrix::new_quad(8, 64, 2.0, vec![0xAA; 64], vec![0x55; 64]);
        let x = vec![1.0; 64];
        let (out, report) = nda_gemv_with_report(&m, &x);
        assert_eq!(out.len(), 8);
        assert_eq!(report.rows, 8);
        assert_eq!(report.cols, 64);
        assert_eq!(report.version, 2);
        assert_eq!(report.output_len, 8);
    }

    // ── GemvReport serialization ────────────────────────────────────────────

    #[test]
    fn gemv_report_serialize() {
        let report = GemvReport {
            rows: 32,
            cols: 64,
            version: 2,
            elapsed_us: 150,
            output_len: 32,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"rows\":32"));
        assert!(json.contains("\"elapsed_us\":150"));
    }

    // ── BatchGemvReport serialization ────────────────────────────────────────

    #[test]
    fn batch_gemv_report_serialize() {
        let report = BatchGemvReport {
            count: 5,
            total_elapsed_us: 500,
            per_op_avg_us: 100.0,
            total_rows: 160,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"count\":5"));
        assert!(json.contains("\"per_op_avg_us\":100.0"));
    }

    // ── quantize_activations_i8 ─────────────────────────────────────────────

    #[test]
    fn quantize_i8_zeros() {
        let x = vec![0.0; 8];
        let (q, scale) = quantize_activations_i8(&x);
        assert_eq!(q.len(), 8);
        assert_eq!(scale, 1.0); // fallback for near-zero
        for &qi in &q {
            assert_eq!(qi, 0);
        }
    }

    #[test]
    fn quantize_i8_known_values() {
        let x = vec![1.0, -1.0, 0.5, -0.5];
        let (q, scale) = quantize_activations_i8(&x);
        assert_eq!(q.len(), 4);
        // amax=1.0, scale=1.0/127≈0.00787
        // q[0] = round(1.0/0.00787) = 127
        assert_eq!(q[0], 127);
        assert_eq!(q[1], -127);
        assert!((scale - 1.0 / 127.0).abs() < 1e-6);
    }

    #[test]
    fn quantize_i8_negative_values() {
        let x = vec![-5.0, -3.0, -1.0];
        let (q, scale) = quantize_activations_i8(&x);
        // amax=5.0, scale=5.0/127
        assert!((scale - 5.0 / 127.0).abs() < 1e-5);
        assert_eq!(q[0], -127); // -5.0 is max abs
        assert!(q[1] < 0);
        assert!(q[2] < 0);
    }

    // ── quantize batch operations ────────────────────────────────────────────

    #[test]
    fn quantize_v2_quad_batch_empty() {
        let results = quantize_activations_v2_quad_batch(&[]);
        assert!(results.is_empty());
    }

    #[test]
    fn quantize_v2_quad_batch_multiple() {
        let xs = vec![vec![1.0, -1.0, 0.5, 0.0], vec![0.0; 4], vec![2.0; 4]];
        let results = quantize_activations_v2_quad_batch(&xs);
        assert_eq!(results.len(), 3);
        for (sign, extra, scale) in &results {
            assert_eq!(sign.len(), 1); // 4 elements → 1 byte
            assert_eq!(extra.len(), 1);
            assert!(*scale > 0.0);
        }
    }

    #[test]
    fn quantize_i8_batch_empty() {
        let results = quantize_activations_i8_batch(&[]);
        assert!(results.is_empty());
    }

    #[test]
    fn quantize_i8_batch_multiple() {
        let xs = vec![vec![1.0, -1.0], vec![0.0, 0.0], vec![5.0, 3.0]];
        let results = quantize_activations_i8_batch(&xs);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0.len(), 2);
        assert_eq!(results[1].0.len(), 2);
    }

    // ── quantize_with_report ─────────────────────────────────────────────────

    #[test]
    fn quantize_with_report_fields() {
        let x = vec![1.0, -2.0, 0.5, -0.5, 0.0, 0.0, 0.0, 0.0];
        let ((sign, extra, scale), report) = quantize_with_report(&x);
        assert_eq!(sign.len(), 1);
        assert_eq!(extra.len(), 1);
        assert_eq!(report.input_len, 8);
        assert!(report.output_scale > 0.0);
        assert!(report.input_amax > 0.0);
        assert!(report.max_abs_error >= 0.0);
        assert!(report.mean_abs_error >= 0.0);
        assert!(report.compression_ratio > 0.0);
        assert!(report.validation_issues.is_empty());
    }

    #[test]
    fn quantize_with_report_empty_input() {
        let x: Vec<f32> = vec![];
        let (_, report) = quantize_with_report(&x);
        assert_eq!(report.input_len, 0);
        assert!(!report.validation_issues.is_empty());
        assert!(report.validation_issues[0].contains("empty"));
    }

    #[test]
    fn quantize_with_report_compression_ratio() {
        let x = vec![1.0; 64]; // 64 floats = 256 bytes
        let (_, report) = quantize_with_report(&x);
        // Output: 8 bytes sign + 8 bytes extra = 16 bytes
        // Ratio: 256 / 16 = 16.0
        assert!((report.compression_ratio - 16.0).abs() < 0.01,
            "ratio={}", report.compression_ratio);
    }

    // ── NdaQuantizationReport serialization ──────────────────────────────────

    #[test]
    fn quantization_report_serialize() {
        let report = NdaQuantizationReport {
            input_len: 64,
            output_scale: 2.0,
            input_amax: 5.0,
            max_abs_error: 1.5,
            mean_abs_error: 0.75,
            compression_ratio: 16.0,
            validation_issues: vec![],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"input_len\":64"));
        assert!(json.contains("\"compression_ratio\":16.0"));
    }

    // ── validate_matrix_compatibility ────────────────────────────────────────

    #[test]
    fn validate_compatibility_matching() {
        let a = NdaMatrix::new_quad(4, 8, 1.0, vec![0; 4], vec![0; 4]);
        let b = NdaMatrix::new_quad(8, 16, 1.0, vec![0; 16], vec![0; 16]);
        let issues = validate_matrix_compatibility(&a, &b);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_compatibility_dimension_mismatch() {
        let a = NdaMatrix::new_quad(4, 8, 1.0, vec![0; 4], vec![0; 4]);
        let b = NdaMatrix::new_quad(16, 32, 1.0, vec![0; 64], vec![0; 64]);
        let issues = validate_matrix_compatibility(&a, &b);
        assert!(issues.iter().any(|i| i.contains("dimension mismatch")));
    }

    // ── NdaMatrix::new_quad edge cases ───────────────────────────────────────

    #[test]
    fn new_quad_single_row() {
        let m = NdaMatrix::new_quad(1, 32, 1.0, vec![0xAA; 4], vec![0x55; 4]);
        assert_eq!(m.rows, 1);
        assert_eq!(m.cols, 32);
        assert!(m.is_quad());
    }

    #[test]
    fn new_quad_large_scale() {
        let m = NdaMatrix::new_quad(4, 32, 1e6, vec![0; 8], vec![0; 8]);
        assert_eq!(m.scale, 1e6);
    }

    // ── NdaMatrixInfo extras ─────────────────────────────────────────────────

    #[test]
    fn info_v2_quad_matrix() {
        let m = NdaMatrix::new_quad(16, 64, 0.5, vec![0; 128], vec![0; 128]);
        let info = m.info();
        assert_eq!(info.rows, 16);
        assert_eq!(info.cols, 64);
        assert_eq!(info.version, 2);
        assert!(info.version_name.contains("v2 quad"));
        assert!(info.is_quad);
        assert_eq!(info.scale, 0.5);
    }

    #[test]
    fn info_fp2_matrix() {
        let m = NdaMatrix {
            rows: 4, cols: 32, scale: 1.0, version: NDA_VERSION_FP2,
            sign: vec![], extra: vec![],
            block_size: 32, n_blocks: 4,
            q_scales: vec![64; 4], packed_codes: vec![0; 64],
        };
        let info = m.info();
        assert_eq!(info.version_name, "v4 FP2 E1M0");
        assert!(!info.is_quad);
    }

    // ── nda_gemv_v2_i8 ──────────────────────────────────────────────────────

    #[test]
    fn gemv_v2_i8_zero_input() {
        let m = NdaMatrix::new_quad(4, 32, 1.0, vec![0xAA; 16], vec![0x55; 16]);
        let q = vec![0i8; 32];
        let out = nda_gemv_v2_i8(&m, &q, 1.0);
        assert_eq!(out.len(), 4);
        for &v in &out {
            assert_eq!(v, 0.0);
        }
    }

    #[test]
    fn gemv_v2_i8_output_length() {
        // 8x64 quad: stride=8, sign/extra need 8*8=64 bytes
        let m = NdaMatrix::new_quad(8, 64, 1.0, vec![0xAA; 64], vec![0x55; 64]);
        let q = vec![1i8; 64];
        let out = nda_gemv_v2_i8(&m, &q, 0.5);
        assert_eq!(out.len(), 8);
    }
}
