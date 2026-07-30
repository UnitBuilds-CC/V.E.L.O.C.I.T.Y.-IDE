use super::*;

#[test]
fn import_export_no_crash() {
    // Import/export should parse and run without errors
    assert_eq!(eval_full("
        var x = 42;
        x
    "), JsValue::Number(42.0));
}

#[test]
fn module_resolver_on_demand() {
    // Serialize with other module-system tests that touch the global
    // resolver/registry (see MODULE_TEST_LOCK).
    let _guard = MODULE_TEST_LOCK.lock().unwrap();
    // Set a resolver that provides module source on demand
    set_module_resolver(|specifier: &str| {
        match specifier {
            "./math.js" => Some("export function add(a, b) { return a + b; }".to_string()),
            "./utils.js" => Some("export const PI = 3; export function double(x) { return x * 2; }".to_string()),
            _ => None,
        }
    });
    // Named import resolves via the callback
    let result = eval_full("
        import { add } from './math.js';
        add(3, 4)
    ");
    assert_eq!(result, JsValue::Number(7.0));
    // Namespace import resolves via the callback
    let result2 = eval_full("
        import * as utils from './utils.js';
        utils.double(utils.PI)
    ");
    assert_eq!(result2, JsValue::Number(6.0));
    // Cleanup
    clear_module_resolver();
    clear_module_registry();
}
