use super::*;

#[test]
fn object_property_access() {
    assert_eq!(
        eval_full("var obj = { a: 1, b: 2 }; obj.a + obj.b"),
        JsValue::Number(3.0)
    );
}

#[test]
fn destructuring_object() {
    assert_eq!(
        eval_full("var obj = { a: 1, b: 2 }; let { a, b } = obj; a + b"),
        JsValue::Number(3.0)
    );
}

#[test]
fn destructuring_array() {
    assert_eq!(
        eval_full("let [x, y] = [10, 20]; x + y"),
        JsValue::Number(30.0)
    );
}

#[test]
fn destructuring_with_alias() {
    assert_eq!(
        eval_full("let { a: first } = { a: 42 }; first"),
        JsValue::Number(42.0)
    );
}

#[test]
fn class_basic() {
    assert_eq!(
        eval_full(
            "
        class Animal {
            constructor(name) {
                this.name = name;
            }
            speak() {
                return this.name;
            }
        }
        var a = new Animal('dog');
        a.name
    "
        ),
        JsValue::String("dog".into())
    );
}

#[test]
fn class_inheritance() {
    assert_eq!(
        eval_full(
            "
        class Base {
            constructor(x) { this.x = x; }
        }
        class Child extends Base {
            constructor(x, y) { this.x = x; this.y = y; }
        }
        var c = new Child(1, 2);
        c.x + c.y
    "
        ),
        JsValue::Number(3.0)
    );
}

#[test]
fn instanceof_direct_class() {
    assert_eq!(
        eval_full(
            "
        class Animal {}
        var a = new Animal();
        a instanceof Animal
    "
        ),
        JsValue::Boolean(true)
    );
}

#[test]
fn instanceof_inherited_class() {
    assert_eq!(
        eval_full(
            "
        class Animal {}
        class Dog extends Animal {}
        var d = new Dog();
        (d instanceof Dog) && (d instanceof Animal)
    "
        ),
        JsValue::Boolean(true)
    );
}

#[test]
fn instanceof_negative() {
    assert_eq!(
        eval_full(
            "
        class Animal {}
        class Cat {}
        var a = new Animal();
        a instanceof Cat
    "
        ),
        JsValue::Boolean(false)
    );
}

#[test]
fn instanceof_plain_object_is_false() {
    assert_eq!(
        eval_full(
            "
        class Animal {}
        var o = { a: 1 };
        o instanceof Animal
    "
        ),
        JsValue::Boolean(false)
    );
}

#[test]
fn super_constructor_call() {
    assert_eq!(
        eval_full(
            "
        class Animal {
            constructor(n) { this.name = n; }
        }
        class Dog extends Animal {
            constructor(n) { super(n); this.kind = 'dog'; }
        }
        var d = new Dog('Rex');
        d.name + '/' + d.kind
    "
        ),
        JsValue::String("Rex/dog".to_string())
    );
}

#[test]
fn super_method_call() {
    assert_eq!(
        eval_full(
            "
        class Animal {
            speak() { return 'generic'; }
        }
        class Dog extends Animal {
            speak() { return super.speak() + ' woof'; }
        }
        var d = new Dog();
        d.speak()
    "
        ),
        JsValue::String("generic woof".to_string())
    );
}

#[test]
fn super_method_uses_this() {
    assert_eq!(
        eval_full(
            "
        class Base {
            greet() { return 'hi ' + this.name; }
        }
        class Sub extends Base {
            constructor() { this.name = 'vel'; }
            greet() { return super.greet() + '!'; }
        }
        var s = new Sub();
        s.greet()
    "
        ),
        JsValue::String("hi vel!".to_string())
    );
}

#[test]
fn super_chained_constructors() {
    assert_eq!(
        eval_full(
            "
        class A { constructor() { this.a = 1; } }
        class B extends A { constructor() { super(); this.b = 2; } }
        class C extends B { constructor() { super(); this.c = 3; } }
        var x = new C();
        x.a + x.b + x.c
    "
        ),
        JsValue::Number(6.0)
    );
}

#[test]
fn static_method_call() {
    assert_eq!(
        eval_full(
            "
        class Math2 {
            static double(n) { return n * 2; }
        }
        Math2.double(21)
    "
        ),
        JsValue::Number(42.0)
    );
}

#[test]
fn static_method_this_is_class() {
    assert_eq!(
        eval_full(
            "
        class Counter {
            static base() { return 10; }
            static doubled() { return this.base() * 2; }
        }
        Counter.doubled()
    "
        ),
        JsValue::Number(20.0)
    );
}

#[test]
fn static_method_inherited() {
    assert_eq!(
        eval_full(
            "
        class Base {
            static hello() { return 'hi'; }
        }
        class Sub extends Base {}
        Sub.hello()
    "
        ),
        JsValue::String("hi".to_string())
    );
}

#[test]
fn class_getter() {
    assert_eq!(
        eval_full(
            "
        class C {
            get x() { return 42; }
        }
        var c = new C();
        c.x
    "
        ),
        JsValue::Number(42.0)
    );
}

#[test]
fn class_getter_uses_this() {
    assert_eq!(
        eval_full(
            "
        class C {
            constructor() { this._v = 7; }
            get x() { return this._v; }
        }
        var c = new C();
        c.x
    "
        ),
        JsValue::Number(7.0)
    );
}

#[test]
fn class_getter_setter_pair() {
    assert_eq!(
        eval_full(
            "
        class C {
            set x(v) { this._v = v * 3; }
            get x() { return this._v; }
        }
        var c = new C();
        c.x = 4;
        c.x
    "
        ),
        JsValue::Number(12.0)
    );
}

#[test]
fn class_getter_inherited() {
    assert_eq!(
        eval_full(
            "
        class Base {
            get kind() { return 'base'; }
        }
        class Sub extends Base {}
        var s = new Sub();
        s.kind
    "
        ),
        JsValue::String("base".to_string())
    );
}

#[test]
fn delete_property_removes_key() {
    assert_eq!(
        eval_full(
            "
        var obj = { a: 1, b: 2 };
        delete obj.a;
        'a' in obj
    "
        ),
        JsValue::Boolean(false)
    );
}

#[test]
fn delete_property_value_gone() {
    assert_eq!(
        eval_full(
            "
        var obj = { a: 1, b: 2 };
        delete obj.a;
        obj.b
    "
        ),
        JsValue::Number(2.0)
    );
}

#[test]
fn delete_returns_true() {
    assert_eq!(
        eval_full(
            "
        var obj = { a: 1 };
        delete obj.a
    "
        ),
        JsValue::Boolean(true)
    );
}

#[test]
fn delete_computed_key() {
    assert_eq!(
        eval_full(
            "
        var obj = { x: 9 };
        delete obj['x'];
        obj.x
    "
        ),
        JsValue::Undefined
    );
}

#[test]
fn delete_array_element_leaves_hole() {
    assert_eq!(
        eval_full(
            "
        var arr = [1, 2, 3];
        delete arr[1];
        arr[0] + arr[2]
    "
        ),
        JsValue::Number(4.0)
    );
}

#[test]
fn computed_property_key_from_var() {
    assert_eq!(
        eval_full(
            "
        var k = 'name';
        var obj = { [k]: 'vel' };
        obj.name
    "
        ),
        JsValue::String("vel".to_string())
    );
}

#[test]
fn computed_property_key_expression() {
    assert_eq!(
        eval_full(
            "
        var obj = { ['a' + 'b']: 1 };
        obj.ab
    "
        ),
        JsValue::Number(1.0)
    );
}

#[test]
fn computed_property_key_number() {
    assert_eq!(
        eval_full(
            "
        var i = 2;
        var obj = { [i]: 'x' };
        obj['2']
    "
        ),
        JsValue::String("x".to_string())
    );
}

#[test]
fn reflect_set_mutates_in_place() {
    assert_eq!(
        eval_full(
            "
        var obj = {};
        var ok = Reflect.set(obj, 'x', 5);
        obj.x
    "
        ),
        JsValue::Number(5.0)
    );
}

#[test]
fn reflect_delete_property_removes_key() {
    assert_eq!(
        eval_full(
            "
        var obj = { a: 1 };
        Reflect.deleteProperty(obj, 'a');
        obj.a
    "
        ),
        JsValue::Undefined
    );
}

#[test]
fn reflect_construct_class() {
    assert_eq!(
        eval_full(
            "
        class Animal {
            constructor(n) { this.name = n; }
        }
        var a = Reflect.construct(Animal, ['Rex']);
        a.name
    "
        ),
        JsValue::String("Rex".to_string())
    );
}

#[test]
fn reflect_construct_class_instanceof() {
    assert_eq!(
        eval_full(
            "
        class Animal {
            constructor(n) { this.name = n; }
        }
        var a = Reflect.construct(Animal, ['Rex']);
        a instanceof Animal
    "
        ),
        JsValue::Boolean(true)
    );
}

#[test]
fn reflect_construct_function() {
    assert_eq!(
        eval_full(
            "
        function Point(x, y) { this.x = x; this.y = y; }
        var p = Reflect.construct(Point, [3, 4]);
        p.x + p.y
    "
        ),
        JsValue::Number(7.0)
    );
}

#[test]
fn reflect_has_consults_proxy_has_trap() {
    // Reflect.has routes through the proxy `has` trap, like the `in` operator.
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var handler = { has: function(t, k) { return k === 'x' || k === 'y'; } };
        var p = new Proxy(target, handler);
        Reflect.has(p, 'y')
    "
        ),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var handler = { has: function(t, k) { return k === 'x' || k === 'y'; } };
        var p = new Proxy(target, handler);
        Reflect.has(p, 'z')
    "
        ),
        JsValue::Boolean(false)
    );
}

#[test]
fn reflect_has_walks_prototype_chain() {
    // Reflect.has sees inherited members, matching JS semantics.
    assert_eq!(
        eval_full(
            "
        class Base { hello() { return 1; } }
        var b = new Base();
        Reflect.has(b, 'hello')
    "
        ),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full(
            "
        class Base { hello() { return 1; } }
        var b = new Base();
        Reflect.has(b, 'absent')
    "
        ),
        JsValue::Boolean(false)
    );
}

#[test]
fn reflect_delete_property_consults_proxy_trap() {
    // A proxy `deleteProperty` trap returning false vetoes the delete.
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var handler = { deleteProperty: function(t, k) { return false; } };
        var p = new Proxy(target, handler);
        Reflect.deleteProperty(p, 'x')
    "
        ),
        JsValue::Boolean(false)
    );
    // A trap returning true reports success.
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var handler = { deleteProperty: function(t, k) { return true; } };
        var p = new Proxy(target, handler);
        Reflect.deleteProperty(p, 'x')
    "
        ),
        JsValue::Boolean(true)
    );
}

#[test]
fn reflect_delete_property_identifier_writeback() {
    // Deleting via an identifier target mutates the binding in place.
    assert_eq!(
        eval_full(
            "
        var obj = { a: 1, b: 2 };
        Reflect.deleteProperty(obj, 'a');
        obj.a
    "
        ),
        JsValue::Undefined
    );
    // Deleting an absent key yields true (JS non-strict semantics).
    assert_eq!(
        eval_full(
            "
        var obj = { a: 1 };
        Reflect.deleteProperty(obj, 'missing')
    "
        ),
        JsValue::Boolean(true)
    );
}

#[test]
fn reflect_own_keys_consults_proxy_trap() {
    assert_eq!(
        eval_full(
            "
        var target = { a: 1, b: 2 };
        var handler = { ownKeys: function(t) { return ['a', 'b', 'c']; } };
        var p = new Proxy(target, handler);
        Reflect.ownKeys(p).length
    "
        ),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full(
            "
        var target = { a: 1, b: 2 };
        var handler = { ownKeys: function(t) { return ['a', 'b', 'c']; } };
        var p = new Proxy(target, handler);
        Reflect.ownKeys(p)[2]
    "
        ),
        JsValue::String("c".to_string())
    );
}

#[test]
fn reflect_own_keys_on_array_includes_length() {
    // Array own keys are the indices plus `length`.
    assert_eq!(
        eval_full("Reflect.ownKeys([10, 20, 30]).length"),
        JsValue::Number(4.0)
    );
    assert_eq!(
        eval_full("Reflect.ownKeys([10, 20, 30])[3]"),
        JsValue::String("length".to_string())
    );
}

#[test]
fn object_define_property_data_descriptor() {
    // Data descriptor installs the value and preserves existing keys (write-back).
    assert_eq!(
        eval_full(
            "
        var obj = { a: 1 };
        Object.defineProperty(obj, 'b', { value: 2 });
        obj.a + obj.b
    "
        ),
        JsValue::Number(3.0)
    );
}

#[test]
fn object_define_property_getter() {
    // An accessor getter is invoked on property read.
    assert_eq!(
        eval_full(
            "
        var obj = {};
        Object.defineProperty(obj, 'x', { get: function() { return 42; } });
        obj.x
    "
        ),
        JsValue::Number(42.0)
    );
}

#[test]
fn object_define_property_setter() {
    // An accessor setter is invoked on property write.
    assert_eq!(
        eval_full(
            "
        var captured = 0;
        var obj = {};
        Object.defineProperty(obj, 'x', { set: function(v) { captured = v; } });
        obj.x = 99;
        captured
    "
        ),
        JsValue::Number(99.0)
    );
}

#[test]
fn object_define_properties_multiple() {
    assert_eq!(
        eval_full(
            "
        var obj = {};
        Object.defineProperties(obj, { x: { value: 10 }, y: { value: 20 } });
        obj.x + obj.y
    "
        ),
        JsValue::Number(30.0)
    );
}

#[test]
fn object_get_own_property_descriptor_value() {
    assert_eq!(
        eval_full(
            "
        var obj = { name: 'velocity' };
        var d = Object.getOwnPropertyDescriptor(obj, 'name');
        d.value
    "
        ),
        JsValue::String("velocity".to_string())
    );
}

#[test]
fn object_get_own_property_descriptor_accessor() {
    // Accessor descriptors report get/set, not value.
    assert_eq!(
        eval_full(
            "
        var obj = {};
        Object.defineProperty(obj, 'x', { get: function() { return 1; } });
        var d = Object.getOwnPropertyDescriptor(obj, 'x');
        typeof d.get
    "
        ),
        JsValue::String("function".to_string())
    );
}

#[test]
fn object_literal_getter() {
    assert_eq!(
        eval_full(
            "
        var obj = { get x() { return 42; } };
        obj.x
    "
        ),
        JsValue::Number(42.0)
    );
}

#[test]
fn object_literal_getter_uses_this() {
    assert_eq!(
        eval_full(
            "
        var obj = { _v: 7, get x() { return this._v; } };
        obj.x
    "
        ),
        JsValue::Number(7.0)
    );
}

#[test]
fn object_literal_setter() {
    assert_eq!(
        eval_full(
            "
        var captured = 0;
        var obj = { set x(v) { captured = v; } };
        obj.x = 99;
        captured
    "
        ),
        JsValue::Number(99.0)
    );
}

#[test]
fn object_literal_getter_setter_pair() {
    assert_eq!(
        eval_full(
            "
        var obj = {
            _n: 1,
            get n() { return this._n; },
            set n(v) { this._n = v * 2; }
        };
        obj.n = 5;
        obj.n
    "
        ),
        JsValue::Number(10.0)
    );
}

#[test]
fn object_keys_hides_internal_keys() {
    // Class instances carry `__class_name__`/`__instanceof__` bookkeeping that
    // must not leak into Object.keys.
    assert_eq!(
        eval_full(
            "
        class Animal { constructor(n) { this.name = n; } }
        var a = new Animal('rex');
        Object.keys(a).length
    "
        ),
        JsValue::Number(1.0)
    );
    assert_eq!(
        eval_full(
            "
        class Animal { constructor(n) { this.name = n; } }
        var a = new Animal('rex');
        Object.keys(a).indexOf('name') >= 0
    "
        ),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full(
            "
        class Animal { constructor(n) { this.name = n; } }
        var a = new Animal('rex');
        Object.keys(a).indexOf('__instanceof__')
    "
        ),
        JsValue::Number(-1.0)
    );
}

#[test]
fn for_in_hides_internal_keys() {
    assert_eq!(
        eval_full(
            "
        class Animal { constructor(n) { this.name = n; this.age = 5; } }
        var a = new Animal('rex');
        var count = 0;
        for (var k in a) { count = count + 1; }
        count
    "
        ),
        JsValue::Number(2.0)
    );
}

#[test]
fn object_values_resolves_getter() {
    assert_eq!(
        eval_full(
            "
        var o = { get x() { return 42; } };
        Object.values(o)[0]
    "
        ),
        JsValue::Number(42.0)
    );
    assert_eq!(
        eval_full(
            "
        var o = { get x() { return 42; } };
        Object.keys(o).indexOf('x') >= 0
    "
        ),
        JsValue::Boolean(true)
    );
}

#[test]
fn object_entries_resolves_getter() {
    assert_eq!(
        eval_full(
            "
        var o = { a: 1, get x() { return 9; } };
        Object.entries(o).length
    "
        ),
        JsValue::Number(2.0)
    );
}

#[test]
fn user_double_underscore_key_not_internal() {
    // `__foo` (no trailing delimiter) is a legitimate user key and must survive.
    assert_eq!(
        eval_full(
            "
        var o = { __foo: 7 };
        Object.keys(o).indexOf('__foo') >= 0
    "
        ),
        JsValue::Boolean(true)
    );
}

#[test]
fn object_keys_values_on_array() {
    assert_eq!(
        eval_full("Object.keys([10, 20, 30]).length"),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full("Object.values([10, 20, 30])[1]"),
        JsValue::Number(20.0)
    );
}

#[test]
fn object_from_entries_builds_object() {
    // Round-trips with Object.entries and accepts a literal array of pairs.
    assert_eq!(
        eval_full("Object.fromEntries([['a', 1], ['b', 2]]).b"),
        JsValue::Number(2.0)
    );
    assert_eq!(
        eval_full("Object.fromEntries(Object.entries({ x: 5 })).x"),
        JsValue::Number(5.0)
    );
    assert_eq!(
        eval_full("Object.keys(Object.fromEntries([['k', 9]])).length"),
        JsValue::Number(1.0)
    );
}

#[test]
fn object_get_own_property_descriptors_basic() {
    // Each own data property yields a descriptor carrying its value.
    assert_eq!(
        eval_full("Object.getOwnPropertyDescriptors({ a: 1, b: 2 }).a.value"),
        JsValue::Number(1.0)
    );
    assert_eq!(
        eval_full("Object.getOwnPropertyDescriptors({ a: 1, b: 2 }).b.value"),
        JsValue::Number(2.0)
    );
    // Data descriptors default to writable/enumerable/configurable true.
    assert_eq!(
        eval_full("Object.getOwnPropertyDescriptors({ a: 1 }).a.writable"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("Object.getOwnPropertyDescriptors({ a: 1 }).a.enumerable"),
        JsValue::Boolean(true)
    );
    // A round-trip through Object.keys sees exactly the own keys.
    assert_eq!(
        eval_full("Object.keys(Object.getOwnPropertyDescriptors({ only: 5 }))[0]"),
        JsValue::String("only".to_string())
    );
}

#[test]
fn object_has_own_static() {
    // True for a directly-owned key, false for an absent one.
    assert_eq!(
        eval_full("Object.hasOwn({ a: 1 }, 'a')"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("Object.hasOwn({ a: 1 }, 'b')"),
        JsValue::Boolean(false)
    );
    // Array indices in range are owned; out-of-range are not; length is owned.
    assert_eq!(
        eval_full("Object.hasOwn([10, 20], '1')"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("Object.hasOwn([10, 20], '5')"),
        JsValue::Boolean(false)
    );
    assert_eq!(
        eval_full("Object.hasOwn([10, 20], 'length')"),
        JsValue::Boolean(true)
    );
}

#[test]
fn object_get_own_property_names_basic() {
    // Reports all own string keys of a plain object (order-independent for multi-key).
    assert_eq!(
        eval_full("Object.getOwnPropertyNames({ a: 1, b: 2 }).length"),
        JsValue::Number(2.0)
    );
    // A single-key object yields that key deterministically.
    assert_eq!(
        eval_full("Object.getOwnPropertyNames({ only: 1 })[0]"),
        JsValue::String("only".to_string())
    );
}

#[test]
fn object_get_own_property_names_array_includes_length() {
    assert_eq!(
        eval_full("Object.getOwnPropertyNames([10, 20, 30]).length"),
        JsValue::Number(4.0)
    );
    assert_eq!(
        eval_full("Object.getOwnPropertyNames([10, 20, 30])[3]"),
        JsValue::String("length".to_string())
    );
}

#[test]
fn object_get_own_property_names_consults_proxy_trap() {
    assert_eq!(
        eval_full(
            "
        var target = { a: 1, b: 2 };
        var handler = { ownKeys: function(t) { return ['a', 'b', 'c']; } };
        var p = new Proxy(target, handler);
        Object.getOwnPropertyNames(p).length
    "
        ),
        JsValue::Number(3.0)
    );
}

#[test]
fn object_is_and_set_prototype_of() {
    // Object.is uses SameValue: NaN equals NaN, +0 differs from -0.
    assert_eq!(eval_full("Object.is(NaN, NaN)"), JsValue::Boolean(true));
    assert_eq!(eval_full("Object.is(0, -0)"), JsValue::Boolean(false));
    assert_eq!(eval_full("Object.is(1, 1)"), JsValue::Boolean(true));
    assert_eq!(eval_full("Object.is('a', 'b')"), JsValue::Boolean(false));
    // setPrototypeOf installs a prototype whose members become reachable.
    assert_eq!(
        eval_full(
            "var proto = { greet: 42 }; var o = {}; Object.setPrototypeOf(o, proto); o.greet"
        ),
        JsValue::Number(42.0)
    );
}

#[test]
fn object_keys_on_string_and_array() {
    // Object.keys on a string returns character indices.
    assert_eq!(eval_full("Object.keys('abc').length"), JsValue::Number(3.0));
    assert_eq!(
        eval_full("Object.keys('abc')[0]"),
        JsValue::String("0".to_string())
    );
    // Object.values on a string returns the characters.
    assert_eq!(
        eval_full("Object.values('hi')[1]"),
        JsValue::String("i".to_string())
    );
    // Object.keys on an array returns indices.
    assert_eq!(
        eval_full("Object.keys([10,20,30]).length"),
        JsValue::Number(3.0)
    );
}

#[test]
fn object_spread() {
    assert_eq!(
        eval_full(
            "
        var base = { a: 1, b: 2 };
        var extended = { ...base, c: 3 };
        extended.a + extended.b + extended.c
    "
        ),
        JsValue::Number(6.0)
    );
}

#[test]
fn object_spread_override() {
    assert_eq!(
        eval_full(
            "
        var base = { a: 1, b: 2 };
        var override_obj = { ...base, b: 10 };
        override_obj.b
    "
        ),
        JsValue::Number(10.0)
    );
}

#[test]
fn method_shorthand() {
    assert_eq!(
        eval_full(
            "
        var obj = { add(a, b) { return a + b; } };
        obj.add(3, 4)
    "
        ),
        JsValue::Number(7.0)
    );
}

#[test]
fn this_binding_in_method() {
    assert_eq!(
        eval_full(
            "
        var obj = { x: 10, getX() { return this.x; } };
        obj.getX()
    "
        ),
        JsValue::Number(10.0)
    );
}

#[test]
fn this_binding_class_method() {
    assert_eq!(
        eval_full(
            "
        class Counter {
            constructor() { this.count = 0; }
            inc() { this.count = this.count + 1; return this.count; }
        }
        var c = new Counter();
        c.inc();
        c.inc()
    "
        ),
        JsValue::Number(2.0)
    );
}

#[test]
fn class_field_basic() {
    assert_eq!(
        eval_full(
            "
        class C { x = 5; }
        new C().x
    "
        ),
        JsValue::Number(5.0)
    );
}

#[test]
fn class_field_declaration_order() {
    // Later fields may reference earlier ones via `this`; order must be preserved.
    assert_eq!(
        eval_full(
            "
        class C { a = 2; b = this.a + 3; }
        new C().b
    "
        ),
        JsValue::Number(5.0)
    );
}

#[test]
fn class_field_bare_is_undefined() {
    assert_eq!(
        eval_full(
            "
        class C { x; }
        new C().x
    "
        ),
        JsValue::Undefined
    );
}

#[test]
fn class_static_field() {
    assert_eq!(
        eval_full(
            "
        class C { static n = 10; }
        C.n
    "
        ),
        JsValue::Number(10.0)
    );
}

#[test]
fn class_field_overridden_by_constructor() {
    assert_eq!(
        eval_full(
            "
        class C { x = 1; constructor() { this.x = 9; } }
        new C().x
    "
        ),
        JsValue::Number(9.0)
    );
}

#[test]
fn class_field_inherited() {
    assert_eq!(
        eval_full(
            "
        class A { a = 1; }
        class B extends A { b = 2; }
        var o = new B();
        o.a + o.b
    "
        ),
        JsValue::Number(3.0)
    );
}

#[test]
fn new_expression_member_chain() {
    // `new Foo().member` and `new Foo().method()` must parse and evaluate.
    assert_eq!(
        eval_full(
            "
        class P { constructor(x) { this.x = x; } double() { return this.x * 2; } }
        new P(21).double()
    "
        ),
        JsValue::Number(42.0)
    );
    assert_eq!(
        eval_full(
            "
        class P { constructor(x) { this.x = x; } }
        new P(7).x
    "
        ),
        JsValue::Number(7.0)
    );
}

#[test]
fn proxy_construction() {
    let result = eval_full(
        "
        var target = { x: 42 };
        var handler = {};
        var p = new Proxy(target, handler);
        p
    ",
    );
    // Phase 7: Proxy is now a native JsValue::Proxy variant
    match &result {
        JsValue::Proxy { target, handler } => {
            assert_eq!(
                **target,
                JsValue::Object({
                    let mut t = HashMap::new();
                    t.insert("x".to_string(), JsValue::Number(42.0));
                    t
                })
            );
            assert_eq!(**handler, JsValue::Object(HashMap::new()));
        }
        _ => panic!("Expected JsValue::Proxy, got {:?}", result),
    }
}

#[test]
fn proxy_has_trap_controls_in_operator() {
    // The handler.has trap intercepts the `in` operator.
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var handler = { has: function(t, k) { return k === 'x' || k === 'y'; } };
        var p = new Proxy(target, handler);
        ('y' in p)
    "
        ),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var handler = { has: function(t, k) { return k === 'x' || k === 'y'; } };
        var p = new Proxy(target, handler);
        ('z' in p)
    "
        ),
        JsValue::Boolean(false)
    );
}

#[test]
fn proxy_in_operator_falls_through_to_target_without_has_trap() {
    // Without a has trap, `in` reflects the target's own keys.
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var p = new Proxy(target, {});
        ('x' in p)
    "
        ),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var p = new Proxy(target, {});
        ('missing' in p)
    "
        ),
        JsValue::Boolean(false)
    );
}

#[test]
fn in_operator_sees_inherited_members() {
    // `in` reports prototype methods, matching JS semantics.
    assert_eq!(
        eval_full(
            "
        class Base { hello() { return 1; } }
        var b = new Base();
        ('hello' in b)
    "
        ),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full(
            "
        class Base { hello() { return 1; } }
        var b = new Base();
        ('absent' in b)
    "
        ),
        JsValue::Boolean(false)
    );
}

#[test]
fn in_operator_array_and_string() {
    assert_eq!(eval_full("('length' in [1, 2, 3])"), JsValue::Boolean(true));
    assert_eq!(eval_full("('1' in [1, 2, 3])"), JsValue::Boolean(true));
    assert_eq!(eval_full("('5' in [1, 2, 3])"), JsValue::Boolean(false));
    assert_eq!(eval_full("('0' in 'abc')"), JsValue::Boolean(true));
    assert_eq!(eval_full("('3' in 'abc')"), JsValue::Boolean(false));
}

#[test]
fn proxy_delete_property_trap_controls_result() {
    // A deleteProperty trap returning false vetoes the delete.
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var handler = { deleteProperty: function(t, k) { return false; } };
        var p = new Proxy(target, handler);
        (delete p.x)
    "
        ),
        JsValue::Boolean(false)
    );
    // A trap returning true reports success.
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var handler = { deleteProperty: function(t, k) { return true; } };
        var p = new Proxy(target, handler);
        (delete p.x)
    "
        ),
        JsValue::Boolean(true)
    );
}

#[test]
fn proxy_delete_forwards_to_target_without_trap() {
    assert_eq!(
        eval_full(
            "
        var target = { x: 1 };
        var p = new Proxy(target, {});
        delete p.x;
        ('x' in p)
    "
        ),
        JsValue::Boolean(false)
    );
}

#[test]
fn proxy_own_keys_trap_drives_object_keys() {
    assert_eq!(
        eval_full(
            "
        var target = { a: 1, b: 2 };
        var handler = { ownKeys: function(t) { return ['a', 'b', 'c']; } };
        var p = new Proxy(target, handler);
        Object.keys(p).length
    "
        ),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full(
            "
        var target = { a: 1, b: 2 };
        var handler = { ownKeys: function(t) { return ['a', 'b', 'c']; } };
        var p = new Proxy(target, handler);
        Object.keys(p)[2]
    "
        ),
        JsValue::String("c".to_string())
    );
}

#[test]
fn proxy_apply_trap_intercepts_call() {
    // handler.apply(target, thisArg, args) intercepts calling the proxy.
    assert_eq!(
        eval_full(
            "
        function greet(n) { return 'hi ' + n; }
        var handler = { apply: function(t, th, a) { return 'intercepted:' + a[0]; } };
        var p = new Proxy(greet, handler);
        p('bob')
    "
        ),
        JsValue::String("intercepted:bob".to_string())
    );
}

#[test]
fn proxy_apply_trap_can_delegate_to_target() {
    // The trap may call the target itself and transform the result.
    assert_eq!(
        eval_full(
            "
        function sum(a, b) { return a + b; }
        var handler = { apply: function(t, th, a) { return t(a[0], a[1]) * 10; } };
        var p = new Proxy(sum, handler);
        p(3, 4)
    "
        ),
        JsValue::Number(70.0)
    );
}

#[test]
fn proxy_call_forwards_to_target_without_apply_trap() {
    assert_eq!(
        eval_full(
            "
        function add(a, b) { return a + b; }
        var p = new Proxy(add, {});
        p(2, 3)
    "
        ),
        JsValue::Number(5.0)
    );
}
