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
