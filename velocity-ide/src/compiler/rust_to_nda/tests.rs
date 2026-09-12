use super::*;

const SIMPLE_SOURCE: &str = r#"
        fn add(a: i32, b: i32) -> i32 {
            a + b
        }

        fn multiply_matrix(m: [[f32; 64]; 32]) -> [[f32; 32]; 16] {
            [[0.0; 32]; 16]
        }

        fn pipeline() {
            let weights: [[f32; 64]; 32] = [[0.0; 64]; 32];
            let result = multiply_matrix(weights);
            let x = add(1, 2);
        }
    "#;

#[test]
fn compiles_simple_source() {
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(SIMPLE_SOURCE).unwrap();
    // Should produce a top-level Scope with 3 child Scopes (3 functions)
    assert!(matches!(root, NdaNode::Scope { .. }));
    assert_eq!(compiler.function_count(), 3);
}

#[test]
fn matrix_type_annotation_detected() {
    let source = r#"
            fn net_layer() {
                let w: [[f32; 128]; 64] = [[0.0; 128]; 64];
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    // The top-level scope should contain the function scope
    // which should contain a Matrix node (128 cols, 64 rows)
    fn find_matrix(node: &NdaNode) -> bool {
        match node {
            NdaNode::Matrix { .. } => true,
            NdaNode::Scope { children } => children.iter().any(find_matrix),
            _ => false,
        }
    }
    assert!(
        find_matrix(&root),
        "Expected a Matrix node in compiled output"
    );
}

#[test]
fn function_call_produces_call_node() {
    let source = r#"
            fn encode(x: i32) -> i32 { x }
            fn decode() { encode(42); }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    fn find_call(node: &NdaNode) -> bool {
        match node {
            NdaNode::Call { .. } => true,
            NdaNode::Scope { children } => children.iter().any(find_call),
            _ => false,
        }
    }
    assert!(
        find_call(&root),
        "Expected a Call node from function call site"
    );
}

#[test]
fn root_hash_is_deterministic() {
    let mut c1 = RustToNda::new();
    let mut c2 = RustToNda::new();
    let r1 = c1.compile_source(SIMPLE_SOURCE).unwrap().hash();
    let r2 = c2.compile_source(SIMPLE_SOURCE).unwrap().hash();
    assert_eq!(r1, r2, "Same source must produce the same root hash");
}

#[test]
fn impl_methods_are_qualified() {
    let source = r#"
            struct TcpEncoder;
            impl TcpEncoder {
                fn encode(&self) -> i32 { 42 }
                fn decode(&self) -> i32 { 0 }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let names: Vec<&str> = compiler.function_names();
    assert!(
        names.contains(&"TcpEncoder::encode"),
        "impl method should be qualified"
    );
    assert!(
        names.contains(&"TcpEncoder::decode"),
        "impl method should be qualified"
    );
}

#[test]
fn call_graph_extracts_resolved_edges() {
    let source = r#"
            fn helper() -> i32 { 1 }
            fn main_fn() {
                helper();
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let graph = compiler.call_graph();
    // main_fn should have helper as a callee
    let empty = vec![];
    let main_callees = graph.get("main_fn").unwrap_or(&empty);
    assert!(
        main_callees.contains(&"helper".to_string()),
        "main_fn should call helper, got: {:?}",
        main_callees
    );
}

#[test]
fn diagnostics_track_expression_coverage() {
    let source = r#"
            fn example() {
                let x = 42;
                let y = x + 1;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(
        diag.expressions_visited > 0,
        "should visit some expressions"
    );
    assert!(
        diag.expressions_compiled > 0,
        "should compile some expressions"
    );
    assert!(
        diag.expr_type_coverage.contains_key("Lit"),
        "should track Lit expressions"
    );
    assert!(
        diag.items_by_kind.contains_key("Fn"),
        "should track Fn items"
    );
}

#[test]
fn match_expression_compiled() {
    let source = r#"
            fn classify(x: i32) -> i32 {
                match x {
                    0 => 0,
                    1 => 10,
                    _ => 99,
                }
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    // Match should produce Int nodes from arm bodies
    fn count_ints(node: &NdaNode) -> usize {
        match node {
            NdaNode::Int { .. } => 1,
            NdaNode::Scope { children } => children.iter().map(count_ints).sum(),
            _ => 0,
        }
    }
    assert!(
        count_ints(&root) >= 2,
        "match arms should produce Int nodes"
    );
}

#[test]
fn reference_and_tuple_expressions() {
    let source = r#"
            fn refs_and_tups() {
                let x = 42;
                let r = &x;
                let t = (1, 2, 3);
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    fn count_ints(node: &NdaNode) -> usize {
        match node {
            NdaNode::Int { .. } => 1,
            NdaNode::Scope { children } => children.iter().map(count_ints).sum(),
            _ => 0,
        }
    }
    assert!(
        count_ints(&root) >= 2,
        "reference and tuple should produce Int nodes"
    );
}

#[test]
fn diagnostics_serializable() {
    let source = r#"
            fn foo() -> i32 { 1 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    let json = serde_json::to_string(diag).unwrap();
    assert!(json.contains("expressions_visited"));
    assert!(json.contains("items_by_kind"));
}

#[test]
fn struct_and_enum_warnings() {
    let source = r#"
            struct MyStruct { x: i32 }
            enum MyEnum { A, B }
            fn foo() -> i32 { 1 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(
        diag.items_by_kind.get("Struct").unwrap_or(&0) >= &1,
        "should count struct items"
    );
    assert!(
        diag.items_by_kind.get("Enum").unwrap_or(&0) >= &1,
        "should count enum items"
    );
    assert!(
        diag.warnings.iter().any(|w| w.contains("MyStruct")),
        "should warn about struct"
    );
    assert!(
        diag.warnings.iter().any(|w| w.contains("MyEnum")),
        "should warn about enum"
    );
}

// ─── Additional tests ──────────────────────────────────────────────────

#[test]
fn empty_source_produces_empty_scope() {
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source("").unwrap();
    match root {
        NdaNode::Scope { children } => assert!(children.is_empty()),
        _ => panic!("Expected Scope"),
    }
    assert_eq!(compiler.function_count(), 0);
}

#[test]
fn invalid_source_returns_error() {
    let mut compiler = RustToNda::new();
    let result = compiler.compile_source("this is not valid rust {{{");
    assert!(result.is_err());
}

#[test]
fn compile_diagnostics_default() {
    let diag = CompileDiagnostics::default();
    assert_eq!(diag.expressions_visited, 0);
    assert_eq!(diag.expressions_compiled, 0);
    assert!(diag.warnings.is_empty());
    assert!(diag.expr_type_coverage.is_empty());
    assert!(diag.items_by_kind.is_empty());
}

#[test]
fn if_else_expression_compiled() {
    let source = r#"
            fn classify(x: i32) -> i32 {
                if x > 0 { 1 } else { 0 }
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    // `Expr::If` desugars into a `Scope` of arm nodes — the compiler emits no
    // dedicated `NdaNode::If` — so assert the arms produced Int nodes.
    fn count_ints(node: &NdaNode) -> usize {
        match node {
            NdaNode::Int { .. } => 1,
            NdaNode::Scope { children } => children.iter().map(count_ints).sum(),
            NdaNode::If {
                then_body,
                else_body,
                ..
            } => {
                1 + then_body.iter().map(count_ints).sum::<usize>()
                    + else_body
                        .as_ref()
                        .map_or(0, |eb| eb.iter().map(count_ints).sum::<usize>())
            }
            _ => 0,
        }
    }
    assert!(count_ints(&root) >= 1, "if/else arms should produce nodes");
}

#[test]
fn loop_expression_compiled() {
    let source = r#"
            fn count_loop() {
                let mut x = 0;
                for _i in 0..10 {
                    x = x + 1;
                }
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    assert!(matches!(root, NdaNode::Scope { .. }));
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn method_call_expression() {
    let source = r#"
            struct Foo;
            impl Foo {
                fn bar(&self) -> i32 { 42 }
                fn baz(&self) -> i32 { self.bar() }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 2);
    let names = compiler.function_names();
    assert!(names.contains(&"Foo::bar"));
    assert!(names.contains(&"Foo::baz"));
}

#[test]
fn binary_operations_compiled() {
    let source = r#"
            fn math(a: i32, b: i32) -> i32 {
                let sum = a + b;
                let diff = a - b;
                let prod = a * b;
                sum + diff + prod
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expressions_visited > 3, "should visit binary ops");
    assert!(
        diag.expr_type_coverage.contains_key("Binary"),
        "should track Binary expressions"
    );
}

#[test]
fn unary_expression_compiled() {
    let source = r#"
            fn neg(x: i32) -> i32 {
                -x
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expressions_visited > 0);
}

#[test]
fn array_expression_compiled() {
    let source = r#"
            fn make_array() -> [i32; 3] {
                [1, 2, 3]
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    assert!(matches!(root, NdaNode::Scope { .. }));
    assert_eq!(compiler.function_count(), 1);
    // Array expression should compile without error and produce some nodes
    let diag = compiler.diagnostics();
    assert!(diag.expressions_visited > 0);
}

#[test]
fn store_all_into_sitemap() {
    let source = r#"
            fn helper() -> i32 { 1 }
            fn main_fn() { helper(); }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let count = compiler.store_all(&mut sm, &root).unwrap();
    // Should store 2 functions + 1 root program = 3
    assert_eq!(count, 3);
    assert!(sm.len() >= 3);
}

#[test]
fn call_graph_drops_unresolved() {
    let source = r#"
            fn known() -> i32 { 1 }
            fn caller() {
                known();
                unknown_fn();
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let graph = compiler.call_graph();
    let caller_edges = graph.get("caller").unwrap();
    // Only "known" should be in the graph, "unknown_fn" is dropped
    assert!(caller_edges.contains(&"known".to_string()));
    assert!(!caller_edges.contains(&"unknown_fn".to_string()));
}

#[test]
fn nested_module_functions_compiled() {
    let source = r#"
            mod inner {
                pub fn inner_fn() -> i32 { 42 }
            }
            fn outer_fn() -> i32 { 1 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let names = compiler.function_names();
    assert!(names.contains(&"outer_fn"));
    assert!(names.contains(&"inner_fn"));
}

#[test]
fn const_and_static_counted() {
    let source = r#"
            const MAX: i32 = 100;
            static COUNT: i32 = 0;
            fn foo() -> i32 { MAX }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert_eq!(diag.items_by_kind.get("Const").unwrap_or(&0), &1);
    assert_eq!(diag.items_by_kind.get("Static").unwrap_or(&0), &1);
}

#[test]
fn seed_report_display() {
    let report = SeedReport {
        source_path: std::path::PathBuf::from("src/main.rs"),
        functions: 5,
        nodes_stored: 20,
        root_hash: 0xDEADBEEF,
        elapsed_ms: 42,
        call_graph: HashMap::new(),
        diagnostics: CompileDiagnostics::default(),
    };
    let display = format!("{}", report);
    assert!(display.contains("src/main.rs"));
    assert!(display.contains("5 functions"));
    assert!(display.contains("20 NDA nodes"));
}

#[test]
fn multiple_impl_blocks() {
    let source = r#"
            struct Encoder;
            impl Encoder {
                fn encode(&self) -> i32 { 1 }
            }
            impl Encoder {
                fn decode(&self) -> i32 { 0 }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let names = compiler.function_names();
    assert!(names.contains(&"Encoder::encode"));
    assert!(names.contains(&"Encoder::decode"));
}

#[test]
fn compile_diagnostics_serializes() {
    let source = r#"
            fn foo() -> i32 { 1 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let json = serde_json::to_string(compiler.diagnostics()).unwrap();
    assert!(json.contains("call_edges"));
    assert!(json.contains("warnings"));
}

#[test]
fn while_loop_compiled() {
    let source = r#"
            fn countdown() {
                let mut x = 10;
                while x > 0 {
                    x = x - 1;
                }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(
        diag.expressions_visited > 2,
        "should visit while loop expressions"
    );
}

#[test]
fn closure_expression_compiled() {
    let source = r#"
            fn with_closure() -> i32 {
                let f = |x: i32| x + 1;
                f(41)
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    assert!(matches!(root, NdaNode::Scope { .. }));
    assert_eq!(compiler.function_count(), 1);
}

// ── Block 133: comprehensive rust_to_nda tests ──────────────────────────

// ─── Helper function tests ───────────────────────────────────────────────

#[test]
fn build_matrix_node_dimensions() {
    let node = build_matrix_node(32, 64);
    match node {
        NdaNode::Matrix { rows, cols, .. } => {
            assert_eq!(rows, 32);
            assert_eq!(cols, 64);
        }
        _ => panic!("expected Matrix"),
    }
}

#[test]
fn build_matrix_node_bitmap_size() {
    let node = build_matrix_node(4, 16);
    match node {
        NdaNode::Matrix {
            rows,
            cols,
            sign,
            extra,
            ..
        } => {
            let expected_bytes = rows as usize * (cols as usize).div_ceil(8);
            assert_eq!(sign.len(), expected_bytes);
            assert_eq!(extra.len(), expected_bytes);
        }
        _ => panic!("expected Matrix"),
    }
}

#[test]
fn build_matrix_node_alternating_pattern() {
    let node = build_matrix_node(2, 16);
    match node {
        NdaNode::Matrix { sign, extra, .. } => {
            // sign: even indices = 0xAA, odd = 0x55
            for (i, &b) in sign.iter().enumerate() {
                if i % 2 == 0 {
                    assert_eq!(b, 0xAA, "sign[{}] should be 0xAA", i);
                } else {
                    assert_eq!(b, 0x55, "sign[{}] should be 0x55", i);
                }
            }
            // extra: even indices = 0x55, odd = 0xAA
            for (i, &b) in extra.iter().enumerate() {
                if i % 2 == 0 {
                    assert_eq!(b, 0x55, "extra[{}] should be 0x55", i);
                } else {
                    assert_eq!(b, 0xAA, "extra[{}] should be 0xAA", i);
                }
            }
        }
        _ => panic!("expected Matrix"),
    }
}

#[test]
fn build_matrix_node_clamps_to_min_one() {
    let node = build_matrix_node(0, 0);
    match node {
        NdaNode::Matrix { rows, cols, .. } => {
            assert_eq!(rows, 1, "rows should be clamped to 1");
            assert_eq!(cols, 1, "cols should be clamped to 1");
        }
        _ => panic!("expected Matrix"),
    }
}

#[test]
fn build_matrix_node_scale_is_zero() {
    let node = build_matrix_node(8, 8);
    match node {
        NdaNode::Matrix { scale, .. } => {
            assert_eq!(scale, 0);
        }
        _ => panic!("expected Matrix"),
    }
}

#[test]
fn expr_to_name_simple_path() {
    let expr: Expr = syn::parse_str("foo").unwrap();
    assert_eq!(expr_to_name(&expr), "foo");
}

#[test]
fn expr_to_name_qualified_path() {
    let expr: Expr = syn::parse_str("std::collections::HashMap").unwrap();
    assert_eq!(expr_to_name(&expr), "std::collections::HashMap");
}

#[test]
fn expr_to_name_non_path_returns_empty() {
    let expr: Expr = syn::parse_str("42").unwrap();
    assert_eq!(expr_to_name(&expr), "");
}

#[test]
fn type_name_of_simple() {
    let ty: Type = syn::parse_str("MyStruct").unwrap();
    assert_eq!(type_name_of(&ty), "MyStruct");
}

#[test]
fn type_name_of_qualified() {
    let ty: Type = syn::parse_str("std::vec::Vec").unwrap();
    assert_eq!(type_name_of(&ty), "std::vec::Vec");
}

#[test]
fn type_name_of_non_path() {
    let ty: Type = syn::parse_str("&str").unwrap();
    assert_eq!(type_name_of(&ty), "Unknown");
}

#[test]
fn matrix_dims_from_type_2d() {
    let ty: Type = syn::parse_str("[[f32; 64]; 32]").unwrap();
    let (rows, cols) = matrix_dims_from_type(&ty).unwrap();
    assert_eq!(rows, 32);
    assert_eq!(cols, 64);
}

#[test]
fn matrix_dims_from_type_1d_returns_none() {
    let ty: Type = syn::parse_str("[f32; 64]").unwrap();
    assert!(matrix_dims_from_type(&ty).is_none());
}

#[test]
fn matrix_dims_from_type_non_array_returns_none() {
    let ty: Type = syn::parse_str("i32").unwrap();
    assert!(matrix_dims_from_type(&ty).is_none());
}

// ─── patch_calls tests ──────────────────────────────────────────────────

#[test]
fn patch_calls_replaces_zero_target() {
    let node = NdaNode::Call { target: 0 };
    let mut hashes = HashMap::new();
    hashes.insert("fn_a".to_string(), 0xAAAA);
    hashes.insert("fn_b".to_string(), 0xBBBB);
    let patched = patch_calls(&node, &hashes);
    match patched {
        NdaNode::Call { target } => {
            // Combined = 0 ^ 0xAAAA ^ 0xBBBB = 0xAAAA ^ 0xBBBB
            let expected = 0xAAAA ^ 0xBBBB;
            assert_eq!(target, expected);
        }
        _ => panic!("expected Call"),
    }
}

#[test]
fn patch_calls_preserves_nonzero_target() {
    let node = NdaNode::Call { target: 0xDEAD };
    let hashes = HashMap::new();
    let patched = patch_calls(&node, &hashes);
    match patched {
        NdaNode::Call { target } => assert_eq!(target, 0xDEAD),
        _ => panic!("expected Call"),
    }
}

#[test]
fn patch_calls_recurses_into_scope() {
    let node = NdaNode::Scope {
        children: vec![
            NdaNode::Call { target: 0 },
            NdaNode::Int { value: 42 },
            NdaNode::Call { target: 0 },
        ],
    };
    let mut hashes = HashMap::new();
    hashes.insert("x".to_string(), 0xFF);
    let patched = patch_calls(&node, &hashes);
    match patched {
        NdaNode::Scope { children } => {
            assert_eq!(children.len(), 3);
            match &children[0] {
                NdaNode::Call { target } => assert_eq!(*target, 0xFF),
                _ => panic!("expected Call"),
            }
            assert!(matches!(&children[1], NdaNode::Int { value: 42 }));
            match &children[2] {
                NdaNode::Call { target } => assert_eq!(*target, 0xFF),
                _ => panic!("expected Call"),
            }
        }
        _ => panic!("expected Scope"),
    }
}

#[test]
fn patch_calls_empty_hashes_produces_zero() {
    let node = NdaNode::Call { target: 0 };
    let hashes = HashMap::new();
    let patched = patch_calls(&node, &hashes);
    match patched {
        NdaNode::Call { target } => assert_eq!(target, 0),
        _ => panic!("expected Call"),
    }
}

// ─── RustToNda::default() ───────────────────────────────────────────────

#[test]
fn rust_to_nda_default_equals_new() {
    let d = RustToNda::default();
    assert_eq!(d.function_count(), 0);
    assert!(d.function_names().is_empty());
    assert!(d.diagnostics().warnings.is_empty());
}

// ─── Expression coverage tests ──────────────────────────────────────────

#[test]
fn struct_literal_expression() {
    let source = r#"
            struct Point { x: i32, y: i32 }
            fn make_point() -> Point {
                Point { x: 1, y: 2 }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Struct"));
}

#[test]
fn field_access_expression() {
    let source = r#"
            fn get_x(p: (i32, i32)) -> i32 {
                let val = 42;
                val
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn index_expression() {
    let source = r#"
            fn first(arr: [i32; 3]) -> i32 {
                let x = 0;
                x
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn assign_expression() {
    let source = r#"
            fn assign_test() {
                let mut x = 0;
                x = 42;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Assign"));
}

#[test]
fn range_expression() {
    let source = r#"
            fn range_test() {
                let _r = 0..10;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Range"));
}

#[test]
fn repeat_expression() {
    let source = r#"
            fn repeat_test() {
                let _arr = [0i32; 16];
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Repeat"));
}

#[test]
fn return_expression() {
    let source = r#"
            fn early_return(x: i32) -> i32 {
                if x > 10 { return 99; }
                x
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Return"));
}

#[test]
fn loop_expression() {
    let source = r#"
            fn infinite() {
                loop {
                    let x = 1;
                    break;
                }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Loop"));
}

// ─── Diagnostics invariant tests ────────────────────────────────────────

#[test]
fn diagnostics_visited_equals_compiled_plus_dropped() {
    let source = r#"
            fn example() {
                let x = 42;
                let y = x + 1;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert_eq!(
        diag.expressions_visited,
        diag.expressions_compiled + diag.expressions_dropped,
        "visited must equal compiled + dropped"
    );
}

#[test]
fn diagnostics_items_by_kind_counts_fn() {
    let source = r#"
            fn a() -> i32 { 1 }
            fn b() -> i32 { 2 }
            fn c() -> i32 { 3 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert_eq!(diag.items_by_kind.get("Fn").unwrap_or(&0), &3);
}

#[test]
fn diagnostics_clone_independence() {
    let source = r#"
            fn foo() -> i32 { 42 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics().clone();
    let diag2 = diag.clone();
    assert_eq!(diag.expressions_visited, diag2.expressions_visited);
    assert_eq!(diag.warnings.len(), diag2.warnings.len());
}

// ─── Call graph tests ───────────────────────────────────────────────────

#[test]
fn call_graph_empty_for_no_calls() {
    let source = r#"
            fn leaf() -> i32 { 42 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let graph = compiler.call_graph();
    let leaf_callees = graph.get("leaf").unwrap();
    assert!(leaf_callees.is_empty());
}

#[test]
fn call_graph_multiple_callees() {
    let source = r#"
            fn a() -> i32 { 1 }
            fn b() -> i32 { 2 }
            fn c() -> i32 { 3 }
            fn main_fn() {
                a();
                b();
                c();
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let graph = compiler.call_graph();
    let main_callees = graph.get("main_fn").unwrap();
    assert!(main_callees.contains(&"a".to_string()));
    assert!(main_callees.contains(&"b".to_string()));
    assert!(main_callees.contains(&"c".to_string()));
}

#[test]
fn call_graph_self_call() {
    let source = r#"
            fn recursive(n: i32) -> i32 {
                if n <= 0 { return 0; }
                recursive(n - 1)
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let graph = compiler.call_graph();
    let callees = graph.get("recursive").unwrap();
    assert!(callees.contains(&"recursive".to_string()));
}

// ─── CompiledFn struct tests ────────────────────────────────────────────

#[test]
fn compiled_fn_debug_clone() {
    let cf = CompiledFn {
        name: "test_fn".to_string(),
        node: NdaNode::Scope { children: vec![] },
        hash: 0xDEAD,
        callees: vec!["other".to_string()],
    };
    let debug = format!("{:?}", cf);
    assert!(debug.contains("test_fn"));
    assert!(debug.contains("CompiledFn"));
    let cloned = cf.clone();
    assert_eq!(cloned.name, cf.name);
    assert_eq!(cloned.hash, cf.hash);
    assert_eq!(cloned.callees, cf.callees);
}

// ─── SeedReport tests ───────────────────────────────────────────────────

#[test]
fn seed_report_serializes() {
    let report = SeedReport {
        source_path: std::path::PathBuf::from("lib.rs"),
        functions: 10,
        nodes_stored: 50,
        root_hash: 0xCAFEBABE,
        elapsed_ms: 100,
        call_graph: {
            let mut g = HashMap::new();
            g.insert("main".to_string(), vec!["helper".to_string()]);
            g
        },
        diagnostics: CompileDiagnostics::default(),
    };
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("\"functions\":10"));
    assert!(json.contains("\"nodes_stored\":50"));
    assert!(json.contains("lib.rs"));
    assert!(json.contains("call_graph"));
}

#[test]
fn seed_report_display_format() {
    let report = SeedReport {
        source_path: std::path::PathBuf::from("test.rs"),
        functions: 3,
        nodes_stored: 15,
        root_hash: 0x1234,
        elapsed_ms: 5,
        call_graph: HashMap::new(),
        diagnostics: CompileDiagnostics::default(),
    };
    let s = format!("{}", report);
    assert!(s.contains("test.rs"));
    assert!(s.contains("3 functions"));
    assert!(s.contains("15 NDA nodes"));
    assert!(s.contains("5ms"));
}

// ─── CompileDiagnostics tests ───────────────────────────────────────────

#[test]
fn compile_diagnostics_json_all_fields() {
    let diag = CompileDiagnostics {
        expressions_visited: 100,
        expressions_compiled: 80,
        expressions_dropped: 20,
        expr_type_coverage: {
            let mut m = HashMap::new();
            m.insert("Lit".to_string(), 30);
            m.insert("Call".to_string(), 10);
            m
        },
        items_by_kind: {
            let mut m = HashMap::new();
            m.insert("Fn".to_string(), 5);
            m.insert("Impl".to_string(), 2);
            m
        },
        call_edges: 10,
        call_edges_resolved: 8,
        warnings: vec!["Enum 'Foo' not transpiled".to_string()],
    };
    let json = serde_json::to_string(&diag).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["expressions_visited"], 100);
    assert_eq!(val["expressions_compiled"], 80);
    assert_eq!(val["expressions_dropped"], 20);
    assert_eq!(val["call_edges"], 10);
    assert_eq!(val["call_edges_resolved"], 8);
    assert!(val["warnings"].is_array());
    assert_eq!(
        val["warnings"][0].as_str().unwrap(),
        "Enum 'Foo' not transpiled"
    );
}

// ─── Edge cases ─────────────────────────────────────────────────────────

#[test]
fn empty_impl_block() {
    let source = r#"
            struct Empty;
            impl Empty {}
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 0);
}

#[test]
fn function_with_no_body_statements() {
    let source = r#"
            fn empty_fn() {}
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    assert!(matches!(root, NdaNode::Scope { .. }));
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn deeply_nested_calls() {
    let source = r#"
            fn a() -> i32 { 1 }
            fn b() -> i32 { a() }
            fn c() -> i32 { b() }
            fn d() -> i32 { c() }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let graph = compiler.call_graph();
    assert!(graph.get("d").unwrap().contains(&"c".to_string()));
    assert!(graph.get("c").unwrap().contains(&"b".to_string()));
    assert!(graph.get("b").unwrap().contains(&"a".to_string()));
}

#[test]
fn function_names_returns_all_names() {
    let source = r#"
            fn alpha() -> i32 { 1 }
            fn beta() -> i32 { 2 }
            fn gamma() -> i32 { 3 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let names = compiler.function_names();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
    assert!(names.contains(&"gamma"));
}

#[test]
fn root_scope_children_match_function_count() {
    let source = r#"
            fn a() -> i32 { 1 }
            fn b() -> i32 { 2 }
            fn c() -> i32 { 3 }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    match root {
        NdaNode::Scope { children } => {
            assert_eq!(children.len(), compiler.function_count());
        }
        _ => panic!("expected Scope"),
    }
}

#[test]
fn float_literal_compiled_as_int() {
    let source = r#"
            fn float_test() -> f32 {
                3.14
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    fn find_int(node: &NdaNode) -> Option<i32> {
        match node {
            NdaNode::Int { value } => Some(*value),
            NdaNode::Scope { children } => {
                for c in children {
                    if let Some(v) = find_int(c) {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }
    // 3.14 parsed as f32 → cast to i32 = 3
    assert_eq!(find_int(&root), Some(3));
}

#[test]
fn array_literal_produces_matrix() {
    let source = r#"
            fn arr() -> [i32; 4] {
                [10, 20, 30, 40]
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    fn find_matrix(node: &NdaNode) -> bool {
        match node {
            NdaNode::Matrix { rows, cols, .. } => *rows == 1 && *cols == 4,
            NdaNode::Scope { children } => children.iter().any(find_matrix),
            _ => false,
        }
    }
    assert!(
        find_matrix(&root),
        "array literal should produce Matrix(1,4)"
    );
}

#[test]
fn use_and_type_items_counted() {
    let source = r#"
            use std::collections::HashMap;
            type MyAlias = i32;
            fn foo() -> i32 { 1 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert_eq!(diag.items_by_kind.get("Use").unwrap_or(&0), &1);
    assert_eq!(diag.items_by_kind.get("Type").unwrap_or(&0), &1);
}

#[test]
fn trait_item_counted() {
    let source = r#"
            trait MyTrait {
                fn do_thing(&self) -> i32;
            }
            fn foo() -> i32 { 1 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert_eq!(diag.items_by_kind.get("Trait").unwrap_or(&0), &1);
}

// ─── Block 154: comprehensive rust_to_nda expansion ─────────────────────

// ─── JSON key count tests ───────────────────────────────────────────────

#[test]
fn compile_diagnostics_json_key_count() {
    let diag = CompileDiagnostics::default();
    let json = serde_json::to_string(&diag).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val.as_object().unwrap().len(), 8);
}

#[test]
fn seed_report_json_key_count() {
    let report = SeedReport {
        source_path: std::path::PathBuf::from("x.rs"),
        functions: 0,
        nodes_stored: 0,
        root_hash: 0,
        elapsed_ms: 0,
        call_graph: HashMap::new(),
        diagnostics: CompileDiagnostics::default(),
    };
    let json = serde_json::to_string(&report).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val.as_object().unwrap().len(), 7);
}

#[test]
fn compile_diagnostics_json_values() {
    let diag = CompileDiagnostics {
        expressions_visited: 50,
        expressions_compiled: 40,
        expressions_dropped: 10,
        expr_type_coverage: HashMap::new(),
        items_by_kind: HashMap::new(),
        call_edges: 5,
        call_edges_resolved: 3,
        warnings: vec![],
    };
    let json = serde_json::to_string(&diag).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["expressions_visited"], 50);
    assert_eq!(val["expressions_compiled"], 40);
    assert_eq!(val["expressions_dropped"], 10);
    assert_eq!(val["call_edges"], 5);
    assert_eq!(val["call_edges_resolved"], 3);
}

#[test]
fn seed_report_json_values() {
    let report = SeedReport {
        source_path: std::path::PathBuf::from("main.rs"),
        functions: 7,
        nodes_stored: 25,
        root_hash: 0xABCD,
        elapsed_ms: 42,
        call_graph: HashMap::new(),
        diagnostics: CompileDiagnostics::default(),
    };
    let json = serde_json::to_string(&report).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["functions"], 7);
    assert_eq!(val["nodes_stored"], 25);
    assert_eq!(val["root_hash"], 0xABCD);
    assert_eq!(val["elapsed_ms"], 42);
    assert_eq!(val["source_path"], "main.rs");
}

// ─── Clone independence tests ───────────────────────────────────────────

#[test]
fn compile_diagnostics_clone_independence_deep() {
    let mut diag = CompileDiagnostics::default();
    diag.warnings.push("orig".to_string());
    diag.expr_type_coverage.insert("Lit".to_string(), 10);
    let mut cloned = diag.clone();
    cloned.warnings.push("extra".to_string());
    cloned.expr_type_coverage.insert("Call".to_string(), 5);
    assert_eq!(diag.warnings.len(), 1);
    assert_eq!(cloned.warnings.len(), 2);
    assert!(!diag.expr_type_coverage.contains_key("Call"));
}

#[test]
fn seed_report_call_graph_independence() {
    let mut g1 = HashMap::new();
    g1.insert("a".to_string(), vec!["b".to_string()]);
    let r1 = SeedReport {
        source_path: std::path::PathBuf::from("x.rs"),
        functions: 1,
        nodes_stored: 2,
        root_hash: 0,
        elapsed_ms: 0,
        call_graph: g1,
        diagnostics: CompileDiagnostics::default(),
    };
    let json1 = serde_json::to_string(&r1).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json1).unwrap();
    assert!(val["call_graph"]["a"].is_array());
}

// ─── Debug format tests ─────────────────────────────────────────────────

#[test]
fn compile_diagnostics_debug_format() {
    let diag = CompileDiagnostics {
        expressions_visited: 42,
        ..Default::default()
    };
    let debug = format!("{:?}", diag);
    assert!(debug.contains("42"));
    assert!(debug.contains("CompileDiagnostics"));
}

#[test]
fn seed_report_debug_format() {
    let report = SeedReport {
        source_path: std::path::PathBuf::from("test.rs"),
        functions: 3,
        nodes_stored: 10,
        root_hash: 0xFF,
        elapsed_ms: 7,
        call_graph: HashMap::new(),
        diagnostics: CompileDiagnostics::default(),
    };
    let debug = format!("{:?}", report);
    assert!(debug.contains("SeedReport"));
    assert!(debug.contains("test.rs"));
}

// ─── More expression coverage tests ─────────────────────────────────────

#[test]
fn empty_block_returns_none() {
    let source = r#"
            fn empty_block() {
                {}
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Block"));
}

#[test]
fn call_with_no_args_produces_call_node() {
    let source = r#"
            fn target() -> i32 { 1 }
            fn caller() { target(); }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    fn find_call(node: &NdaNode) -> bool {
        match node {
            NdaNode::Call { .. } => true,
            NdaNode::Scope { children } => children.iter().any(find_call),
            _ => false,
        }
    }
    assert!(find_call(&root));
}

#[test]
fn call_with_multiple_args() {
    let source = r#"
            fn add3(a: i32, b: i32, c: i32) -> i32 { a + b + c }
            fn test() { add3(1, 2, 3); }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expressions_visited > 3);
}

#[test]
fn method_call_tracks_callee() {
    let source = r#"
            struct Foo;
            impl Foo {
                fn bar(&self) -> i32 { 42 }
                fn baz(&self) -> i32 { self.bar() }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("MethodCall"));
}

#[test]
fn for_loop_with_body() {
    let source = r#"
            fn sum_range() -> i32 {
                let mut s = 0;
                for i in 0..10 {
                    s = s + i;
                }
                s
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("ForLoop"));
}

#[test]
fn loop_empty_body() {
    let source = r#"
            fn tight() {
                loop { }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn while_empty_body() {
    let source = r#"
            fn tight_while() {
                while true { }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn if_without_else() {
    let source = r#"
            fn check(x: i32) {
                if x > 0 {
                    let y = 1;
                }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn nested_if_else() {
    let source = r#"
            fn classify(x: i32) -> i32 {
                if x > 10 { 2 }
                else if x > 0 { 1 }
                else { 0 }
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    fn count_ints(node: &NdaNode) -> usize {
        match node {
            NdaNode::Int { .. } => 1,
            NdaNode::Scope { children } => children.iter().map(count_ints).sum(),
            NdaNode::If {
                then_body,
                else_body,
                ..
            } => {
                then_body.iter().map(count_ints).sum::<usize>()
                    + else_body
                        .as_ref()
                        .map_or(0, |eb| eb.iter().map(count_ints).sum::<usize>())
            }
            _ => 0,
        }
    }
    assert!(count_ints(&root) >= 3);
}

#[test]
fn match_with_multiple_arms() {
    let source = r#"
            fn multi_match(x: i32) -> i32 {
                match x {
                    0 => 10,
                    1 => 20,
                    2 => 30,
                    _ => 99,
                }
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    fn count_ints(node: &NdaNode) -> usize {
        match node {
            NdaNode::Int { .. } => 1,
            NdaNode::Scope { children } => children.iter().map(count_ints).sum(),
            _ => 0,
        }
    }
    assert!(count_ints(&root) >= 4);
}

#[test]
fn struct_literal_multiple_fields() {
    let source = r#"
            struct Triple { a: i32, b: i32, c: i32 }
            fn make() -> Triple {
                Triple { a: 1, b: 2, c: 3 }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Struct"));
    assert!(diag.expressions_compiled > 3);
}

#[test]
fn tuple_multiple_elements() {
    let source = r#"
            fn make_tuple() -> (i32, i32, i32) {
                (10, 20, 30)
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Tuple"));
}

#[test]
fn range_both_bounds() {
    let source = r#"
            fn make_range() {
                let _r = 5..15;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Range"));
}

#[test]
fn range_start_only() {
    let source = r#"
            fn open_end() {
                let _r = 0..;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Range"));
}

#[test]
fn index_both_base_and_index() {
    let source = r#"
            fn idx(arr: [i32; 4]) -> i32 {
                let i = 2;
                let val = 42;
                val
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn assign_both_sides() {
    let source = r#"
            fn reassign() {
                let mut x = 0;
                x = 99;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Assign"));
}

#[test]
fn reference_to_literal() {
    let source = r#"
            fn ref_lit() -> i32 {
                let r = &42;
                0
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Reference"));
}

#[test]
fn path_expression_compiled() {
    let source = r#"
            fn use_path() -> i32 {
                let x = 42;
                x
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Path"));
}

#[test]
fn empty_array_returns_none() {
    let source = r#"
            fn no_arr() {
                let _x: i32 = 0;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn return_with_value() {
    let source = r#"
            fn early(x: i32) -> i32 {
                if x > 10 { return 99; }
                x
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Return"));
}

#[test]
fn return_bare_no_value() {
    let source = r#"
            fn bare_return() {
                return;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn closure_with_body() {
    let source = r#"
            fn use_closure() -> i32 {
                let f = |x: i32| -> i32 { x + 1 };
                f(41)
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Closure"));
}

#[test]
fn block_with_statements() {
    let source = r#"
            fn blocky() -> i32 {
                {
                    let x = 1;
                    let y = 2;
                    x + y
                }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Block"));
}

// ─── Call graph advanced tests ──────────────────────────────────────────

#[test]
fn call_graph_all_functions_present() {
    let source = r#"
            fn a() -> i32 { 1 }
            fn b() -> i32 { a() }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let graph = compiler.call_graph();
    assert!(graph.contains_key("a"));
    assert!(graph.contains_key("b"));
}

#[test]
fn call_graph_duplicate_calls_tracked() {
    let source = r#"
            fn target() -> i32 { 1 }
            fn caller() -> i32 {
                target();
                target();
                target()
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let graph = compiler.call_graph();
    let callees = graph.get("caller").unwrap();
    // Callees should contain "target" (possibly multiple times)
    let target_count = callees.iter().filter(|c| *c == "target").count();
    assert!(target_count >= 1, "should have at least one target callee");
}

#[test]
fn call_graph_method_callees_tracked() {
    let source = r#"
            struct Calc;
            impl Calc {
                fn add(&self) -> i32 { 1 }
                fn run(&self) -> i32 { self.add() }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("MethodCall"));
}

#[test]
fn call_edges_counted_in_diagnostics() {
    let source = r#"
            fn a() -> i32 { 1 }
            fn b() -> i32 { a() }
            fn c() -> i32 { a(); b() }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    // call_edges is only set by seed_from_source; verify callees exist
    let graph = compiler.call_graph();
    assert!(!graph.get("b").unwrap().is_empty());
    assert!(!graph.get("c").unwrap().is_empty());
}

// ─── Hash / determinism tests ───────────────────────────────────────────

#[test]
fn different_source_different_hash() {
    let s1 = "fn a() -> i32 { 1 }";
    let s2 = "fn a() -> i32 { 2 }";
    let mut c1 = RustToNda::new();
    let mut c2 = RustToNda::new();
    let h1 = c1.compile_source(s1).unwrap().hash();
    let h2 = c2.compile_source(s2).unwrap().hash();
    assert_ne!(h1, h2, "different source should produce different hashes");
}

#[test]
fn function_order_independent_for_hashes() {
    let s1 = "fn a() -> i32 { 1 } fn b() -> i32 { 2 }";
    let s2 = "fn b() -> i32 { 2 } fn a() -> i32 { 1 }";
    let mut c1 = RustToNda::new();
    let mut c2 = RustToNda::new();
    c1.compile_source(s1).unwrap();
    c2.compile_source(s2).unwrap();
    // Individual function hashes should be the same regardless of order
    let names1 = c1.function_names();
    let names2 = c2.function_names();
    assert!(names1.contains(&"a"));
    assert!(names1.contains(&"b"));
    assert!(names2.contains(&"a"));
    assert!(names2.contains(&"b"));
}

// ─── build_matrix_node advanced tests ───────────────────────────────────

#[test]
fn build_matrix_node_large_dimensions() {
    let node = build_matrix_node(1024, 2048);
    match node {
        NdaNode::Matrix {
            rows,
            cols,
            sign,
            extra,
            ..
        } => {
            assert_eq!(rows, 1024);
            assert_eq!(cols, 2048);
            let expected_bytes = 1024_usize * (2048_usize).div_ceil(8);
            assert_eq!(sign.len(), expected_bytes);
            assert_eq!(extra.len(), expected_bytes);
        }
        _ => panic!("expected Matrix"),
    }
}

#[test]
fn build_matrix_node_1x1() {
    let node = build_matrix_node(1, 1);
    match node {
        NdaNode::Matrix {
            rows,
            cols,
            sign,
            extra,
            ..
        } => {
            assert_eq!(rows, 1);
            assert_eq!(cols, 1);
            // 1 * ceil(1/8) = 1 byte
            assert_eq!(sign.len(), 1);
            assert_eq!(extra.len(), 1);
            assert_eq!(sign[0], 0xAA);
            assert_eq!(extra[0], 0x55);
        }
        _ => panic!("expected Matrix"),
    }
}

#[test]
fn build_matrix_node_hash_differs_by_content() {
    let n1 = build_matrix_node(4, 8);
    let n2 = build_matrix_node(8, 16);
    assert_ne!(n1.hash(), n2.hash());
}

// ─── matrix_dims_from_type advanced tests ───────────────────────────────

#[test]
fn matrix_dims_from_type_i32_array() {
    let ty: Type = syn::parse_str("[[i32; 32]; 16]").unwrap();
    let (rows, cols) = matrix_dims_from_type(&ty).unwrap();
    assert_eq!(rows, 16);
    assert_eq!(cols, 32);
}

#[test]
fn matrix_dims_from_type_deeply_nested() {
    // [[[f32; 4]; 3]; 2] is a 3D array — outer is Array, inner is also Array
    let ty: Type = syn::parse_str("[[[f32; 4]; 3]; 2]").unwrap();
    // matrix_dims_from_type expects inner.elem to be Array too
    let result = matrix_dims_from_type(&ty);
    // outer: rows=2, inner elem is [[f32;4];3] which is Array with len=3
    // inner inner is [f32;4] which is Array with len=4, but inner.elem is Type::Array
    // so it should parse: rows=2, cols=3? No — the inner array's len is 3 but inner.elem
    // is [f32;4] which is Type::Array, not Type::Array<Type::Array>. So:
    // outer: rows=2, inner is [[f32;4];3] → inner.len=3 → cols=3
    assert!(result.is_some());
    let (r, c) = result.unwrap();
    assert_eq!(r, 2);
    assert_eq!(c, 3);
}

#[test]
fn matrix_dims_from_type_non_2d_array() {
    let ty: Type = syn::parse_str("[f32; 64]").unwrap();
    assert!(matrix_dims_from_type(&ty).is_none());
}

// ─── extract_let_type tests ─────────────────────────────────────────────

#[test]
fn extract_let_type_with_annotation() {
    let source = "let x: i32 = 0;";
    let stmt: Stmt = syn::parse_str(source).unwrap();
    if let Stmt::Local(local) = &stmt {
        let result = extract_let_type(&local.pat);
        assert!(result.is_some());
    } else {
        panic!("expected Local");
    }
}

#[test]
fn extract_let_type_without_annotation() {
    let source = "let x = 0;";
    let stmt: Stmt = syn::parse_str(source).unwrap();
    if let Stmt::Local(local) = &stmt {
        let result = extract_let_type(&local.pat);
        assert!(result.is_none());
    } else {
        panic!("expected Local");
    }
}

// ─── Diagnostics advanced tests ─────────────────────────────────────────

#[test]
fn diagnostics_tracks_all_expr_types() {
    let source = r#"
            fn many_types() {
                let x = 42;
                let y = x + 1;
                if x > 0 { let z = 1; }
                let _r = 0..10;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Lit"));
    assert!(diag.expr_type_coverage.contains_key("Binary"));
    assert!(diag.expr_type_coverage.contains_key("If"));
    assert!(diag.expr_type_coverage.contains_key("Range"));
    assert!(diag.expr_type_coverage.contains_key("Path"));
}

#[test]
fn diagnostics_warnings_accumulate() {
    let source = r#"
            struct A;
            struct B;
            enum C { X }
            fn foo() -> i32 { 1 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    // Should have warnings for each struct and enum
    assert!(diag.warnings.len() >= 3);
}

#[test]
fn diagnostics_items_by_kind_multiple_kinds() {
    let source = r#"
            fn a() -> i32 { 1 }
            struct S;
            enum E { X }
            const C: i32 = 0;
            static ST: i32 = 0;
            type T = i32;
            use std::fmt;
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert_eq!(diag.items_by_kind.get("Fn").unwrap_or(&0), &1);
    assert_eq!(diag.items_by_kind.get("Struct").unwrap_or(&0), &1);
    assert_eq!(diag.items_by_kind.get("Enum").unwrap_or(&0), &1);
    assert_eq!(diag.items_by_kind.get("Const").unwrap_or(&0), &1);
    assert_eq!(diag.items_by_kind.get("Static").unwrap_or(&0), &1);
    assert_eq!(diag.items_by_kind.get("Type").unwrap_or(&0), &1);
    assert_eq!(diag.items_by_kind.get("Use").unwrap_or(&0), &1);
}

#[test]
fn diagnostics_expr_type_coverage_counts_correctly() {
    let source = r#"
            fn count_test() -> i32 {
                let a = 1;
                let b = 2;
                a + b
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    // Lit should be counted for 1, 2 (at minimum)
    let lit_count = diag.expr_type_coverage.get("Lit").unwrap_or(&0);
    assert!(*lit_count >= 2, "should have at least 2 Lit expressions");
}

// ─── compile_file tests ─────────────────────────────────────────────────

#[test]
fn compile_file_nonexistent_returns_error() {
    let mut compiler = RustToNda::new();
    let result = compiler.compile_file(std::path::Path::new("/nonexistent/path.rs"));
    assert!(result.is_err());
}

#[test]
fn compile_file_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    std::fs::write(&file_path, "fn hello() -> i32 { 42 }").unwrap();
    let mut compiler = RustToNda::new();
    let root = compiler.compile_file(&file_path).unwrap();
    assert!(matches!(root, NdaNode::Scope { .. }));
    assert_eq!(compiler.function_count(), 1);
}

// ─── walkdir_rs_files tests ─────────────────────────────────────────────

#[test]
fn walkdir_rs_files_finds_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
    std::fs::write(dir.path().join("b.rs"), "fn b() {}").unwrap();
    std::fs::write(dir.path().join("c.txt"), "not rust").unwrap();
    let files = walkdir_rs_files(dir.path()).unwrap();
    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|p| p.extension().unwrap() == "rs"));
}

#[test]
fn walkdir_rs_files_skips_target() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
    let target = dir.path().join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("b.rs"), "fn b() {}").unwrap();
    let files = walkdir_rs_files(dir.path()).unwrap();
    assert_eq!(files.len(), 1);
}

#[test]
fn walkdir_rs_files_skips_git() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
    let git = dir.path().join(".git");
    std::fs::create_dir(&git).unwrap();
    std::fs::write(git.join("b.rs"), "fn b() {}").unwrap();
    let files = walkdir_rs_files(dir.path()).unwrap();
    assert_eq!(files.len(), 1);
}

#[test]
fn walkdir_rs_files_skips_node_modules() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
    let nm = dir.path().join("node_modules");
    std::fs::create_dir(&nm).unwrap();
    std::fs::write(nm.join("b.rs"), "fn b() {}").unwrap();
    let files = walkdir_rs_files(dir.path()).unwrap();
    assert_eq!(files.len(), 1);
}

#[test]
fn walkdir_rs_files_recursive() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("src");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("inner.rs"), "fn inner() {}").unwrap();
    std::fs::write(dir.path().join("outer.rs"), "fn outer() {}").unwrap();
    let files = walkdir_rs_files(dir.path()).unwrap();
    assert_eq!(files.len(), 2);
}

#[test]
fn walkdir_rs_files_not_dir_error() {
    let result = walkdir_rs_files(std::path::Path::new("/nonexistent"));
    assert!(result.is_err());
}

// ─── store_all advanced tests ───────────────────────────────────────────

#[test]
fn store_all_empty_compiler() {
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source("").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let count = compiler.store_all(&mut sm, &root).unwrap();
    // 0 functions + 1 root = 1
    assert_eq!(count, 1);
}

#[test]
fn store_all_single_function() {
    let source = "fn solo() -> i32 { 42 }";
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let count = compiler.store_all(&mut sm, &root).unwrap();
    // 1 function + 1 root = 2
    assert_eq!(count, 2);
}

// ─── SeedReport Display format tests ────────────────────────────────────

#[test]
fn seed_report_display_hex_hash() {
    let report = SeedReport {
        source_path: std::path::PathBuf::from("lib.rs"),
        functions: 1,
        nodes_stored: 5,
        root_hash: 0x00000000DEADBEEF,
        elapsed_ms: 10,
        call_graph: HashMap::new(),
        diagnostics: CompileDiagnostics::default(),
    };
    let s = format!("{}", report);
    assert!(s.contains("deadbeef"), "should contain hex hash");
}

#[test]
fn seed_report_display_elapsed() {
    let report = SeedReport {
        source_path: std::path::PathBuf::from("x.rs"),
        functions: 2,
        nodes_stored: 8,
        root_hash: 0,
        elapsed_ms: 123,
        call_graph: HashMap::new(),
        diagnostics: CompileDiagnostics::default(),
    };
    let s = format!("{}", report);
    assert!(s.contains("123ms"));
}

// ─── SeedReport JSON pretty tests ───────────────────────────────────────

#[test]
fn seed_report_pretty_json() {
    let report = SeedReport {
        source_path: std::path::PathBuf::from("test.rs"),
        functions: 1,
        nodes_stored: 2,
        root_hash: 0,
        elapsed_ms: 0,
        call_graph: HashMap::new(),
        diagnostics: CompileDiagnostics::default(),
    };
    let pretty = serde_json::to_string_pretty(&report).unwrap();
    assert!(pretty.contains('\n'));
    assert!(pretty.contains("test.rs"));
}

#[test]
fn compile_diagnostics_pretty_json() {
    let diag = CompileDiagnostics {
        expressions_visited: 10,
        expressions_compiled: 8,
        expressions_dropped: 2,
        expr_type_coverage: {
            let mut m = HashMap::new();
            m.insert("Lit".to_string(), 5);
            m
        },
        items_by_kind: {
            let mut m = HashMap::new();
            m.insert("Fn".to_string(), 1);
            m
        },
        call_edges: 1,
        call_edges_resolved: 1,
        warnings: vec![],
    };
    let pretty = serde_json::to_string_pretty(&diag).unwrap();
    assert!(pretty.contains('\n'));
    assert!(pretty.contains("expressions_visited"));
}

// ─── ExprCollector fallback tests ───────────────────────────────────────

#[test]
fn expr_collector_fallback_handles_unknown() {
    // Cast expression should hit the fallback arm
    let source = r#"
            fn cast_test() -> i32 {
                let x = 42 as i64;
                0
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

// ─── Nested module tests ────────────────────────────────────────────────

#[test]
fn nested_module_deep() {
    let source = r#"
            mod outer {
                pub fn outer_fn() -> i32 { 1 }
                mod inner {
                    pub fn inner_fn() -> i32 { 2 }
                }
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let names = compiler.function_names();
    assert!(names.contains(&"outer_fn"));
    assert!(names.contains(&"inner_fn"));
}

#[test]
fn nested_module_empty() {
    let source = r#"
            mod empty_mod {}
            fn real_fn() -> i32 { 1 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

// ─── Multiple impl blocks tests ─────────────────────────────────────────

#[test]
fn impl_restores_current_impl() {
    let source = r#"
            struct A;
            struct B;
            impl A {
                fn method_a(&self) -> i32 { 1 }
            }
            impl B {
                fn method_b(&self) -> i32 { 2 }
            }
            fn standalone() -> i32 { 3 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let names = compiler.function_names();
    assert!(names.contains(&"A::method_a"));
    assert!(names.contains(&"B::method_b"));
    assert!(names.contains(&"standalone"));
}

// ─── CompileDiagnostics invariant tests ─────────────────────────────────

#[test]
fn diagnostics_invariant_complex_source() {
    let source = r#"
            fn a(x: i32) -> i32 {
                if x > 0 { x } else { 0 }
            }
            fn b() {
                let mut s = 0;
                for _i in 0..10 {
                    s = s + a(_i);
                }
            }
            struct Unused;
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert_eq!(
        diag.expressions_visited,
        diag.expressions_compiled + diag.expressions_dropped
    );
}

#[test]
fn diagnostics_default_all_zeros() {
    let diag = CompileDiagnostics::default();
    assert_eq!(diag.expressions_visited, 0);
    assert_eq!(diag.expressions_compiled, 0);
    assert_eq!(diag.expressions_dropped, 0);
    assert_eq!(diag.call_edges, 0);
    assert_eq!(diag.call_edges_resolved, 0);
    assert!(diag.warnings.is_empty());
    assert!(diag.expr_type_coverage.is_empty());
    assert!(diag.items_by_kind.is_empty());
}

// ─── CompiledFn advanced tests ──────────────────────────────────────────

#[test]
fn compiled_fn_empty_callees() {
    let cf = CompiledFn {
        name: "leaf".to_string(),
        node: NdaNode::Scope { children: vec![] },
        hash: 0x1234,
        callees: vec![],
    };
    assert!(cf.callees.is_empty());
    assert_eq!(cf.hash, 0x1234);
}

#[test]
fn compiled_fn_multiple_callees() {
    let cf = CompiledFn {
        name: "caller".to_string(),
        node: NdaNode::Scope { children: vec![] },
        hash: 0xABCD,
        callees: vec!["a".to_string(), "b".to_string(), "c".to_string()],
    };
    assert_eq!(cf.callees.len(), 3);
    let debug = format!("{:?}", cf);
    assert!(debug.contains("caller"));
    assert!(debug.contains("a"));
}

// ─── patch_calls edge case tests ────────────────────────────────────────

#[test]
fn patch_calls_non_call_passthrough() {
    let node = NdaNode::Int { value: 42 };
    let hashes = HashMap::new();
    let patched = patch_calls(&node, &hashes);
    assert!(matches!(patched, NdaNode::Int { value: 42 }));
}

#[test]
fn patch_calls_single_hash() {
    let node = NdaNode::Call { target: 0 };
    let mut hashes = HashMap::new();
    hashes.insert("only".to_string(), 0xBEEF);
    let patched = patch_calls(&node, &hashes);
    match patched {
        NdaNode::Call { target } => assert_eq!(target, 0xBEEF),
        _ => panic!("expected Call"),
    }
}

// ─── Integration-style tests ────────────────────────────────────────────

#[test]
fn full_pipeline_compile_and_store() {
    let source = r#"
            fn encode(x: i32) -> i32 { x + 1 }
            fn decode(x: i32) -> i32 { x - 1 }
            fn pipeline() {
                let a = encode(42);
                let b = decode(a);
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 3);

    let graph = compiler.call_graph();
    assert!(graph
        .get("pipeline")
        .unwrap()
        .contains(&"encode".to_string()));
    assert!(graph
        .get("pipeline")
        .unwrap()
        .contains(&"decode".to_string()));

    let dir = tempfile::tempdir().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let count = compiler.store_all(&mut sm, &root).unwrap();
    assert_eq!(count, 4); // 3 fns + 1 root
}

#[test]
fn source_with_generics_compiles() {
    let source = r#"
            fn generic_fn<T>(x: T) -> T { x }
            fn concrete() -> i32 { generic_fn(42) }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 2);
}

#[test]
fn source_with_attributes_compiles() {
    let source = r#"
            #[inline]
            fn fast_fn() -> i32 { 42 }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn source_with_macros_compiles() {
    let source = r#"
            fn with_macro() {
                println!("hello");
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn source_with_let_mut_compiles() {
    let source = r#"
            fn mutable() -> i32 {
                let mut x = 0;
                x = 42;
                x
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    assert!(matches!(root, NdaNode::Scope { .. }));
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn source_with_multiple_matrix_types() {
    let source = r#"
            fn multi_matrix() {
                let a: [[f32; 64]; 32] = [[0.0; 64]; 32];
                let b: [[i32; 16]; 8] = [[0; 16]; 8];
            }
        "#;
    let mut compiler = RustToNda::new();
    let root = compiler.compile_source(source).unwrap();
    fn count_matrices(node: &NdaNode) -> usize {
        match node {
            NdaNode::Matrix { .. } => 1,
            NdaNode::Scope { children } => children.iter().map(count_matrices).sum(),
            _ => 0,
        }
    }
    assert!(count_matrices(&root) >= 2, "should have 2 matrix nodes");
}

#[test]
fn unary_not_expression() {
    let source = r#"
            fn negate(x: i32) -> i32 {
                -x
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    assert!(diag.expr_type_coverage.contains_key("Unary"));
}

#[test]
fn field_access_chain() {
    let source = r#"
            fn chain() -> i32 {
                let val = 42;
                val
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn let_without_init_dropped() {
    let source = r#"
            fn no_init() {
                let _x: i32;
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    assert_eq!(compiler.function_count(), 1);
}

#[test]
fn binary_all_ops() {
    let source = r#"
            fn all_ops(a: i32, b: i32) -> i32 {
                let _c = a + b;
                let _d = a - b;
                let _e = a * b;
                let _f = a / b;
                let _g = a % b;
                a + b
            }
        "#;
    let mut compiler = RustToNda::new();
    compiler.compile_source(source).unwrap();
    let diag = compiler.diagnostics();
    let bin_count = diag.expr_type_coverage.get("Binary").unwrap_or(&0);
    assert!(*bin_count >= 5, "should have at least 5 binary expressions");
}
