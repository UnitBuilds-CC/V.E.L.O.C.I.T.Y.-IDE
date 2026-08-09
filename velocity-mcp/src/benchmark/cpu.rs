#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[inline(never)]
pub fn benchmark_int4_gemm(size: usize) -> u32 {
    let mut sum = 0u32;
    let inputs = vec![0x55u8; size / 2];
    let weights = vec![0x33u8; (size * size) / 2];

    for row in 0..size {
        let mut row_sum = 0i32;
        let weight_row_offset = (row * size) / 2;
        for col_pair in 0..(size / 2) {
            let i_byte = inputs[col_pair];
            let w_byte = weights[weight_row_offset + col_pair];

            let i0 = (i_byte & 0x0F) as i32 - 8;
            let i1 = (i_byte >> 4) as i32 - 8;
            let w0 = (w_byte & 0x0F) as i32 - 8;
            let w1 = (w_byte >> 4) as i32 - 8;

            row_sum += w0 * i0 + w1 * i1;
        }
        sum = sum.wrapping_add(row_sum as u32);
    }
    sum
}

#[inline(never)]
pub fn benchmark_ternary_gemm(size: usize) -> u32 {
    let mut sum = 0u32;
    let inputs = vec![0x55555555u32; size / 16];
    let weights = vec![0x33333333u32; (size * size) / 16];

    for row in 0..size {
        let mut row_sum = 0i32;
        let weight_row_offset = (row * size) / 16;
        for col_group in 0..(size / 16) {
            let i_val = inputs[col_group];
            let w_val = weights[weight_row_offset + col_group];

            let matches = !(i_val ^ w_val);
            let count = matches.count_ones() as i32;
            row_sum += count - 8;
        }
        sum = sum.wrapping_add(row_sum as u32);
    }
    sum
}

#[inline(always)]
pub fn gemv_int4(k: usize, n: usize, inputs: &[u8], weights: &[u8], outputs: &mut [i32]) {
    for row in 0..n {
        let mut sum = 0i32;
        let weight_row_offset = (row * k) / 2;
        for col_pair in 0..(k / 2) {
            let i_byte = inputs[col_pair];
            let w_byte = weights[weight_row_offset + col_pair];

            let i0 = (i_byte & 0x0F) as i32 - 8;
            let i1 = (i_byte >> 4) as i32 - 8;
            let w0 = (w_byte & 0x0F) as i32 - 8;
            let w1 = (w_byte >> 4) as i32 - 8;

            sum += w0 * i0 + w1 * i1;
        }
        outputs[row] = sum;
    }
}

#[inline(always)]
pub fn gemv_ternary(k: usize, n: usize, inputs: &[u32], weights: &[u32], outputs: &mut [i32]) {
    for row in 0..n {
        let mut row_sum = 0i32;
        let weight_row_offset = (row * k) / 16;
        for col_group in 0..(k / 16) {
            let i_val = inputs[col_group];
            let w_val = weights[weight_row_offset + col_group];

            let matches = !(i_val ^ w_val);
            row_sum += matches.count_ones() as i32 - 8;
        }
        outputs[row] = row_sum;
    }
}

pub struct Qwen3BLayerData {
    pub inputs_2304: Vec<u8>,
    pub inputs_11008: Vec<u8>,
    pub weight_q: Vec<u8>,
    pub weight_o: Vec<u8>,
    pub weight_k: Vec<u8>,
    pub weight_v: Vec<u8>,
    pub weight_gate: Vec<u8>,
    pub weight_up: Vec<u8>,
    pub weight_down: Vec<u8>,
    pub out_2304_a: Vec<i32>,
    pub out_2304_b: Vec<i32>,
    pub out_256_k: Vec<i32>,
    pub out_256_v: Vec<i32>,
    pub out_11008_gate: Vec<i32>,
    pub out_11008_up: Vec<i32>,
}

impl Qwen3BLayerData {
    pub fn new() -> Self {
        Self {
            inputs_2304: vec![0x55u8; 2304 / 2],
            inputs_11008: vec![0x66u8; 11008 / 2],
            weight_q: vec![0x33u8; (2304 * 2304) / 2],
            weight_o: vec![0x44u8; (2304 * 2304) / 2],
            weight_k: vec![0x11u8; (2304 * 256) / 2],
            weight_v: vec![0x22u8; (2304 * 256) / 2],
            weight_gate: vec![0x77u8; (2304 * 11008) / 2],
            weight_up: vec![0x88u8; (2304 * 11008) / 2],
            weight_down: vec![0x99u8; (11008 * 2304) / 2],
            out_2304_a: vec![0; 2304],
            out_2304_b: vec![0; 2304],
            out_256_k: vec![0; 256],
            out_256_v: vec![0; 256],
            out_11008_gate: vec![0; 11008],
            out_11008_up: vec![0; 11008],
        }
    }
}

#[inline(never)]
pub fn bench_qwen_3b_layer_int4(data: &mut Qwen3BLayerData) {
    gemv_int4(
        2304,
        2304,
        &data.inputs_2304,
        &data.weight_q,
        &mut data.out_2304_a,
    );
    gemv_int4(
        2304,
        256,
        &data.inputs_2304,
        &data.weight_k,
        &mut data.out_256_k,
    );
    gemv_int4(
        2304,
        256,
        &data.inputs_2304,
        &data.weight_v,
        &mut data.out_256_v,
    );
    gemv_int4(
        2304,
        2304,
        &data.inputs_2304,
        &data.weight_o,
        &mut data.out_2304_b,
    );
    gemv_int4(
        2304,
        11008,
        &data.inputs_2304,
        &data.weight_gate,
        &mut data.out_11008_gate,
    );
    gemv_int4(
        2304,
        11008,
        &data.inputs_2304,
        &data.weight_up,
        &mut data.out_11008_up,
    );

    for i in 0..11008 {
        data.inputs_11008[i / 2] = (data.out_11008_gate[i] ^ data.out_11008_up[i]) as u8;
    }

    gemv_int4(
        11008,
        2304,
        &data.inputs_11008,
        &data.weight_down,
        &mut data.out_2304_a,
    );
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn gemv_nda_sse2(
    k: usize,
    n: usize,
    inputs_active: &[u32],
    inputs_pos: &[u32],
    weights_active: &[u32],
    weights_pos: &[u32],
    outputs: &mut [i32],
) {
    let num_col_groups_128 = k / 128;
    for row in 0..n {
        let mut row_sum = 0i32;
        let all_ones = _mm_set1_epi32(-1);
        for cg in 0..num_col_groups_128 {
            let in_act = _mm_loadu_si128(inputs_active.as_ptr().add(cg * 4) as *const __m128i);
            let in_pos = _mm_loadu_si128(inputs_pos.as_ptr().add(cg * 4) as *const __m128i);

            let w_act = _mm_loadu_si128(
                weights_active.as_ptr().add(cg * n * 4 + row * 4) as *const __m128i
            );
            let w_pos =
                _mm_loadu_si128(weights_pos.as_ptr().add(cg * n * 4 + row * 4) as *const __m128i);

            let act_both = _mm_and_si128(in_act, w_act);
            let same_sign = _mm_xor_si128(in_pos, w_pos);
            let same_sign = _mm_xor_si128(same_sign, all_ones);

            let pos_contrib = _mm_and_si128(act_both, same_sign);
            let neg_contrib = _mm_andnot_si128(same_sign, act_both);

            let pos_arr: [u32; 4] = std::mem::transmute(pos_contrib);
            let neg_arr: [u32; 4] = std::mem::transmute(neg_contrib);

            row_sum += (pos_arr[0].count_ones() as i32 - neg_arr[0].count_ones() as i32)
                + (pos_arr[1].count_ones() as i32 - neg_arr[1].count_ones() as i32)
                + (pos_arr[2].count_ones() as i32 - neg_arr[2].count_ones() as i32)
                + (pos_arr[3].count_ones() as i32 - neg_arr[3].count_ones() as i32);
        }
        outputs[row] = row_sum;
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn gemv_nda_scalar(
    k: usize,
    n: usize,
    inputs_active: &[u32],
    inputs_pos: &[u32],
    weights_active: &[u32],
    weights_pos: &[u32],
    outputs: &mut [i32],
) {
    let num_col_groups_128 = k / 128;
    for row in 0..n {
        let mut row_sum = 0i32;
        for cg in 0..num_col_groups_128 {
            for i in 0..4 {
                let in_a = inputs_active[cg * 4 + i];
                let in_p = inputs_pos[cg * 4 + i];

                let w_a = weights_active[cg * n * 4 + row * 4 + i];
                let w_p = weights_pos[cg * n * 4 + row * 4 + i];

                let act_both = in_a & w_a;
                let same_sign = !(in_p ^ w_p);
                let pos_contrib = act_both & same_sign;
                let neg_contrib = act_both & !same_sign;

                row_sum += pos_contrib.count_ones() as i32 - neg_contrib.count_ones() as i32;
            }
        }
        outputs[row] = row_sum;
    }
}

#[inline(always)]
pub fn gemv_nda(
    k: usize,
    n: usize,
    inputs_active: &[u32],
    inputs_pos: &[u32],
    weights_active: &[u32],
    weights_pos: &[u32],
    outputs: &mut [i32],
) {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 GEMV call. Input/output slices are valid and have correct lengths
        // guaranteed by the caller. SSE2 is available on all x86_64 processors.
        unsafe {
            gemv_nda_sse2(
                k,
                n,
                inputs_active,
                inputs_pos,
                weights_active,
                weights_pos,
                outputs,
            );
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        gemv_nda_scalar(
            k,
            n,
            inputs_active,
            inputs_pos,
            weights_active,
            weights_pos,
            outputs,
        );
    }
}

pub struct BitNet3BLayerData {
    pub inputs_3200: Vec<u32>,
    pub inputs_8640: Vec<u32>,
    pub weight_q: Vec<u32>,
    pub weight_k: Vec<u32>,
    pub weight_v: Vec<u32>,
    pub weight_o: Vec<u32>,
    pub weight_gate: Vec<u32>,
    pub weight_up: Vec<u32>,
    pub weight_down: Vec<u32>,

    pub nda_inputs_3200_active: Vec<u32>,
    pub nda_inputs_3200_pos: Vec<u32>,
    pub nda_inputs_8640_active: Vec<u32>,
    pub nda_inputs_8640_pos: Vec<u32>,

    pub nda_weight_q_active: Vec<u32>,
    pub nda_weight_q_pos: Vec<u32>,
    pub nda_weight_k_active: Vec<u32>,
    pub nda_weight_k_pos: Vec<u32>,
    pub nda_weight_v_active: Vec<u32>,
    pub nda_weight_v_pos: Vec<u32>,
    pub nda_weight_o_active: Vec<u32>,
    pub nda_weight_o_pos: Vec<u32>,
    pub nda_weight_gate_active: Vec<u32>,
    pub nda_weight_gate_pos: Vec<u32>,
    pub nda_weight_up_active: Vec<u32>,
    pub nda_weight_up_pos: Vec<u32>,
    pub nda_weight_down_active: Vec<u32>,
    pub nda_weight_down_pos: Vec<u32>,

    pub out_3200_q: Vec<i32>,
    pub out_3200_k: Vec<i32>,
    pub out_3200_v: Vec<i32>,
    pub out_3200_o: Vec<i32>,
    pub out_8640_gate: Vec<i32>,
    pub out_8640_up: Vec<i32>,
    pub out_3200_down: Vec<i32>,

    pub nda_out_3200_q: Vec<i32>,
    pub nda_out_3200_k: Vec<i32>,
    pub nda_out_3200_v: Vec<i32>,
    pub nda_out_3200_o: Vec<i32>,
    pub nda_out_8640_gate: Vec<i32>,
    pub nda_out_8640_up: Vec<i32>,
    pub nda_out_3200_down: Vec<i32>,
}

impl BitNet3BLayerData {
    pub fn new() -> Self {
        let inputs_3200 = vec![0x55555555u32; 3200 / 16];
        let inputs_8640 = vec![0x66666666u32; 8640 / 16];
        let weight_q = vec![0x33333333u32; (3200 * 3200) / 16];
        let weight_k = vec![0x11111111u32; (3200 * 3200) / 16];
        let weight_v = vec![0x22222222u32; (3200 * 3200) / 16];
        let weight_o = vec![0x44444444u32; (3200 * 3200) / 16];
        let weight_gate = vec![0x77777777u32; (3200 * 8640) / 16];
        let weight_up = vec![0x88888888u32; (3200 * 8640) / 16];
        let weight_down = vec![0x99999999u32; (8640 * 3200) / 16];

        let to_bytes_u32 = |slice: &[u32]| -> &[u8] {
            let byte_len = slice
                .len()
                .checked_mul(4)
                .expect("benchmark slice overflow");
            // SAFETY: `slice` is &[u32]; reinterpreting as &[u8] is valid.
            // Length checked via checked_mul.
            unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, byte_len) }
        };
        let to_u32_vec = |bytes: Vec<u8>| -> Vec<u32> {
            // SAFETY: `bytes` came from a Vec<u32> via to_bytes; alignment and length
            // are guaranteed to be correct for u32 reinterpretation.
            unsafe {
                let u32_ptr = bytes.as_ptr() as *const u32;
                std::slice::from_raw_parts(u32_ptr, bytes.len() / 4).to_vec()
            }
        };

        let (nda_inputs_3200_active, nda_inputs_3200_pos) =
            velocity_ide::compiler::driver::pack_inputs_nda(&inputs_3200);
        let (nda_inputs_8640_active, nda_inputs_8640_pos) =
            velocity_ide::compiler::driver::pack_inputs_nda(&inputs_8640);

        let (wq_a, wq_p) =
            velocity_ide::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_q), 3200, 3200);
        let (wk_a, wk_p) =
            velocity_ide::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_k), 3200, 3200);
        let (wv_a, wv_p) =
            velocity_ide::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_v), 3200, 3200);
        let (wo_a, wo_p) =
            velocity_ide::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_o), 3200, 3200);
        let (wgate_a, wgate_p) = velocity_ide::compiler::driver::pack_weights_nda(
            to_bytes_u32(&weight_gate),
            3200,
            8640,
        );
        let (wup_a, wup_p) =
            velocity_ide::compiler::driver::pack_weights_nda(to_bytes_u32(&weight_up), 3200, 8640);
        let (wdown_a, wdown_p) = velocity_ide::compiler::driver::pack_weights_nda(
            to_bytes_u32(&weight_down),
            8640,
            3200,
        );

        Self {
            inputs_3200,
            inputs_8640,
            weight_q,
            weight_k,
            weight_v,
            weight_o,
            weight_gate,
            weight_up,
            weight_down,
            nda_inputs_3200_active,
            nda_inputs_3200_pos,
            nda_inputs_8640_active,
            nda_inputs_8640_pos,
            nda_weight_q_active: to_u32_vec(wq_a),
            nda_weight_q_pos: to_u32_vec(wq_p),
            nda_weight_k_active: to_u32_vec(wk_a),
            nda_weight_k_pos: to_u32_vec(wk_p),
            nda_weight_v_active: to_u32_vec(wv_a),
            nda_weight_v_pos: to_u32_vec(wv_p),
            nda_weight_o_active: to_u32_vec(wo_a),
            nda_weight_o_pos: to_u32_vec(wo_p),
            nda_weight_gate_active: to_u32_vec(wgate_a),
            nda_weight_gate_pos: to_u32_vec(wgate_p),
            nda_weight_up_active: to_u32_vec(wup_a),
            nda_weight_up_pos: to_u32_vec(wup_p),
            nda_weight_down_active: to_u32_vec(wdown_a),
            nda_weight_down_pos: to_u32_vec(wdown_p),
            out_3200_q: vec![0; 3200],
            out_3200_k: vec![0; 3200],
            out_3200_v: vec![0; 3200],
            out_3200_o: vec![0; 3200],
            out_8640_gate: vec![0; 8640],
            out_8640_up: vec![0; 8640],
            out_3200_down: vec![0; 3200],
            nda_out_3200_q: vec![0; 3200],
            nda_out_3200_k: vec![0; 3200],
            nda_out_3200_v: vec![0; 3200],
            nda_out_3200_o: vec![0; 3200],
            nda_out_8640_gate: vec![0; 8640],
            nda_out_8640_up: vec![0; 8640],
            nda_out_3200_down: vec![0; 3200],
        }
    }
}

#[inline(never)]
pub fn bench_bitnet_3b_layer_ternary(data: &mut BitNet3BLayerData) {
    gemv_ternary(
        3200,
        3200,
        &data.inputs_3200,
        &data.weight_q,
        &mut data.out_3200_q,
    );
    gemv_ternary(
        3200,
        3200,
        &data.inputs_3200,
        &data.weight_k,
        &mut data.out_3200_k,
    );
    gemv_ternary(
        3200,
        3200,
        &data.inputs_3200,
        &data.weight_v,
        &mut data.out_3200_v,
    );
    gemv_ternary(
        3200,
        3200,
        &data.inputs_3200,
        &data.weight_o,
        &mut data.out_3200_o,
    );
    gemv_ternary(
        3200,
        8640,
        &data.inputs_3200,
        &data.weight_gate,
        &mut data.out_8640_gate,
    );
    gemv_ternary(
        3200,
        8640,
        &data.inputs_3200,
        &data.weight_up,
        &mut data.out_8640_up,
    );

    for i in 0..(8640 / 16) {
        data.inputs_8640[i] = (data.out_8640_gate[i * 16] ^ data.out_8640_up[i * 16]) as u32;
    }

    gemv_ternary(
        8640,
        3200,
        &data.inputs_8640,
        &data.weight_down,
        &mut data.out_3200_down,
    );
}

#[inline(never)]
pub fn bench_bitnet_3b_layer_nda(data: &mut BitNet3BLayerData) {
    gemv_nda(
        3200,
        3200,
        &data.nda_inputs_3200_active,
        &data.nda_inputs_3200_pos,
        &data.nda_weight_q_active,
        &data.nda_weight_q_pos,
        &mut data.nda_out_3200_q,
    );
    gemv_nda(
        3200,
        3200,
        &data.nda_inputs_3200_active,
        &data.nda_inputs_3200_pos,
        &data.nda_weight_k_active,
        &data.nda_weight_k_pos,
        &mut data.nda_out_3200_k,
    );
    gemv_nda(
        3200,
        3200,
        &data.nda_inputs_3200_active,
        &data.nda_inputs_3200_pos,
        &data.nda_weight_v_active,
        &data.nda_weight_v_pos,
        &mut data.nda_out_3200_v,
    );
    gemv_nda(
        3200,
        3200,
        &data.nda_inputs_3200_active,
        &data.nda_inputs_3200_pos,
        &data.nda_weight_o_active,
        &data.nda_weight_o_pos,
        &mut data.nda_out_3200_o,
    );
    gemv_nda(
        3200,
        8640,
        &data.nda_inputs_3200_active,
        &data.nda_inputs_3200_pos,
        &data.nda_weight_gate_active,
        &data.nda_weight_gate_pos,
        &mut data.nda_out_8640_gate,
    );
    gemv_nda(
        3200,
        8640,
        &data.nda_inputs_3200_active,
        &data.nda_inputs_3200_pos,
        &data.nda_weight_up_active,
        &data.nda_weight_up_pos,
        &mut data.nda_out_8640_up,
    );

    for i in 0..(8640 / 32) {
        let mut active_word = 0u32;
        let mut pos_word = 0u32;
        for bit in 0..32 {
            let idx = i * 32 + bit;
            let val = (data.nda_out_8640_gate[idx] * data.nda_out_8640_up[idx]) as f32 * 0.0001;
            let q_val = if val > 0.0 {
                1
            } else if val < 0.0 {
                -1
            } else {
                0
            };
            if q_val != 0 {
                active_word |= 1 << bit;
                if q_val == 1 {
                    pos_word |= 1 << bit;
                }
            }
        }
        data.nda_inputs_8640_active[i] = active_word;
        data.nda_inputs_8640_pos[i] = pos_word;
    }

    gemv_nda(
        8640,
        3200,
        &data.nda_inputs_8640_active,
        &data.nda_inputs_8640_pos,
        &data.nda_weight_down_active,
        &data.nda_weight_down_pos,
        &mut data.nda_out_3200_down,
    );
}
