#[cfg(test)]
use super::compiler::compile;
#[cfg(test)]
use super::compiler::jit_tier_info;
#[cfg(test)]
use super::x86_emitter::X86Emitter;
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
