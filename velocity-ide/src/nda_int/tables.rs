pub static DOT_4_LUT: [[i8; 256]; 256] = {
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
                    if qe_bit == 1 {
                        2
                    } else {
                        1
                    }
                } else if qe_bit == 1 {
                    -1
                } else {
                    -2
                };

                let ks_bit = (ks >> bit) & 1;
                let ke_bit = (ke >> bit) & 1;
                let kv = if ks_bit == 1 {
                    if ke_bit == 1 {
                        2
                    } else {
                        1
                    }
                } else if ke_bit == 1 {
                    -1
                } else {
                    -2
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

pub static ADD_LUT_Q16: [u8; 65536] = {
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
                if x_e_bit == 1 {
                    2
                } else {
                    1
                }
            } else if x_e_bit == 1 {
                -1
            } else {
                -2
            };

            let dv = if d_s_bit == 1 {
                if d_e_bit == 1 {
                    2
                } else {
                    1
                }
            } else if d_e_bit == 1 {
                -1
            } else {
                -2
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

pub static SWIGLU_LUT_Q16: [u8; 65536] = {
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
                if g_e_bit == 1 {
                    2
                } else {
                    1
                }
            } else if g_e_bit == 1 {
                -1
            } else {
                -2
            };

            let uv = if u_s_bit == 1 {
                if u_e_bit == 1 {
                    2
                } else {
                    1
                }
            } else if u_e_bit == 1 {
                -1
            } else {
                -2
            };

            let prod = gv * uv;
            let val = prod + 4;
            let clamped = if val < 0 {
                0
            } else if val > 8 {
                8
            } else {
                val
            } as usize;
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
    let w_vals = [0, 1, 4, 6, 8, 12, 16, 24, 0, -1, -4, -6, -8, -12, -16, -24];
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
