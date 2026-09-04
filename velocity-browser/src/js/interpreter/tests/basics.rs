use super::*;

#[test]
fn arithmetic_precedence() {
    assert_eq!(eval("1 + 2 * 3"), JsValue::Number(7.0));
    assert_eq!(eval("(1 + 2) * 3"), JsValue::Number(9.0));
    assert_eq!(eval("10 % 3"), JsValue::Number(1.0));
}

#[test]
fn string_concatenation() {
    assert_eq!(eval("'a' + 'b'"), JsValue::String("ab".to_string()));
    assert_eq!(eval("'x' + 1"), JsValue::String("x1".to_string()));
}

#[test]
fn comparisons_and_logic() {
    assert_eq!(eval("2 > 1 && 1 < 2"), JsValue::Boolean(true));
    assert_eq!(eval("1 == 1"), JsValue::Boolean(true));
    assert_eq!(eval("1 != 2"), JsValue::Boolean(true));
    assert_eq!(eval("!false"), JsValue::Boolean(true));
}

#[test]
fn short_circuit_returns_operand() {
    assert_eq!(
        eval("0 || 'fallback'"),
        JsValue::String("fallback".to_string())
    );
    assert_eq!(eval("'a' && 'b'"), JsValue::String("b".to_string()));
}

#[test]
fn identifiers_resolve_from_scope() {
    let mut scope = HashMap::new();
    scope.insert("x".to_string(), JsValue::Number(5.0));
    assert_eq!(eval_expr("x * 2", &scope).unwrap(), JsValue::Number(10.0));
    assert_eq!(eval_expr("missing", &scope).unwrap(), JsValue::Undefined);
}

#[test]
fn unary_minus() {
    assert_eq!(eval("-5 + 3"), JsValue::Number(-2.0));
}

#[test]
fn if_else_works() {
    assert_eq!(
        eval_full("var x = 5; if (x > 3) { x = 10; } x"),
        JsValue::Number(10.0)
    );
    assert_eq!(
        eval_full("var x = 1; if (x > 3) { x = 10; } else { x = 20; } x"),
        JsValue::Number(20.0)
    );
}

#[test]
fn while_loop() {
    assert_eq!(
        eval_full("var i = 0; while (i < 5) { i = i + 1; } i"),
        JsValue::Number(5.0)
    );
}

#[test]
fn for_loop() {
    assert_eq!(
        eval_full("var sum = 0; for (var i = 0; i < 5; i = i + 1) { sum = sum + i; } sum"),
        JsValue::Number(10.0)
    );
}

#[test]
fn typeof_operator() {
    assert_eq!(eval("typeof 42"), JsValue::String("number".into()));
    assert_eq!(eval("typeof 'hi'"), JsValue::String("string".into()));
    assert_eq!(
        eval("typeof undefined"),
        JsValue::String("undefined".into())
    );
}

#[test]
fn break_in_loop() {
    assert_eq!(
        eval_full("var i = 0; while (true) { i = i + 1; if (i == 3) { break; } } i"),
        JsValue::Number(3.0)
    );
}

#[test]
fn ternary_expression() {
    assert_eq!(eval("true ? 1 : 2"), JsValue::Number(1.0));
    assert_eq!(eval("false ? 1 : 2"), JsValue::Number(2.0));
}

#[test]
fn nullish_coalescing() {
    assert_eq!(
        eval_full("var x = null; x ?? 'default'"),
        JsValue::String("default".into())
    );
    assert_eq!(eval_full("var x = 5; x ?? 'default'"), JsValue::Number(5.0));
}

#[test]
fn template_literal_interpolation() {
    assert_eq!(
        eval_full("var x = 5; `value is ${x}`"),
        JsValue::String("value is 5".into())
    );
    assert_eq!(
        eval_full("var a = 2; var b = 3; `${a} + ${b} = ${a + b}`"),
        JsValue::String("2 + 3 = 5".into())
    );
    assert_eq!(
        eval_full("`no interpolation`"),
        JsValue::String("no interpolation".into())
    );
}

#[test]
fn try_catch() {
    assert_eq!(
        eval_full("var result = 0; try { throw 42; } catch (e) { result = e; } result"),
        JsValue::Number(42.0)
    );
}

#[test]
fn try_finally_runs_on_normal_completion() {
    // `finally` runs after a try block that completes normally.
    assert_eq!(
        eval_full("var x = 0; try { x = 1; } finally { x = x + 10; } x"),
        JsValue::Number(11.0)
    );
}

#[test]
fn try_throw_without_catch_rethrows_after_finally() {
    // A throw with no catch clause runs `finally` and then propagates outward.
    assert_eq!(
        eval_full(
            "
        var r = '';
        try {
            try { throw 'boom'; } finally { r = 'fin'; }
        } catch (e) { r = r + ':' + e; }
        r
    "
        ),
        JsValue::String("fin:boom".to_string())
    );
}

#[test]
fn try_catch_throw_escapes_to_outer_catch() {
    // A throw inside a catch block propagates to the enclosing handler.
    assert_eq!(
        eval_full(
            "
        var r = '';
        try {
            try { throw 'a'; } catch (e) { throw 'b'; }
        } catch (e2) { r = e2; }
        r
    "
        ),
        JsValue::String("b".to_string())
    );
}

#[test]
fn optional_chaining_member() {
    assert_eq!(
        eval_full("var obj = { a: { b: 5 } }; obj?.a?.b"),
        JsValue::Number(5.0)
    );
    assert_eq!(eval_full("var obj = null; obj?.a?.b"), JsValue::Undefined);
}

#[test]
fn optional_chaining_call() {
    assert_eq!(eval_full("var fn = null; fn?.()"), JsValue::Undefined);
}

#[test]
fn nullish_assignment() {
    assert_eq!(
        eval_full("var x = null; x ??= 42; x"),
        JsValue::Number(42.0)
    );
    assert_eq!(eval_full("var x = 5; x ??= 42; x"), JsValue::Number(5.0));
}

// ── VarKind: let/const block scoping ──────────────────────────────────────

#[test]
fn varkind_let_const() {
    // var hoists to function scope
    assert_eq!(eval_full("{ var x = 10; } x"), JsValue::Number(10.0));
    // let is block-scoped
    assert_eq!(
        eval_full("let x = 5; { let x = 10; } x"),
        JsValue::Number(5.0)
    );
    // const is block-scoped and doesn't change
    assert_eq!(eval_full("const x = 42; x"), JsValue::Number(42.0));
    // const reassignment is silently ignored
    assert_eq!(eval_full("const x = 1; x = 2; x"), JsValue::Number(1.0));
}

// ── Switch/Case ──────────────────────────────────────────────────────────

#[test]
fn switch_case() {
    assert_eq!(eval_full("var x = 2; var r = 0; switch(x) { case 1: r = 10; break; case 2: r = 20; break; case 3: r = 30; break; } r"), JsValue::Number(20.0));
    assert_eq!(
        eval_full(
            "var x = 5; var r = 0; switch(x) { case 1: r = 10; break; default: r = 99; break; } r"
        ),
        JsValue::Number(99.0)
    );
    // Fall-through
    assert_eq!(
        eval_full(
            "var x = 1; var r = 0; switch(x) { case 1: r = r + 1; case 2: r = r + 10; break; } r"
        ),
        JsValue::Number(11.0)
    );
}

#[test]
fn labeled_statement() {
    assert_eq!(
        eval_full(
            "
        var x = 0;
        outer: for (var i = 0; i < 3; i = i + 1) {
            x = x + i;
        }
        x
    "
        ),
        JsValue::Number(3.0)
    );
}
