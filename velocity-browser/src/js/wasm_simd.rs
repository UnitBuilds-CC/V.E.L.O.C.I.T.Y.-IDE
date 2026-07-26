//! WebAssembly SIMD (v128) pipeline with typed lane operations.
//!
//! Implements the Wasm SIMD proposal's v128 vector type with operations
//! for i8x16, i16x8, i32x4, and i64x2 lane widths.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WasmV128Vector {
    pub lane_bytes: [u8; 16],
}

impl WasmV128Vector {
    pub fn zero() -> Self { Self { lane_bytes: [0; 16] } }
    pub fn from_bytes(bytes: [u8; 16]) -> Self { Self { lane_bytes: bytes } }

    // ── Typed lane constructors ──

    pub fn i8x16(lanes: [i8; 16]) -> Self {
        let mut bytes = [0u8; 16];
        for (i, &v) in lanes.iter().enumerate() { bytes[i] = v as u8; }
        Self { lane_bytes: bytes }
    }
    pub fn i16x8(lanes: [i16; 8]) -> Self {
        let mut bytes = [0u8; 16];
        for (i, &v) in lanes.iter().enumerate() {
            let b = v.to_le_bytes();
            bytes[i * 2] = b[0]; bytes[i * 2 + 1] = b[1];
        }
        Self { lane_bytes: bytes }
    }
    pub fn i32x4(lanes: [i32; 4]) -> Self {
        let mut bytes = [0u8; 16];
        for (i, &v) in lanes.iter().enumerate() {
            let b = v.to_le_bytes();
            bytes[i * 4..i * 4 + 4].copy_from_slice(&b);
        }
        Self { lane_bytes: bytes }
    }
    pub fn i64x2(lanes: [i64; 2]) -> Self {
        let mut bytes = [0u8; 16];
        for (i, &v) in lanes.iter().enumerate() {
            let b = v.to_le_bytes();
            bytes[i * 8..i * 8 + 8].copy_from_slice(&b);
        }
        Self { lane_bytes: bytes }
    }

    // ── Typed lane extractors ──

    pub fn as_i8_lanes(&self) -> [i8; 16] {
        let mut lanes = [0i8; 16];
        for (i, b) in self.lane_bytes.iter().enumerate() { lanes[i] = *b as i8; }
        lanes
    }
    pub fn as_i16_lanes(&self) -> [i16; 8] {
        let mut lanes = [0i16; 8];
        for i in 0..8 {
            lanes[i] = i16::from_le_bytes([self.lane_bytes[i * 2], self.lane_bytes[i * 2 + 1]]);
        }
        lanes
    }
    pub fn as_i32_lanes(&self) -> [i32; 4] {
        let mut lanes = [0i32; 4];
        for i in 0..4 {
            lanes[i] = i32::from_le_bytes([
                self.lane_bytes[i * 4], self.lane_bytes[i * 4 + 1],
                self.lane_bytes[i * 4 + 2], self.lane_bytes[i * 4 + 3],
            ]);
        }
        lanes
    }
    pub fn as_i64_lanes(&self) -> [i64; 2] {
        let mut lanes = [0i64; 2];
        for i in 0..2 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&self.lane_bytes[i * 8..i * 8 + 8]);
            lanes[i] = i64::from_le_bytes(b);
        }
        lanes
    }

    // ── Splat ──

    pub fn i8x16_splat(val: i8) -> Self { Self::i8x16([val; 16]) }
    pub fn i16x8_splat(val: i16) -> Self { Self::i16x8([val; 8]) }
    pub fn i32x4_splat(val: i32) -> Self { Self::i32x4([val; 4]) }
    pub fn i64x2_splat(val: i64) -> Self { Self::i64x2([val; 2]) }

    // ── Extract / Replace ──

    pub fn i8x16_extract_lane(&self, idx: usize) -> i8 { self.as_i8_lanes()[idx.min(15)] }
    pub fn i16x8_extract_lane(&self, idx: usize) -> i16 { self.as_i16_lanes()[idx.min(7)] }
    pub fn i32x4_extract_lane(&self, idx: usize) -> i32 { self.as_i32_lanes()[idx.min(3)] }
    pub fn i64x2_extract_lane(&self, idx: usize) -> i64 { self.as_i64_lanes()[idx.min(1)] }

    pub fn i8x16_replace_lane(&self, idx: usize, val: i8) -> Self {
        let mut lanes = self.as_i8_lanes(); lanes[idx.min(15)] = val; Self::i8x16(lanes)
    }
    pub fn i16x8_replace_lane(&self, idx: usize, val: i16) -> Self {
        let mut lanes = self.as_i16_lanes(); lanes[idx.min(7)] = val; Self::i16x8(lanes)
    }
    pub fn i32x4_replace_lane(&self, idx: usize, val: i32) -> Self {
        let mut lanes = self.as_i32_lanes(); lanes[idx.min(3)] = val; Self::i32x4(lanes)
    }
    pub fn i64x2_replace_lane(&self, idx: usize, val: i64) -> Self {
        let mut lanes = self.as_i64_lanes(); lanes[idx.min(1)] = val; Self::i64x2(lanes)
    }
}

pub struct WasmSimdPipeline;

impl Default for WasmSimdPipeline {
    fn default() -> Self { Self::new() }
}

impl WasmSimdPipeline {
    pub fn new() -> Self { Self }

    // ── Byte-level ops (legacy) ──

    pub fn execute_vector_add(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let mut res = [0u8; 16];
        for i in 0..16 { res[i] = a.lane_bytes[i].wrapping_add(b.lane_bytes[i]); }
        WasmV128Vector::from_bytes(res)
    }

    // ── Typed lane arithmetic: i8x16 ──

    pub fn i8x16_add(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i8_lanes(); let lb = b.as_i8_lanes();
        let mut r = [0i8; 16];
        for i in 0..16 { r[i] = la[i].wrapping_add(lb[i]); }
        WasmV128Vector::i8x16(r)
    }
    pub fn i8x16_sub(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i8_lanes(); let lb = b.as_i8_lanes();
        let mut r = [0i8; 16];
        for i in 0..16 { r[i] = la[i].wrapping_sub(lb[i]); }
        WasmV128Vector::i8x16(r)
    }
    pub fn i8x16_min(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i8_lanes(); let lb = b.as_i8_lanes();
        let mut r = [0i8; 16];
        for i in 0..16 { r[i] = la[i].min(lb[i]); }
        WasmV128Vector::i8x16(r)
    }
    pub fn i8x16_max(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i8_lanes(); let lb = b.as_i8_lanes();
        let mut r = [0i8; 16];
        for i in 0..16 { r[i] = la[i].max(lb[i]); }
        WasmV128Vector::i8x16(r)
    }

    // ── Typed lane arithmetic: i16x8 ──

    pub fn i16x8_add(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i16_lanes(); let lb = b.as_i16_lanes();
        let mut r = [0i16; 8];
        for i in 0..8 { r[i] = la[i].wrapping_add(lb[i]); }
        WasmV128Vector::i16x8(r)
    }
    pub fn i16x8_sub(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i16_lanes(); let lb = b.as_i16_lanes();
        let mut r = [0i16; 8];
        for i in 0..8 { r[i] = la[i].wrapping_sub(lb[i]); }
        WasmV128Vector::i16x8(r)
    }
    pub fn i16x8_mul(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i16_lanes(); let lb = b.as_i16_lanes();
        let mut r = [0i16; 8];
        for i in 0..8 { r[i] = la[i].wrapping_mul(lb[i]); }
        WasmV128Vector::i16x8(r)
    }

    // ── Typed lane arithmetic: i32x4 ──

    pub fn i32x4_add(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i32_lanes(); let lb = b.as_i32_lanes();
        let mut r = [0i32; 4];
        for i in 0..4 { r[i] = la[i].wrapping_add(lb[i]); }
        WasmV128Vector::i32x4(r)
    }
    pub fn i32x4_sub(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i32_lanes(); let lb = b.as_i32_lanes();
        let mut r = [0i32; 4];
        for i in 0..4 { r[i] = la[i].wrapping_sub(lb[i]); }
        WasmV128Vector::i32x4(r)
    }
    pub fn i32x4_mul(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i32_lanes(); let lb = b.as_i32_lanes();
        let mut r = [0i32; 4];
        for i in 0..4 { r[i] = la[i].wrapping_mul(lb[i]); }
        WasmV128Vector::i32x4(r)
    }

    // ── Typed lane arithmetic: i64x2 ──

    pub fn i64x2_add(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i64_lanes(); let lb = b.as_i64_lanes();
        let mut r = [0i64; 2];
        for i in 0..2 { r[i] = la[i].wrapping_add(lb[i]); }
        WasmV128Vector::i64x2(r)
    }
    pub fn i64x2_sub(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i64_lanes(); let lb = b.as_i64_lanes();
        let mut r = [0i64; 2];
        for i in 0..2 { r[i] = la[i].wrapping_sub(lb[i]); }
        WasmV128Vector::i64x2(r)
    }
    pub fn i64x2_mul(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i64_lanes(); let lb = b.as_i64_lanes();
        let mut r = [0i64; 2];
        for i in 0..2 { r[i] = la[i].wrapping_mul(lb[i]); }
        WasmV128Vector::i64x2(r)
    }

    // ── Bitwise ──

    pub fn v128_and(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let mut r = [0u8; 16];
        for i in 0..16 { r[i] = a.lane_bytes[i] & b.lane_bytes[i]; }
        WasmV128Vector::from_bytes(r)
    }
    pub fn v128_or(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let mut r = [0u8; 16];
        for i in 0..16 { r[i] = a.lane_bytes[i] | b.lane_bytes[i]; }
        WasmV128Vector::from_bytes(r)
    }
    pub fn v128_xor(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let mut r = [0u8; 16];
        for i in 0..16 { r[i] = a.lane_bytes[i] ^ b.lane_bytes[i]; }
        WasmV128Vector::from_bytes(r)
    }
    pub fn v128_not(&self, a: &WasmV128Vector) -> WasmV128Vector {
        let mut r = [0u8; 16];
        for i in 0..16 { r[i] = !a.lane_bytes[i]; }
        WasmV128Vector::from_bytes(r)
    }
    pub fn v128_andnot(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let mut r = [0u8; 16];
        for i in 0..16 { r[i] = a.lane_bytes[i] & !b.lane_bytes[i]; }
        WasmV128Vector::from_bytes(r)
    }
    pub fn v128_bitselect(&self, v1: &WasmV128Vector, v2: &WasmV128Vector, mask: &WasmV128Vector) -> WasmV128Vector {
        let mut r = [0u8; 16];
        for i in 0..16 { r[i] = (v1.lane_bytes[i] & mask.lane_bytes[i]) | (v2.lane_bytes[i] & !mask.lane_bytes[i]); }
        WasmV128Vector::from_bytes(r)
    }

    // ── Shifts ──

    pub fn i32x4_shl(&self, a: &WasmV128Vector, shift: u32) -> WasmV128Vector {
        let la = a.as_i32_lanes();
        let mut r = [0i32; 4];
        for i in 0..4 { r[i] = la[i].wrapping_shl(shift % 32); }
        WasmV128Vector::i32x4(r)
    }
    pub fn i32x4_shr(&self, a: &WasmV128Vector, shift: u32) -> WasmV128Vector {
        let la = a.as_i32_lanes();
        let mut r = [0i32; 4];
        for i in 0..4 { r[i] = la[i].wrapping_shr(shift % 32); }
        WasmV128Vector::i32x4(r)
    }
    pub fn i16x8_shl(&self, a: &WasmV128Vector, shift: u32) -> WasmV128Vector {
        let la = a.as_i16_lanes();
        let mut r = [0i16; 8];
        for i in 0..8 { r[i] = la[i].wrapping_shl(shift % 16); }
        WasmV128Vector::i16x8(r)
    }

    // ── Shuffle / Swizzle ──

    /// Rearrange bytes from two input vectors using an index vector.
    pub fn v8x16_shuffle(&self, a: &WasmV128Vector, b: &WasmV128Vector, indices: &WasmV128Vector) -> WasmV128Vector {
        let mut r = [0u8; 16];
        let combined: Vec<u8> = a.lane_bytes.iter().chain(b.lane_bytes.iter()).copied().collect();
        for i in 0..16 {
            let idx = indices.lane_bytes[i] as usize % 32;
            r[i] = combined[idx];
        }
        WasmV128Vector::from_bytes(r)
    }
    /// Rearrange bytes within a single vector using an index vector.
    pub fn v8x16_swizzle(&self, a: &WasmV128Vector, indices: &WasmV128Vector) -> WasmV128Vector {
        let mut r = [0u8; 16];
        for i in 0..16 {
            let idx = indices.lane_bytes[i] as usize;
            r[i] = if idx < 16 { a.lane_bytes[idx] } else { 0 };
        }
        WasmV128Vector::from_bytes(r)
    }

    // ── Comparison (produce mask vectors) ──

    pub fn i32x4_eq(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i32_lanes(); let lb = b.as_i32_lanes();
        let mut r = [0i32; 4];
        for i in 0..4 { r[i] = if la[i] == lb[i] { -1 } else { 0 }; }
        WasmV128Vector::i32x4(r)
    }
    pub fn i32x4_lt(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i32_lanes(); let lb = b.as_i32_lanes();
        let mut r = [0i32; 4];
        for i in 0..4 { r[i] = if la[i] < lb[i] { -1 } else { 0 }; }
        WasmV128Vector::i32x4(r)
    }
    pub fn i32x4_gt(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let la = a.as_i32_lanes(); let lb = b.as_i32_lanes();
        let mut r = [0i32; 4];
        for i in 0..4 { r[i] = if la[i] > lb[i] { -1 } else { 0 }; }
        WasmV128Vector::i32x4(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simd() -> WasmSimdPipeline { WasmSimdPipeline::new() }

    #[test]
    fn i32x4_add_sub_mul() {
        let s = simd();
        let a = WasmV128Vector::i32x4([1, 2, 3, 4]);
        let b = WasmV128Vector::i32x4([10, 20, 30, 40]);
        assert_eq!(s.i32x4_add(&a, &b).as_i32_lanes(), [11, 22, 33, 44]);
        assert_eq!(s.i32x4_sub(&b, &a).as_i32_lanes(), [9, 18, 27, 36]);
        assert_eq!(s.i32x4_mul(&a, &b).as_i32_lanes(), [10, 40, 90, 160]);
    }

    #[test]
    fn bitwise_and_or_xor() {
        let s = simd();
        let a = WasmV128Vector::i32x4([0xFF00FF00u32 as i32, 0, 0, 0]);
        let b = WasmV128Vector::i32x4([0x0FF00FF0u32 as i32, 0, 0, 0]);
        let and = s.v128_and(&a, &b);
        assert_eq!(and.as_i32_lanes()[0], 0x0F000F00u32 as i32);
    }

    #[test]
    fn splat_extract_replace() {
        let v = WasmV128Vector::i32x4_splat(42);
        assert_eq!(v.as_i32_lanes(), [42, 42, 42, 42]);
        let v2 = v.i32x4_replace_lane(2, 99);
        assert_eq!(v2.as_i32_lanes(), [42, 42, 99, 42]);
        assert_eq!(v2.i32x4_extract_lane(2), 99);
    }

    #[test]
    fn comparison_masks() {
        let s = simd();
        let a = WasmV128Vector::i32x4([1, 5, 3, 7]);
        let b = WasmV128Vector::i32x4([2, 5, 1, 7]);
        let eq = s.i32x4_eq(&a, &b);
        assert_eq!(eq.as_i32_lanes(), [0, -1, 0, -1]);
        let lt = s.i32x4_lt(&a, &b);
        assert_eq!(lt.as_i32_lanes(), [-1, 0, 0, 0]);
    }

    #[test]
    fn shift_lanes() {
        let s = simd();
        let a = WasmV128Vector::i32x4([1, 2, 4, 8]);
        let shifted = s.i32x4_shl(&a, 3);
        assert_eq!(shifted.as_i32_lanes(), [8, 16, 32, 64]);
    }
}
