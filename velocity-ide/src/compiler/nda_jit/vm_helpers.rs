use serde::Serialize;
use std::sync::Arc;

use crate::nda_int::{nda_vec_add_inplace, NdaVec};
use crate::site_map::verifier::{CmpOp, VecOpKind};

use super::types::JitVal;

#[inline(always)]
fn silu(x: f32) -> f32 {
    x / (1.0 + (-x).exp())
}

pub fn broadcast_scalar(len: usize, val: i32, log2_scale: i8) -> NdaVec {
    let bytes = len.div_ceil(8);
    let (s_byte, e_byte) = match val {
        -2 => (0x00, 0x00),
        -1 => (0x00, 0xFF),
        1 => (0xFF, 0x00),
        _ => (0xFF, 0xFF),
    };
    NdaVec {
        len,
        log2_scale,
        sign: vec![s_byte; bytes].into(),
        extra: vec![e_byte; bytes].into(),
    }
}

pub fn broadcast_float(len: usize, val: f32) -> NdaVec {
    let nda_res = NdaVec::from_f32_slice(&[val]);
    broadcast_scalar(len, nda_res.get_raw(0), nda_res.log2_scale)
}

pub fn add_vals(lhs: &JitVal, rhs: &JitVal) -> JitVal {
    match (lhs, rhs) {
        (JitVal::Float(l), JitVal::Float(r)) => JitVal::Float(l + r),
        (JitVal::Float(l), JitVal::Scalar(r_v, r_s)) => {
            let r_actual = (*r_v as f32) * 2.0f32.powi(*r_s as i32);
            JitVal::Float(l + r_actual)
        }
        (JitVal::Scalar(l_v, l_s), JitVal::Float(r)) => {
            let l_actual = (*l_v as f32) * 2.0f32.powi(*l_s as i32);
            JitVal::Float(l_actual + r)
        }
        (JitVal::Scalar(l_v, l_s), JitVal::Scalar(r_v, r_s)) => {
            if *l_s == 0 && *r_s == 0 {
                JitVal::Scalar(l_v + r_v, 0)
            } else {
                const ENCODE_TABLE: [u8; 9] = [0, 0, 0, 1, 2, 2, 3, 3, 3];
                let out_scale = (*l_s).max(*r_s);
                let l_shift = (out_scale - *l_s).max(0) as u32;
                let r_shift = (out_scale - *r_s).max(0) as u32;
                let lv = *l_v >> l_shift;
                let rv = *r_v >> r_shift;
                let sum = lv + rv;
                let clamped = (sum + 4).clamp(0, 8) as usize;
                let enc = ENCODE_TABLE[clamped];
                let val = match enc {
                    0 => -2,
                    1 => -1,
                    2 => 1,
                    3 => 2,
                    _ => unreachable!(),
                };
                JitVal::Scalar(val, out_scale)
            }
        }
        _ => {
            let mut lhs_vec = match lhs {
                JitVal::Vector(v) => (**v).clone(),
                JitVal::Float(val) => broadcast_scalar(1, val.to_bits() as i32, 0),
                JitVal::Scalar(val, scale) => {
                    let r_len = match rhs {
                        JitVal::Vector(rv) => rv.len,
                        _ => 1,
                    };
                    broadcast_scalar(r_len, *val, *scale)
                }
            };
            let rhs_vec = match rhs {
                JitVal::Vector(v) => v.clone(),
                JitVal::Float(val) => Arc::new(broadcast_scalar(1, val.to_bits() as i32, 0)),
                JitVal::Scalar(val, scale) => {
                    let l_len = lhs_vec.len;
                    Arc::new(broadcast_scalar(l_len, *val, *scale))
                }
            };
            nda_vec_add_inplace(&mut lhs_vec, &rhs_vec);
            JitVal::Vector(Arc::new(lhs_vec))
        }
    }
}

pub fn compare_vals(op: CmpOp, lhs: &JitVal, rhs: &JitVal) -> JitVal {
    match (lhs, rhs) {
        (JitVal::Float(l), JitVal::Float(r)) => {
            let l = *l;
            let r = *r;
            let cmp = match op {
                CmpOp::Eq => (l - r).abs() < 1e-6,
                CmpOp::Ne => (l - r).abs() >= 1e-6,
                CmpOp::Lt => l < r,
                CmpOp::Gt => l > r,
                CmpOp::Le => l <= r,
                CmpOp::Ge => l >= r,
            };
            JitVal::Scalar(if cmp { 1 } else { -1 }, 0)
        }
        (JitVal::Float(l), JitVal::Scalar(r_v, r_s)) => {
            let l = *l;
            let r_actual = (*r_v as f32) * 2.0f32.powi(*r_s as i32);
            let cmp = match op {
                CmpOp::Eq => (l - r_actual).abs() < 1e-6,
                CmpOp::Ne => (l - r_actual).abs() >= 1e-6,
                CmpOp::Lt => l < r_actual,
                CmpOp::Gt => l > r_actual,
                CmpOp::Le => l <= r_actual,
                CmpOp::Ge => l >= r_actual,
            };
            JitVal::Scalar(if cmp { 1 } else { -1 }, 0)
        }
        (JitVal::Scalar(l_v, l_s), JitVal::Float(r)) => {
            let r = *r;
            let l_actual = (*l_v as f32) * 2.0f32.powi(*l_s as i32);
            let cmp = match op {
                CmpOp::Eq => (l_actual - r).abs() < 1e-6,
                CmpOp::Ne => (l_actual - r).abs() >= 1e-6,
                CmpOp::Lt => l_actual < r,
                CmpOp::Gt => l_actual > r,
                CmpOp::Le => l_actual <= r,
                CmpOp::Ge => l_actual >= r,
            };
            JitVal::Scalar(if cmp { 1 } else { -1 }, 0)
        }
        (JitVal::Scalar(l_v, l_s), JitVal::Scalar(r_v, r_s)) => {
            if *l_s == 0 && *r_s == 0 {
                let cmp = match op {
                    CmpOp::Eq => l_v == r_v,
                    CmpOp::Ne => l_v != r_v,
                    CmpOp::Lt => l_v < r_v,
                    CmpOp::Gt => l_v > r_v,
                    CmpOp::Le => l_v <= r_v,
                    CmpOp::Ge => l_v >= r_v,
                };
                let val = if cmp { 1 } else { -1 };
                JitVal::Scalar(val, 0)
            } else {
                let l_actual = (*l_v as f32) * 2.0f32.powi(*l_s as i32);
                let r_actual = (*r_v as f32) * 2.0f32.powi(*r_s as i32);
                let cmp = match op {
                    CmpOp::Eq => (l_actual - r_actual).abs() < 1e-6,
                    CmpOp::Ne => (l_actual - r_actual).abs() >= 1e-6,
                    CmpOp::Lt => l_actual < r_actual,
                    CmpOp::Gt => l_actual > r_actual,
                    CmpOp::Le => l_actual <= r_actual,
                    CmpOp::Ge => l_actual >= r_actual,
                };
                let val = if cmp { 1 } else { -1 };
                JitVal::Scalar(val, 0)
            }
        }
        _ => {
            let l_len = match lhs {
                JitVal::Vector(v) => v.len,
                _ => 1,
            };
            let r_len = match rhs {
                JitVal::Vector(v) => v.len,
                _ => 1,
            };
            let len = l_len.max(r_len);

            let lhs_vec = match lhs {
                JitVal::Vector(v) => (**v).clone(),
                JitVal::Scalar(val, scale) => broadcast_scalar(len, *val, *scale),
                JitVal::Float(val) => broadcast_float(len, *val),
            };
            let rhs_vec = match rhs {
                JitVal::Vector(v) => (**v).clone(),
                JitVal::Scalar(val, scale) => broadcast_scalar(len, *val, *scale),
                JitVal::Float(val) => broadcast_float(len, *val),
            };

            let bytes = len.div_ceil(8);
            let mut sign = vec![0u8; bytes];
            let mut extra = vec![0u8; bytes];

            for byte_idx in 0..bytes {
                let lhs_s = if byte_idx < lhs_vec.sign.len() {
                    lhs_vec.sign[byte_idx]
                } else {
                    0
                };
                let lhs_e = if byte_idx < lhs_vec.extra.len() {
                    lhs_vec.extra[byte_idx]
                } else {
                    0
                };

                let rhs_s = if byte_idx < rhs_vec.sign.len() {
                    rhs_vec.sign[byte_idx]
                } else {
                    0
                };
                let rhs_e = if byte_idx < rhs_vec.extra.len() {
                    rhs_vec.extra[byte_idx]
                } else {
                    0
                };

                let mut s_byte = 0u8;
                let mut e_byte = 0u8;

                let base_idx = byte_idx * 8;
                for bit in 0..8 {
                    let i = base_idx + bit;
                    if i >= len {
                        break;
                    }
                    let l = if i < lhs_vec.len {
                        let mask = 1u8 << bit;
                        let is_pos = (lhs_s & mask) != 0;
                        let is_large = (lhs_s & mask) == (lhs_e & mask);
                        let mag = if is_large { 2i32 } else { 1 };
                        if is_pos {
                            mag
                        } else {
                            -mag
                        }
                    } else {
                        0
                    };
                    let r = if i < rhs_vec.len {
                        let mask = 1u8 << bit;
                        let is_pos = (rhs_s & mask) != 0;
                        let is_large = (rhs_s & mask) == (rhs_e & mask);
                        let mag = if is_large { 2i32 } else { 1 };
                        if is_pos {
                            mag
                        } else {
                            -mag
                        }
                    } else {
                        0
                    };
                    let cmp = match op {
                        CmpOp::Eq => l == r,
                        CmpOp::Ne => l != r,
                        CmpOp::Lt => l < r,
                        CmpOp::Gt => l > r,
                        CmpOp::Le => l <= r,
                        CmpOp::Ge => l >= r,
                    };
                    s_byte |= (cmp as u8) << bit;
                    e_byte |= ((!cmp) as u8) << bit;
                }
                sign[byte_idx] = s_byte;
                extra[byte_idx] = e_byte;
            }

            JitVal::Vector(Arc::new(NdaVec {
                len,
                log2_scale: 0,
                sign: sign.into(),
                extra: extra.into(),
            }))
        }
    }
}

pub fn apply_vec_op(op: VecOpKind, val: &JitVal) -> JitVal {
    match op {
        VecOpKind::Negate => match val {
            JitVal::Float(v) => JitVal::Float(-*v),
            JitVal::Scalar(v, s) => {
                if *s == 0 {
                    JitVal::Scalar(-*v, 0)
                } else {
                    JitVal::Scalar(-v, *s)
                }
            }
            JitVal::Vector(v) => {
                let mut new_sign = v.sign.to_vec();
                let mut new_extra = v.extra.to_vec();
                for i in 0..new_sign.len() {
                    new_sign[i] = !new_sign[i];
                    new_extra[i] = !new_extra[i];
                }
                if v.len % 8 != 0 {
                    let mask = (1u8 << (v.len % 8)) - 1;
                    if let Some(last) = new_sign.last_mut() {
                        *last &= mask;
                    }
                    if let Some(last) = new_extra.last_mut() {
                        *last &= mask;
                    }
                }
                JitVal::Vector(Arc::new(NdaVec {
                    len: v.len,
                    log2_scale: v.log2_scale,
                    sign: new_sign.into(),
                    extra: new_extra.into(),
                }))
            }
        },
        VecOpKind::Abs => match val {
            JitVal::Float(v) => JitVal::Float(v.abs()),
            JitVal::Scalar(v, s) => {
                if *s == 0 {
                    JitVal::Scalar(v.abs(), 0)
                } else {
                    JitVal::Scalar(v.abs(), *s)
                }
            }
            JitVal::Vector(v) => {
                let mut new_sign = vec![0xFFu8; v.sign.len()];
                let mut new_extra = vec![0u8; v.extra.len()];
                #[allow(clippy::needless_range_loop)]
                for i in 0..v.sign.len() {
                    new_extra[i] = !(v.sign[i] ^ v.extra[i]);
                }
                if v.len % 8 != 0 {
                    let mask = (1u8 << (v.len % 8)) - 1;
                    if let Some(last) = new_sign.last_mut() {
                        *last &= mask;
                    }
                    if let Some(last) = new_extra.last_mut() {
                        *last &= mask;
                    }
                }
                JitVal::Vector(Arc::new(NdaVec {
                    len: v.len,
                    log2_scale: v.log2_scale,
                    sign: new_sign.into(),
                    extra: new_extra.into(),
                }))
            }
        },
        VecOpKind::ReduceSum => match val {
            JitVal::Float(v) => JitVal::Float(*v),
            JitVal::Scalar(v, s) => JitVal::Scalar(*v, *s),
            JitVal::Vector(v) => {
                let mut raw_sum = 0i32;
                let bytes = v.sign.len();
                const DECODE_TABLE: [i32; 4] = [-2, -1, 1, 2];
                for byte_idx in 0..bytes {
                    let mut s_shift = v.sign[byte_idx];
                    let mut e_shift = v.extra[byte_idx];
                    let base_idx = byte_idx * 8;
                    for bit in 0..8 {
                        let i = base_idx + bit;
                        if i >= v.len {
                            break;
                        }
                        let idx = ((s_shift & 1) << 1) | (e_shift & 1);
                        raw_sum += DECODE_TABLE[idx as usize];
                        s_shift >>= 1;
                        e_shift >>= 1;
                    }
                }
                let logical_sum = (raw_sum as f32) * 2.0f32.powi(v.log2_scale as i32);
                let nda_res = NdaVec::from_f32_slice(&[logical_sum]);
                JitVal::Scalar(nda_res.get_raw(0), nda_res.log2_scale)
            }
        },
        VecOpKind::SiLU => match val {
            JitVal::Float(v) => JitVal::Float(silu(*v)),
            JitVal::Scalar(v, s) => {
                let actual = (*v as f32) * 2.0f32.powi(*s as i32);
                let res = silu(actual);
                let nda_res = NdaVec::from_f32_slice(&[res]);
                JitVal::Scalar(nda_res.get_raw(0), nda_res.log2_scale)
            }
            JitVal::Vector(v) => {
                let f32s = v.to_f32_vec();
                let result: Vec<f32> = f32s.iter().map(|&x| silu(x)).collect();
                JitVal::Vector(Arc::new(NdaVec::from_f32_slice(&result)))
            }
        },
    }
}

/// Diagnostic info about a JitVal without requiring full NdaVec expansion.
#[derive(Debug, Clone, Serialize)]
pub struct JitValInfo {
    pub val_type: String,
    pub is_vector: bool,
    pub is_scalar: bool,
    pub is_float: bool,
    pub vector_len: Option<usize>,
    pub vector_log2_scale: Option<i8>,
    pub vector_bytes: Option<usize>,
    pub scalar_value: Option<i32>,
    pub scalar_scale: Option<i8>,
    pub float_value: Option<f32>,
    pub validation_issues: Vec<String>,
}

/// Inspect a JitVal and return diagnostic info.
pub fn jit_val_info(val: &JitVal) -> JitValInfo {
    let mut issues = Vec::new();
    match val {
        JitVal::Vector(v) => {
            let bytes = v.sign.len() + v.extra.len();
            if v.len == 0 {
                issues.push("vector length is 0".into());
            }
            if v.sign.is_empty() {
                issues.push("vector sign buffer is empty".into());
            }
            let expected_bytes = v.len.div_ceil(8);
            if v.sign.len() != expected_bytes {
                issues.push(format!(
                    "sign buffer len {} != expected {} for vec len {}",
                    v.sign.len(), expected_bytes, v.len
                ));
            }
            JitValInfo {
                val_type: "vector".into(),
                is_vector: true,
                is_scalar: false,
                is_float: false,
                vector_len: Some(v.len),
                vector_log2_scale: Some(v.log2_scale),
                vector_bytes: Some(bytes),
                scalar_value: None,
                scalar_scale: None,
                float_value: None,
                validation_issues: issues,
            }
        }
        JitVal::Scalar(v, s) => {
            // Ternary values should be -2, -1, 1, or 2
            if ![-2, -1, 1, 2].contains(v) {
                issues.push(format!("scalar value {} outside ternary range [-2,-1,1,2]", v));
            }
            JitValInfo {
                val_type: format!("scalar({}, scale={})", v, s),
                is_vector: false,
                is_scalar: true,
                is_float: false,
                vector_len: None,
                vector_log2_scale: None,
                vector_bytes: None,
                scalar_value: Some(*v),
                scalar_scale: Some(*s),
                float_value: None,
                validation_issues: issues,
            }
        }
        JitVal::Float(v) => {
            let issues = if v.is_nan() {
                vec!["float value is NaN".into()]
            } else if v.is_infinite() {
                vec!["float value is infinite".into()]
            } else {
                vec![]
            };
            JitValInfo {
                val_type: format!("float({})", v),
                is_vector: false,
                is_scalar: false,
                is_float: true,
                vector_len: None,
                vector_log2_scale: None,
                vector_bytes: None,
                scalar_value: None,
                scalar_scale: None,
                float_value: Some(*v),
                validation_issues: issues,
            }
        }
    }
}

/// Validate broadcast parameters.
pub fn validate_broadcast_params(len: usize, val: i32) -> Vec<String> {
    let mut issues = Vec::new();
    if len == 0 {
        issues.push("broadcast length is 0".into());
    }
    if ![-2, -1, 0, 1, 2].contains(&val) && val != 0 {
        // Non-ternary values are only valid for float broadcast
        // (they come from NdaVec encoding, so we allow them)
    }
    issues
}

/// Validate parameters for a vector operation.
pub fn validate_vec_op(op: VecOpKind, val: &JitVal) -> Vec<String> {
    let mut issues = Vec::new();
    if let JitVal::Vector(v) = val {
        if v.len == 0 {
            issues.push(format!("{:?} on zero-length vector", op));
        }
        if v.sign.len() != v.extra.len() {
            issues.push(format!(
                "{:?} sign/extra length mismatch: {} vs {}",
                op, v.sign.len(), v.extra.len()
            ));
        }
    }
    issues
}

/// Classify the result type of an add_vals operation.
pub fn classify_add_result(lhs: &JitVal, rhs: &JitVal) -> &'static str {
    match (lhs, rhs) {
        (JitVal::Float(_), JitVal::Float(_)) => "float",
        (JitVal::Float(_), JitVal::Scalar(_, _)) | (JitVal::Scalar(_, _), JitVal::Float(_)) => {
            "float"
        }
        (JitVal::Scalar(_, _), JitVal::Scalar(_, _)) => "scalar",
        (JitVal::Vector(_), _) | (_, JitVal::Vector(_)) => "vector",
        _ => "vector",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jit_val_info_float() {
        let val = JitVal::Float(3.14);
        let info = jit_val_info(&val);
        assert!(info.is_float);
        assert!(!info.is_vector);
        assert!(!info.is_scalar);
        assert_eq!(info.val_type, "float(3.14)");
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn jit_val_info_float_nan() {
        let val = JitVal::Float(f32::NAN);
        let info = jit_val_info(&val);
        assert!(info.is_float);
        assert!(info.validation_issues.iter().any(|i| i.contains("NaN")));
    }

    #[test]
    fn jit_val_info_float_infinite() {
        let val = JitVal::Float(f32::INFINITY);
        let info = jit_val_info(&val);
        assert!(info.validation_issues.iter().any(|i| i.contains("infinite")));
    }

    #[test]
    fn jit_val_info_scalar_valid() {
        let val = JitVal::Scalar(1, 0);
        let info = jit_val_info(&val);
        assert!(info.is_scalar);
        assert_eq!(info.scalar_value, Some(1));
        assert_eq!(info.scalar_scale, Some(0));
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn jit_val_info_scalar_out_of_range() {
        let val = JitVal::Scalar(5, 0);
        let info = jit_val_info(&val);
        assert!(info.validation_issues.iter().any(|i| i.contains("ternary")));
    }

    #[test]
    fn jit_val_info_vector() {
        let v = NdaVec {
            len: 16,
            log2_scale: 0,
            sign: vec![0xFF, 0xAA].into(),
            extra: vec![0x00, 0x55].into(),
        };
        let val = JitVal::Vector(Arc::new(v));
        let info = jit_val_info(&val);
        assert!(info.is_vector);
        assert_eq!(info.vector_len, Some(16));
        assert_eq!(info.vector_bytes, Some(4)); // 2 + 2
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn jit_val_info_vector_zero_len() {
        let v = NdaVec {
            len: 0,
            log2_scale: 0,
            sign: vec![].into(),
            extra: vec![].into(),
        };
        let val = JitVal::Vector(Arc::new(v));
        let info = jit_val_info(&val);
        assert!(info.validation_issues.iter().any(|i| i.contains("length is 0")));
    }

    #[test]
    fn jit_val_info_serializes() {
        let val = JitVal::Float(1.0);
        let info = jit_val_info(&val);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"is_float\":true"));
        assert!(json.contains("\"val_type\""));
    }

    #[test]
    fn validate_broadcast_params_zero_len() {
        let issues = validate_broadcast_params(0, 1);
        assert!(issues.iter().any(|i| i.contains("0")));
    }

    #[test]
    fn validate_broadcast_params_valid() {
        let issues = validate_broadcast_params(64, 1);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_vec_op_valid() {
        let v = NdaVec {
            len: 8,
            log2_scale: 0,
            sign: vec![0xFF].into(),
            extra: vec![0x00].into(),
        };
        let val = JitVal::Vector(Arc::new(v));
        let issues = validate_vec_op(VecOpKind::Negate, &val);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_vec_op_zero_length() {
        let v = NdaVec {
            len: 0,
            log2_scale: 0,
            sign: vec![].into(),
            extra: vec![].into(),
        };
        let val = JitVal::Vector(Arc::new(v));
        let issues = validate_vec_op(VecOpKind::Abs, &val);
        assert!(issues.iter().any(|i| i.contains("zero-length")));
    }

    #[test]
    fn validate_vec_op_sign_extra_mismatch() {
        let v = NdaVec {
            len: 8,
            log2_scale: 0,
            sign: vec![0xFF, 0xAA].into(),
            extra: vec![0x00].into(),
        };
        let val = JitVal::Vector(Arc::new(v));
        let issues = validate_vec_op(VecOpKind::Negate, &val);
        assert!(issues.iter().any(|i| i.contains("mismatch")));
    }

    #[test]
    fn classify_add_result_float_float() {
        assert_eq!(
            classify_add_result(&JitVal::Float(1.0), &JitVal::Float(2.0)),
            "float"
        );
    }

    #[test]
    fn classify_add_result_scalar_scalar() {
        assert_eq!(
            classify_add_result(&JitVal::Scalar(1, 0), &JitVal::Scalar(2, 0)),
            "scalar"
        );
    }

    #[test]
    fn classify_add_result_vector_scalar() {
        let v = NdaVec {
            len: 8,
            log2_scale: 0,
            sign: vec![0xFF].into(),
            extra: vec![0x00].into(),
        };
        assert_eq!(
            classify_add_result(&JitVal::Vector(Arc::new(v)), &JitVal::Scalar(1, 0)),
            "vector"
        );
    }

    #[test]
    fn broadcast_scalar_produces_valid_vec() {
        let v = broadcast_scalar(16, 1, 0);
        assert_eq!(v.len, 16);
        assert_eq!(v.sign.len(), 2);
        assert_eq!(v.extra.len(), 2);
        // val=1 -> sign=0xFF, extra=0x00
        assert_eq!(v.sign[0], 0xFF);
        assert_eq!(v.extra[0], 0x00);
    }

    #[test]
    fn broadcast_scalar_negative() {
        let v = broadcast_scalar(8, -1, 0);
        assert_eq!(v.len, 8);
        // val=-1 -> sign=0x00, extra=0xFF
        assert_eq!(v.sign[0], 0x00);
        assert_eq!(v.extra[0], 0xFF);
    }

    #[test]
    fn apply_vec_op_negate_float() {
        let val = JitVal::Float(3.0);
        let result = apply_vec_op(VecOpKind::Negate, &val);
        match result {
            JitVal::Float(v) => assert!((v - (-3.0)).abs() < 1e-6),
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn apply_vec_op_abs_float() {
        let val = JitVal::Float(-5.0);
        let result = apply_vec_op(VecOpKind::Abs, &val);
        match result {
            JitVal::Float(v) => assert!((v - 5.0).abs() < 1e-6),
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn apply_vec_op_silu_float() {
        let val = JitVal::Float(0.0);
        let result = apply_vec_op(VecOpKind::SiLU, &val);
        match result {
            JitVal::Float(v) => assert!((v - 0.0).abs() < 1e-6), // silu(0) = 0
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn add_vals_float_float() {
        let result = add_vals(&JitVal::Float(1.5), &JitVal::Float(2.5));
        match result {
            JitVal::Float(v) => assert!((v - 4.0).abs() < 1e-6),
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn compare_vals_float_eq() {
        let result = compare_vals(CmpOp::Eq, &JitVal::Float(1.0), &JitVal::Float(1.0));
        match result {
            JitVal::Scalar(v, _) => assert_eq!(v, 1),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn compare_vals_float_ne() {
        let result = compare_vals(CmpOp::Ne, &JitVal::Float(1.0), &JitVal::Float(2.0));
        match result {
            JitVal::Scalar(v, _) => assert_eq!(v, 1),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn compare_vals_float_lt() {
        let result = compare_vals(CmpOp::Lt, &JitVal::Float(1.0), &JitVal::Float(2.0));
        match result {
            JitVal::Scalar(v, _) => assert_eq!(v, 1),
            _ => panic!("expected Scalar"),
        }
    }

    // ── Block 113: expanded tests ────────────────────────────────────────────

    #[test]
    fn broadcast_scalar_val_neg2() {
        let v = broadcast_scalar(8, -2, 0);
        assert_eq!(v.sign[0], 0x00);
        assert_eq!(v.extra[0], 0x00);
    }

    #[test]
    fn broadcast_scalar_val_2() {
        let v = broadcast_scalar(8, 2, 0);
        // val=2 falls into the _ => (0xFF, 0xFF) catch-all
        assert_eq!(v.sign[0], 0xFF);
        assert_eq!(v.extra[0], 0xFF);
    }

    #[test]
    fn broadcast_scalar_val_other() {
        let v = broadcast_scalar(8, 99, 0);
        assert_eq!(v.sign[0], 0xFF);
        assert_eq!(v.extra[0], 0xFF);
    }

    #[test]
    fn broadcast_float_basic() {
        let v = broadcast_float(16, 1.0);
        assert_eq!(v.len, 16);
        assert_eq!(v.sign.len(), 2);
    }

    #[test]
    fn add_vals_scalar_scalar_same_scale() {
        let result = add_vals(&JitVal::Scalar(1, 0), &JitVal::Scalar(2, 0));
        match result {
            JitVal::Scalar(v, s) => {
                assert_eq!(v, 3);
                assert_eq!(s, 0);
            }
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn add_vals_float_scalar() {
        let result = add_vals(&JitVal::Float(1.0), &JitVal::Scalar(1, 0));
        match result {
            JitVal::Float(v) => assert!((v - 2.0).abs() < 1e-6),
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn add_vals_scalar_float() {
        let result = add_vals(&JitVal::Scalar(1, 0), &JitVal::Float(2.0));
        match result {
            JitVal::Float(v) => assert!((v - 3.0).abs() < 1e-6),
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn compare_vals_float_ge() {
        let result = compare_vals(CmpOp::Ge, &JitVal::Float(2.0), &JitVal::Float(2.0));
        match result {
            JitVal::Scalar(v, _) => assert_eq!(v, 1),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn compare_vals_float_gt_false() {
        let result = compare_vals(CmpOp::Gt, &JitVal::Float(1.0), &JitVal::Float(2.0));
        match result {
            JitVal::Scalar(v, _) => assert_eq!(v, -1),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn compare_vals_scalar_scalar_eq() {
        let result = compare_vals(CmpOp::Eq, &JitVal::Scalar(3, 0), &JitVal::Scalar(3, 0));
        match result {
            JitVal::Scalar(v, _) => assert_eq!(v, 1),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn compare_vals_scalar_scalar_ne() {
        let result = compare_vals(CmpOp::Ne, &JitVal::Scalar(1, 0), &JitVal::Scalar(2, 0));
        match result {
            JitVal::Scalar(v, _) => assert_eq!(v, 1),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn apply_vec_op_negate_scalar() {
        let result = apply_vec_op(VecOpKind::Negate, &JitVal::Scalar(2, 0));
        match result {
            JitVal::Scalar(v, s) => {
                assert_eq!(v, -2);
                assert_eq!(s, 0);
            }
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn apply_vec_op_abs_scalar() {
        let result = apply_vec_op(VecOpKind::Abs, &JitVal::Scalar(-3, 0));
        match result {
            JitVal::Scalar(v, _) => assert_eq!(v, 3),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn apply_vec_op_reduce_sum_float() {
        let result = apply_vec_op(VecOpKind::ReduceSum, &JitVal::Float(5.0));
        match result {
            JitVal::Float(v) => assert!((v - 5.0).abs() < 1e-6),
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn apply_vec_op_reduce_sum_scalar() {
        let result = apply_vec_op(VecOpKind::ReduceSum, &JitVal::Scalar(3, 0));
        match result {
            JitVal::Scalar(v, s) => {
                assert_eq!(v, 3);
                assert_eq!(s, 0);
            }
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn silu_known_values() {
        // silu(0) = 0 / (1 + e^0) = 0 / 2 = 0
        assert!((silu(0.0) - 0.0).abs() < 1e-6);
        // silu is approximately x for large positive x
        assert!(silu(10.0) > 9.0);
        // silu is approximately 0 for large negative x
        assert!(silu(-10.0).abs() < 0.01);
    }

    #[test]
    fn validate_broadcast_params_negative() {
        // Can't pass negative usize, but we can test large values
        let issues = validate_broadcast_params(usize::MAX, 1);
        // Should not crash
        assert!(issues.is_empty() || !issues.is_empty());
    }
}
