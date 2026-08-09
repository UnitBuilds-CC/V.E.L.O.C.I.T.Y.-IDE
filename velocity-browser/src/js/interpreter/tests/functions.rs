use super::*;

#[test]
fn function_declaration_and_call() {
    assert_eq!(
        eval_full("function add(a, b) { return a + b; } add(3, 4)"),
        JsValue::Number(7.0)
    );
}

#[test]
fn arrow_function() {
    assert_eq!(
        eval_full("var double = (x) => x * 2; double(5)"),
        JsValue::Number(10.0)
    );
}

#[test]
fn closure_captures_scope() {
    assert_eq!(
        eval_full("function make() { var x = 10; return () => x; } var get = make(); get()"),
        JsValue::Number(10.0)
    );
}

#[test]
fn function_call_apply_bind() {
    // call invokes with an explicit this and trailing arguments.
    assert_eq!(
        eval_full("function f(a, b) { return this.x + a + b; } f.call({x: 1}, 2, 3)"),
        JsValue::Number(6.0)
    );
    // apply takes its arguments from an array.
    assert_eq!(
        eval_full("function g(a, b) { return this.x + a + b; } g.apply({x: 10}, [1, 2])"),
        JsValue::Number(13.0)
    );
    // bind fixes this and returns a callable that prepends bound arguments.
    assert_eq!(
        eval_full(
            "function h(a, b) { return this.x + a + b; } var bh = h.bind({x: 100}, 1); bh(2)"
        ),
        JsValue::Number(103.0)
    );
}

#[test]
fn generator_function_basic() {
    assert_eq!(
        eval_full(
            "
        function* gen() {
            yield 1;
            yield 2;
            yield 3;
        }
        var it = gen();
        var sum = 0;
        for (var x of it) { sum = sum + x; }
        sum
    "
        ),
        JsValue::Number(6.0)
    );
}

#[test]
fn for_of_array() {
    assert_eq!(
        eval_full(
            "
        var arr = [10, 20, 30];
        var sum = 0;
        for (var x of arr) { sum = sum + x; }
        sum
    "
        ),
        JsValue::Number(60.0)
    );
}

#[test]
fn for_of_string_iterates_chars() {
    assert_eq!(
        eval_full(
            "
        var out = '';
        for (var c of 'abc') { out = out + c; }
        out
    "
        ),
        JsValue::String("abc".to_string())
    );
}

#[test]
fn for_of_map_yields_entries() {
    assert_eq!(
        eval_full(
            "
        var m = new Map([['a', 1], ['b', 2]]);
        var total = 0;
        for (var e of m) { total = total + e[1]; }
        total
    "
        ),
        JsValue::Number(3.0)
    );
}

#[test]
fn for_of_set_yields_items() {
    assert_eq!(
        eval_full(
            "
        var s = new Set([1, 2, 3]);
        var total = 0;
        for (var x of s) { total = total + x; }
        total
    "
        ),
        JsValue::Number(6.0)
    );
}

#[test]
fn for_of_custom_iterator_protocol() {
    // An object that is itself an iterator (has a stateful next()) drives for...of.
    assert_eq!(
        eval_full(
            "
        function makeRange(lo, hi) {
            var cur = lo;
            return {
                next: function() {
                    if (cur <= hi) {
                        var v = cur;
                        cur = cur + 1;
                        return { value: v, done: false };
                    }
                    return { done: true };
                }
            };
        }
        var sum = 0;
        for (var x of makeRange(1, 3)) { sum = sum + x; }
        sum
    "
        ),
        JsValue::Number(6.0)
    );
}

#[test]
fn for_of_iterable_with_iterator_method() {
    // An iterable exposing __iterator__ that returns a fresh iterator.
    assert_eq!(
        eval_full(
            "
        var iterable = {
            __iterator__: function() {
                var i = 0;
                return {
                    next: function() {
                        if (i < 3) { var v = i; i = i + 1; return { value: v, done: false }; }
                        return { done: true };
                    }
                };
            }
        };
        var sum = 0;
        for (var x of iterable) { sum = sum + x; }
        sum
    "
        ),
        JsValue::Number(3.0)
    );
}

#[test]
fn yield_keyword_in_expression() {
    // yield used as expression returns value
    assert_eq!(
        eval_full(
            "
        function* nums() { yield 10; yield 20; }
        var it = nums();
        var first = it.next();
        first.value
    "
        ),
        JsValue::Number(10.0)
    );
}

#[test]
fn new_function_constructor() {
    assert_eq!(
        eval_full(
            "
        var add = new Function('a', 'b', 'return a + b');
        add(3, 4)
    "
        ),
        JsValue::Number(7.0)
    );
}

#[test]
fn eval_function() {
    assert_eq!(
        eval_full(
            "
        eval('1 + 2')
    "
        ),
        JsValue::Number(3.0)
    );
}

#[test]
fn prototype_chain_method_lookup() {
    assert_eq!(
        eval_full(
            "
        var proto = { greet() { return 'hello'; } };
        var obj = Object.create(proto);
        obj.greet()
    "
        ),
        JsValue::String("hello".into())
    );
}
