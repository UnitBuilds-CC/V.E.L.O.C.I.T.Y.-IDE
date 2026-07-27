use super::nda_vec::*;
use super::tables::*;

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

    let mut x = if shift <= 14 {
        1u64 << (14 - shift)
    } else {
        1
    };

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
        let distance = (q_pos - k_pos) as i32;
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

pub struct NdaEmbedding {
    #[allow(dead_code)]
    pub vocab_size: usize,
    pub hidden_size: usize,
    #[allow(dead_code)]
    pub log2_scale: i8,
    pub sign: Vec<u8>,
    pub extra: Vec<u8>,
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
