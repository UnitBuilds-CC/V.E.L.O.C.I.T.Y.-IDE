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

    #[test]
    fn zero_vector_is_all_zeros() {
        let v = WasmV128Vector::zero();
        assert_eq!(v.lane_bytes, [0u8; 16]);
        assert_eq!(v.as_i32_lanes(), [0, 0, 0, 0]);
        assert_eq!(v.as_i64_lanes(), [0i64, 0]);
    }

    #[test]
    fn i8x16_add_sub() {
        let s = simd();
        let a = WasmV128Vector::i8x16([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let b = WasmV128Vector::i8x16([16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1]);
        let sum = s.i8x16_add(&a, &b);
        assert_eq!(sum.as_i8_lanes(), [17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17]);
        let diff = s.i8x16_sub(&sum, &b);
        assert_eq!(diff.as_i8_lanes(), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    }

    #[test]
    fn i8x16_min_max() {
        let s = simd();
        let a = WasmV128Vector::i8x16([1, 20, 3, 40, 5, 60, 7, 80, 9, 100, 11, 120, 13, 14, 15, 16]);
        let b = WasmV128Vector::i8x16([16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1]);
        let mn = s.i8x16_min(&a, &b);
        let mx = s.i8x16_max(&a, &b);
        assert_eq!(mn.as_i8_lanes()[0], 1);   // min(1, 16)
        assert_eq!(mx.as_i8_lanes()[0], 16);  // max(1, 16)
        assert_eq!(mn.as_i8_lanes()[1], 15);  // min(20, 15)
        assert_eq!(mx.as_i8_lanes()[1], 20);  // max(20, 15)
    }

    #[test]
    fn i16x8_add_sub_mul() {
        let s = simd();
        let a = WasmV128Vector::i16x8([1, 2, 3, 4, 5, 6, 7, 8]);
        let b = WasmV128Vector::i16x8([10, 20, 30, 40, 50, 60, 70, 80]);
        assert_eq!(s.i16x8_add(&a, &b).as_i16_lanes(), [11, 22, 33, 44, 55, 66, 77, 88]);
        assert_eq!(s.i16x8_sub(&b, &a).as_i16_lanes(), [9, 18, 27, 36, 45, 54, 63, 72]);
        assert_eq!(s.i16x8_mul(&a, &b).as_i16_lanes(), [10, 40, 90, 160, 250, 360, 490, 640]);
    }

    #[test]
    fn i64x2_add_sub_mul() {
        let s = simd();
        let a = WasmV128Vector::i64x2([100, 200]);
        let b = WasmV128Vector::i64x2([300, 400]);
        assert_eq!(s.i64x2_add(&a, &b).as_i64_lanes(), [400, 600]);
        assert_eq!(s.i64x2_sub(&b, &a).as_i64_lanes(), [200, 200]);
        assert_eq!(s.i64x2_mul(&a, &b).as_i64_lanes(), [30000, 80000]);
    }

    #[test]
    fn v128_not_andnot() {
        let s = simd();
        let a = WasmV128Vector::from_bytes([0xFF; 16]);
        let notted = s.v128_not(&a);
        assert_eq!(notted.lane_bytes, [0u8; 16]);
        let b = WasmV128Vector::from_bytes([0x0F; 16]);
        let andnot = s.v128_andnot(&a, &b);
        assert_eq!(andnot.lane_bytes, [0xF0; 16]);
    }

    #[test]
    fn v128_bitselect_operation() {
        let s = simd();
        let v1 = WasmV128Vector::from_bytes([0xFF; 16]);
        let v2 = WasmV128Vector::from_bytes([0x00; 16]);
        let mask = WasmV128Vector::from_bytes([
            0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00,
            0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00,
        ]);
        let result = s.v128_bitselect(&v1, &v2, &mask);
        // Where mask=0xFF → v1 byte (0xFF), where mask=0x00 → v2 byte (0x00)
        assert_eq!(result.lane_bytes[0], 0xFF);
        assert_eq!(result.lane_bytes[1], 0x00);
    }

    #[test]
    fn i32x4_shr_shifts_right() {
        let s = simd();
        let a = WasmV128Vector::i32x4([16, 32, 64, 128]);
        let shifted = s.i32x4_shr(&a, 2);
        assert_eq!(shifted.as_i32_lanes(), [4, 8, 16, 32]);
    }

    #[test]
    fn i16x8_shl_shifts_left() {
        let s = simd();
        let a = WasmV128Vector::i16x8([1, 2, 3, 4, 5, 6, 7, 8]);
        let shifted = s.i16x8_shl(&a, 4);
        assert_eq!(shifted.as_i16_lanes(), [16, 32, 48, 64, 80, 96, 112, 128]);
    }

    #[test]
    fn v8x16_shuffle_combines_two_vectors() {
        let s = simd();
        let a = WasmV128Vector::from_bytes([10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25]);
        let b = WasmV128Vector::from_bytes([30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45]);
        // Index 0..15 → from a, index 16..31 → from b
        let indices = WasmV128Vector::from_bytes([0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23]);
        let result = s.v8x16_shuffle(&a, &b, &indices);
        assert_eq!(result.lane_bytes[0], 10);  // a[0]
        assert_eq!(result.lane_bytes[1], 30);  // b[0] (index 16)
        assert_eq!(result.lane_bytes[2], 11);  // a[1]
        assert_eq!(result.lane_bytes[3], 31);  // b[1] (index 17)
    }

    #[test]
    fn v8x16_swizzle_rearranges_single_vector() {
        let s = simd();
        let a = WasmV128Vector::from_bytes([10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]);
        // Reverse the vector using swizzle
        let indices = WasmV128Vector::from_bytes([15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]);
        let result = s.v8x16_swizzle(&a, &indices);
        assert_eq!(result.lane_bytes[0], 160);
        assert_eq!(result.lane_bytes[15], 10);
    }

    #[test]
    fn i64x2_extract_replace_lanes() {
        let v = WasmV128Vector::i64x2([42, 99]);
        assert_eq!(v.i64x2_extract_lane(0), 42);
        assert_eq!(v.i64x2_extract_lane(1), 99);
        let v2 = v.i64x2_replace_lane(0, 7);
        assert_eq!(v2.as_i64_lanes(), [7, 99]);
    }

    #[test]
    fn i8x16_extract_replace_lanes() {
        let v = WasmV128Vector::i8x16([10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, -1, -2, -3, -4]);
        assert_eq!(v.i8x16_extract_lane(0), 10);
        assert_eq!(v.i8x16_extract_lane(12), -1);
        let v2 = v.i8x16_replace_lane(5, 77);
        assert_eq!(v2.i8x16_extract_lane(5), 77);
    }

    #[test]
    fn i16x8_extract_replace_lanes() {
        let v = WasmV128Vector::i16x8([100, 200, 300, 400, 500, 600, 700, 800]);
        assert_eq!(v.i16x8_extract_lane(3), 400);
        let v2 = v.i16x8_replace_lane(7, 999);
        assert_eq!(v2.i16x8_extract_lane(7), 999);
    }

    #[test]
    fn wrapping_overflow_i8x16() {
        let s = simd();
        let a = WasmV128Vector::i8x16_splat(127);
        let b = WasmV128Vector::i8x16_splat(1);
        let sum = s.i8x16_add(&a, &b);
        // 127 + 1 wraps to -128 in i8
        assert_eq!(sum.as_i8_lanes()[0], -128);
    }

    #[test]
    fn from_bytes_roundtrip() {
        let bytes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let v = WasmV128Vector::from_bytes(bytes);
        assert_eq!(v.lane_bytes, bytes);
    }
}
