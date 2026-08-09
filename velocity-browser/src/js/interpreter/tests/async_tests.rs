use super::*;

#[test]
fn promise_resolve_then() {
    assert_eq!(
        eval_full(
            "
        var result = 0;
        Promise.resolve(10).then((v) => { result = v * 2; return result; });
        result
    "
        ),
        JsValue::Number(20.0)
    );
}

#[test]
fn async_await_sync_model() {
    assert_eq!(
        eval_full(
            "
        async function fetchData() { return 42; }
        var result = await fetchData();
        result
    "
        ),
        JsValue::Number(42.0)
    );
}

#[test]
fn promise_resolve_reject() {
    // Promise.resolve chains
    assert_eq!(
        eval_full(
            "
        var p = Promise.resolve(99);
        var result = 0;
        p.then(function(v) { result = v; });
        result
    "
        ),
        JsValue::Number(99.0)
    );
}

#[test]
fn promise_reject_catch() {
    // Rejected promise is caught by .catch()
    assert_eq!(
        eval_full(
            "
        var p = Promise.reject('oops');
        var caught = '';
        p.catch(function(e) { caught = e; });
        caught
    "
        ),
        JsValue::String("oops".to_string())
    );
}

#[test]
fn promise_then_skips_on_reject() {
    // .then() is skipped when promise is rejected
    assert_eq!(
        eval_full(
            "
        var p = Promise.reject('err');
        var called = false;
        var caught = '';
        p.then(function(v) { called = true; }).catch(function(e) { caught = e; });
        called
    "
        ),
        JsValue::Boolean(false)
    );
}

#[test]
fn await_rejected_throws() {
    // await on a rejected promise throws, caught by try/catch
    assert_eq!(
        eval_full(
            "
        var msg = '';
        try {
            var p = Promise.reject('fail');
            await p;
        } catch(e) {
            msg = e;
        }
        msg
    "
        ),
        JsValue::String("fail".to_string())
    );
}

#[test]
fn promise_executor_resolve() {
    // new Promise with resolve() call
    assert_eq!(
        eval_full(
            "
        var p = new Promise(function(resolve, reject) { resolve(77); });
        var out = 0;
        p.then(function(v) { out = v; });
        out
    "
        ),
        JsValue::Number(77.0)
    );
}

#[test]
fn promise_executor_reject() {
    // new Promise with reject() call
    assert_eq!(
        eval_full(
            "
        var p = new Promise(function(resolve, reject) { reject('bad'); });
        var out = '';
        p.catch(function(e) { out = e; });
        out
    "
        ),
        JsValue::String("bad".to_string())
    );
}

#[test]
fn promise_then_flattens() {
    // .then() returning a promise is flattened
    assert_eq!(
        eval_full(
            "
        var p = Promise.resolve(10);
        var out = 0;
        p.then(function(v) { return Promise.resolve(v * 2); }).then(function(v) { out = v; });
        out
    "
        ),
        JsValue::Number(20.0)
    );
}
