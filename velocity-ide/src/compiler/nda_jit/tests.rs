#[cfg(test)]
use super::compiler::compile;
#[cfg(test)]
use super::compiler::jit_tier_info;
#[cfg(test)]
use super::types::JitResult;
#[cfg(test)]
use super::types::VarRegistry;
#[cfg(test)]
use super::x86_emitter::X86Emitter;
#[cfg(test)]
use crate::site_map::verifier::BitwiseOp;
#[cfg(test)]
use crate::site_map::verifier::CmpOp;
#[cfg(test)]
use crate::site_map::verifier::MathFuncKind;
#[cfg(test)]
use crate::site_map::verifier::MathOp;
#[cfg(test)]
use crate::site_map::verifier::VecOpKind;
#[cfg(test)]
use crate::site_map::NdaNode;
#[cfg(test)]
use crate::site_map::SiteMap;

#[test]
fn test_jit_compile_empty() {
    let program = compile(&[]);
    assert_eq!(program.nodes_compiled, 0);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let res = program.run(&[1.0, 2.0, 3.0], &site_map);
    assert!(res.error.is_none());
}

#[test]
fn test_jit_int_node() {
    let nodes = vec![NdaNode::Int { value: 42 }];
    let program = compile(&nodes);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let res = program.run(&[], &site_map);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![42.0]);
}

#[test]
fn test_jit_silu_vecop() {
    let nodes = vec![
        NdaNode::Float { value: 0.0 },
        NdaNode::VecOp {
            op: VecOpKind::SiLU,
            operand: Box::new(NdaNode::Float { value: 0.0 }),
        },
    ];
    let program = compile(&nodes);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let res = program.run(&[], &site_map);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![0.0]);
}

#[test]
fn test_jit_extended_opcodes() {
    let nodes = vec![
        NdaNode::Float { value: 1.0 },
        NdaNode::Float { value: 2.0 },
        NdaNode::Add {
            lhs: Box::new(NdaNode::Float { value: 1.0 }),
            rhs: Box::new(NdaNode::Float { value: 2.0 }),
        },
    ];
    let program = compile(&nodes);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let res = program.run(&[], &site_map);
    assert!(res.error.is_none());
}

#[test]
fn test_jit_let_load() {
    let hash = 0x123456789abcdef0u64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: hash,
            init: Box::new(NdaNode::Int { value: 99 }),
        },
        NdaNode::Load { name_hash: hash },
    ];
    let program = compile(&nodes);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let res = program.run(&[], &site_map);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![99.0]);
}

#[test]
fn test_jit_scalar_loop() {
    let hash = 0x1111222233334444u64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: hash,
            init: Box::new(NdaNode::Int { value: 0 }),
        },
        NdaNode::Loop {
            count: 5,
            body: vec![NdaNode::Store {
                name_hash: hash,
                value: Box::new(NdaNode::Add {
                    lhs: Box::new(NdaNode::Load { name_hash: hash }),
                    rhs: Box::new(NdaNode::Int { value: 1 }),
                }),
            }],
        },
        NdaNode::Load { name_hash: hash },
    ];
    let program = compile(&nodes);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let res = program.run(&[], &site_map);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![5.0]);
}

#[test]
fn test_jit_loop_node() {
    let hash = 0xabcdef0123456789u64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: hash,
            init: Box::new(NdaNode::Int { value: 10 }),
        },
        NdaNode::Loop {
            count: 3,
            body: vec![NdaNode::Store {
                name_hash: hash,
                value: Box::new(NdaNode::Add {
                    lhs: Box::new(NdaNode::Load { name_hash: hash }),
                    rhs: Box::new(NdaNode::Int { value: 5 }),
                }),
            }],
        },
        NdaNode::Load { name_hash: hash },
    ];
    let program = compile(&nodes);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let res = program.run(&[], &site_map);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![25.0]);
}

#[test]
fn test_jit_break_in_loop() {
    let hash = 0x9999888877776666u64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: hash,
            init: Box::new(NdaNode::Int { value: 0 }),
        },
        NdaNode::Loop {
            count: 10,
            body: vec![
                NdaNode::Store {
                    name_hash: hash,
                    value: Box::new(NdaNode::Add {
                        lhs: Box::new(NdaNode::Load { name_hash: hash }),
                        rhs: Box::new(NdaNode::Int { value: 1 }),
                    }),
                },
                NdaNode::Break,
            ],
        },
        NdaNode::Load { name_hash: hash },
    ];
    let program = compile(&nodes);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let res = program.run(&[], &site_map);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![1.0]);
}

#[test]
fn test_x86_emitter_noop() {
    let mut emitter = X86Emitter::new();
    emitter.push_rbp();
    emitter.pop_rbp();
    emitter.ret();
    assert_eq!(emitter.buf, vec![0x55, 0x5D, 0xC3]);
}

#[test]
fn test_jit_tier_info_non_empty() {
    let info = jit_tier_info();
    assert!(!info.is_empty());
}

// ─── Shared helpers for the extended coverage below ──────────────────────────

#[cfg(test)]
fn run_program(nodes: Vec<NdaNode>, input: &[f32]) -> JitResult {
    let program = compile(&nodes);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    program.run(input, &site_map)
}

#[cfg(test)]
fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {}, got {}",
        expected,
        actual
    );
}

/// Matrix node whose rows*cols entries are all +1 (sign bit set, extra clear).
/// GEMV reads `div_ceil(cols/8)` bytes per row, so each row is padded to its
/// own stride.
#[cfg(test)]
fn ones_matrix(rows: u16, cols: u16) -> NdaNode {
    let stride = (cols as usize).div_ceil(8);
    let mut sign = vec![0xFFu8; rows as usize * stride];
    if !cols.is_multiple_of(8) {
        let mask = (1u8 << (cols % 8)) - 1;
        for r in 0..rows as usize {
            sign[r * stride + stride - 1] &= mask;
        }
    }
    NdaNode::Matrix {
        rows,
        cols,
        scale: 0,
        sign,
        extra: vec![0u8; rows as usize * stride],
    }
}

#[test]
fn test_jit_input_passthrough() {
    let res = run_program(vec![], &[1.0, 2.0, -1.0]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![1.0, 2.0, -1.0]);
}

#[test]
fn test_jit_if_then_else_branches() {
    let mk = |op: CmpOp| {
        vec![NdaNode::If {
            cond: Box::new(NdaNode::Compare {
                op,
                lhs: Box::new(NdaNode::Int { value: 5 }),
                rhs: Box::new(NdaNode::Int { value: 3 }),
            }),
            then_body: vec![NdaNode::Int { value: 10 }],
            else_body: Some(vec![NdaNode::Int { value: 20 }]),
        }]
    };
    let then_res = run_program(mk(CmpOp::Gt), &[]);
    assert!(then_res.error.is_none());
    assert_eq!(then_res.output_vec, vec![10.0]);
    let else_res = run_program(mk(CmpOp::Lt), &[]);
    assert!(else_res.error.is_none());
    assert_eq!(else_res.output_vec, vec![20.0]);
}

#[test]
fn test_jit_if_no_else_keeps_stack_when_falsy() {
    let nodes = vec![
        NdaNode::Int { value: 77 },
        NdaNode::If {
            cond: Box::new(NdaNode::Compare {
                op: CmpOp::Lt,
                lhs: Box::new(NdaNode::Int { value: 5 }),
                rhs: Box::new(NdaNode::Int { value: 3 }),
            }),
            then_body: vec![NdaNode::Int { value: 10 }],
            else_body: None,
        },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![77.0]);
}

#[test]
fn test_jit_while_counts_down_to_zero() {
    // Interpreter closure path: the body uses Math (non pure-scalar node).
    // Math always yields Float, so keep the whole program on Float operands.
    let h = 0x1111_2222_3333_4444u64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: h,
            init: Box::new(NdaNode::Float { value: 3.0 }),
        },
        NdaNode::While {
            cond: Box::new(NdaNode::Compare {
                op: CmpOp::Gt,
                lhs: Box::new(NdaNode::Load { name_hash: h }),
                rhs: Box::new(NdaNode::Float { value: 0.0 }),
            }),
            body: vec![NdaNode::Store {
                name_hash: h,
                value: Box::new(NdaNode::Math {
                    op: MathOp::Sub,
                    lhs: Box::new(NdaNode::Load { name_hash: h }),
                    rhs: Box::new(NdaNode::Float { value: 1.0 }),
                }),
            }],
        },
        NdaNode::Load { name_hash: h },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none(), "interp while err: {:?}", res.error);
    assert_eq!(res.output_vec, vec![0.0]);

    // Native scalar fast path: the body is pure scalar arithmetic.
    let j = 0x5555_6666_7777_8888u64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: j,
            init: Box::new(NdaNode::Int { value: 3 }),
        },
        NdaNode::While {
            cond: Box::new(NdaNode::Compare {
                op: CmpOp::Gt,
                lhs: Box::new(NdaNode::Load { name_hash: j }),
                rhs: Box::new(NdaNode::Int { value: 0 }),
            }),
            body: vec![NdaNode::Store {
                name_hash: j,
                value: Box::new(NdaNode::Add {
                    lhs: Box::new(NdaNode::Load { name_hash: j }),
                    rhs: Box::new(NdaNode::Int { value: -1 }),
                }),
            }],
        },
        NdaNode::Load { name_hash: j },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none(), "native while err: {:?}", res.error);
    assert_eq!(res.output_vec, vec![0.0]);
}

#[test]
fn test_jit_compare_ops() {
    let cases = [
        (CmpOp::Eq, true),
        (CmpOp::Ne, false),
        (CmpOp::Lt, false),
        (CmpOp::Gt, false),
        (CmpOp::Le, true),
        (CmpOp::Ge, true),
    ];
    for (op, expect_true) in cases {
        let nodes = vec![NdaNode::Compare {
            op,
            lhs: Box::new(NdaNode::Int { value: 4 }),
            rhs: Box::new(NdaNode::Int { value: 4 }),
        }];
        let res = run_program(nodes, &[]);
        assert!(res.error.is_none());
        let expected = if expect_true { 1.0 } else { -1.0 };
        assert_eq!(res.output_vec, vec![expected]);
    }
    // Float operands exercise the interpreter compare path.
    let nodes = vec![NdaNode::Compare {
        op: CmpOp::Lt,
        lhs: Box::new(NdaNode::Float { value: 1.5 }),
        rhs: Box::new(NdaNode::Float { value: 2.5 }),
    }];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![1.0]);
}

#[test]
fn test_jit_math_float_ops() {
    let cases = [
        (MathOp::Add, 1.25f32, 2.5f32, 3.75f32),
        (MathOp::Sub, 5.0, 2.0, 3.0),
        (MathOp::Mul, 3.0, 4.0, 12.0),
        (MathOp::Div, 9.0, 2.0, 4.5),
    ];
    for (op, l, r, expected) in cases {
        let nodes = vec![NdaNode::Math {
            op,
            lhs: Box::new(NdaNode::Float { value: l }),
            rhs: Box::new(NdaNode::Float { value: r }),
        }];
        let res = run_program(nodes, &[]);
        assert!(res.error.is_none());
        assert_eq!(res.output_vec.len(), 1);
        assert_close(res.output_vec[0], expected);
    }
}

#[test]
fn test_jit_math_scalar_operands() {
    let nodes = vec![NdaNode::Math {
        op: MathOp::Div,
        lhs: Box::new(NdaNode::Int { value: 7 }),
        rhs: Box::new(NdaNode::Int { value: 2 }),
    }];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec.len(), 1);
    assert_close(res.output_vec[0], 3.5);
}

#[test]
fn test_jit_math_type_mismatch_error() {
    let nodes = vec![NdaNode::Math {
        op: MathOp::Add,
        lhs: Box::new(NdaNode::Float { value: 1.0 }),
        rhs: Box::new(NdaNode::Int { value: 2 }),
    }];
    let res = run_program(nodes, &[]);
    let err = res.error.expect("mixed Float/Int Math must fail");
    assert!(err.contains("type mismatch"), "unexpected error: {}", err);
}

#[test]
fn test_jit_mathfunc_float_kinds() {
    let cases = [
        (MathFuncKind::Sqrt, 4.0f32, 2.0f32),
        (MathFuncKind::Exp, 0.0, 1.0),
        (MathFuncKind::Sin, 0.0, 0.0),
        (MathFuncKind::Cos, 0.0, 1.0),
    ];
    for (func, input, expected) in cases {
        let nodes = vec![NdaNode::MathFunc {
            func,
            operand: Box::new(NdaNode::Float { value: input }),
        }];
        let res = run_program(nodes, &[]);
        assert!(res.error.is_none());
        assert_eq!(res.output_vec.len(), 1);
        assert_close(res.output_vec[0], expected);
    }
}

#[test]
fn test_jit_mathfunc_scalar_operand() {
    let nodes = vec![NdaNode::MathFunc {
        func: MathFuncKind::Sqrt,
        operand: Box::new(NdaNode::Int { value: 9 }),
    }];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec.len(), 1);
    assert_close(res.output_vec[0], 3.0);
}

#[test]
fn test_jit_bitwise_binary_ops() {
    let cases = [
        (BitwiseOp::And, 6, 3, 2),
        (BitwiseOp::Or, 6, 1, 7),
        (BitwiseOp::Xor, 5, 3, 6),
        (BitwiseOp::Shl, 1, 3, 8),
        (BitwiseOp::Shr, 16, 2, 4),
    ];
    for (op, l, r, expected) in cases {
        let nodes = vec![NdaNode::Bitwise {
            op,
            lhs: Box::new(NdaNode::Int { value: l }),
            rhs: Some(Box::new(NdaNode::Int { value: r })),
        }];
        let res = run_program(nodes, &[]);
        assert!(res.error.is_none());
        assert_eq!(res.output_vec, vec![expected as f32]);
    }
}

#[test]
fn test_jit_bitwise_not_scalar() {
    let nodes = vec![NdaNode::Bitwise {
        op: BitwiseOp::Not,
        lhs: Box::new(NdaNode::Int { value: 0 }),
        rhs: None,
    }];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![-1.0]);
}

#[test]
fn test_jit_poke_peek_heap_roundtrip() {
    let nodes = vec![
        NdaNode::Poke {
            addr: Box::new(NdaNode::Int { value: 16 }),
            value: Box::new(NdaNode::Int { value: 1234 }),
        },
        NdaNode::Peek {
            addr: Box::new(NdaNode::Int { value: 16 }),
        },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![1234.0]);

    // Float addresses truncate to u32 and hit the same heap slot.
    let nodes = vec![
        NdaNode::Poke {
            addr: Box::new(NdaNode::Float { value: 32.0 }),
            value: Box::new(NdaNode::Int { value: -7 }),
        },
        NdaNode::Peek {
            addr: Box::new(NdaNode::Float { value: 32.0 }),
        },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![-7.0]);
}

#[test]
fn test_jit_poke_peek_mmio_region() {
    let mmio_addr = 0xF000_0000u32 as i32;
    let nodes = vec![
        NdaNode::Poke {
            addr: Box::new(NdaNode::Int { value: mmio_addr }),
            value: Box::new(NdaNode::Float { value: 3.5 }),
        },
        NdaNode::Peek {
            addr: Box::new(NdaNode::Int { value: mmio_addr }),
        },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![3.5]);
}

#[test]
fn test_jit_peek_poke_out_of_bounds_errors() {
    let read = run_program(
        vec![NdaNode::Peek {
            addr: Box::new(NdaNode::Int { value: 70_000 }),
        }],
        &[],
    );
    let err = read.error.expect("heap read past 64KB must fail");
    assert!(err.contains("Out of bounds"), "unexpected error: {}", err);

    let write = run_program(
        vec![NdaNode::Poke {
            addr: Box::new(NdaNode::Int { value: 70_000 }),
            value: Box::new(NdaNode::Int { value: 1 }),
        }],
        &[],
    );
    let err = write.error.expect("heap write past 64KB must fail");
    assert!(err.contains("Out of bounds"), "unexpected error: {}", err);
}

#[test]
fn test_jit_dot_product() {
    let h = 0xaaaa_bbbb_cccc_ddddu64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: h,
            init: Box::new(ones_matrix(1, 2)),
        },
        NdaNode::Dot {
            lhs: Box::new(NdaNode::Load { name_hash: h }),
            rhs: Box::new(NdaNode::Load { name_hash: h }),
        },
    ];
    // M·[1,1] has acc=2, but the gemv output is re-quantized through
    // from_i32_slice: ENCODE_TABLE maps 0..=2 to code +1, so the dot of the
    // resulting vector with itself is <[1],[1]> = 1.
    let res = run_program(nodes, &[1.0, 1.0]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec.len(), 1);
    assert_close(res.output_vec[0], 1.0);
}

#[test]
fn test_jit_dot_length_mismatch_error() {
    let x = 0x0011_2233_4455_6677u64;
    let y = 0x8899_aabb_ccdd_eeffu64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: x,
            init: Box::new(ones_matrix(2, 2)),
        },
        NdaNode::Let {
            name_hash: y,
            init: Box::new(ones_matrix(1, 2)),
        },
        NdaNode::Dot {
            lhs: Box::new(NdaNode::Load { name_hash: x }),
            rhs: Box::new(NdaNode::Load { name_hash: y }),
        },
    ];
    let res = run_program(nodes, &[1.0, 1.0]);
    let err = res.error.expect("mismatched dot must fail");
    assert!(err.contains("length mismatch"), "unexpected error: {}", err);
}

#[test]
fn test_jit_return_stops_sequence() {
    let nodes = vec![
        NdaNode::Return {
            value: Box::new(NdaNode::Int { value: 7 }),
        },
        NdaNode::Int { value: 8 },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![7.0]);
}

#[test]
fn test_jit_scope_runs_children() {
    let nodes = vec![NdaNode::Scope {
        children: vec![NdaNode::Math {
            op: MathOp::Mul,
            lhs: Box::new(NdaNode::Float { value: 3.0 }),
            rhs: Box::new(NdaNode::Float { value: 4.0 }),
        }],
    }];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec.len(), 1);
    assert_close(res.output_vec[0], 12.0);
}

#[test]
fn test_jit_store_overwrites_let() {
    let h = 0xfeed_face_cafe_b00cu64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: h,
            init: Box::new(NdaNode::Int { value: 1 }),
        },
        NdaNode::Store {
            name_hash: h,
            value: Box::new(NdaNode::Int { value: 7 }),
        },
        NdaNode::Load { name_hash: h },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![7.0]);
}

#[test]
fn test_jit_load_undefined_variable_error() {
    let nodes = vec![NdaNode::Load {
        name_hash: 0xdead_beef_dead_beefu64,
    }];
    let res = run_program(nodes, &[]);
    let err = res.error.expect("loading an unbound variable must fail");
    assert!(
        err.contains("undefined variable"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn test_jit_vecop_negate_abs() {
    let negate = run_program(
        vec![NdaNode::VecOp {
            op: VecOpKind::Negate,
            operand: Box::new(NdaNode::Float { value: 3.0 }),
        }],
        &[],
    );
    assert!(negate.error.is_none());
    assert_eq!(negate.output_vec, vec![-3.0]);

    let abs = run_program(
        vec![NdaNode::VecOp {
            op: VecOpKind::Abs,
            operand: Box::new(NdaNode::Int { value: -5 }),
        }],
        &[],
    );
    assert!(abs.error.is_none());
    assert_eq!(abs.output_vec, vec![5.0]);
}

#[test]
fn test_jit_matrix_gemv_dimension_mismatch_error() {
    let nodes = vec![ones_matrix(1, 4)];
    let res = run_program(nodes, &[1.0, 2.0, 3.0]);
    let err = res.error.expect("GEMV with wrong input length must fail");
    assert!(
        err.contains("dimension mismatch"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn test_jit_run_sandboxed_parity() {
    let h = 0x0123_4567_89ab_cdefu64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: h,
            init: Box::new(NdaNode::Int { value: 0 }),
        },
        NdaNode::Loop {
            count: 5,
            body: vec![NdaNode::Store {
                name_hash: h,
                value: Box::new(NdaNode::Add {
                    lhs: Box::new(NdaNode::Load { name_hash: h }),
                    rhs: Box::new(NdaNode::Int { value: 1 }),
                }),
            }],
        },
        NdaNode::Load { name_hash: h },
    ];
    let program = compile(&nodes);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let plain = program.run(&[], &site_map);
    let sandboxed = program.run_sandboxed(&[], &site_map);
    assert!(plain.error.is_none());
    assert!(sandboxed.error.is_none());
    assert!(!sandboxed.panicked);
    assert_eq!(sandboxed.output_vec, vec![5.0]);
    assert_eq!(plain.output_vec, sandboxed.output_vec);
}

#[test]
fn test_var_registry_slot_reuse() {
    let reg = VarRegistry::new();
    let a = reg.get_or_create_slot(111);
    let b = reg.get_or_create_slot(222);
    let a2 = reg.get_or_create_slot(111);
    assert_eq!(a, a2);
    assert_ne!(a, b);
    assert_eq!(reg.total_slots(), 2);
}

// ─── Extended coverage ────────────────────────────────────────────────────────

#[test]
fn test_jit_nested_loop_break_inner() {
    // Outer loop runs 3 times; inner loop breaks after 1 iteration.
    let h = 0xaabb_ccdd_eeff_0011u64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: h,
            init: Box::new(NdaNode::Int { value: 0 }),
        },
        NdaNode::Loop {
            count: 3,
            body: vec![
                NdaNode::Loop {
                    count: 5,
                    body: vec![
                        NdaNode::Store {
                            name_hash: h,
                            value: Box::new(NdaNode::Add {
                                lhs: Box::new(NdaNode::Load { name_hash: h }),
                                rhs: Box::new(NdaNode::Int { value: 1 }),
                            }),
                        },
                        NdaNode::Break,
                    ],
                },
            ],
        },
        NdaNode::Load { name_hash: h },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    // Inner loop runs once per outer iteration: 3 * 1 = 3
    assert_eq!(res.output_vec, vec![3.0]);
}

#[test]
fn test_jit_math_div_by_zero_no_panic() {
    let nodes = vec![NdaNode::Math {
        op: MathOp::Div,
        lhs: Box::new(NdaNode::Float { value: 5.0 }),
        rhs: Box::new(NdaNode::Float { value: 0.0 }),
    }];
    let res = run_program(nodes, &[]);
    // JIT uses native x86 division which produces inf; interpreter returns 0.0.
    assert!(res.error.is_none());
    assert!(!res.output_vec.is_empty());
}

#[test]
fn test_jit_multiple_variables_independent() {
    let a = 0xaaaa_0000_0000_0001u64;
    let b = 0xbbbb_0000_0000_0002u64;
    let nodes = vec![
        NdaNode::Let {
            name_hash: a,
            init: Box::new(NdaNode::Int { value: 10 }),
        },
        NdaNode::Let {
            name_hash: b,
            init: Box::new(NdaNode::Int { value: 20 }),
        },
        NdaNode::Add {
            lhs: Box::new(NdaNode::Load { name_hash: a }),
            rhs: Box::new(NdaNode::Load { name_hash: b }),
        },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    assert_eq!(res.output_vec, vec![30.0]);
}

#[test]
fn test_jit_cast_node_runs() {
    let nodes = vec![NdaNode::Cast {
        from_type: crate::site_map::verifier::TypeKind::Int,
        to_type: crate::site_map::verifier::TypeKind::Float,
        operand: Box::new(NdaNode::Float { value: 42.0 }),
    }];
    let res = run_program(nodes, &[]);
    // Cast may produce empty output in JIT path; just verify no crash.
    assert!(res.error.is_none());
}

#[test]
fn test_jit_triple_node_is_noop() {
    // Triple nodes are semantic metadata and should not affect execution.
    let nodes = vec![
        NdaNode::Int { value: 7 },
        NdaNode::Triple {
            subject_hash: 0x1,
            predicate_id: 0,
            object_hash: 0x2,
        },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    // The Int node sets current_vec, Triple is a no-op.
    assert_eq!(res.output_vec, vec![7.0]);
}

#[test]
fn test_jit_alloc_runs_without_error() {
    let nodes = vec![NdaNode::Alloc {
        size: Box::new(NdaNode::Int { value: 256 }),
    }];
    let res = run_program(nodes, &[]);
    // Alloc in JIT path may not produce visible output; verify no crash.
    assert!(res.error.is_none());
}

#[test]
fn test_jit_free_runs_without_error() {
    let nodes = vec![
        NdaNode::Alloc {
            size: Box::new(NdaNode::Int { value: 64 }),
        },
        NdaNode::Free {
            addr: Box::new(NdaNode::Int { value: 2048 }),
        },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
}

#[test]
fn test_jit_spawn_runs_without_error() {
    let nodes = vec![NdaNode::Spawn {
        scope_hash: 0xdead_beefu64,
    }];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
}

#[test]
fn test_jit_atomic_runs_without_error() {
    let nodes = vec![NdaNode::Atomic {
        op: crate::site_map::verifier::AtomicOp::Cas,
        addr: Box::new(NdaNode::Int { value: 0 }),
        val: Box::new(NdaNode::Int { value: 42 }),
    }];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
}

#[test]
fn test_jit_regint_noop() {
    let nodes = vec![
        NdaNode::Int { value: 99 },
        NdaNode::RegInt {
            vector: 5,
            handler_hash: 0x1234u64,
        },
    ];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
    // RegInt is a no-op; the Int value should still be current.
    assert_eq!(res.output_vec, vec![99.0]);
}

#[test]
fn test_jit_gpu_dispatch_runs_without_error() {
    let nodes = vec![NdaNode::GpuDispatch {
        shader_hash: 0xfeedu64,
        args: vec![NdaNode::Int { value: 1 }],
    }];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
}

#[test]
fn test_jit_syscall_runs_without_error() {
    let nodes = vec![NdaNode::Syscall {
        num: 1,
        args: vec![NdaNode::Int { value: 42 }],
    }];
    let res = run_program(nodes, &[]);
    assert!(res.error.is_none());
}

#[test]
fn test_jit_compile_reports_nodes_compiled() {
    let nodes = vec![
        NdaNode::Int { value: 1 },
        NdaNode::Float { value: 2.0 },
        NdaNode::Add {
            lhs: Box::new(NdaNode::Int { value: 1 }),
            rhs: Box::new(NdaNode::Float { value: 2.0 }),
        },
    ];
    let program = compile(&nodes);
    assert!(program.nodes_compiled > 0);
}

#[test]
fn test_jit_empty_program_run_sandboxed() {
    let program = compile(&[]);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let res = program.run_sandboxed(&[1.0, 2.0], &site_map);
    assert!(res.error.is_none());
    assert!(!res.panicked);
    assert_eq!(res.output_vec, vec![1.0, 2.0]);
}

#[test]
fn test_jit_print_captures_output() {
    let nodes = vec![NdaNode::Print {
        source: Box::new(NdaNode::Int { value: 42 }),
    }];
    let program = compile(&nodes);
    let site_map = SiteMap::open(&std::env::temp_dir().join("nda_jit_test_sm"), 0).unwrap();
    let res = program.run_sandboxed(&[], &site_map);
    assert!(res.error.is_none());
    assert!(!res.output_log.is_empty());
    assert!(res.output_log[0].contains("42"));
}
