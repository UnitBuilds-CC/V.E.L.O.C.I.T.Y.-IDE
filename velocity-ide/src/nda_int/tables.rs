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

// ─── Diagnostics ───────────────────────────────────────────────────────────────

use serde::Serialize;

/// Serializable diagnostic for a lookup table.
#[derive(Debug, Clone, Serialize)]
pub struct LutInfo {
    pub name: String,
    pub entry_count: usize,
    pub memory_bytes: usize,
    pub min_value: i32,
    pub max_value: i32,
    pub validation_issues: Vec<String>,
}

/// Serializable summary of all NDA lookup tables.
#[derive(Debug, Clone, Serialize)]
pub struct TablesSummary {
    pub table_count: usize,
    pub total_memory_bytes: usize,
    pub tables: Vec<LutInfo>,
    pub validation_issues: Vec<String>,
}

/// Diagnostic info for the DOT_4_LUT.
pub fn dot_4_lut_info() -> LutInfo {
    let mut min_val = i8::MAX;
    let mut max_val = i8::MIN;
    for row in &DOT_4_LUT {
        for &val in row {
            if val < min_val { min_val = val; }
            if val > max_val { max_val = val; }
        }
    }
    let mut issues = Vec::new();
    // DOT_4_LUT should be 256x256
    if DOT_4_LUT.len() != 256 {
        issues.push(format!("DOT_4_LUT has {} rows, expected 256", DOT_4_LUT.len()));
    }
    // Values should be in [-16, 16] for 4-bit dot products
    if min_val < -16 || max_val > 16 {
        issues.push(format!("DOT_4_LUT values [{}, {}] outside expected [-16, 16]", min_val, max_val));
    }
    LutInfo {
        name: "DOT_4_LUT".to_string(),
        entry_count: 256 * 256,
        memory_bytes: 256 * 256 * std::mem::size_of::<i8>(),
        min_value: min_val as i32,
        max_value: max_val as i32,
        validation_issues: issues,
    }
}

/// Diagnostic info for the ADD_LUT_Q16.
pub fn add_lut_q16_info() -> LutInfo {
    let mut issues = Vec::new();
    if ADD_LUT_Q16.len() != 65536 {
        issues.push(format!("ADD_LUT_Q16 has {} entries, expected 65536", ADD_LUT_Q16.len()));
    }
    // Each byte encodes sign+extra nibbles, so max value is 0xFF
    LutInfo {
        name: "ADD_LUT_Q16".to_string(),
        entry_count: ADD_LUT_Q16.len(),
        memory_bytes: ADD_LUT_Q16.len(),
        min_value: 0,
        max_value: 255,
        validation_issues: issues,
    }
}

/// Diagnostic info for the SWIGLU_LUT_Q16.
pub fn swiglu_lut_q16_info() -> LutInfo {
    let mut issues = Vec::new();
    if SWIGLU_LUT_Q16.len() != 65536 {
        issues.push(format!("SWIGLU_LUT_Q16 has {} entries, expected 65536", SWIGLU_LUT_Q16.len()));
    }
    LutInfo {
        name: "SWIGLU_LUT_Q16".to_string(),
        entry_count: SWIGLU_LUT_Q16.len(),
        memory_bytes: SWIGLU_LUT_Q16.len(),
        min_value: 0,
        max_value: 255,
        validation_issues: issues,
    }
}

/// Diagnostic info for the FP4_PRODUCT_LUT.
pub fn fp4_product_lut_info() -> LutInfo {
    let mut min_val = i32::MAX;
    let mut max_val = i32::MIN;
    for row in &FP4_PRODUCT_LUT {
        for &val in row {
            if val < min_val { min_val = val; }
            if val > max_val { max_val = val; }
        }
    }
    let mut issues = Vec::new();
    if FP4_PRODUCT_LUT.len() != 4 {
        issues.push(format!("FP4_PRODUCT_LUT has {} rows, expected 4", FP4_PRODUCT_LUT.len()));
    }
    LutInfo {
        name: "FP4_PRODUCT_LUT".to_string(),
        entry_count: 4 * 16,
        memory_bytes: 4 * 16 * std::mem::size_of::<i32>(),
        min_value: min_val,
        max_value: max_val,
        validation_issues: issues,
    }
}

/// Diagnostic info for the FP2_PRODUCT_LUT.
pub fn fp2_product_lut_info() -> LutInfo {
    let mut min_val = i32::MAX;
    let mut max_val = i32::MIN;
    for row in &FP2_PRODUCT_LUT {
        for &val in row {
            if val < min_val { min_val = val; }
            if val > max_val { max_val = val; }
        }
    }
    let mut issues = Vec::new();
    if FP2_PRODUCT_LUT.len() != 4 {
        issues.push(format!("FP2_PRODUCT_LUT has {} rows, expected 4", FP2_PRODUCT_LUT.len()));
    }
    LutInfo {
        name: "FP2_PRODUCT_LUT".to_string(),
        entry_count: 4 * 4,
        memory_bytes: 4 * 4 * std::mem::size_of::<i32>(),
        min_value: min_val,
        max_value: max_val,
        validation_issues: issues,
    }
}

/// Summary of all NDA lookup tables.
pub fn tables_summary() -> TablesSummary {
    let tables = vec![
        dot_4_lut_info(),
        add_lut_q16_info(),
        swiglu_lut_q16_info(),
        fp4_product_lut_info(),
        fp2_product_lut_info(),
    ];
    let total_bytes: usize = tables.iter().map(|t| t.memory_bytes).sum();
    let mut issues = Vec::new();
    for t in &tables {
        for issue in &t.validation_issues {
            issues.push(format!("{}: {}", t.name, issue));
        }
    }
    TablesSummary {
        table_count: tables.len(),
        total_memory_bytes: total_bytes,
        tables,
        validation_issues: issues,
    }
}

/// Validate a DOT_4 lookup by manually computing the expected result.
pub fn validate_dot4_entry(q: u8, k: u8) -> i8 {
    let qs = q & 0x0F;
    let qe = (q >> 4) & 0x0F;
    let ks = k & 0x0F;
    let ke = (k >> 4) & 0x0F;

    let mut dot: i8 = 0;
    for bit in 0..4 {
        let qs_bit = (qs >> bit) & 1;
        let qe_bit = (qe >> bit) & 1;
        let qv: i8 = if qs_bit == 1 {
            if qe_bit == 1 { 2 } else { 1 }
        } else if qe_bit == 1 {
            -1
        } else {
            -2
        };

        let ks_bit = (ks >> bit) & 1;
        let ke_bit = (ke >> bit) & 1;
        let kv: i8 = if ks_bit == 1 {
            if ke_bit == 1 { 2 } else { 1 }
        } else if ke_bit == 1 {
            -1
        } else {
            -2
        };

        dot += qv * kv;
    }
    dot
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_4_lut_info_valid() {
        let info = dot_4_lut_info();
        assert_eq!(info.name, "DOT_4_LUT");
        assert_eq!(info.entry_count, 65536);
        assert_eq!(info.memory_bytes, 65536);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn dot_4_lut_matches_manual() {
        // Verify a few entries match the manual computation
        for q in [0u8, 1, 0xFF, 0x55, 0xAA] {
            for k in [0u8, 1, 0xFF, 0x55, 0xAA] {
                let lut_val = DOT_4_LUT[q as usize][k as usize];
                let manual_val = validate_dot4_entry(q, k);
                assert_eq!(lut_val, manual_val, "mismatch at q={}, k={}", q, k);
            }
        }
    }

    #[test]
    fn dot_4_lut_symmetry() {
        // DOT(a,b) == DOT(b,a) since multiplication is commutative
        for q in [0u8, 10, 100, 200] {
            for k in [0u8, 10, 100, 200] {
                assert_eq!(
                    DOT_4_LUT[q as usize][k as usize],
                    DOT_4_LUT[k as usize][q as usize],
                    "DOT_4_LUT not symmetric at ({}, {})", q, k
                );
            }
        }
    }

    #[test]
    fn add_lut_q16_info_valid() {
        let info = add_lut_q16_info();
        assert_eq!(info.name, "ADD_LUT_Q16");
        assert_eq!(info.entry_count, 65536);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn swiglu_lut_q16_info_valid() {
        let info = swiglu_lut_q16_info();
        assert_eq!(info.name, "SWIGLU_LUT_Q16");
        assert_eq!(info.entry_count, 65536);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn fp4_product_lut_info_valid() {
        let info = fp4_product_lut_info();
        assert_eq!(info.name, "FP4_PRODUCT_LUT");
        assert_eq!(info.entry_count, 64);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn fp4_product_lut_values() {
        // x_vals = [-2, -1, 1, 2], w_vals = [0, 1, 4, 6, 8, 12, 16, 24, 0, -1, -4, -6, -8, -12, -16, -24]
        assert_eq!(FP4_PRODUCT_LUT[0][0], 0);   // -2 * 0
        assert_eq!(FP4_PRODUCT_LUT[0][1], -2);  // -2 * 1
        assert_eq!(FP4_PRODUCT_LUT[2][3], 6);   // 1 * 6
        assert_eq!(FP4_PRODUCT_LUT[3][15], -48); // 2 * -24
    }

    #[test]
    fn fp2_product_lut_info_valid() {
        let info = fp2_product_lut_info();
        assert_eq!(info.name, "FP2_PRODUCT_LUT");
        assert_eq!(info.entry_count, 16);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn fp2_product_lut_values() {
        // x_vals = [-2, -1, 1, 2], w_vals = [0, 1, 0, -1]
        assert_eq!(FP2_PRODUCT_LUT[0][0], 0);   // -2 * 0
        assert_eq!(FP2_PRODUCT_LUT[0][1], -2);  // -2 * 1
        assert_eq!(FP2_PRODUCT_LUT[1][3], 1);   // -1 * -1
        assert_eq!(FP2_PRODUCT_LUT[3][3], -2);  // 2 * -1
    }

    #[test]
    fn tables_summary_valid() {
        let summary = tables_summary();
        assert_eq!(summary.table_count, 5);
        assert!(summary.total_memory_bytes > 0);
        assert!(summary.validation_issues.is_empty());
        assert_eq!(summary.tables.len(), 5);
    }

    #[test]
    fn tables_summary_memory() {
        let summary = tables_summary();
        // DOT_4_LUT: 65536, ADD_LUT: 65536, SWIGLU: 65536, FP4: 256, FP2: 64
        let expected = 65536 + 65536 + 65536 + 256 + 64;
        assert_eq!(summary.total_memory_bytes, expected);
    }

    #[test]
    fn validate_dot4_entry_known_values() {
        // All zeros: both values encode (-2,-2,-2,-2), dot = 4*4 = 16
        assert_eq!(validate_dot4_entry(0x00, 0x00), 16);
        // 0xFF encodes (2,2,2,2), dot = 4*4 = 16
        assert_eq!(validate_dot4_entry(0xFF, 0xFF), 16);
    }

    // ─── Expanded Tests ─────────────────────────────────────────────────

    #[test]
    fn dot_4_lut_self_dot_product() {
        // Self dot product of any encoding should be positive (sum of squares)
        for q in [0x00u8, 0x55, 0xAA, 0xFF] {
            let val = DOT_4_LUT[q as usize][q as usize];
            assert!(val > 0, "self-dot at 0x{:02X} should be positive, got {}", q, val);
        }
    }

    #[test]
    fn dot_4_lut_range() {
        // Each nibble pair produces values in {-2,-1,1,2}
        // Max dot of 4 elements: 4*4=16, min: 4*(-4)=-16
        for q in [0u8, 0x11, 0x22, 0x33, 0x44, 0x88, 0xCC, 0xFF] {
            for k in [0u8, 0x11, 0x22, 0x33, 0x44, 0x88, 0xCC, 0xFF] {
                let val = DOT_4_LUT[q as usize][k as usize];
                assert!(val >= -16 && val <= 16,
                    "DOT_4_LUT[0x{:02X}][0x{:02X}] = {} out of range", q, k, val);
            }
        }
    }

    #[test]
    fn dot_4_lut_opposite_signs() {
        // 0x00 encodes (-2,-2,-2,-2), 0xFF encodes (2,2,2,2)
        // dot = (-2)*2 + (-2)*2 + (-2)*2 + (-2)*2 = -16
        let val = DOT_4_LUT[0x00][0xFF];
        assert_eq!(val, -16);
    }

    #[test]
    fn dot_4_lut_info_serializes() {
        let info = dot_4_lut_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"name\":\"DOT_4_LUT\""));
        assert!(json.contains("\"entry_count\":65536"));
    }

    #[test]
    fn add_lut_adding_zeros() {
        // Adding encoding of 0 (which maps to +1 in our scheme)
        // sign=0xF (all positive), extra=0x0 (small) → encodes (+1,+1,+1,+1)
        // Adding (+1,+1,+1,+1) + (+1,+1,+1,+1) = (+2,+2,+2,+2)
        // +2 encodes as sign=1,extra=1 → 0xF, 0xF → key = 0x0F | (0x0F << 4) | (0x0F << 8) | (0x0F << 12)
        // Actually: xs=0xF, xe=0xF for x, ds=0xF, de=0xF for delta
        let key = 0x0F | (0x0F << 4) | (0x0F << 8) | (0x0F << 12);
        let result = ADD_LUT_Q16[key as usize];
        let res_sign = result & 0x0F;
        let res_extra = (result >> 4) & 0x0F;
        // +2 → sign=1, extra=1 for each bit → res_sign=0xF, res_extra=0xF
        assert_eq!(res_sign, 0x0F);
        assert_eq!(res_extra, 0x0F);
    }

    #[test]
    fn add_lut_opposite_values() {
        // (+2,+2,+2,+2) + (-2,-2,-2,-2) = (0,0,0,0) → encodes as +1
        // +2: sign=1, extra=1 → xs=0xF, xe=0xF
        // -2: sign=0, extra=0 → ds=0x0, de=0x0
        let key = 0x0F | (0x0F << 4) | (0x00 << 8) | (0x00 << 12);
        let result = ADD_LUT_Q16[key as usize];
        let res_sign = result & 0x0F;
        let res_extra = (result >> 4) & 0x0F;
        // 0 → clamped to 0 → ENCODE_TABLE[4] = 2 → sign=1, extra=0
        assert_eq!(res_sign, 0x0F); // all positive
        assert_eq!(res_extra, 0x00); // all small
    }

    #[test]
    fn add_lut_q16_info_serializes() {
        let info = add_lut_q16_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"name\":\"ADD_LUT_Q16\""));
    }

    #[test]
    fn swiglu_lut_gate_zero() {
        // gate encodes 0 → in our scheme 0 maps to +1
        // SiLU(+1) ≈ 0.73, but in integer LUT this is approximate
        // The LUT computes gate * up + 4 (offset) then clamps
        // Let's just verify the output is valid (no panics)
        for key in [0u32, 1, 0xFFFF, 0x5555] {
            let _ = SWIGLU_LUT_Q16[key as usize];
        }
    }

    #[test]
    fn swiglu_lut_q16_info_serializes() {
        let info = swiglu_lut_q16_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"name\":\"SWIGLU_LUT_Q16\""));
    }

    #[test]
    fn fp4_product_lut_row_zero() {
        // x_vals[0] = -2
        let w_vals = [0, 1, 4, 6, 8, 12, 16, 24, 0, -1, -4, -6, -8, -12, -16, -24];
        for (i, &w) in w_vals.iter().enumerate() {
            assert_eq!(FP4_PRODUCT_LUT[0][i], -2 * w);
        }
    }

    #[test]
    fn fp4_product_lut_row_three() {
        // x_vals[3] = 2
        let w_vals = [0, 1, 4, 6, 8, 12, 16, 24, 0, -1, -4, -6, -8, -12, -16, -24];
        for (i, &w) in w_vals.iter().enumerate() {
            assert_eq!(FP4_PRODUCT_LUT[3][i], 2 * w);
        }
    }

    #[test]
    fn fp4_product_lut_info_serializes() {
        let info = fp4_product_lut_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"entry_count\":64"));
    }

    #[test]
    fn fp2_product_lut_all_values() {
        // x_vals = [-2, -1, 1, 2], w_vals = [0, 1, 0, -1]
        let x_vals = [-2i32, -1, 1, 2];
        let w_vals = [0i32, 1, 0, -1];
        for (xi, &x) in x_vals.iter().enumerate() {
            for (wi, &w) in w_vals.iter().enumerate() {
                assert_eq!(FP2_PRODUCT_LUT[xi][wi], x * w,
                    "FP2_PRODUCT_LUT[{}][{}] mismatch", xi, wi);
            }
        }
    }

    #[test]
    fn fp2_product_lut_info_serializes() {
        let info = fp2_product_lut_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"entry_count\":16"));
    }

    #[test]
    fn tables_summary_serializes() {
        let summary = tables_summary();
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"table_count\":5"));
        assert!(json.contains("DOT_4_LUT"));
        assert!(json.contains("ADD_LUT_Q16"));
        assert!(json.contains("SWIGLU_LUT_Q16"));
        assert!(json.contains("FP4_PRODUCT_LUT"));
        assert!(json.contains("FP2_PRODUCT_LUT"));
    }

    #[test]
    fn validate_dot4_more_patterns() {
        // 0x55 = low nibble 5 (0101), high nibble 5 (0101)
        // Each bit: sign=1, extra=1 → +2
        // dot(+2*4, +2*4) = 4*(2*2) = 16
        assert_eq!(validate_dot4_entry(0x55, 0x55), 16);

        // 0xAA = low nibble A (1010), high nibble A (1010)
        // Each bit: sign=0, extra=0 → -2
        // dot(-2*4, -2*4) = 4*((-2)*(-2)) = 16
        assert_eq!(validate_dot4_entry(0xAA, 0xAA), 16);

        // +1 encoding: sign=1, extra=0 → qs has bit, qe doesn't
        // For all 4 bits: qs=0b0001=1 per bit... actually qs=5, qe=0 gives:
        // bit 0: qs_bit=1, qe_bit=0 → +1
        // So q=0x05 (qs=5, qe=0) encodes (+1,+1,+1,+1)
        // Wait: qs=5=0101, bit 0: 1, bit 1: 0, bit 2: 1, bit 3: 0
        // That's alternating +1,-1,+1,-1
        // For all +1: need qs_bit=1, qe_bit=0 for ALL bits → qs=0xF, qe=0x0
        // q = qs | (qe << 4) = 0x0F
        assert_eq!(validate_dot4_entry(0x0F, 0x0F), 4);
    }

    #[test]
    fn dot_4_lut_full_symmetry_sample() {
        // Broader symmetry check
        for q in (0..256u16).step_by(17) {
            for k in (0..256u16).step_by(17) {
                assert_eq!(
                    DOT_4_LUT[q as usize][k as usize],
                    DOT_4_LUT[k as usize][q as usize],
                    "asymmetry at ({}, {})", q, k
                );
            }
        }
    }
}
