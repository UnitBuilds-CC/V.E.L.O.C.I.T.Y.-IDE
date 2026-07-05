// V.E.L.O.C.I.T.Y.-IDE — NDA v2 activation vectors and zero-float primitives
//
// NDA v2 Uniform Format
// ─────────────────────
// ALL data (weights, activations, residuals, embeddings, KV cache) uses the same
// two-bitmap + power-of-2 scale representation:
//
//   NdaVec { sign: Vec<u8>, extra: Vec<u8>, log2_scale: i8, len: usize }
//
//   actual_value[i] = decode(sign[i], extra[i]) × 2^log2_scale
//   where decode:
//     sign=1, extra=1  →  +2
//     sign=1, extra=0  →  +1
//     sign=0, extra=1  →  −1
//     sign=0, extra=0  →  −2
//
// Power-of-2 scale means:
//   • "multiply by scale"  =  left bit-shift  (pure integer)
//   • "divide by scale"    =  right bit-shift  (pure integer)
//   • "scale_out = scale_w × scale_x" = log2_scale_w + log2_scale_x  (integer ADD)
//
// Positional encoding: ALiBi (Attention with Linear Biases)
//   bias = (q_pos - k_pos) >> head_shift   ← bit shift (NOT multiplication)
//   score -= bias                           ← subtraction
//   No RoPE, no sin/cos, no tables, no multiplications.
//
// Every operation in the entire forward pass is:
//   addition, subtraction, bitwise XOR/AND, popcount, or bit-shift.
//   Zero multiplications. Zero floats.

use rayon::prelude::*;

// ─── Compile-Time Lookup Tables ──────────────────────────────────────────────

pub const DOT_4_LUT: [[i8; 256]; 256] = {
    let mut table = [[0i8; 256]; 256];

    let mut q = 0;
    while q < 256 {
        let qs = (q & 0x0F) as u8;
        let qe = ((q >> 4) & 0x0F) as u8;

        let mut k = 0;
        while k < 256 {
            let ks = (k & 0x0F) as u8;
            let ke = ((k >> 4) & 0x0F) as u8;

            let mut dot = 0i8;
            let mut bit = 0;
            while bit < 4 {
                let qs_bit = (qs >> bit) & 1;
                let qe_bit = (qe >> bit) & 1;
                let qv = if qs_bit == 1 {
                    if qe_bit == 1 { 2 } else { 1 }
                } else {
                    if qe_bit == 1 { -1 } else { -2 }
                };

                let ks_bit = (ks >> bit) & 1;
                let ke_bit = (ke >> bit) & 1;
                let kv = if ks_bit == 1 {
                    if ke_bit == 1 { 2 } else { 1 }
                } else {
                    if ke_bit == 1 { -1 } else { -2 }
                };

                dot += qv * kv;
                bit += 1;
            }
            table[q as usize][k as usize] = dot;
            k += 1;
        }
        q += 1;
    }
    table
};

pub const ADD_LUT_Q16: [u8; 65536] = {
    let mut table = [0u8; 65536];
    let encode_table = [0, 0, 0, 1, 2, 2, 3, 3, 3];

    let mut key = 0;
    while key < 65536 {
        let xs = key & 0x0F;
        let xe = (key >> 4) & 0x0F;
        let ds = (key >> 8) & 0x0F;
        let de = (key >> 12) & 0x0F;

        let mut res_sign = 0;
        let mut res_extra = 0;

        let mut bit = 0;
        while bit < 4 {
            let x_s_bit = (xs >> bit) & 1;
            let x_e_bit = (xe >> bit) & 1;
            let d_s_bit = (ds >> bit) & 1;
            let d_e_bit = (de >> bit) & 1;

            let xv = if x_s_bit == 1 {
                if x_e_bit == 1 { 2 } else { 1 }
            } else {
                if x_e_bit == 1 { -1 } else { -2 }
            };

            let dv = if d_s_bit == 1 {
                if d_e_bit == 1 { 2 } else { 1 }
            } else {
                if d_e_bit == 1 { -1 } else { -2 }
            };

            let sum = xv + dv;
            let clamped = (sum + 4) as usize;
            let enc = encode_table[clamped];

            let res_s_bit = (enc >> 1) & 1;
            let res_e_bit = enc & 1;

            res_sign |= res_s_bit << bit;
            res_extra |= res_e_bit << bit;

            bit += 1;
        }

        table[key] = (res_sign as u8) | ((res_extra as u8) << 4);
        key += 1;
    }
    table
};

pub const SWIGLU_LUT_Q16: [u8; 65536] = {
    let mut table = [0u8; 65536];
    let encode_table = [0, 0, 0, 1, 2, 2, 3, 3, 3];

    let mut key = 0;
    while key < 65536 {
        let gs = key & 0x0F;
        let ge = (key >> 4) & 0x0F;
        let us = (key >> 8) & 0x0F;
        let ue = (key >> 12) & 0x0F;

        let mut res_sign = 0;
        let mut res_extra = 0;

        let mut bit = 0;
        while bit < 4 {
            let g_s_bit = (gs >> bit) & 1;
            let g_e_bit = (ge >> bit) & 1;
            let u_s_bit = (us >> bit) & 1;
            let u_e_bit = (ue >> bit) & 1;

            let gv = if g_s_bit == 1 {
                if g_e_bit == 1 { 2 } else { 1 }
            } else {
                if g_e_bit == 1 { -1 } else { -2 }
            };

            let uv = if u_s_bit == 1 {
                if u_e_bit == 1 { 2 } else { 1 }
            } else {
                if u_e_bit == 1 { -1 } else { -2 }
            };

            let prod = gv * uv;
            let val = prod + 4;
            let clamped = if val < 0 { 0 } else if val > 8 { 8 } else { val } as usize;
            let enc = encode_table[clamped];

            res_sign |= ((enc >> 1) & 1) << bit;
            res_extra |= (enc & 1) << bit;

            bit += 1;
        }

        table[key as usize] = (res_sign & 0x0F) | ((res_extra & 0x0F) << 4);
        key += 1;
    }
    table
};

pub const FP4_PRODUCT_LUT: [[i32; 16]; 4] = {
    let mut table = [[0i32; 16]; 4];
    let x_vals = [-2i32, -1, 1, 2];
    let w_vals = [
        0, 1, 4, 6, 8, 12, 16, 24,
        0, -1, -4, -6, -8, -12, -16, -24,
    ];
    let mut x_idx = 0;
    while x_idx < 4 {
        let xv = x_vals[x_idx];
        let mut w_idx = 0;
        while w_idx < 16 {
            table[x_idx][w_idx] = xv * w_vals[w_idx];
            w_idx += 1;
        }
        x_idx += 1;
    }
    table
};

pub const FP2_PRODUCT_LUT: [[i32; 4]; 4] = {
    let mut table = [[0i32; 4]; 4];
    let x_vals = [-2i32, -1, 1, 2];
    let w_vals = [0, 1, 0, -1];
    let mut x_idx = 0;
    while x_idx < 4 {
        let xv = x_vals[x_idx];
        let mut w_idx = 0;
        while w_idx < 4 {
            table[x_idx][w_idx] = xv * w_vals[w_idx];
            w_idx += 1;
        }
        x_idx += 1;
    }
    table
};

// ─── NdaVec ───────────────────────────────────────────────────────────────────

/// A 1-D activation or embedding vector in NDA v2 format.
///
/// The integer value of element `i` is:
///   raw[i] = if sign_bit { +1 } else { -1 } × if large { 2 } else { 1 }
///
/// The real (logical) value is:
///   val[i] = raw[i] × 2^log2_scale
///
/// Using a power-of-2 scale means all scale adjustments are bit-shifts, never
/// float multiplications.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NdaVec {
    pub len:        usize,
    /// Power-of-2 exponent: actual_scale = 2^log2_scale.
    /// Typical range: -20 to +4.
    pub log2_scale: i8,
    /// sign bitmap: bit=1 → element is positive.
    pub sign:       std::sync::Arc<[u8]>,
    /// extra bitmap: bit=1 → large magnitude (XNOR with sign).
    pub extra:      std::sync::Arc<[u8]>,
}

impl NdaVec {
    /// Allocate a zeroed (all +1 × 2^scale) NdaVec — note NDA has no true zero.
    /// Use `from_i32_slice` for real values.
    #[allow(dead_code)]
    pub fn zeros(len: usize, log2_scale: i8) -> Self {
        let bytes = (len + 7) / 8;
        // sign=1, extra=0 → +1 (closest to zero in NDA v2)
        Self {
            len,
            log2_scale,
            sign:  vec![0xFF; bytes].into(),   // all positive
            extra: vec![0x00; bytes].into(),   // all magnitude-1
        }
    }

    /// Encode a slice of f32 into NdaVec.
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

    /// Decode the NdaVec back to f32 elements.
    pub fn to_f32_vec(&self) -> Vec<f32> {
        let scale = 2.0f32.powi(self.log2_scale as i32);
        let mut out = vec![0.0f32; self.len];
        for i in 0..self.len {
            out[i] = (self.get_raw(i) as f32) * scale;
        }
        out
    }


    /// Encode a slice of raw integers (already in the scaled domain, i.e. × 2^log2_scale)
    /// into NDA v2 bitmap format. Values should be in [-4, +4]; values of 0 snap to +1.
    ///
    /// The `log2_scale` must be set by the caller based on the data range.
    pub fn from_i32_slice(data: &[i32], log2_scale: i8) -> Self {
        let len   = data.len();
        let bytes = (len + 7) / 8;
        let mut sign  = vec![0u8; bytes];
        let mut extra = vec![0u8; bytes];

        const ENCODE_TABLE: [u8; 9] = [0, 0, 0, 1, 2, 2, 3, 3, 3];

        for (i, &v) in data.iter().enumerate() {
            let clamped = v.clamp(-4, 4);
            let enc = ENCODE_TABLE[(clamped + 4) as usize];

            let byte_idx = i / 8;
            let bit_idx  = i % 8;

            sign[byte_idx]  |= ((enc >> 1) & 1) << bit_idx;
            extra[byte_idx] |= (enc & 1) << bit_idx;
        }

        Self { len, log2_scale, sign: sign.into(), extra: extra.into() }
    }

    /// Decode element `i` as its raw integer value {-2,-1,+1,+2}.
    #[inline]
    pub fn get_raw(&self, i: usize) -> i32 {
        let byte_idx = i / 8;
        let bit_idx  = i % 8;
        let mask     = 1u8 << bit_idx;
        let is_pos   = (self.sign[byte_idx]  & mask) != 0;
        let is_large = (self.sign[byte_idx]  & mask) == (self.extra[byte_idx] & mask); // XNOR
        let mag      = if is_large { 2i32 } else { 1 };
        if is_pos { mag } else { -mag }
    }

    /// Byte size of this vector's bitmaps.
    #[inline]
    pub fn bitmap_bytes(&self) -> usize {
        (self.len + 7) / 8
    }
}

// ─── Scale arithmetic (pure integer) ─────────────────────────────────────────

/// Combine two power-of-2 scales by addition of exponents.
/// This replaces floating-point multiplication: 2^a × 2^b = 2^(a+b).
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

// ─── NDA v2 GEMV: NdaVec input → NdaVec output ───────────────────────────────

use crate::nda::{NdaMatrix, NDA_VERSION_FP4, NDA_VERSION_FP2};

/// Full NDA v2 GEMV where both weights AND activations are in NDA format.
///
/// Input:  `matrix` (NDA weight), `x` (NdaVec activation)
/// Output: NdaVec where:
///   - Inner loop: pure bitwise popcount (u8 XOR / AND / count_ones)
///   - Output log2_scale = matrix_log2_scale + x.log2_scale  (integer ADD — no multiply)
///
/// This is the universal zero-float GEMV kernel.
pub fn nda_gemv_nda_to_nda(matrix: &NdaMatrix, x: &NdaVec) -> NdaVec {
    if matrix.version == NDA_VERSION_FP4 {
        debug_assert_eq!(x.len, matrix.cols);

        let mut out_i32 = vec![0i32; matrix.rows];
        let global_scale_log2 = matrix.scale.log2().round() as i8;

        out_i32.par_iter_mut().enumerate().for_each(|(row, out_val)| {
            let row_start = row * matrix.cols;
            let mut acc = 0i32;
            let block_size = matrix.block_size;
            let n_blocks = (matrix.cols + block_size - 1) / block_size;

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

        let out_log2 = combine_log2_scales(global_scale_log2.saturating_add(12), x.log2_scale);
        return NdaVec::from_i32_slice(&out_i32, out_log2);
    }

    if matrix.version == NDA_VERSION_FP2 {
        debug_assert_eq!(x.len, matrix.cols);

        let mut out_i32 = vec![0i32; matrix.rows];
        let global_scale_log2 = matrix.scale.log2().round() as i8;

        out_i32.par_iter_mut().enumerate().for_each(|(row, out_val)| {
            let row_start = row * matrix.cols;
            let mut acc = 0i32;
            let block_size = matrix.block_size;
            let n_blocks = (matrix.cols + block_size - 1) / block_size;

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

        let out_log2 = combine_log2_scales(global_scale_log2.saturating_add(14), x.log2_scale);
        return NdaVec::from_i32_slice(&out_i32, out_log2);
    }

    // Default legacy v2 quad path
    debug_assert!(matrix.is_quad(), "nda_gemv_nda_to_nda requires v2 quad matrix");
    debug_assert_eq!(x.len, matrix.cols);

    let stride     = (matrix.cols + 7) / 8;
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

    out_i32.par_iter_mut().enumerate().for_each(|(row, out_val)| {
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

// ─── Residual addition (pure integer) ─────────────────────────────────────────

/// In-place residual: `x += delta`
///
/// Both x and delta are NdaVec. If their scales differ, we bit-shift the
/// higher-resolution one down to match the lower-resolution (coarser) scale.
/// This preserves the dominant signal without any float arithmetic.
pub fn nda_vec_add_inplace(x: &mut NdaVec, delta: &NdaVec) {
    debug_assert_eq!(x.len, delta.len);

    // Determine common output scale: use the larger (coarser) of the two
    let out_log2  = x.log2_scale.max(delta.log2_scale);
    let x_shift   = (out_log2 - x.log2_scale).max(0) as u32;     // right-shift x elements
    let del_shift = (out_log2 - delta.log2_scale).max(0) as u32;  // right-shift delta elements

    let len = x.len;
    let bytes = (len + 7) / 8;

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

        if len % 8 != 0 {
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

            // Decode x[i]
            let x_idx = ((x_s_shift & 1) << 1) | (x_e_shift & 1);
            let xv = div_pow2_i32(DECODE_TABLE[x_idx as usize], x_shift);

            // Decode delta[i]
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

// ─── RMSNorm (pure integer, no sqrt float) ────────────────────────────────────

/// Integer inverse square root using Newton-Raphson.
/// Returns an approximation of floor(2^14 / sqrt(v)) for v > 0.
///
/// Newton-Raphson iterations give < 1% error for typical Q14 values.
fn isqrt_inv_q14(v: u64) -> u32 {
    if v == 0 { return 1 << 14; }

    // Initial estimate using leading-zero count
    let leading = v.leading_zeros();
    let k = 64 - leading;
    let shift = k / 2;
    
    // We want 2^14 / sqrt(v) ≈ 2^(14 - shift)
    let mut x = if shift <= 14 {
        1u64 << (14 - shift)
    } else {
        1
    };

    // Newton-Raphson: x_{n+1} = x_n × (3 - v × x_n²/2^14) / 2
    for _ in 0..3 {
        let x2  = x * x;
        let vx2 = v.saturating_mul(x2) >> 14;
        let term = (3u64 << 14).saturating_sub(vx2);
        x = x.saturating_mul(term) >> 15;
        if x == 0 { break; }
    }

    (x as u32).min(1 << 14)
}

/// RMSNorm on an NdaVec using purely integer arithmetic.
///
/// norm[i] = x[i] / sqrt(mean(x²)) * w[i]
///
/// Where w is the norm weight vector (also NdaVec).
/// All arithmetic is i64/i32 with bit-shifts. No floats, no multiplications.
pub fn rms_norm_nda(x: &NdaVec, w: &NdaVec, eps_shift: u32) -> NdaVec {
    debug_assert_eq!(x.len, w.len);
    let n = x.len;

    // Step 1: Compute sum of squares of raw values (i64 to avoid overflow)
    // raw values ∈ {-2,-1,+1,+2}, squares ∈ {1,4}, max sum = n×4
    let mut sum_sq: i64 = 0;
    let bytes = x.sign.len();
    let full_bytes = n / 8;

    for byte_idx in 0..full_bytes {
        let xs = x.sign[byte_idx];
        let xe = x.extra[byte_idx];
        let large_mask = !(xs ^ xe);
        sum_sq += 8 + (large_mask.count_ones() as i64) * 3;
    }

    if n % 8 != 0 {
        let byte_idx = full_bytes;
        let xs = x.sign[byte_idx];
        let xe = x.extra[byte_idx];
        let active_mask = (1u8 << (n % 8)) - 1;
        let large_mask = (!(xs ^ xe)) & active_mask;
        sum_sq += (n % 8) as i64 + (large_mask.count_ones() as i64) * 3;
    }

    // mean_sq (using shift for division): mean_sq_q14 = sum_sq × 2^14 / n
    let mean_sq_q14 = (sum_sq << 14) / n as i64;

    // Add epsilon (eps_shift: treat eps as 2^(-eps_shift) in Q14 space)
    let mean_sq_eps = mean_sq_q14 as u64 + (1u64 << (14u32.saturating_sub(eps_shift)));

    // Step 2: Integer inverse sqrt: inv_rms_q14 ≈ 2^14 / sqrt(mean_sq_eps)
    let inv_rms_q14 = isqrt_inv_q14(mean_sq_eps);

    // Step 3: Normalize: out[i] = x.raw[i] × inv_rms_q14 × w.raw[i]  >> 7
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

// ─── ALiBi positional encoding (pure integer, zero multiplication) ────────────────

/// ALiBi (Attention with Linear Biases) slopes as power-of-2 right-shift amounts.
///
/// ALiBi replaces RoPE entirely:
///   Traditional: bias = m_h × (q_pos − k_pos)         ← needs multiplication
///   NDA-Zero:    bias = (q_pos − k_pos) >> shift_h     ← pure bit-shift
///
/// Slope m_h = 2^(−8h/n_heads) is approximated as 2^(−shift_h) where
///   shift_h = round(8h / n_heads)
///
/// This makes each head’s positional bias a right-shift of the integer
/// position distance. No tables. No sin/cos. No multiplications.
///
/// # Example (14 heads)
/// | head | exact 8h/n | shift_h | effective m_h |
/// |------|------------|---------|---------------|
/// |    1 |      0.571 |       1 |          0.50 |
/// |    2 |      1.143 |       1 |          0.50 |
/// |    3 |      1.714 |       2 |          0.25 |
/// |    4 |      2.286 |       2 |          0.25 |
/// |    5 |      2.857 |       3 |          0.125|
/// |  ... |        ... |     ... |           ... |
/// |   14 |      8.000 |       8 |     1/256     |
///
/// Heads with smaller shift = stronger locality bias (attend to nearby tokens).
/// Heads with larger shift = weaker bias (attend to distant context).
#[derive(Clone, Debug)]
pub struct AliBiSlopes {
    /// Right-shift amounts per head. bias = (q_pos - k_pos) >> shifts[head]
    pub shifts: Vec<u8>,
    #[allow(dead_code)]
    pub n_heads: usize,
}

impl AliBiSlopes {
    /// Compute ALiBi shifts for `n_heads` heads.
    ///
    /// Uses m_h = 2^(-8h/n_heads) → shift_h = round(8h/n_heads),
    /// clamped to [1, 30] to keep biases sensible.
    pub fn new(n_heads: usize) -> Self {
        let shifts = (1..=n_heads)
            .map(|h| {
                let exact = 8.0 * h as f32 / n_heads as f32;
                exact.round().clamp(1.0, 30.0) as u8
            })
            .collect();
        Self { shifts, n_heads }
    }

    /// Get the right-shift amount for `head` (0-indexed).
    #[inline]
    pub fn shift(&self, head: usize) -> u8 {
        self.shifts[head]
    }
}

/// Apply ALiBi positional bias to a row of Q·K attention scores (in-place).
///
/// For each cached position `k_pos` (block index), subtract the bias:
///   `scores[k_pos] -= (q_pos - k_pos) >> shift`
///
/// All arithmetic: integer subtraction and bit-shift. Zero multiplications.
///
/// # Arguments
/// * `scores`  — mutable slice of i32 Q·K scores, one per KV block
/// * `q_pos`   — current query position (causal: q_pos ≥ k_pos always)
/// * `shift`   — ALiBi right-shift for this head
pub fn apply_alibi_bias_i32(scores: &mut [i32], q_pos: usize, shift: u8, scale_shift: u32) {
    for (k_pos, score) in scores.iter_mut().enumerate() {
        // Position distance: always non-negative in causal attention
        let distance = (q_pos - k_pos) as i32;
        // Scale distance up to match integer scores scale before shifting, preventing loss of fractional bias
        let bias_int = ((distance as i64) << scale_shift) >> shift;
        // Apply: add bias to match the positive training bias
        *score += bias_int as i32;
    }
}



// ─── SiLU lookup table (INT8, no float) ───────────────────────────────────────

/// SiLU activation via a precomputed lookup table.
///
/// Input: NdaVec (values in {-2,-1,+1,+2} × 2^log2_scale)
/// LUT covers raw integer values {-2,-1,+1,+2} — only 4 distinct inputs.
/// SiLU(x·s) ≈ lut[x] × s  where s = 2^log2_scale
///
/// Since NDA v2 only has 4 possible raw values, the LUT is exactly 4 entries:
///   silu(-2) ≈ -2 × sigmoid(-2) ≈ -0.238
///   silu(-1) ≈ -1 × sigmoid(-1) ≈ -0.269
///   silu(+1) ≈ +1 × sigmoid(+1) ≈ +0.731
///   silu(+2) ≈ +2 × sigmoid(+2) ≈ +1.762
///
/// Mapped to {-2,-1,+1,+2} NDA encoding (nearest representable):
///   silu(-2) → -1 (nearest to -0.238 in unit scale)
///   silu(-1) → -1
///   silu(+1) → +1
///   silu(+2) → +2
///
/// Scale: output uses the same log2_scale as input.
/// This is approximate but consistent — and zero multiplications.
#[derive(Clone)]
pub struct SiluLut {
    /// Maps raw NDA value {-2,-1,+1,+2} → output NDA value {-2,-1,+1,+2}
    /// Indexed by: (raw + 2) = index in [0..4]
    ///   0 → raw=-2, 1 → raw=-1, 2 → raw=+1, 3 → raw=+2
    #[allow(dead_code)]
    table: [i32; 4],
}

impl SiluLut {
    pub fn new() -> Self {
        // SiLU(x) = x * sigmoid(x) for x ∈ {-2,-1,+1,+2}
        // Then snap to nearest NDA v2 value in proportion
        Self {
            table: [
                -1,   // silu(-2) ≈ -0.238  → -1 (nearest)
                -1,   // silu(-1) ≈ -0.269  → -1
                 1,   // silu(+1) ≈ +0.731  → +1
                 2,   // silu(+2) ≈ +1.762  → +2
            ],
        }
    }

    /// Apply SiLU element-wise to an NdaVec.
    pub fn apply(&self, x: &NdaVec) -> NdaVec {
        let sign = x.sign.clone();
        let mut extra = x.extra.to_vec();
        for i in 0..sign.len() {
            extra[i] |= !sign[i];
        }
        if x.len % 8 != 0 {
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
    fn default() -> Self { Self::new() }
}

// ─── SwiGLU gate (NDA, pure integer) ─────────────────────────────────────────

/// SwiGLU: out[i] = SiLU(gate[i]) ⊙ up[i]
///
/// Both operands are NDA v2 {-2,-1,+1,+2}. SiLU output is also {-2,-1,+1,+2}.
/// The element-wise product has only 16 possible results (4×4).
/// We replace the multiply with a const lookup table — zero multiplications.
pub fn swiglu_nda(gate: &NdaVec, up: &NdaVec, silu: &SiluLut) -> NdaVec {
    debug_assert_eq!(gate.len, up.len);
    let gate_activated = silu.apply(gate);

    let len = gate.len;
    let bytes = (len + 7) / 8;
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

    if len % 8 != 0 {
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


// ─── Embedding lookup (NDA, no float) ─────────────────────────────────────────

/// Embedding table stored as NDA v2 rows.
///
/// Each row is len=hidden_size elements packed in NDA v2 format.
/// All bitmaps are stored flat; row `i` starts at byte offset `i * stride`.
pub struct NdaEmbedding {
    #[allow(dead_code)]
    pub vocab_size:  usize,
    pub hidden_size: usize,
    #[allow(dead_code)]
    pub log2_scale:  i8,
    pub sign:        Vec<u8>,   // [vocab_size × stride]
    pub extra:       Vec<u8>,
}

impl NdaEmbedding {
    pub fn stride(&self) -> usize {
        (self.hidden_size + 7) / 8
    }

    /// Look up token `id` and return its NdaVec embedding.
    #[allow(dead_code)]
    pub fn get(&self, id: usize) -> NdaVec {
        let stride = self.stride();
        let start  = id * stride;
        NdaVec {
            len:        self.hidden_size,
            log2_scale: self.log2_scale,
            sign:       self.sign[start..start + stride].to_vec().into(),
            extra:      self.extra[start..start + stride].to_vec().into(),
        }
    }

    /// Build from a flat FP32 embedding table [vocab_size × hidden_size].
    pub fn from_f32(embed: &[f32], vocab_size: usize, hidden_size: usize) -> Self {
        let amax = embed.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
        // log2_scale = floor(log2(amax / 2.0))
        let log2_scale = if amax > 1e-8 {
            (amax / 2.0).log2().floor() as i8
        } else {
            0i8
        };
        let scale = 2f32.powi(log2_scale as i32);
        let inv_scale = 1.0 / scale;

        let stride = (hidden_size + 7) / 8;
        let mut sign  = vec![0u8; vocab_size * stride];
        let mut extra = vec![0u8; vocab_size * stride];

        for (tok_id, row) in embed.chunks_exact(hidden_size).enumerate() {
            for (i, &v) in row.iter().enumerate() {
                let vs = v * inv_scale;
                let is_pos   = vs >= 0.0;
                let is_large = vs.abs() >= 1.5;

                let byte_idx = tok_id * stride + i / 8;
                let bit_idx  = i % 8;

                if is_pos   { sign[byte_idx]  |= 1 << bit_idx; }
                if is_pos == is_large { extra[byte_idx] |= 1 << bit_idx; }  // XNOR
            }
        }

        Self { vocab_size, hidden_size, log2_scale, sign, extra }
    }
}

// ─── Argmax LM head (pure integer, no softmax) ────────────────────────────────

/// Greedy next-token selection: argmax over the integer logit accumulator.
///
/// The LM head GEMV produces an i32 per vocab token. At temperature=0,
/// the next token is simply the argmax. Zero floats, zero exp(), zero division.
pub fn argmax_i32(logits: &[i32]) -> u32 {
    logits
        .iter()
        .enumerate()
        .max_by_key(|(_, &v)| v)
        .map(|(i, _)| i as u32)
        .unwrap_or(0)
}

/// LM head NDA GEMV → raw i32 logits (no re-encoding to NdaVec, just raw accumulator).
///
/// Used with argmax_i32 for greedy decoding — the scale cancels out in argmax
/// so we don't even need to dequantize.
#[allow(dead_code)]
pub fn lm_head_nda_to_i32(matrix: &NdaMatrix, x: &NdaVec) -> Vec<i32> {
    debug_assert!(matrix.is_quad());
    debug_assert_eq!(x.len, matrix.cols);

    let stride = (matrix.cols + 7) / 8;
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
            let diff =   ws ^ xs;
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
/// NDA GEMV → raw i32 accumulator (no re-encoding).
///
/// Identical kernel to lm_head_nda_to_i32 — used for intermediate projections
/// where we want the raw accumulator before scale re-encoding.
/// Scale cancels in argmax so dequantization is omitted.
#[allow(dead_code)]
pub fn nda_gemv_nda_to_i32(matrix: &NdaMatrix, x: &NdaVec) -> Vec<i32> {
    lm_head_nda_to_i32(matrix, x)
}
