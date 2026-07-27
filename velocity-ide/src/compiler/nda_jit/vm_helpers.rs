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
