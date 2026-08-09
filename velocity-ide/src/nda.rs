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
#[derive(Debug)]
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

        let magic = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let version = u16::from_le_bytes(data[4..6].try_into().unwrap());

        if magic != NDA_MAGIC {
            bail!("Invalid NDA magic {magic:#010x} in {path:?}");
        }

        if version == NDA_V1_TERN || version == NDA_V2_QUAD {
            // Legacy header: HDR = 18
            const HDR: usize = 18;
            if data.len() < HDR {
                bail!("NDA file too small ({} B): {path:?}", data.len());
            }
            let rows = u32::from_le_bytes(data[6..10].try_into().unwrap()) as usize;
            let cols = u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize;
            let scale = f32::from_le_bytes(data[14..18].try_into().unwrap());

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
            let rows = u16::from_le_bytes(data[6..8].try_into().unwrap()) as usize;
            let cols = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
            let block_size = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
            let n_blocks = u32::from_le_bytes(data[16..20].try_into().unwrap()) as usize;
            let global_scale = f32::from_le_bytes(data[20..24].try_into().unwrap());

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
    pub fn byte_size(&self) -> usize {
        if self.version == NDA_V1_TERN || self.version == NDA_V2_QUAD {
            18 + self.sign.len() + self.extra.len()
        } else {
            24 + self.q_scales.len() + self.packed_codes.len()
        }
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
        ("QKV proj  3200×3200", 3200_usize, 3200_usize),
        ("FFN gate  8640×3200", 8640_usize, 3200_usize),
        ("FFN down  3200×8640", 3200_usize, 8640_usize),
        ("LM head  32002×3200", 32002_usize, 3200_usize),
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
