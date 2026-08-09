//! Weight and input packing routines for NDA and uvec4 GPU formats.
//!
//! # Safety Invariants
//!
//! All `unsafe` blocks reinterpret between `&[u8]` and `&[u32]` via `from_raw_parts`.
//! Callers must ensure input byte slices are 4-byte aligned and have lengths that are
//! multiples of 4. All packing functions are internal and called with pre-validated
//! weight buffers from the model loader.

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
