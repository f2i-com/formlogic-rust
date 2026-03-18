#![allow(clippy::approx_constant)]

use formlogic_core::engine::FormLogicEngine;
use formlogic_core::object::Object;

// ═══════════════════════════════════════════════════════════════════════
// 1. Destructuring Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn destructuring_array_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [1, 2, 3]; let [a, b, c] = arr; a + b + c;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn destructuring_object_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let obj = {"x": 10, "y": 20}; let {x, y} = obj; x + y;"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn destructuring_nested_object() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let data = {"user": {"name": "John"}}; let {user} = data; user.name;"#)
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "John"),
        _ => panic!("expected string output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Spread Operator Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn spread_in_array_literals() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr1 = [1, 2]; let arr2 = [3, 4]; let combined = [...arr1, ...arr2]; combined.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn spread_in_function_calls() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let add = function(a, b, c) { return a + b + c; }; let nums = [1, 2, 3]; add(...nums);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn spread_in_object_literals() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let obj1 = {"a": 1, "b": 2}; let obj2 = {"c": 3}; let combined = {...obj1, ...obj2}; combined.a + combined.c;"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Template Literals Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn template_literal_plain() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("`hello world`;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello world"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn template_literal_interpolation() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("`1 + 1 = ${1 + 1}`;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1 + 1 = 2"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn template_literal_nested() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("`outer ${`inner ${1 + 1}`}`;")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "outer inner 2"),
        _ => panic!("expected string output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Higher-Order Function Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn higher_order_map() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [1, 2, 3]; arr.map(function(x) { return x * 2; });")
        .expect("eval");
    match out {
        Object::Array(items) => {
            let items = items.borrow();
            assert_eq!(items.len(), 3);
            assert!(items[0].is_i32() && unsafe { items[0].as_i32_unchecked() } == 2);
            assert!(items[1].is_i32() && unsafe { items[1].as_i32_unchecked() } == 4);
            assert!(items[2].is_i32() && unsafe { items[2].as_i32_unchecked() } == 6);
        }
        _ => panic!("expected array output"),
    }
}

#[test]
fn higher_order_filter() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [1, 2, 3, 4, 5]; arr.filter(function(x) { return x > 2; });")
        .expect("eval");
    match out {
        Object::Array(items) => {
            let items = items.borrow();
            assert_eq!(items.len(), 3);
            assert!(items[0].is_i32() && unsafe { items[0].as_i32_unchecked() } == 3);
            assert!(items[1].is_i32() && unsafe { items[1].as_i32_unchecked() } == 4);
            assert!(items[2].is_i32() && unsafe { items[2].as_i32_unchecked() } == 5);
        }
        _ => panic!("expected array output"),
    }
}

#[test]
fn higher_order_reduce() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [1, 2, 3, 4]; arr.reduce(function(acc, x) { return acc + x; }, 0);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn higher_order_foreach() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [1, 2, 3]; let sum = 0; arr.forEach(function(x) { sum = sum + x; }); sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. String Method Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_length() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""hello".length;"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn string_char_at() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""hello".charAt(1);"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "e"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_substring() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""hello".substring(1, 4);"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "ell"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_to_upper_case() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""hello".toUpperCase();"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "HELLO"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_to_lower_case() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""HELLO".toLowerCase();"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_split() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""a,b,c".split(",");"#).expect("eval");
    match out {
        Object::Array(items) => {
            let items = items.borrow();
            assert_eq!(items.len(), 3);
        }
        _ => panic!("expected array output"),
    }
}

#[test]
fn string_index_of() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""hello".indexOf("l");"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn string_includes() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""hello world".includes("world");"#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn string_trim() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""  hello  ".trim();"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello"),
        _ => panic!("expected string output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. String Static Method Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_from_char_code_single() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("String.fromCharCode(65);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "A"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_from_char_code_multiple() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("String.fromCharCode(72, 101, 108, 108, 111);")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "Hello"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_constructor_number() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("String(123);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "123"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_constructor_boolean() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("String(true);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "true"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_typeof() {
    let engine = FormLogicEngine::default();
    // In this engine, String is exposed as an object module, so typeof
    // reports "object" rather than "function".
    let out = engine.eval("typeof String;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "object"),
        _ => panic!("expected string output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Array Method Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_join() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"[1, 2, 3].join(",");"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1,2,3"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn array_reverse() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("[1, 2, 3].reverse();").expect("eval");
    match out {
        Object::Array(items) => {
            let items = items.borrow();
            assert_eq!(items.len(), 3);
            assert!(items[0].is_i32() && unsafe { items[0].as_i32_unchecked() } == 3);
            assert!(items[1].is_i32() && unsafe { items[1].as_i32_unchecked() } == 2);
            assert!(items[2].is_i32() && unsafe { items[2].as_i32_unchecked() } == 1);
        }
        _ => panic!("expected array output"),
    }
}

#[test]
fn array_slice() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("[1, 2, 3, 4, 5].slice(1, 4);").expect("eval");
    match out {
        Object::Array(items) => {
            let items = items.borrow();
            assert_eq!(items.len(), 3);
            assert!(items[0].is_i32() && unsafe { items[0].as_i32_unchecked() } == 2);
            assert!(items[1].is_i32() && unsafe { items[1].as_i32_unchecked() } == 3);
            assert!(items[2].is_i32() && unsafe { items[2].as_i32_unchecked() } == 4);
        }
        _ => panic!("expected array output"),
    }
}

#[test]
fn array_index_of() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("[1, 2, 3].indexOf(2);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn array_includes() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("[1, 2, 3].includes(2);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn array_find() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[1, 2, 3, 4].find(function(x) { return x > 2; });")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn array_some() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[1, 2, 3].some(function(x) { return x > 2; });")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn array_every() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[1, 2, 3].every(function(x) { return x > 0; });")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Math Module Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn math_abs() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Math.abs(-5);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn math_floor() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Math.floor(3.7);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn math_ceil() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Math.ceil(3.2);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn math_round() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Math.round(3.5);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn math_min() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Math.min(5, 3, 8, 1);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn math_max() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Math.max(5, 3, 8, 1);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn math_pow() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Math.pow(2, 3);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn math_sqrt() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Math.sqrt(16);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. JSON Module Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn json_stringify() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"JSON.stringify({"name": "John", "age": 30});"#)
        .expect("eval");
    match out {
        Object::String(v) => {
            assert!(v.contains("name"));
            assert!(v.contains("John"));
            assert!(v.contains("age"));
            assert!(v.contains("30"));
        }
        _ => panic!("expected string output"),
    }
}

#[test]
fn json_parse() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let obj = JSON.parse("{\"name\":\"John\",\"age\":30}"); obj.name;"#)
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "John"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn json_parse_age() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let obj = JSON.parse("{\"name\":\"John\",\"age\":30}"); obj.age;"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Closures and Scoping Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn closure_captures_by_reference() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let x = 10; let getX = function() { return x; }; x = 20; getX();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 20),
        Object::Float(v) => assert!((v - 20.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn nested_closures() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let outer = function() { let x = 10; let middle = function() { let y = 5; let inner = function() { return x + y; }; return inner(); }; return middle(); }; outer();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        Object::Float(v) => assert!((v - 15.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn function_returning_function() {
    let engine = FormLogicEngine::default();
    // Verify higher-order function behavior: a function that defines
    // local variables and creates + invokes an inner function that
    // captures those locals (mirroring the nested_closures pattern).
    let out = engine
        .eval("let outer = function() { let x = 5; let adder = function() { let y = 3; return x + y; }; return adder(); }; outer();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected numeric output, got {:?}", out),
    }
}

#[test]
fn recursive_fibonacci() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("function fib(n) { if (n <= 1) { return n; } return fib(n - 1) + fib(n - 2); } fib(10);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 55),
        Object::Float(v) => assert!((v - 55.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. This Binding Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn this_in_object_methods() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let obj = {"value": 42, "getValue": function() { return this.value; }}; obj.getValue();"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn this_in_class_methods() {
    let engine = FormLogicEngine::default();
    // Verify that a class method can read and write this properties.
    // Constructor sets count=0, increment adds 1 and returns it.
    let out = engine
        .eval("class Counter { constructor() { this.count = 0; } increment() { this.count += 1; return this.count; } } let c = new Counter(); c.increment();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output, got {:?}", out),
    }

    // Verify method can return a computed value from this properties.
    let out = engine
        .eval("class Acc { constructor(v) { this.val = v; } add(n) { this.val += n; return this.val; } } let a = new Acc(10); a.add(5);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        Object::Float(v) => assert!((v - 15.0).abs() < 1e-9),
        _ => panic!("expected numeric output, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Super Call Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn super_in_constructors() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("class Animal { constructor(name) { this.name = name; } } class Dog extends Animal { constructor(name) { super(name); } } let d = new Dog(\"Rex\"); d.name;")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "Rex"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn super_in_methods() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("class Animal { speak() { return \"sound\"; } } class Dog extends Animal { speak() { return super.speak() + \" bark\"; } } let d = new Dog(); d.speak();")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "sound bark"),
        _ => panic!("expected string output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Getter/Setter Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn class_getter() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"class Person { constructor(first, last) { this.first = first; this.last = last; } get fullName() { return this.first + " " + this.last; } } let p = new Person("John", "Doe"); p.fullName;"#)
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "John Doe"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn class_setter() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("class Temperature { constructor(c) { this._celsius = c; } get celsius() { return this._celsius; } set fahrenheit(f) { this._celsius = (f - 32) * 5 / 9; } } let t = new Temperature(0); t.fahrenheit = 100; t.celsius;")
        .expect("eval");
    match out {
        Object::Float(v) => assert!((v - 37.77777777777778).abs() < 0.01),
        Object::Integer(v) => assert!((v as f64 - 37.78).abs() < 0.01),
        _ => panic!("expected numeric output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Edge Cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn division_by_zero_positive() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("1 / 0;").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_infinite() && v.is_sign_positive()),
        _ => panic!("expected +infinity float"),
    }
}

#[test]
fn division_by_zero_negative() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("-1 / 0;").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_infinite() && v.is_sign_negative()),
        _ => panic!("expected -infinity float"),
    }
}

#[test]
fn division_by_zero_nan() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("0 / 0;").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_nan()),
        _ => panic!("expected NaN float"),
    }
}

#[test]
fn array_out_of_bounds_high() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("let arr = [1, 2, 3]; arr[10];").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn array_out_of_bounds_negative() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("let arr = [1, 2, 3]; arr[-1];").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn modulo_negative_dividend() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("-5 % 3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -2),
        Object::Float(v) => assert!((v - (-2.0)).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn modulo_negative_divisor() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("5 % -3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn modulo_both_negative() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("-5 % -3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -2),
        Object::Float(v) => assert!((v - (-2.0)).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn typeof_number() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("typeof 42;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "number"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn typeof_string() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("typeof \"hello\";").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "string"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn typeof_boolean() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("typeof true;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "boolean"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn typeof_null() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("typeof null;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "object"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn typeof_function() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("typeof function(){};").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "function"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn equality_null_null() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("null == null;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn equality_zero_false() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("0 == false;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn equality_string_number_coercion() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""1" == 1;"#).expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn strict_equality_same_type() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("1 === 1;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn strict_equality_different_types() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"1 === "1";"#).expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn falsy_false() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("false ? \"truthy\" : \"falsy\";")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "falsy"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn falsy_zero() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("0 ? \"truthy\" : \"falsy\";")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "falsy"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn falsy_empty_string() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""" ? "truthy" : "falsy";"#)
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "falsy"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn falsy_null() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("null ? \"truthy\" : \"falsy\";")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "falsy"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn truthy_true() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("true ? \"truthy\" : \"falsy\";")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "truthy"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn truthy_one() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("1 ? \"truthy\" : \"falsy\";")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "truthy"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn truthy_empty_array() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[] ? \"truthy\" : \"falsy\";")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "truthy"),
        _ => panic!("expected string output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Operator Precedence
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn operator_precedence_add_mul() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("2 + 3 * 4;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 14),
        Object::Float(v) => assert!((v - 14.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn operator_precedence_parens() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("(2 + 3) * 4;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 20),
        Object::Float(v) => assert!((v - 20.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn operator_precedence_mixed() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("2 + 3 * 4 - 5 / 5;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 13),
        Object::Float(v) => assert!((v - 13.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Short-Circuit Evaluation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn short_circuit_and_skips_rhs() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let called = false; let sideEffect = function() { called = true; return true; }; let result = false && sideEffect(); called;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn short_circuit_or_skips_rhs() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let called = false; let sideEffect = function() { called = true; return false; }; let result = true || sideEffect(); called;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

// ═══════════════════════════════════════════════════════════════════════
// 17. RegExp Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn regexp_test_match() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let re = new RegExp("hello"); re.test("hello world");"#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn regexp_test_no_match() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let re = new RegExp("foo"); re.test("hello world");"#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Bug Fix Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nested_template_literals() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("`A ${ `B ${10}` }`;")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "A B 10"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn function_hoisting() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("function add(a, b) { return a + b; } add(3, 4);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn array_destructuring_with_holes() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let [a, , c] = [1, 2, 3]; a + c;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn return_without_value() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("function f() { return; } f();")
        .expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn trailing_comma_in_array() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("[1, 2, 3,].length;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn trailing_comma_in_function_call() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("function add(a, b) { return a + b; } add(3, 4,);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn for_in_iterates_keys() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let r = []; for (let k in [10, 20, 30]) { r.push(k); } r.join(",");"#)
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "0,1,2"),
        _ => panic!("expected string output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 19. String Edge Cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_slice_negative_start() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""hello".slice(-2);"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "lo"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_slice_negative_range() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""hello".slice(-3, -1);"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "ll"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_repeat() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""ab".repeat(3);"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "ababab"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_repeat_zero() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""hello".repeat(0);"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, ""),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_pad_start() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""5".padStart(3, "0");"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "005"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_pad_end() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""5".padEnd(3, "0");"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "500"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn string_last_index_of() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""hello hello".lastIndexOf("hello");"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn string_last_index_of_not_found() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""hello".lastIndexOf("x");"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -1),
        Object::Float(v) => assert!((v - (-1.0)).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn string_char_code_at() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""A".charCodeAt(0);"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 65),
        Object::Float(v) => assert!((v - 65.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Array lastIndexOf
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_last_index_of_found() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[1, 2, 3, 2, 1].lastIndexOf(2);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn array_last_index_of_not_found() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("[1, 2, 3].lastIndexOf(5);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -1),
        Object::Float(v) => assert!((v - (-1.0)).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 21. New Built-in Methods (shift, unshift, splice, concat, trimStart, etc.)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_shift() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [1, 2, 3]; let first = arr.shift(); first;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        _ => panic!("expected integer"),
    }
}

#[test]
fn array_shift_mutates() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [1, 2, 3]; arr.shift(); arr.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        _ => panic!("expected integer"),
    }
}

#[test]
fn array_shift_empty() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("let arr = []; arr.shift();").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn array_unshift() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [2, 3]; arr.unshift(1); arr.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        _ => panic!("expected integer"),
    }
}

#[test]
fn array_unshift_returns_length() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [3]; arr.unshift(1, 2);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        _ => panic!("expected integer"),
    }
}

#[test]
fn array_splice_remove() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [1, 2, 3, 4, 5]; let removed = arr.splice(1, 2); arr.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        _ => panic!("expected integer"),
    }
}

#[test]
fn array_splice_insert() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let arr = [1, 4, 5]; arr.splice(1, 0, 2, 3); arr.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        _ => panic!("expected integer"),
    }
}

#[test]
fn array_concat() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("[1, 2].concat([3, 4]);").expect("eval");
    if let Object::Array(items) = out {
        let items = items.borrow();
        assert_eq!(items.len(), 4);
    } else {
        panic!("expected array");
    }
}

#[test]
fn string_concat_method() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""hello".concat(" ", "world");"#)
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello world"),
        _ => panic!("expected string"),
    }
}

#[test]
fn string_trim_start() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""  hello  ".trimStart();"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello  "),
        _ => panic!("expected string"),
    }
}

#[test]
fn string_trim_end() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""  hello  ".trimEnd();"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "  hello"),
        _ => panic!("expected string"),
    }
}

#[test]
fn string_from_code_point() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("String.fromCodePoint(65, 66, 67);")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "ABC"),
        _ => panic!("expected string"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 22. Control Flow (do-while, switch)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn do_while_loop() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let i = 0; let sum = 0; do { sum = sum + i; i = i + 1; } while (i < 5); sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric"),
    }
}

#[test]
fn do_while_executes_at_least_once() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let count = 0; do { count = count + 1; } while (false); count;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        _ => panic!("expected integer"),
    }
}

#[test]
fn switch_statement() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let x = 2; let result = ""; switch (x) { case 1: result = "one"; break; case 2: result = "two"; break; default: result = "other"; } result;"#)
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "two"),
        _ => panic!("expected string"),
    }
}

#[test]
fn switch_default() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let x = 99; let result = ""; switch (x) { case 1: result = "one"; break; default: result = "default"; } result;"#)
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "default"),
        _ => panic!("expected string"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 23. Additional Parity Tests (from TypeScript audit_operators, audit_control)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn typeof_undefined() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("typeof undefined;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "undefined"),
        _ => panic!("expected string"),
    }
}

#[test]
fn typeof_array() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("typeof [1,2,3];").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "object"),
        _ => panic!("expected string"),
    }
}

#[test]
fn typeof_object() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"typeof {"a": 1};"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "object"),
        _ => panic!("expected string"),
    }
}

#[test]
fn nan_not_equal_to_self() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("NaN == NaN;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
    let out = engine.eval("NaN === NaN;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn loose_equality_null_undefined() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("null == undefined;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
    let out = engine.eval("undefined == null;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn loose_equality_null_not_zero() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("null == 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
    let out = engine.eval("null == false;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn void_returns_undefined() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("void 123;").expect("eval");
    assert!(matches!(out, Object::Undefined));
    let out = engine.eval(r#"void "hello";"#).expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn bitwise_operations() {
    let engine = FormLogicEngine::default();
    // AND
    let out = engine.eval("5 & 3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        _ => panic!("expected integer"),
    }
    // OR
    let out = engine.eval("5 | 3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        _ => panic!("expected integer"),
    }
    // XOR
    let out = engine.eval("5 ^ 3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        _ => panic!("expected integer"),
    }
    // NOT
    let out = engine.eval("~5;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -6),
        _ => panic!("expected integer"),
    }
    // Left shift
    let out = engine.eval("1 << 3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        _ => panic!("expected integer"),
    }
    // Right shift
    let out = engine.eval("16 >> 2;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        _ => panic!("expected integer"),
    }
    // Unsigned right shift
    let out = engine.eval("-1 >>> 0;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4294967295),
        Object::Float(v) => assert!((v - 4294967295.0).abs() < 1.0),
        _ => panic!("expected numeric"),
    }
}

#[test]
fn exponentiation_right_associative() {
    let engine = FormLogicEngine::default();
    // 2 ** 3 ** 2 = 2 ** (3 ** 2) = 2 ** 9 = 512
    let out = engine.eval("2 ** 3 ** 2;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 512),
        Object::Float(v) => assert!((v - 512.0).abs() < 1e-9),
        _ => panic!("expected numeric"),
    }
}

#[test]
fn increment_decrement_postfix() {
    let engine = FormLogicEngine::default();
    // Postfix: returns old value, then increments
    let out = engine
        .eval("let i = 5; let x = i++; x;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        _ => panic!("expected integer"),
    }
}

#[test]
fn increment_decrement_prefix() {
    let engine = FormLogicEngine::default();
    // Prefix: increments first, returns new value
    let out = engine
        .eval("let i = 5; let x = ++i; x;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        _ => panic!("expected integer"),
    }
}

#[test]
fn nullish_coalescing_only_null_undefined() {
    let engine = FormLogicEngine::default();
    // false is NOT nullish
    let out = engine.eval(r#"false ?? "default";"#).expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
    // 0 is NOT nullish
    let out = engine.eval(r#"0 ?? "default";"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        _ => panic!("expected 0"),
    }
    // empty string is NOT nullish
    let out = engine.eval(r#""" ?? "default";"#).expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, ""),
        _ => panic!("expected empty string"),
    }
}

#[test]
fn optional_chaining_returns_undefined() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("let o = null; o?.name;").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn optional_chaining_accesses_property() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let o = {"name": "test"}; o?.name;"#)
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "test"),
        _ => panic!("expected string"),
    }
}

#[test]
fn map_and_set_basic() {
    let engine = FormLogicEngine::default();
    // Map
    let out = engine
        .eval(r#"let m = new Map(); m.set("a", 1); m.set("b", 2); m.get("a");"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        _ => panic!("expected integer"),
    }
    // Set
    let out = engine
        .eval("let s = new Set(); s.add(1); s.add(2); s.add(1); s.size;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        _ => panic!("expected integer"),
    }
}

#[test]
fn generator_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("function* gen() { yield 1; yield 2; yield 3; } let g = gen(); g.next().value + g.next().value + g.next().value;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric"),
    }
}

#[test]
fn tagged_template_literal() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"function tag(strings) { return strings.length; } tag`hello`;"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        _ => panic!("expected integer"),
    }
}

#[test]
fn class_static_method() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("class MathHelper { static add(a, b) { return a + b; } } MathHelper.add(3, 4);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric"),
    }
}

#[test]
fn class_instance_fields() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("class Foo { x = 10; y = 20; } let f = new Foo(); f.x + f.y;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected numeric"),
    }
}

#[test]
fn class_static_field() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("class Config { static version = 42; } Config.version;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected integer"),
    }
}

#[test]
fn number_parse_int() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"parseInt("42");"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected integer"),
    }
}

#[test]
fn number_parse_float() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"parseFloat("3.14");"#).expect("eval");
    match out {
        Object::Float(v) => assert!((v - 3.14).abs() < 1e-9),
        _ => panic!("expected float"),
    }
}

#[test]
fn number_is_nan() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Number.isNaN(NaN);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
    let out = engine.eval("Number.isNaN(42);").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn number_is_finite() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Number.isFinite(42);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
    let out = engine.eval("Number.isFinite(1 / 0);").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn object_keys_values_entries() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"Object.keys({"a": 1, "b": 2}).length;"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        _ => panic!("expected integer"),
    }
    let out = engine
        .eval(r#"Object.values({"a": 10, "b": 20}).length;"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        _ => panic!("expected integer"),
    }
}

#[test]
fn promise_resolve_reject() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("await Promise.resolve(42);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected integer"),
    }
}

#[test]
fn async_await_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("async function add(a, b) { return a + b; } await add(3, 4);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric"),
    }
}

#[test]
fn regex_literal_test() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"/hello/.test("hello world");"#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
    let out = engine
        .eval(r#"/xyz/.test("hello world");"#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn instanceof_operator() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("[] instanceof Array;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn in_operator_object() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""a" in {"a": 1};"#).expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
    let out = engine.eval(r#""b" in {"a": 1};"#).expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn delete_operator() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let o = {"a": 1, "b": 2}; delete o.a; "a" in o;"#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn logical_assignment_operators() {
    let engine = FormLogicEngine::default();
    // &&=
    let out = engine.eval("let x = 1; x &&= 5; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        _ => panic!("expected integer"),
    }
    // ||=
    let out = engine.eval("let x = 0; x ||= 5; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        _ => panic!("expected integer"),
    }
    // ??=
    let out = engine.eval("let x = null; x ??= 7; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        _ => panic!("expected integer"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Closure over function parameters
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn closure_captures_parameter() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            function makeAdder(x) {
                return function(y) { return x + y; };
            }
            let add5 = makeAdder(5);
            add5(3);
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected 8, got {:?}", out),
    }
}

#[test]
fn closure_captures_multiple_parameters() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            function makePair(a, b) {
                return function() { return a + b; };
            }
            let f = makePair(10, 20);
            f();
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected 30, got {:?}", out),
    }
}

#[test]
fn closure_over_parameter_and_local() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            function make(x) {
                let y = x * 2;
                return function() { return x + y; };
            }
            make(5)();
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        Object::Float(v) => assert!((v - 15.0).abs() < 1e-9),
        _ => panic!("expected 15, got {:?}", out),
    }
}

#[test]
fn closure_counter_pattern() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            function makeCounter(start) {
                let count = start;
                return function() {
                    count = count + 1;
                    return count;
                };
            }
            let c = makeCounter(0);
            c();
            c();
            c();
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected 3, got {:?}", out),
    }
}

#[test]
fn higher_order_function_with_callback() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            function apply(fn, val) {
                return fn(val);
            }
            function double(x) { return x * 2; }
            apply(double, 21);
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected 42, got {:?}", out),
    }
}

#[test]
fn currying() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            function curry(f) {
                return function(a) {
                    return function(b) {
                        return f(a, b);
                    };
                };
            }
            function add(a, b) { return a + b; }
            curry(add)(3)(4);
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected 7, got {:?}", out),
    }
}

#[test]
fn plain_object_property_mutation() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let obj = {count: 0};
            obj.count = obj.count + 1;
            obj.count = obj.count + 1;
            obj.count;
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected 2, got {:?}", out),
    }
}

#[test]
fn plain_object_method_mutation() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let obj = {
                count: 0,
                inc() { this.count = this.count + 1; return this.count; }
            };
            obj.inc();
            obj.inc();
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected 2, got {:?}", out),
    }
}

#[test]
fn class_read_property_after_method() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            class C {
                constructor() { this.count = 0; }
                inc() { this.count = this.count + 1; return this.count; }
            }
            let c = new C();
            c.inc();
            c.count;
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected 1, got {:?}", out),
    }
}

#[test]
fn class_instance_repeated_method_calls() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            class Counter {
                constructor() { this.count = 0; }
                increment() {
                    this.count = this.count + 1;
                    return this.count;
                }
            }
            let c = new Counter();
            c.increment();
            c.increment();
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2, "second increment should return 2"),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9, "second increment should return 2"),
        _ => panic!("expected 2, got {:?}", out),
    }
}

#[test]
fn for_loop_closure_captures_per_iteration() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let fns = [];
            for (let i = 0; i < 3; i = i + 1) {
                fns.push(function() { return i; });
            }
            fns[0]() + "," + fns[1]() + "," + fns[2]();
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "0,1,2"),
        _ => panic!("expected '0,1,2', got {:?}", out),
    }
}

#[test]
fn class_instance_state_persists() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            class Box {
                constructor(val) { this.value = val; }
                add(n) { this.value = this.value + n; }
                get() { return this.value; }
            }
            let b = new Box(10);
            b.add(5);
            b.add(3);
            b.get();
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 18),
        Object::Float(v) => assert!((v - 18.0).abs() < 1e-9),
        _ => panic!("expected 18, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Error handling: try-catch-finally
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn try_catch_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let r; try { throw "err"; } catch(e) { r = e; } r;"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "err"),
        _ => panic!("expected 'err', got {:?}", out),
    }
}

#[test]
fn try_catch_error_message() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let r; try { throw new Error("boom"); } catch(e) { r = e.message; } r;"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "boom"),
        _ => panic!("expected 'boom', got {:?}", out),
    }
}

#[test]
fn try_finally_runs() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let log = [];
            try { log.push("try"); }
            finally { log.push("finally"); }
            log.join(",");
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "try,finally"),
        _ => panic!("expected 'try,finally', got {:?}", out),
    }
}

#[test]
fn try_catch_finally_with_throw() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let log = [];
            try {
                log.push("try");
                throw "x";
            } catch(e) {
                log.push("catch");
            } finally {
                log.push("finally");
            }
            log.join(",");
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "try,catch,finally"),
        _ => panic!("expected 'try,catch,finally', got {:?}", out),
    }
}

#[test]
fn nested_try_catch() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let msg = "";
            try {
                try { throw new Error("inner"); }
                catch(e) { throw e; }
            } catch(e) { msg = e.message; }
            msg;
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "inner"),
        _ => panic!("expected 'inner', got {:?}", out),
    }
}

#[test]
fn throw_non_error_value() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let r; try { throw 42; } catch(e) { r = e; } r;"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected 42, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Destructuring: defaults, rest, nested
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_destructuring_with_defaults() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let [a = 10, b = 20, c = 30] = [1]; a + "," + b + "," + c;"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1,20,30"),
        _ => panic!("expected '1,20,30', got {:?}", out),
    }
}

#[test]
fn array_destructuring_with_rest() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let [first, ...rest] = [1, 2, 3, 4];
            first + "," + rest.length;
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1,3"),
        _ => panic!("expected '1,3', got {:?}", out),
    }
}

#[test]
fn object_destructuring_with_rename_and_default() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let {a: x = 5, b: y = 6} = {a: 1}; x + "," + y;"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1,6"),
        _ => panic!("expected '1,6', got {:?}", out),
    }
}

#[test]
fn nested_object_destructuring() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let {a: {b: {c}}} = {a: {b: {c: 42}}}; c;"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected 42, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Labeled break/continue
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn labeled_break_outer_loop() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let r = [];
            outer: for (let i = 0; i < 3; i = i + 1) {
                for (let j = 0; j < 3; j = j + 1) {
                    if (i === 1 && j === 1) break outer;
                    r.push(i + "," + j);
                }
            }
            r.join(";");
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "0,0;0,1;0,2;1,0"),
        _ => panic!("expected '0,0;0,1;0,2;1,0', got {:?}", out),
    }
}

#[test]
fn labeled_continue_outer_loop() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let r = [];
            outer: for (let i = 0; i < 3; i = i + 1) {
                for (let j = 0; j < 3; j = j + 1) {
                    if (j === 1) continue outer;
                    r.push(i + "," + j);
                }
            }
            r.join(";");
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "0,0;1,0;2,0"),
        _ => panic!("expected '0,0;1,0;2,0', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// For-of on strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn for_of_string_iterates_chars() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let chars = [];
            for (let c of "abc") { chars.push(c); }
            chars.join(",");
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "a,b,c"),
        _ => panic!("expected 'a,b,c', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Computed property names
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn computed_property_names() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let key = "dynamic";
            let obj = {[key]: 42};
            obj.dynamic;
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected 42, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Unary plus/minus coercion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unary_plus_string_to_number() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"+"42";"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected 42, got {:?}", out),
    }
}

#[test]
fn unary_plus_boolean() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"+true;"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected 1, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Compound assignment operators
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn compound_assignment_chain() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let x = 10;
            x += 5;
            x -= 3;
            x *= 2;
            x /= 4;
            x %= 4;
            x;
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected 2, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Global functions: parseInt, parseFloat, isNaN, isFinite
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_int_global() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"parseInt("42");"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected 42, got {:?}", out),
    }
}

#[test]
fn parse_float_global() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"parseFloat("3.14");"#).expect("eval");
    match out {
        Object::Float(v) => assert!((v - 3.14).abs() < 1e-9),
        _ => panic!("expected ~3.14, got {:?}", out),
    }
}

#[test]
fn is_nan_global() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"isNaN(NaN);"#).expect("eval");
    match out {
        Object::Boolean(v) => assert!(v),
        _ => panic!("expected true, got {:?}", out),
    }
}

#[test]
fn is_finite_global() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"isFinite(5);"#).expect("eval");
    match out {
        Object::Boolean(v) => assert!(v),
        _ => panic!("expected true, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Array.from / Array.of
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_from_string() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"Array.from("abc").join(",");"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "a,b,c"),
        _ => panic!("expected 'a,b,c', got {:?}", out),
    }
}

#[test]
fn array_from_with_map() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"Array.from([1,2,3], function(x) { return x * 2; }).join(",");"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "2,4,6"),
        _ => panic!("expected '2,4,6', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// String methods: replace, replaceAll, codePointAt
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_replace_first() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""aabaa".replace("a", "x");"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "xabaa"),
        _ => panic!("expected 'xabaa', got {:?}", out),
    }
}

#[test]
fn string_replace_all() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""aabaa".replaceAll("a", "x");"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "xxbxx"),
        _ => panic!("expected 'xxbxx', got {:?}", out),
    }
}

#[test]
fn string_code_point_at() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""abc".codePointAt(0);"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 97),
        Object::Float(v) => assert!((v - 97.0).abs() < 1e-9),
        _ => panic!("expected 97, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Array.at() with negative indexing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_at_positive() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"[10, 20, 30].at(1);"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 20),
        _ => panic!("expected 20, got {:?}", out),
    }
}

#[test]
fn array_at_negative() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"[10, 20, 30].at(-1);"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        _ => panic!("expected 30, got {:?}", out),
    }
}

#[test]
fn string_at_negative() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""hello".at(-1);"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "o"),
        _ => panic!("expected 'o', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Array.flat / flatMap
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_flat_default() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"[[1,2],[3,4]].flat().join(",");"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1,2,3,4"),
        _ => panic!("expected '1,2,3,4', got {:?}", out),
    }
}

#[test]
fn array_flat_map() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"[1,2,3].flatMap(function(x) { return [x, x*2]; }).join(",");"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1,2,2,4,3,6"),
        _ => panic!("expected '1,2,2,4,3,6', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Generator advanced patterns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn generator_return_early() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            function* gen() { yield 1; yield 2; yield 3; }
            let g = gen();
            g.next();
            let r = g.return(42);
            r.value;
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected 42, got {:?}", out),
    }
}

#[test]
fn generator_preserves_local_state() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            function* counter() {
                let count = 0;
                while (true) {
                    count = count + 1;
                    yield count;
                }
            }
            let g = counter();
            g.next().value + "," + g.next().value + "," + g.next().value;
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1,2,3"),
        _ => panic!("expected '1,2,3', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Object.assign / Object.keys / Object.values / Object.entries
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn object_assign_merge() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let a = {x: 1};
            let b = {y: 2};
            Object.assign(a, b);
            a.x + "," + a.y;
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1,2"),
        _ => panic!("expected '1,2', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Bitwise NOT
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn bitwise_not() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"~5;"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -6),
        _ => panic!("expected -6, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Class inheritance with instance state
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn class_inheritance_method_state() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            class Animal {
                constructor(name) { this.name = name; }
                speak() { return this.name + " makes a noise"; }
            }
            class Dog extends Animal {
                constructor(name) { super(name); }
                speak() { return this.name + " barks"; }
            }
            let d = new Dog("Rex");
            d.speak();
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "Rex barks"),
        _ => panic!("expected 'Rex barks', got {:?}", out),
    }
}

#[test]
fn class_field_access_after_multiple_methods() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            class Queue {
                constructor() { this.items = []; }
                enqueue(item) { this.items.push(item); }
                size() { return this.items.length; }
            }
            let q = new Queue();
            q.enqueue("a");
            q.enqueue("b");
            q.enqueue("c");
            q.size();
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected 3, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Arrow functions (if supported)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn arrow_function_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let add = (a, b) => a + b; add(3, 4);"#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected 7, got {:?}", out),
    }
}

#[test]
fn arrow_function_with_body() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let factorial = (n) => {
                if (n <= 1) return 1;
                return n * factorial(n - 1);
            };
            factorial(5);
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 120),
        Object::Float(v) => assert!((v - 120.0).abs() < 1e-9),
        _ => panic!("expected 120, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Ternary operator
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ternary_operator() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"let x = 5; x > 3 ? "big" : "small";"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "big"),
        _ => panic!("expected 'big', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Optional chaining
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn optional_chaining_null() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"let obj = null; obj?.x;"#).expect("eval");
    match out {
        Object::Undefined | Object::Null => {}
        _ => panic!("expected undefined/null, got {:?}", out),
    }
}

#[test]
fn optional_chaining_value() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"let obj = {x: 42}; obj?.x;"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected 42, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Nullish coalescing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nullish_coalescing() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"let x = null; x ?? "default";"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "default"),
        _ => panic!("expected 'default', got {:?}", out),
    }
}

#[test]
fn nullish_coalescing_zero() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"let x = 0; x ?? "default";"#).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!((v - 0.0).abs() < 1e-9),
        _ => panic!("expected 0, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// String template with expressions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn template_literal_with_method_call() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let arr = [1,2,3]; `length: ${arr.length}`;"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "length: 3"),
        _ => panic!("expected 'length: 3', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// typeof operator
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn typeof_returns_number() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"typeof 42;"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "number"),
        _ => panic!("expected 'number', got {:?}", out),
    }
}

#[test]
fn typeof_returns_string() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"typeof "hello";"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "string"),
        _ => panic!("expected 'string', got {:?}", out),
    }
}

#[test]
fn typeof_returns_boolean() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"typeof true;"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "boolean"),
        _ => panic!("expected 'boolean', got {:?}", out),
    }
}

#[test]
fn typeof_null_is_object() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"typeof null;"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "object"),
        _ => panic!("expected 'object', got {:?}", out),
    }
}

#[test]
fn typeof_returns_undefined() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"typeof undefined;"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "undefined"),
        _ => panic!("expected 'undefined', got {:?}", out),
    }
}

#[test]
fn typeof_returns_function() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"typeof function(){};"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "function"),
        _ => panic!("expected 'function', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Void operator
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn void_operator() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#"void 0;"#).expect("eval");
    match out {
        Object::Undefined => {}
        _ => panic!("expected undefined, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// For-in loop
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn for_in_object_keys() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let obj = {a: 1, b: 2, c: 3};
            let keys = [];
            for (let k in obj) { keys.push(k); }
            keys.join(",");
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "a,b,c"),
        _ => panic!("expected 'a,b,c', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Symbol-like patterns (computed access)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn bracket_access_with_variable() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let obj = {x: 10, y: 20};
            let key = "y";
            obj[key];
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 20),
        _ => panic!("expected 20, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// String methods: padStart, padEnd
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_pad_start_basic() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""5".padStart(3, "0");"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "005"),
        _ => panic!("expected '005', got {:?}", out),
    }
}

#[test]
fn string_pad_end_basic() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""5".padEnd(3, "0");"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "500"),
        _ => panic!("expected '500', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Method chaining with return this
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn method_chaining_return_this() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            class Builder {
                constructor() { this.parts = []; }
                add(p) { this.parts.push(p); return this; }
                build() { return this.parts.join(","); }
            }
            new Builder().add("a").add("b").add("c").build();
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "a,b,c"),
        _ => panic!("expected 'a,b,c', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Default parameters
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn default_parameter() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"function greet(name = "World") { return "Hello " + name; } greet();"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "Hello World"),
        _ => panic!("expected 'Hello World', got {:?}", out),
    }
}

#[test]
fn default_parameter_overridden() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"function greet(name = "World") { return "Hello " + name; } greet("Rust");"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "Hello Rust"),
        _ => panic!("expected 'Hello Rust', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Nested ternary
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nested_ternary() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let x = 5; x > 10 ? "big" : x > 3 ? "medium" : "small";"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "medium"),
        _ => panic!("expected 'medium', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Rest parameters
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rest_parameters_sum() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            function sum(...nums) {
                let total = 0;
                for (let n of nums) { total = total + n; }
                return total;
            }
            sum(1, 2, 3, 4, 5);
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        Object::Float(v) => assert!((v - 15.0).abs() < 1e-9),
        _ => panic!("expected 15, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// For-of with destructuring
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn for_of_destructuring_pairs() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            let pairs = [[1, "a"], [2, "b"]];
            let r = [];
            for (let [n, s] of pairs) { r.push(n + s); }
            r.join(",");
            "#,
        )
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1a,2b"),
        _ => panic!("expected '1a,2b', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Spread in function call
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn spread_in_function_call() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
            function add(a, b, c) { return a + b + c; }
            let args = [1, 2, 3];
            add(...args);
            "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected 6, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Finally block control flow
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn finally_overrides_try_return() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            function test() {
                try { return "try"; }
                finally { return "finally"; }
            }
            test()
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "finally"),
        _ => panic!("expected 'finally', got {:?}", out),
    }
}

#[test]
fn finally_overrides_catch_return() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            function test() {
                try { throw new Error("x"); }
                catch(e) { return "catch"; }
                finally { return "finally"; }
            }
            test()
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "finally"),
        _ => panic!("expected 'finally', got {:?}", out),
    }
}

#[test]
fn try_finally_runs_before_return() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let log = [];
            function test() {
                try {
                    log.push("try");
                    return "val";
                } finally {
                    log.push("finally");
                }
            }
            test() + "," + log.join(",")
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "val,try,finally"),
        _ => panic!("expected 'val,try,finally', got {:?}", out),
    }
}

#[test]
fn rethrow_from_nested_catch() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let log = [];
            try {
                try { throw "inner"; }
                catch(e) {
                    log.push("caught:" + e);
                    throw e;
                }
            } catch(e) {
                log.push("outer:" + e);
            }
            log.join(',')
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "caught:inner,outer:inner"),
        _ => panic!("expected 'caught:inner,outer:inner', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Switch statement edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn switch_fall_through() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let result = [];
            let x = 2;
            switch(x) {
                case 1: result.push('one');
                case 2: result.push('two');
                case 3: result.push('three'); break;
                default: result.push('other');
            }
            result.join(',')
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "two,three"),
        _ => panic!("expected 'two,three', got {:?}", out),
    }
}

#[test]
fn switch_default_in_middle() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            function test(x) {
                switch(x) {
                    case 1: return 'one';
                    default: return 'other';
                    case 2: return 'two';
                }
            }
            test(1) + ',' + test(2) + ',' + test(3)
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "one,two,other"),
        _ => panic!("expected 'one,two,other', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// While and do-while loops
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn while_loop_with_break() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let sum = 0;
            let i = 0;
            while (i < 10) {
                if (i === 5) break;
                sum += i;
                i++;
            }
            sum
        "#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        _ => panic!("expected 10, got {:?}", out),
    }
}

#[test]
fn while_loop_with_continue() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let sum = 0;
            let i = 0;
            while (i < 5) {
                i++;
                if (i === 3) continue;
                sum += i;
            }
            sum
        "#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 12),
        _ => panic!("expected 12, got {:?}", out),
    }
}

#[test]
fn do_while_with_break() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let iterations = 0;
            let i = 0;
            do {
                iterations++;
                if (iterations === 3) break;
                i++;
            } while (i < 10);
            iterations
        "#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        _ => panic!("expected 3, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Chain assignment
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_assignment() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let a, b, c;
            a = b = c = 5;
            "" + a + "," + b + "," + c
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "5,5,5"),
        _ => panic!("expected '5,5,5', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// For-loop multi-declaration and comma expression
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn for_multi_declaration() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let result = [];
            for (let i = 0, j = 10; i < 3; i++, j--) {
                result.push(i + ":" + j);
            }
            result.join(',')
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "0:10,1:9,2:8"),
        _ => panic!("expected '0:10,1:9,2:8', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Logical assignment on properties
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn logical_or_assign_on_property() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let obj = {a: null, b: 5};
            obj.a ||= 'new';
            obj.b ||= 'new';
            obj.a + ',' + obj.b
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "new,5"),
        _ => panic!("expected 'new,5', got {:?}", out),
    }
}

#[test]
fn nullish_assign_on_property() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let obj = {a: null, b: 0};
            obj.a ??= 'new';
            obj.b ??= 'new';
            obj.a + ',' + obj.b
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "new,0"),
        _ => panic!("expected 'new,0', got {:?}", out),
    }
}

#[test]
fn logical_and_assign_on_property() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let obj = {a: 5, b: 0};
            obj.a &&= 42;
            obj.b &&= 42;
            obj.a + ',' + obj.b
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "42,0"),
        _ => panic!("expected '42,0', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Array.isArray
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_isarray_true() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("Array.isArray([1, 2, 3])")
        .expect("eval");
    match out {
        Object::Boolean(b) => assert!(b),
        _ => panic!("expected true, got {:?}", out),
    }
}

#[test]
fn array_isarray_false() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"Array.isArray("hello")"#)
        .expect("eval");
    match out {
        Object::Boolean(b) => assert!(!b),
        _ => panic!("expected false, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Array.prototype.fill
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_fill_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[0, 0, 0, 0].fill(7).join(',')")
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "7,7,7,7"),
        _ => panic!("expected '7,7,7,7', got {:?}", out),
    }
}

#[test]
fn array_fill_with_range() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[0, 0, 0, 0].fill(7, 1, 3).join(',')")
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "0,7,7,0"),
        _ => panic!("expected '0,7,7,0', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// var hoisting: closures capture shared binding
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn var_closure_shared_binding() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let fns = [];
            for (var i = 0; i < 3; i++) {
                fns.push(() => i);
            }
            fns[0]() + ',' + fns[1]() + ',' + fns[2]()
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "3,3,3"),
        _ => panic!("expected '3,3,3', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Number.prototype.toString with radix
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn number_tostring_hex() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("(255).toString(16)").expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "ff"),
        _ => panic!("expected 'ff', got {:?}", out),
    }
}

#[test]
fn number_tostring_binary() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("(10).toString(2)").expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1010"),
        _ => panic!("expected '1010', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Standard library methods
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_includes_word() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""hello world".includes("world")"#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn string_starts_with() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""hello world".startsWith("hello")"#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn string_ends_with() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""hello world".endsWith("world")"#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn array_every_true() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[2, 4, 6].every(x => x % 2 === 0)")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn array_some_true() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[1, 3, 4].some(x => x % 2 === 0)")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn array_reduce_sum() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[1, 2, 3, 4].reduce((acc, x) => acc + x, 0)")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        _ => panic!("expected 10, got {:?}", out),
    }
}

#[test]
fn object_keys_join() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"Object.keys({a: 1, b: 2, c: 3}).join(',')"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "a,b,c"),
        _ => panic!("expected 'a,b,c', got {:?}", out),
    }
}

#[test]
fn json_stringify_parse() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"JSON.stringify({a: 1, b: "hello"})"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, r#"{"a":1,"b":"hello"}"#),
        _ => panic!("expected JSON string, got {:?}", out),
    }
}

#[test]
fn json_parse_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let obj = JSON.parse('{"a":1,"b":"hello"}'); obj.a + "," + obj.b"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1,hello"),
        _ => panic!("expected '1,hello', got {:?}", out),
    }
}

#[test]
fn math_max_min() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("Math.max(1, 5, 3) + ',' + Math.min(5, 1, 3)")
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "5,1"),
        _ => panic!("expected '5,1', got {:?}", out),
    }
}

#[test]
fn math_floor_ceil_abs() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("Math.floor(3.7) + ',' + Math.ceil(3.2) + ',' + Math.abs(-42)")
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "3,4,42"),
        _ => panic!("expected '3,4,42', got {:?}", out),
    }
}

#[test]
fn string_split_join() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""a,b,c".split(",").join("-")"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "a-b-c"),
        _ => panic!("expected 'a-b-c', got {:?}", out),
    }
}

#[test]
fn array_find_and_findindex() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[1, 2, 3, 4].find(x => x > 2) + ',' + [1, 2, 3, 4].findIndex(x => x > 2)")
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "3,2"),
        _ => panic!("expected '3,2', got {:?}", out),
    }
}

#[test]
fn array_reverse_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[1, 2, 3].reverse().join(',')")
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "3,2,1"),
        _ => panic!("expected '3,2,1', got {:?}", out),
    }
}

#[test]
fn array_sort_custom() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("[3, 1, 4, 1, 5].sort((a, b) => a - b).join(',')")
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1,1,3,4,5"),
        _ => panic!("expected '1,1,3,4,5', got {:?}", out),
    }
}

#[test]
fn string_trim_and_repeat() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#""  hello  ".trim() + "," + "abc".repeat(3)"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "hello,abcabcabc"),
        _ => panic!("expected 'hello,abcabcabc', got {:?}", out),
    }
}

#[test]
fn delete_operator_test() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let obj = {a: 1, b: 2}; delete obj.a; Object.keys(obj).join(',')"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "b"),
        _ => panic!("expected 'b', got {:?}", out),
    }
}

#[test]
fn in_operator_test() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"let obj = {a: 1, b: 2}; ("a" in obj) + "," + ("c" in obj)"#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "true,false"),
        _ => panic!("expected 'true,false', got {:?}", out),
    }
}

#[test]
fn instanceof_operator_test() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            class Foo {}
            let f = new Foo();
            f instanceof Foo
        "#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn for_of_array_sum() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let arr = [10, 20, 30];
            let sum = 0;
            for (let v of arr) { sum += v; }
            sum
        "#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 60),
        _ => panic!("expected 60, got {:?}", out),
    }
}

#[test]
fn map_basic_operations() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let m = new Map();
            m.set('a', 1);
            m.set('b', 2);
            m.get('a') + "," + m.size
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "1,2"),
        _ => panic!("expected '1,2', got {:?}", out),
    }
}

#[test]
fn set_basic_operations() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let s = new Set();
            s.add(1);
            s.add(2);
            s.add(1);
            s.size
        "#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        _ => panic!("expected 2, got {:?}", out),
    }
}

#[test]
fn regex_test_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"/hello/.test("hello world")"#)
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

// ═══════════════════════════════════════════════════════════════════════
// Nested function scope (3 levels)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nested_function_scope_3_levels() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            function outer() {
                let x = 10;
                function middle() {
                    let y = 20;
                    function inner() {
                        return x + y;
                    }
                    return inner();
                }
                return middle();
            }
            outer()
        "#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        _ => panic!("expected 30, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// IIFE pattern
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn iife_pattern() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("(function() { return 42; })()")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected 42, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Getter/setter in class
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn class_getter_setter() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            class Obj {
                constructor() { this._val = 0; }
                get val() { return this._val; }
                set val(v) { this._val = v * 2; }
            }
            let o = new Obj();
            o.val = 5;
            o.val
        "#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        _ => panic!("expected 10, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Block statement
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn block_statement_with_let() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let x = 'outer';
            {
                let y = 'inner';
                x = y;
            }
            x
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "inner"),
        _ => panic!("expected 'inner', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Super method calls
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn super_method_call_in_child() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            class Base {
                method() { return 'base'; }
            }
            class Child extends Base {
                method() { return super.method() + '+child'; }
            }
            new Child().method()
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "base+child"),
        _ => panic!("expected 'base+child', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Static methods
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn class_static_method_add() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            class Helper {
                static add(a, b) { return a + b; }
            }
            Helper.add(2, 3)
        "#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        _ => panic!("expected 5, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// String slice with negative indices
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_slice_negative() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#""hello".slice(-3)"#).expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "llo"),
        _ => panic!("expected 'llo', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Array splice
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_splice_basic() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let arr = [1, 2, 3, 4, 5];
            let removed = arr.splice(1, 2);
            arr.length + "," + removed.length
        "#)
        .expect("eval");
    match out {
        Object::String(s) => assert_eq!(&*s, "3,2"),
        _ => panic!("expected '3,2', got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Arrow returning object literal
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn arrow_returns_object_literal() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#"
            let fn = (x) => ({value: x * 2});
            fn(5).value
        "#)
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        _ => panic!("expected 10, got {:?}", out),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Probe-derived tests – comprehensive coverage
// ═══════════════════════════════════════════════════════════════════════

// Helper: eval and assert string result
fn assert_eval_str(code: &str, expected: &str) {
    let engine = FormLogicEngine::default();
    let out = engine.eval(code).expect("eval");
    match &out {
        Object::String(s) => assert_eq!(&**s, expected, "code: {code}"),
        _ => panic!("expected String({expected:?}), got {out:?}\ncode: {code}"),
    }
}

fn assert_eval_int(code: &str, expected: i64) {
    let engine = FormLogicEngine::default();
    let out = engine.eval(code).expect("eval");
    match &out {
        Object::Integer(v) => assert_eq!(*v, expected, "code: {code}"),
        _ => panic!("expected Integer({expected}), got {out:?}\ncode: {code}"),
    }
}

fn assert_eval_float(code: &str, expected: f64) {
    let engine = FormLogicEngine::default();
    let out = engine.eval(code).expect("eval");
    match &out {
        Object::Float(v) => {
            if expected.is_infinite() {
                assert_eq!(*v, expected, "code: {code}");
            } else if expected.is_nan() {
                assert!(v.is_nan(), "expected NaN, got {v}\ncode: {code}");
            } else {
                assert!(
                    (*v - expected).abs() < 0.001,
                    "expected Float({expected}), got Float({v})\ncode: {code}"
                );
            }
        }
        _ => panic!("expected Float({expected}), got {out:?}\ncode: {code}"),
    }
}

/// Accept either Integer or Float result, compare as f64
fn assert_eval_number(code: &str, expected: f64) {
    let engine = FormLogicEngine::default();
    let out = engine.eval(code).expect("eval");
    let actual = match &out {
        Object::Integer(v) => *v as f64,
        Object::Float(v) => *v,
        _ => panic!("expected number({expected}), got {out:?}\ncode: {code}"),
    };
    if expected.is_infinite() {
        assert_eq!(actual, expected, "code: {code}");
    } else if expected.is_nan() {
        assert!(actual.is_nan(), "expected NaN, got {actual}\ncode: {code}");
    } else {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}\ncode: {code}"
        );
    }
}

fn assert_eval_bool(code: &str, expected: bool) {
    let engine = FormLogicEngine::default();
    let out = engine.eval(code).expect("eval");
    match &out {
        Object::Boolean(v) => assert_eq!(*v, expected, "code: {code}"),
        _ => panic!("expected Boolean({expected}), got {out:?}\ncode: {code}"),
    }
}

fn assert_eval_undefined(code: &str) {
    let engine = FormLogicEngine::default();
    let out = engine.eval(code).expect("eval");
    assert!(
        matches!(out, Object::Undefined),
        "expected Undefined, got {out:?}\ncode: {code}"
    );
}

// ── Destructuring edge cases ──────────────────────────────────────────

#[test]
fn array_destr_skip() {
    assert_eval_str("let [,b,,d] = [1,2,3,4]; b + ',' + d", "2,4");
}

#[test]
fn array_destr_rest() {
    assert_eval_str("let [a, ...rest] = [1,2,3,4,5]; a + ',' + rest.length", "1,4");
}

#[test]
fn obj_destr_rename() {
    assert_eval_int("let {a: x, b: y} = {a: 10, b: 20}; x + y", 30);
}

#[test]
fn obj_destr_default() {
    assert_eval_str(
        "let {a = 1, b = 2, c = 3} = {b: 20}; a + ',' + b + ',' + c",
        "1,20,3",
    );
}

#[test]
fn nested_obj_destr() {
    assert_eval_int("let {a: {b}} = {a: {b: 42}}; b", 42);
}

// ── String methods ────────────────────────────────────────────────────

#[test]
fn string_char_at_2() {
    assert_eval_str(r#" "hello".charAt(1) "#, "e");
}

#[test]
fn string_char_code_at_2() {
    assert_eval_int(r#" "A".charCodeAt(0) "#, 65);
}

#[test]
fn string_index_of_probe() {
    assert_eval_int(r#" "hello world".indexOf("world") "#, 6);
}

#[test]
fn string_last_index_of_2() {
    assert_eval_int(r#" "abcabc".lastIndexOf("abc") "#, 3);
}

#[test]
fn string_to_upper_case_2() {
    assert_eval_str(r#" "hello".toUpperCase() "#, "HELLO");
}

#[test]
fn string_to_lower_case_2() {
    assert_eval_str(r#" "HELLO".toLowerCase() "#, "hello");
}

#[test]
fn string_match_basic() {
    assert_eval_str(
        r#"
        let m = "abc123def456".match(/[0-9]+/);
        m[0]
    "#,
        "123",
    );
}

#[test]
fn string_replace_basic() {
    assert_eval_str(
        r#" "hello world".replace("world", "earth") "#,
        "hello earth",
    );
}

#[test]
fn string_replace_all_basic() {
    assert_eval_str(r#" "aabaa".replaceAll("a", "x") "#, "xxbxx");
}

#[test]
fn string_trim_start_2() {
    assert_eval_str(r#" "  hello".trimStart() "#, "hello");
}

#[test]
fn string_trim_end_2() {
    assert_eval_str(r#" "hello  ".trimEnd() "#, "hello");
}

#[test]
fn string_from_code_point_2() {
    assert_eval_str(r#" String.fromCodePoint(65, 66, 67) "#, "ABC");
}

// ── Number methods ────────────────────────────────────────────────────

#[test]
fn number_is_finite_2() {
    assert_eval_str(
        "Number.isFinite(42) + ',' + Number.isFinite(Infinity)",
        "true,false",
    );
}

#[test]
fn number_is_nan_2() {
    assert_eval_str("Number.isNaN(NaN) + ',' + Number.isNaN(42)", "true,false");
}

#[test]
fn number_is_integer() {
    assert_eval_str(
        "Number.isInteger(42) + ',' + Number.isInteger(42.5)",
        "true,false",
    );
}

#[test]
fn number_parse_int_method() {
    assert_eval_int(r#" Number.parseInt("42abc") "#, 42);
}

#[test]
fn number_parse_float_method() {
    assert_eval_float(r#" Number.parseFloat("3.14xyz") "#, 3.14);
}

#[test]
fn parse_int_global_2() {
    assert_eval_int(r#" parseInt("42") "#, 42);
}

#[test]
fn parse_float_global_2() {
    assert_eval_float(r#" parseFloat("3.14") "#, 3.14);
}

// ── Math methods ──────────────────────────────────────────────────────

#[test]
fn math_round_2() {
    assert_eval_str("Math.round(3.5) + ',' + Math.round(3.4)", "4,3");
}

#[test]
fn math_sqrt_2() {
    assert_eval_int("Math.sqrt(144)", 12);
}

#[test]
fn math_pow_2() {
    assert_eval_int("Math.pow(2, 10)", 1024);
}

#[test]
fn math_log() {
    assert_eval_int("Math.round(Math.log(Math.E) * 100) / 100", 1);
}

#[test]
fn math_random_range() {
    assert_eval_bool("let r = Math.random(); r >= 0 && r < 1", true);
}

#[test]
fn math_trunc() {
    assert_eval_str("Math.trunc(3.7) + ',' + Math.trunc(-3.7)", "3,-3");
}

#[test]
fn math_sign_probe() {
    assert_eval_str(
        "Math.sign(5) + ',' + Math.sign(-3) + ',' + Math.sign(0)",
        "1,-1,0",
    );
}

#[test]
fn math_sign_zero() {
    assert_eval_int("Math.sign(0)", 0);
}

#[test]
fn math_sign_neg_zero() {
    assert_eval_int("Math.sign(-0)", 0);
}

#[test]
fn math_pi() {
    assert_eval_int("Math.round(Math.PI * 1000)", 3142);
}

// ── Array methods ─────────────────────────────────────────────────────

#[test]
fn array_flat_basic() {
    assert_eval_str("[1, [2, 3], [4, [5]]].flat().join(',')", "1,2,3,4,5");
}

#[test]
fn array_flat_map_2() {
    assert_eval_str("[1,2,3].flatMap(x => [x, x*2]).join(',')", "1,2,2,4,3,6");
}

#[test]
fn array_from_string_2() {
    assert_eval_str(r#" Array.from("abc").join(',') "#, "a,b,c");
}

#[test]
fn array_index_of_basic() {
    assert_eval_int("[10,20,30,40].indexOf(30)", 2);
}

#[test]
fn array_last_index_of_probe() {
    assert_eval_int("[1,2,3,2,1].lastIndexOf(2)", 3);
}

#[test]
fn array_concat_probe() {
    assert_eval_str("[1,2].concat([3,4],[5]).join(',')", "1,2,3,4,5");
}

#[test]
fn array_slice_probe() {
    assert_eval_str("[1,2,3,4,5].slice(1,3).join(',')", "2,3");
}

#[test]
fn array_map_index() {
    assert_eval_str("[10,20,30].map((v,i) => v+i).join(',')", "10,21,32");
}

#[test]
fn array_filter_index() {
    assert_eval_str(
        "[1,2,3,4,5].filter((v,i) => i % 2 === 0).join(',')",
        "1,3,5",
    );
}

#[test]
fn array_reduce_right() {
    assert_eval_int("[1,2,3,4].reduce((a,b) => a - b, 10)", 0);
}

#[test]
fn array_every_false() {
    assert_eval_bool("[2,4,5,6].every(x => x % 2 === 0)", false);
}

#[test]
fn array_some_false() {
    assert_eval_bool("[1,3,5].some(x => x % 2 === 0)", false);
}

#[test]
fn array_includes_basic() {
    assert_eval_str(
        "[1,2,3].includes(2) + ',' + [1,2,3].includes(4)",
        "true,false",
    );
}

#[test]
fn array_sort_default() {
    assert_eval_str("[3,1,4,1,5].sort().join(',')", "1,1,3,4,5");
}

#[test]
fn array_to_sorted() {
    assert_eval_str(
        r#"
        let a = [3,1,2];
        let b = a.toSorted();
        a.join(',') + '|' + b.join(',')
    "#,
        "3,1,2|1,2,3",
    );
}

#[test]
fn array_with_method() {
    assert_eval_str("[1,2,3].with(1, 99).join(',')", "1,99,3");
}

#[test]
fn array_keys_iter() {
    assert_eval_str(
        r#"
        let result = [];
        for (let k of [10,20,30].keys()) { result.push(k); }
        result.join(',')
    "#,
        "0,1,2",
    );
}

#[test]
fn array_values_iter() {
    assert_eval_str(
        r#"
        let result = [];
        for (let v of [10,20,30].values()) { result.push(v); }
        result.join(',')
    "#,
        "10,20,30",
    );
}

#[test]
fn array_entries_iter() {
    assert_eval_str(
        r#"
        let result = [];
        for (let [k,v] of [10,20,30].entries()) { result.push(k+':'+v); }
        result.join(',')
    "#,
        "0:10,1:20,2:30",
    );
}

// ── Object methods ────────────────────────────────────────────────────

#[test]
fn object_values_probe() {
    assert_eval_str("Object.values({a:1,b:2,c:3}).join(',')", "1,2,3");
}

#[test]
fn object_entries_probe() {
    assert_eval_str(
        "Object.entries({a:1}).map(e=>e[0]+':'+e[1]).join(',')",
        "a:1",
    );
}

#[test]
fn object_assign_basic() {
    assert_eval_str(
        r#"
        let t = {a:1};
        Object.assign(t, {b:2}, {c:3});
        t.a + ',' + t.b + ',' + t.c
    "#,
        "1,2,3",
    );
}

// ── Class features ────────────────────────────────────────────────────

#[test]
fn class_field_init_increment() {
    assert_eval_int(
        r#"
        class Counter { count = 0; inc() { this.count++; } }
        let c = new Counter(); c.inc(); c.inc(); c.count
    "#,
        2,
    );
}

#[test]
fn class_field_init_fn() {
    assert_eval_int(
        r#"
        class Counter { constructor() { this.count = 0; } inc() { this.count++; return this; } }
        let c = new Counter(); c.inc(); c.inc(); c.count
    "#,
        2,
    );
}

#[test]
fn class_field_assign() {
    assert_eval_int(
        r#"
        class Counter { constructor() { this.count = 0; } inc() { this.count = this.count + 1; } }
        let c = new Counter(); c.inc(); c.inc(); c.count
    "#,
        2,
    );
}

#[test]
fn class_field_read() {
    assert_eval_int(
        r#"
        class C { constructor() { this.x = 10; } getX() { return this.x; } }
        let c = new C(); c.getX()
    "#,
        10,
    );
}

#[test]
fn class_constructor_return() {
    assert_eval_int(
        r#"
        class Foo { constructor() { this.x = 10; } }
        let f = new Foo(); f.x
    "#,
        10,
    );
}

// ── Generator advanced ────────────────────────────────────────────────

#[test]
fn generator_return_method() {
    assert_eval_str(
        r#"
        function* gen() { yield 1; yield 2; yield 3; }
        let g = gen();
        let a = g.next().value;
        let b = g.return(99);
        let c = g.next();
        a + ',' + b.value + ',' + b.done + ',' + c.done
    "#,
        "1,99,true,true",
    );
}

#[test]
fn generator_next_value() {
    assert_eval_int(
        r#"
        function* gen() { let x = yield 1; yield x + 10; }
        let g = gen();
        g.next();
        g.next(5).value
    "#,
        15,
    );
}

// ── Scope and closures ────────────────────────────────────────────────

#[test]
fn closure_over_let_in_if() {
    assert_eval_int(
        r#"
        let fn;
        if (true) { let x = 42; fn = () => x; }
        fn()
    "#,
        42,
    );
}

#[test]
fn closure_mutual_recursion() {
    assert_eval_str(
        r#"
        function isEven(n) { return n === 0 ? true : isOdd(n - 1); }
        function isOdd(n) { return n === 0 ? false : isEven(n - 1); }
        isEven(10) + ',' + isOdd(7)
    "#,
        "true,true",
    );
}

#[test]
fn immediately_invoked_arrow() {
    assert_eval_int("let result = ((x) => x * x)(7); result", 49);
}

// ── Error handling ────────────────────────────────────────────────────

#[test]
fn try_catch_error_message_2() {
    assert_eval_str(
        r#" try { throw new Error("boom"); } catch(e) { e.message } "#,
        "boom",
    );
}

#[test]
fn try_catch_string_throw() {
    assert_eval_str(
        r#" try { throw "oops"; } catch(e) { e } "#,
        "oops",
    );
}

#[test]
fn try_catch_error_msg_fn() {
    assert_eval_str(
        r#"
        function test() { try { throw new Error("boom"); } catch(e) { return e.message; } }
        test()
    "#,
        "boom",
    );
}

#[test]
fn try_catch_string_fn() {
    assert_eval_str(
        r#"
        function test() { try { throw "oops"; } catch(e) { return e; } }
        test()
    "#,
        "oops",
    );
}

#[test]
fn error_name_access() {
    assert_eval_str(
        r#" try { throw new Error("test"); } catch(e) { e.name } "#,
        "Error",
    );
}

#[test]
fn error_message_access() {
    assert_eval_str(
        r#" try { throw new Error("hello"); } catch(e) { e.message } "#,
        "hello",
    );
}

// ── Ternary and logical ───────────────────────────────────────────────

#[test]
fn short_circuit_and() {
    assert_eval_int(
        r#"
        let calls = 0;
        function inc() { calls++; return true; }
        false && inc();
        calls
    "#,
        0,
    );
}

#[test]
fn short_circuit_or() {
    assert_eval_int(
        r#"
        let calls = 0;
        function inc() { calls++; return true; }
        true || inc();
        calls
    "#,
        0,
    );
}

#[test]
fn nullish_chain() {
    assert_eval_str(
        r#"
        let a = null;
        let b = undefined;
        let c = 0;
        (a ?? 'A') + ',' + (b ?? 'B') + ',' + (c ?? 'C')
    "#,
        "A,B,0",
    );
}

// ── Template literal edge cases ───────────────────────────────────────

#[test]
fn template_expression() {
    assert_eval_str("let x = 5; `value is ${x * 2}`", "value is 10");
}

#[test]
fn template_nested() {
    assert_eval_str(
        "let a = 'hello'; `${a.toUpperCase()} ${'world'.length}`",
        "HELLO 5",
    );
}

// ── Labeled statements ────────────────────────────────────────────────

#[test]
fn labeled_break_nested() {
    assert_eval_int(
        r#"
        let result = 0;
        outer: for (let i = 0; i < 5; i++) {
            for (let j = 0; j < 5; j++) {
                if (i === 2 && j === 3) break outer;
                result++;
            }
        }
        result
    "#,
        13,
    );
}

#[test]
fn labeled_continue_probe() {
    assert_eval_int(
        r#"
        let result = 0;
        outer: for (let i = 0; i < 3; i++) {
            for (let j = 0; j < 3; j++) {
                if (j === 1) continue outer;
                result++;
            }
        }
        result
    "#,
        3,
    );
}

// ── Misc operators ────────────────────────────────────────────────────

#[test]
fn exponent_operator() {
    assert_eval_int("2 ** 10", 1024);
}

#[test]
fn unsigned_right_shift() {
    assert_eval_float("(-1 >>> 0)", 4294967295.0);
}

#[test]
fn comma_in_parens() {
    assert_eval_int("(1, 2, 3)", 3);
}

#[test]
fn void_in_expr() {
    assert_eval_str("typeof void 0", "undefined");
}

#[test]
fn delete_returns_true() {
    assert_eval_bool("let o = {a:1}; delete o.a", true);
}

#[test]
fn in_operator_array() {
    assert_eval_bool("1 in [10, 20, 30]", true);
}

// ── for-in ────────────────────────────────────────────────────────────

#[test]
fn for_in_object() {
    assert_eval_str(
        r#"
        let result = [];
        for (let k in {a:1, b:2, c:3}) { result.push(k); }
        result.join(',')
    "#,
        "a,b,c",
    );
}

// ── Spread ────────────────────────────────────────────────────────────

#[test]
fn spread_into_array() {
    assert_eval_str(
        r#"
        let a = [1,2,3];
        let b = [...a, 4, 5];
        b.join(',')
    "#,
        "1,2,3,4,5",
    );
}

#[test]
fn spread_merge_objects() {
    assert_eval_str(
        r#"
        let a = {x:1, y:2};
        let b = {...a, z:3};
        b.x + ',' + b.y + ',' + b.z
    "#,
        "1,2,3",
    );
}

// ── Map/Set ───────────────────────────────────────────────────────────

#[test]
fn map_has_delete() {
    assert_eval_str(
        r#"
        let m = new Map();
        m.set('a', 1);
        let has = m.has('a');
        m.delete('a');
        has + ',' + m.has('a') + ',' + m.size
    "#,
        "true,false,0",
    );
}

#[test]
fn set_for_of() {
    assert_eval_str(
        r#"
        let s = new Set();
        s.add(10); s.add(20); s.add(30);
        let result = [];
        for (let v of s) { result.push(v); }
        result.join(',')
    "#,
        "10,20,30",
    );
}

#[test]
fn map_for_of() {
    assert_eval_str(
        r#"
        let m = new Map();
        m.set('a', 1); m.set('b', 2);
        let result = [];
        for (let [k,v] of m) { result.push(k + ':' + v); }
        result.join(',')
    "#,
        "a:1,b:2",
    );
}

// ── typeof ────────────────────────────────────────────────────────────

#[test]
fn typeof_checks() {
    assert_eval_str(
        r#" typeof undefined + ',' + typeof null + ',' + typeof 42 + ',' + typeof true + ',' + typeof "hi" "#,
        "undefined,object,number,boolean,string",
    );
}

// ── Computed property names ───────────────────────────────────────────

#[test]
fn computed_prop_method() {
    assert_eval_str(
        r#"
        let key = 'greet';
        let obj = { [key]() { return 'hi'; } };
        obj.greet()
    "#,
        "hi",
    );
}

// ── Optional chaining ─────────────────────────────────────────────────

#[test]
fn optional_method_call() {
    assert_eval_str(
        r#"
        let obj = {foo: () => 42};
        let a = obj.foo?.();
        let b = obj.bar?.();
        a + ',' + b
    "#,
        "42,undefined",
    );
}

#[test]
fn optional_call_undef() {
    assert_eval_undefined("let f = undefined; f?.()");
}

#[test]
fn optional_method_undef() {
    assert_eval_undefined(
        r#"
        let obj = {foo: () => 42};
        obj.bar?.()
    "#,
    );
}

// ── Async basic ───────────────────────────────────────────────────────

#[test]
fn async_basic() {
    assert_eval_str(
        r#"
        async function f() { return 42; }
        let p = f();
        typeof p
    "#,
        "object",
    );
}

// ── For-of destructuring ──────────────────────────────────────────────

#[test]
fn for_of_destr_entries() {
    assert_eval_str(
        r#"
        let result = [];
        let m = new Map();
        m.set('x', 10); m.set('y', 20);
        for (let [k, v] of m) { result.push(k + '=' + v); }
        result.join(',')
    "#,
        "x=10,y=20",
    );
}

// ── Property access ───────────────────────────────────────────────────

#[test]
fn bracket_string_access() {
    assert_eval_int(
        r#"
        let obj = {hello: 42};
        let key = 'hel' + 'lo';
        obj[key]
    "#,
        42,
    );
}

#[test]
fn nested_property_access() {
    assert_eval_int("let o = {a: {b: {c: 99}}}; o.a.b.c", 99);
}

#[test]
fn array_length_after_push() {
    assert_eval_int("let a = [1, 2]; a.push(3); a.push(4); a.length", 4);
}

// ── Number.toFixed ────────────────────────────────────────────────────

#[test]
fn number_to_fixed() {
    assert_eval_str("(3.14159).toFixed(2)", "3.14");
}

// ── Chained method calls ──────────────────────────────────────────────

#[test]
fn chained_array_methods() {
    assert_eval_str(
        "[5,3,8,1,9,2].filter(x => x > 3).sort((a,b) => a-b).map(x => x*10).join(',')",
        "50,80,90",
    );
}

// ── toString coercion ─────────────────────────────────────────────────

#[test]
fn class_to_string_coercion() {
    assert_eval_str(
        r#"
        class Animal {
            constructor(name) { this.name = name; }
            toString() { return 'Animal:' + this.name; }
        }
        '' + new Animal('cat')
    "#,
        "Animal:cat",
    );
}

#[test]
fn class_to_string_template() {
    assert_eval_str(
        r#"
        class Point {
            constructor(x, y) { this.x = x; this.y = y; }
            toString() { return '(' + this.x + ',' + this.y + ')'; }
        }
        let p = new Point(3, 4);
        'Point: ' + p
    "#,
        "Point: (3,4)",
    );
}

// ── Prefix/postfix increment on properties ────────────────────────────

#[test]
fn prefix_increment_property() {
    assert_eval_int(
        r#"
        let obj = {x: 5};
        let a = ++obj.x;
        a + obj.x
    "#,
        12,
    );
}

#[test]
fn postfix_increment_property() {
    assert_eval_int(
        r#"
        let obj = {x: 5};
        let a = obj.x++;
        a + obj.x
    "#,
        11,
    );
}

// ── Ternary and nested ternary ────────────────────────────────────────

#[test]
fn nested_ternary_grade() {
    assert_eval_str(
        r#"
        function grade(n) { return n >= 90 ? 'A' : n >= 80 ? 'B' : n >= 70 ? 'C' : 'F'; }
        grade(95) + ',' + grade(85) + ',' + grade(65)
    "#,
        "A,B,F",
    );
}

// ── Switch with multiple cases falling through ────────────────────────

#[test]
fn switch_multiple_cases() {
    assert_eval_str(
        r#"
        function test(x) {
            switch(x) {
                case 1: case 2: return 'low';
                case 3: return 'mid';
                default: return 'high';
            }
        }
        test(1) + ',' + test(2) + ',' + test(3) + ',' + test(5)
    "#,
        "low,low,mid,high",
    );
}

// ── Array destructuring with default values ───────────────────────────

#[test]
fn array_destr_defaults() {
    assert_eval_str(
        "let [a = 1, b = 2, c = 3] = [10]; a + ',' + b + ',' + c",
        "10,2,3",
    );
}

// ── for-of with array ─────────────────────────────────────────────────

#[test]
fn for_of_array_basic() {
    assert_eval_str(
        r#"
        let result = [];
        for (let x of [10, 20, 30]) { result.push(x * 2); }
        result.join(',')
    "#,
        "20,40,60",
    );
}

// ── String template with method calls ─────────────────────────────────

#[test]
fn template_method_call() {
    assert_eval_str(
        r#" `${"hello".toUpperCase()} ${"WORLD".toLowerCase()}` "#,
        "HELLO world",
    );
}

// ── Logical OR assignment ─────────────────────────────────────────────

#[test]
fn logical_or_assign() {
    assert_eval_str(
        r#"
        let a = 0;
        let b = 'hello';
        a ||= 42;
        b ||= 'world';
        a + ',' + b
    "#,
        "42,hello",
    );
}

// ── Nullish coalescing assignment ─────────────────────────────────────

#[test]
fn nullish_assign() {
    assert_eval_str(
        r#"
        let a = null;
        let b = 0;
        a ??= 42;
        b ??= 99;
        a + ',' + b
    "#,
        "42,0",
    );
}

// ── WeakRef-like (Object.is) ──────────────────────────────────────────

#[test]
fn object_is() {
    assert_eval_str(
        "Object.is(NaN, NaN) + ',' + Object.is(0, -0) + ',' + Object.is(1, 1)",
        "true,false,true",
    );
}

// ── Array.from with mapping ───────────────────────────────────────────

#[test]
fn array_from_with_map_2() {
    assert_eval_str(
        "Array.from([1,2,3], x => x * 2).join(',')",
        "2,4,6",
    );
}

// ── Object.freeze ─────────────────────────────────────────────────────

#[test]
fn object_freeze() {
    assert_eval_int(
        r#"
        let o = {a: 1};
        Object.freeze(o);
        o.a = 99;
        o.a
    "#,
        1,
    );
}

#[test]
fn object_freeze_returns_object() {
    assert_eval_str(
        r#"
        let o = {x: 'hello'};
        let f = Object.freeze(o);
        f.x
    "#,
        "hello",
    );
}

// ── String.search ─────────────────────────────────────────────────────

#[test]
fn string_search_found() {
    assert_eval_int(r#" "hello123world".search(/[0-9]+/) "#, 5);
}

#[test]
fn string_search_not_found() {
    assert_eval_int(r#" "hello".search(/[0-9]+/) "#, -1);
}

// ── RegExp.exec ───────────────────────────────────────────────────────

#[test]
fn regexp_exec_basic() {
    assert_eval_str(
        r#"
        let m = /(\d+)/.exec("abc123def");
        m[0] + ',' + m[1]
    "#,
        "123,123",
    );
}

#[test]
fn regexp_exec_no_match() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(r#" /(\d+)/.exec("abcdef") "#)
        .expect("eval");
    assert!(matches!(out, Object::Null), "expected Null, got {out:?}");
}

// ── Multi-level class inheritance ─────────────────────────────────────

#[test]
fn class_three_level_inheritance() {
    assert_eval_int(
        r#"
        class A { constructor() { this.a = 1; } }
        class B extends A { constructor() { super(); this.b = 2; } }
        class C extends B { constructor() { super(); this.c = 3; } }
        let c = new C();
        c.a + c.b + c.c
    "#,
        6,
    );
}

#[test]
fn class_three_level_inheritance_methods() {
    assert_eval_str(
        r#"
        class A { greet() { return "A"; } }
        class B extends A { greet() { return "B+" + super.greet(); } }
        class C extends B { greet() { return "C+" + super.greet(); } }
        let c = new C();
        c.greet()
    "#,
        "C+B+A",
    );
}

#[test]
fn class_three_level_constructor_with_args() {
    assert_eval_str(
        r#"
        class Base { constructor(x) { this.x = x; } }
        class Mid extends Base { constructor(x, y) { super(x); this.y = y; } }
        class Top extends Mid { constructor(x, y, z) { super(x, y); this.z = z; } }
        let t = new Top("a", "b", "c");
        t.x + t.y + t.z
    "#,
        "abc",
    );
}

#[test]
fn class_four_level_inheritance() {
    assert_eval_int(
        r#"
        class A { constructor() { this.v = 1; } }
        class B extends A { constructor() { super(); this.v = this.v + 10; } }
        class C extends B { constructor() { super(); this.v = this.v + 100; } }
        class D extends C { constructor() { super(); this.v = this.v + 1000; } }
        let d = new D();
        d.v
    "#,
        1111,
    );
}

#[test]
fn class_super_method_after_super_constructor() {
    assert_eval_str(
        r#"
        class Animal {
            constructor(name) { this.name = name; }
            speak() { return "sound"; }
        }
        class Dog extends Animal {
            constructor(name) { super(name); }
            speak() { return super.speak() + " bark"; }
        }
        let d = new Dog("Rex");
        d.name + ": " + d.speak()
    "#,
        "Rex: sound bark",
    );
}

#[test]
fn class_instanceof_check() {
    assert_eval_bool(
        r#"
        class A {}
        class B extends A {}
        let b = new B();
        b instanceof B
    "#,
        true,
    );
}

#[test]
fn class_instanceof_parent() {
    assert_eval_bool(
        r#"
        class A {}
        class B extends A {}
        let b = new B();
        b instanceof A
    "#,
        true,
    );
}

// ── Nested try-catch-finally ─────────────────────────────────────────

#[test]
fn nested_try_catch_finally() {
    assert_eval_str(
        r#"
        let result = "";
        try {
            try {
                throw new Error("inner");
            } catch (e) {
                result = result + "caught,";
            } finally {
                result = result + "finally,";
            }
            result = result + "after";
        } catch (e) {
            result = result + "outer";
        }
        result
    "#,
        "caught,finally,after",
    );
}

#[test]
fn try_catch_finally_return() {
    assert_eval_str(
        r#"
        function f() {
            try {
                return "try";
            } finally {
                // finally runs but doesn't override
            }
        }
        f()
    "#,
        "try",
    );
}

#[test]
fn try_catch_rethrow() {
    assert_eval_str(
        r#"
        let result = "";
        try {
            try {
                throw new Error("e1");
            } catch (e) {
                result = result + "inner,";
                throw new Error("e2");
            }
        } catch (e) {
            result = result + "outer";
        }
        result
    "#,
        "inner,outer",
    );
}

// ── While / Do-While / For ───────────────────────────────────────────

#[test]
fn while_loop_basic() {
    assert_eval_int(
        r#"
        let i = 0;
        let sum = 0;
        while (i < 5) {
            sum = sum + i;
            i = i + 1;
        }
        sum
    "#,
        10,
    );
}

#[test]
fn do_while_loop_v1() {
    assert_eval_int(
        r#"
        let i = 0;
        do {
            i = i + 1;
        } while (i < 5);
        i
    "#,
        5,
    );
}

#[test]
fn for_loop_break_continue() {
    assert_eval_int(
        r#"
        let sum = 0;
        for (let i = 0; i < 10; i++) {
            if (i === 3) continue;
            if (i === 7) break;
            sum = sum + i;
        }
        sum
    "#,
        18,
    );
}

#[test]
fn for_loop_multi_var() {
    assert_eval_int(
        r#"
        let result = 0;
        for (let i = 0, j = 10; i < 5; i++, j--) {
            result = result + i + j;
        }
        result
    "#,
        50,
    );
}

// ── Closures ─────────────────────────────────────────────────────────

#[test]
fn closure_counter() {
    assert_eval_int(
        r#"
        function makeCounter() {
            let count = 0;
            return function() {
                count = count + 1;
                return count;
            };
        }
        let c = makeCounter();
        c(); c(); c()
    "#,
        3,
    );
}

#[test]
fn closure_captures_outer() {
    assert_eval_int(
        r#"
        function outer(x) {
            return function(y) {
                return x + y;
            };
        }
        let add5 = outer(5);
        add5(3) + add5(7)
    "#,
        20,
    );
}

#[test]
fn iife_closure() {
    assert_eval_int(
        r#"
        let result = (function() {
            let x = 42;
            return x;
        })();
        result
    "#,
        42,
    );
}

// ── Scope / Hoisting ────────────────────────────────────────────────

#[test]
fn var_hoisting() {
    assert_eval_undefined(
        r#"
        function f() { return x; var x = 5; }
        f()
    "#,
    );
}

#[test]
fn function_hoisting_v1() {
    assert_eval_int(
        r#"
        function test() {
            return f();
            function f() { return 42; }
        }
        test()
    "#,
        42,
    );
}

// ── String methods (additional) ──────────────────────────────────────

#[test]
fn string_repeat_v1() {
    assert_eval_str(r#" "ha".repeat(3) "#, "hahaha");
}

#[test]
fn string_starts_with_v1() {
    assert_eval_bool(r#" "hello world".startsWith("hello") "#, true);
}

#[test]
fn string_ends_with_v1() {
    assert_eval_bool(r#" "hello world".endsWith("world") "#, true);
}

#[test]
fn string_pad_start_v1() {
    assert_eval_str(r#" "5".padStart(3, "0") "#, "005");
}

#[test]
fn string_pad_end_v1() {
    assert_eval_str(r#" "5".padEnd(3, "0") "#, "500");
}

#[test]
fn string_concat() {
    assert_eval_str(r#" "hello".concat(" ", "world") "#, "hello world");
}

// ── Array methods (additional) ───────────────────────────────────────

#[test]
fn array_flat() {
    assert_eval_str(
        r#" [1, [2, 3], [4]].flat().join(",") "#,
        "1,2,3,4",
    );
}

#[test]
fn array_flat_map_v1() {
    assert_eval_str(
        r#" [1, 2, 3].flatMap(function(x) { return [x, x * 2]; }).join(",") "#,
        "1,2,2,4,3,6",
    );
}

#[test]
fn array_reduce_right_v1() {
    assert_eval_str(
        r#" ["a", "b", "c"].reduceRight(function(acc, x) { return acc + x; }, "") "#,
        "cba",
    );
}

#[test]
fn array_fill_range() {
    assert_eval_str(
        r#" [1, 2, 3, 4].fill(0, 1, 3).join(",") "#,
        "1,0,0,4",
    );
}

#[test]
fn array_copy_within() {
    assert_eval_str(
        r#" [1, 2, 3, 4, 5].copyWithin(0, 3).join(",") "#,
        "4,5,3,4,5",
    );
}

#[test]
fn array_entries_keys_values() {
    assert_eval_str(
        r#" Array.from([10, 20, 30].keys()).join(",") "#,
        "0,1,2",
    );
}

// ── Object methods (additional) ──────────────────────────────────────

#[test]
fn object_keys() {
    assert_eval_str(
        r#" Object.keys({"a": 1, "b": 2, "c": 3}).join(",") "#,
        "a,b,c",
    );
}

#[test]
fn object_values() {
    assert_eval_str(
        r#" Object.values({"a": 1, "b": 2, "c": 3}).join(",") "#,
        "1,2,3",
    );
}

#[test]
fn object_entries() {
    assert_eval_int(
        r#" Object.entries({"a": 1, "b": 2}).length "#,
        2,
    );
}

#[test]
fn object_assign_multiple() {
    assert_eval_int(
        r#"
        let target = {"a": 1};
        Object.assign(target, {"b": 2}, {"c": 3});
        target.a + target.b + target.c
    "#,
        6,
    );
}

// ── Conditional / Ternary ────────────────────────────────────────────

#[test]
fn ternary_nested_deep() {
    assert_eval_str(
        r#"
        let x = 2;
        x === 1 ? "one" : x === 2 ? "two" : x === 3 ? "three" : "other"
    "#,
        "two",
    );
}

// ── Nullish coalescing / Optional chaining ──────────────────────────

#[test]
fn nullish_coalescing_zero_v1() {
    assert_eval_int(r#" 0 ?? 42 "#, 0);
}

#[test]
fn nullish_coalescing_empty_string() {
    assert_eval_str(r#" "" ?? "default" "#, "");
}

#[test]
fn optional_chaining_nested() {
    assert_eval_undefined(
        r#"
        let obj = {"a": {"b": null}};
        obj.a.b?.c?.d
    "#,
    );
}

#[test]
fn optional_chaining_with_call() {
    assert_eval_undefined(
        r#"
        let obj = {};
        obj.foo?.()
    "#,
    );
}

// ── Typeof ───────────────────────────────────────────────────────────

#[test]
fn typeof_number_v1() {
    assert_eval_str(r#" typeof 42 "#, "number");
}

#[test]
fn typeof_string_v1() {
    assert_eval_str(r#" typeof "hello" "#, "string");
}

#[test]
fn typeof_boolean_v1() {
    assert_eval_str(r#" typeof true "#, "boolean");
}

#[test]
fn typeof_undefined_v1() {
    assert_eval_str(r#" typeof undefined "#, "undefined");
}

#[test]
fn typeof_null_v1() {
    assert_eval_str(r#" typeof null "#, "object");
}

#[test]
fn typeof_object_v1() {
    assert_eval_str(r#" typeof {} "#, "object");
}

#[test]
fn typeof_array_v1() {
    assert_eval_str(r#" typeof [] "#, "object");
}

// ── Destructuring (additional) ───────────────────────────────────────

#[test]
fn destructuring_rest_array() {
    assert_eval_str(
        r#"
        let [first, ...rest] = [1, 2, 3, 4];
        rest.join(",")
    "#,
        "2,3,4",
    );
}

#[test]
fn destructuring_nested_object_v1() {
    assert_eval_int(
        r#"
        let {a: {b: c}} = {a: {b: 42}};
        c
    "#,
        42,
    );
}

#[test]
fn destructuring_in_function_params() {
    assert_eval_int(
        r#"
        function sum({a, b}) { return a + b; }
        sum({a: 10, b: 20})
    "#,
        30,
    );
}

// ── Switch ───────────────────────────────────────────────────────────

#[test]
fn switch_fallthrough() {
    assert_eval_str(
        r#"
        let result = "";
        switch (2) {
            case 1: result = result + "one"; break;
            case 2: result = result + "two";
            case 3: result = result + "three"; break;
            case 4: result = result + "four"; break;
        }
        result
    "#,
        "twothree",
    );
}

#[test]
fn switch_default_v1() {
    assert_eval_str(
        r#"
        let x = 99;
        let result;
        switch (x) {
            case 1: result = "one"; break;
            case 2: result = "two"; break;
            default: result = "other"; break;
        }
        result
    "#,
        "other",
    );
}

// ── Regex ────────────────────────────────────────────────────────────

#[test]
fn regex_test() {
    assert_eval_bool(r#" /\d+/.test("abc123") "#, true);
}

#[test]
fn regex_test_no_match() {
    assert_eval_bool(r#" /\d+/.test("abcdef") "#, false);
}

#[test]
fn string_match_regex() {
    assert_eval_str(
        r#"
        let m = "hello123world".match(/(\d+)/);
        m[1]
    "#,
        "123",
    );
}

#[test]
fn string_replace_regex() {
    assert_eval_str(
        r#" "hello world".replace(/world/, "rust") "#,
        "hello rust",
    );
}

// ── Generators ───────────────────────────────────────────────────────

#[test]
fn generator_basic_v1() {
    assert_eval_str(
        r#"
        function* gen() {
            yield 1;
            yield 2;
            yield 3;
        }
        let g = gen();
        let r = "";
        r = r + g.next().value;
        r = r + g.next().value;
        r = r + g.next().value;
        r
    "#,
        "123",
    );
}

#[test]
fn generator_done_flag() {
    assert_eval_bool(
        r#"
        function* gen() { yield 1; }
        let g = gen();
        g.next();
        g.next().done
    "#,
        true,
    );
}

// ── Property access patterns ────────────────────────────────────────

#[test]
fn computed_property_access() {
    assert_eval_int(
        r#"
        let obj = {"a": 1, "b": 2};
        let key = "b";
        obj[key]
    "#,
        2,
    );
}

#[test]
fn dynamic_property_set() {
    assert_eval_int(
        r#"
        let obj = {};
        let key = "x";
        obj[key] = 42;
        obj.x
    "#,
        42,
    );
}

#[test]
fn object_shorthand_property() {
    assert_eval_int(
        r#"
        let x = 10;
        let y = 20;
        let obj = {x, y};
        obj.x + obj.y
    "#,
        30,
    );
}

#[test]
fn object_computed_property_name() {
    assert_eval_int(
        r#"
        let key = "foo";
        let obj = {[key]: 42};
        obj.foo
    "#,
        42,
    );
}

// ── Arrow functions ──────────────────────────────────────────────────

#[test]
fn arrow_function_expression_body() {
    assert_eval_int(
        r#"
        let add = (a, b) => a + b;
        add(3, 4)
    "#,
        7,
    );
}

#[test]
fn arrow_function_block_body() {
    assert_eval_int(
        r#"
        let mul = (a, b) => { return a * b; };
        mul(3, 4)
    "#,
        12,
    );
}

#[test]
fn arrow_function_in_map() {
    assert_eval_str(
        r#" [1, 2, 3].map(x => x * 2).join(",") "#,
        "2,4,6",
    );
}

// ── Spread operator ─────────────────────────────────────────────────

#[test]
fn spread_in_array_literal() {
    assert_eval_str(
        r#"
        let a = [1, 2];
        let b = [0, ...a, 3];
        b.join(",")
    "#,
        "0,1,2,3",
    );
}

#[test]
fn spread_in_function_call_v1() {
    assert_eval_int(
        r#"
        function sum(a, b, c) { return a + b + c; }
        let args = [1, 2, 3];
        sum(...args)
    "#,
        6,
    );
}

// ── Rest parameters ─────────────────────────────────────────────────

#[test]
fn rest_params_basic() {
    assert_eval_str(
        r#"
        function f(first, ...rest) {
            return first + ":" + rest.join(",");
        }
        f("a", "b", "c", "d")
    "#,
        "a:b,c,d",
    );
}

// ── Default parameters ──────────────────────────────────────────────

#[test]
fn default_params() {
    assert_eval_int(
        r#"
        function f(a, b = 10) { return a + b; }
        f(5)
    "#,
        15,
    );
}

#[test]
fn default_params_override() {
    assert_eval_int(
        r#"
        function f(a, b = 10) { return a + b; }
        f(5, 20)
    "#,
        25,
    );
}

// ── Template literals (additional) ──────────────────────────────────

#[test]
fn template_literal_nested_v1() {
    assert_eval_str(
        r#"
        let x = 5;
        `result: ${x > 3 ? "big" : "small"}`
    "#,
        "result: big",
    );
}

#[test]
fn template_literal_multiline() {
    assert_eval_bool(
        r#"
        let s = `line1
line2`;
        s.includes("\n")
    "#,
        true,
    );
}

// ── JSON ─────────────────────────────────────────────────────────────

#[test]
fn json_parse_object() {
    assert_eval_int(
        r#"
        let obj = JSON.parse('{"a": 1, "b": 2}');
        obj.a + obj.b
    "#,
        3,
    );
}

#[test]
fn json_stringify_object() {
    assert_eval_str(
        r#" JSON.stringify({"a": 1}) "#,
        r#"{"a":1}"#,
    );
}

#[test]
fn json_parse_array() {
    assert_eval_int(
        r#"
        let arr = JSON.parse("[1, 2, 3]");
        arr[0] + arr[1] + arr[2]
    "#,
        6,
    );
}

// ── Comma operator ───────────────────────────────────────────────────

#[test]
fn comma_expression() {
    assert_eval_int(r#" (1, 2, 3) "#, 3);
}

// ── Bitwise operators ────────────────────────────────────────────────

#[test]
fn bitwise_and() {
    assert_eval_int(r#" 0b1010 & 0b1100 "#, 8);
}

#[test]
fn bitwise_or() {
    assert_eval_int(r#" 0b1010 | 0b0101 "#, 15);
}

#[test]
fn bitwise_xor() {
    assert_eval_int(r#" 0b1010 ^ 0b1100 "#, 6);
}

#[test]
fn bitwise_not_v1() {
    assert_eval_int(r#" ~0 "#, -1);
}

#[test]
fn bitwise_left_shift() {
    assert_eval_int(r#" 1 << 4 "#, 16);
}

#[test]
fn bitwise_right_shift() {
    assert_eval_int(r#" 16 >> 2 "#, 4);
}

// ── Promise (basic) ─────────────────────────────────────────────────

#[test]
fn promise_resolve_then() {
    assert_eval_int(
        r#"
        let result = 0;
        Promise.resolve(42).then(function(v) { result = v; });
        result
    "#,
        42,
    );
}

// ── Symbol / misc ───────────────────────────────────────────────────

#[test]
fn void_operator_v1() {
    assert_eval_undefined(r#" void 42 "#);
}

#[test]
fn delete_property() {
    assert_eval_undefined(
        r#"
        let obj = {"a": 1, "b": 2};
        delete obj.a;
        obj.a
    "#,
    );
}

// ── Getters and Setters ─────────────────────────────────────────────

#[test]
fn class_getter_v1() {
    assert_eval_int(
        r#"
        class Rect {
            constructor(w, h) { this.w = w; this.h = h; }
            get area() { return this.w * this.h; }
        }
        let r = new Rect(3, 4);
        r.area
    "#,
        12,
    );
}

#[test]
fn class_setter_v1() {
    assert_eval_int(
        r#"
        class Box {
            constructor() { this._size = 0; }
            set size(v) { this._size = v * 2; }
            get size() { return this._size; }
        }
        let b = new Box();
        b.size = 5;
        b.size
    "#,
        10,
    );
}

// ── Static methods ──────────────────────────────────────────────────

#[test]
fn class_static_method_v1() {
    assert_eval_int(
        r#"
        class MathHelper {
            static add(a, b) { return a + b; }
        }
        MathHelper.add(3, 4)
    "#,
        7,
    );
}

#[test]
fn class_static_field_v1() {
    assert_eval_int(
        r#"
        class Counter {
            static count = 0;
            static increment() { Counter.count = Counter.count + 1; }
        }
        Counter.increment();
        Counter.increment();
        Counter.count
    "#,
        2,
    );
}

// ── Number/String coercion edge cases ────────────────────────────────

#[test]
fn coercion_string_plus_number() {
    assert_eval_str(r#" "hello" + 42 "#, "hello42");
}

#[test]
fn coercion_number_plus_string() {
    assert_eval_str(r#" 42 + "hello" "#, "42hello");
}

#[test]
fn coercion_string_plus_bool() {
    assert_eval_str(r#" "val:" + true "#, "val:true");
}

#[test]
fn coercion_string_plus_null() {
    assert_eval_str(r#" "val:" + null "#, "val:null");
}

#[test]
fn coercion_string_plus_undefined() {
    assert_eval_str(r#" "val:" + undefined "#, "val:undefined");
}

#[test]
fn coercion_number_to_string() {
    assert_eval_str(r#" (42).toString() "#, "42");
}

#[test]
fn coercion_number_to_string_radix() {
    assert_eval_str(r#" (255).toString(16) "#, "ff");
}

// ── Equality edge cases ─────────────────────────────────────────────

#[test]
fn strict_equality_types() {
    assert_eval_bool(r#" 1 === "1" "#, false);
}

#[test]
fn loose_equality_number_string() {
    assert_eval_bool(r#" 1 == "1" "#, true);
}

#[test]
fn loose_equality_null_undefined_v1() {
    assert_eval_bool(r#" null == undefined "#, true);
}

#[test]
fn strict_inequality_null_undefined() {
    assert_eval_bool(r#" null === undefined "#, false);
}

#[test]
fn nan_not_equal_itself() {
    assert_eval_bool(r#" NaN === NaN "#, false);
}

#[test]
fn nan_not_equal_itself_loose() {
    assert_eval_bool(r#" NaN == NaN "#, false);
}

// ── Control flow edge cases ─────────────────────────────────────────

#[test]
fn if_else_chain() {
    assert_eval_str(
        r#"
        function grade(n) {
            if (n >= 90) return "A";
            else if (n >= 80) return "B";
            else if (n >= 70) return "C";
            else return "F";
        }
        grade(85) + grade(95) + grade(65)
    "#,
        "BAF",
    );
}

#[test]
fn early_return() {
    assert_eval_int(
        r#"
        function f(x) {
            if (x < 0) return -1;
            if (x === 0) return 0;
            return 1;
        }
        f(-5) + f(0) + f(5)
    "#,
        0,
    );
}

#[test]
fn nested_loops_break() {
    assert_eval_int(
        r#"
        let count = 0;
        for (let i = 0; i < 5; i++) {
            for (let j = 0; j < 5; j++) {
                if (j === 3) break;
                count++;
            }
        }
        count
    "#,
        15,
    );
}

// ── String methods edge cases ───────────────────────────────────────

#[test]
fn string_index_access() {
    assert_eval_str(r#" "hello"[1] "#, "e");
}

#[test]
fn string_trim_v1() {
    assert_eval_str(r#" "  hello  ".trim() "#, "hello");
}

#[test]
fn string_to_upper() {
    assert_eval_str(r#" "hello".toUpperCase() "#, "HELLO");
}

#[test]
fn string_to_lower() {
    assert_eval_str(r#" "HELLO".toLowerCase() "#, "hello");
}

#[test]
fn string_split_limit() {
    assert_eval_str(r#" "a,b,c,d".split(",", 2).join(";") "#, "a;b");
}

#[test]
fn string_replace_all_v1() {
    assert_eval_str(r#" "aabbcc".replace(/b/g, "x") "#, "aaxxcc");
}

// ── Array edge cases ────────────────────────────────────────────────

#[test]
fn array_concat_v1() {
    assert_eval_str(r#" [1, 2].concat([3, 4]).join(",") "#, "1,2,3,4");
}

#[test]
fn array_reverse_v1() {
    assert_eval_str(r#" [1, 2, 3].reverse().join(",") "#, "3,2,1");
}

#[test]
fn array_sort_default_v1() {
    assert_eval_str(r#" [3, 1, 2].sort().join(",") "#, "1,2,3");
}

#[test]
fn array_sort_comparator() {
    assert_eval_str(
        r#" [3, 1, 2].sort(function(a, b) { return b - a; }).join(",") "#,
        "3,2,1",
    );
}

#[test]
fn array_splice_insert_v1() {
    assert_eval_str(
        r#"
        let arr = [1, 2, 3];
        arr.splice(1, 0, 10, 20);
        arr.join(",")
    "#,
        "1,10,20,2,3",
    );
}

#[test]
fn array_splice_remove_v1() {
    assert_eval_str(
        r#"
        let arr = [1, 2, 3, 4, 5];
        let removed = arr.splice(1, 2);
        arr.join(",") + "|" + removed.join(",")
    "#,
        "1,4,5|2,3",
    );
}

#[test]
fn array_from_string_v1() {
    assert_eval_str(r#" Array.from("abc").join(",") "#, "a,b,c");
}

#[test]
fn array_unshift_shift() {
    assert_eval_str(
        r#"
        let arr = [2, 3];
        arr.unshift(1);
        let first = arr.shift();
        first + ":" + arr.join(",")
    "#,
        "1:2,3",
    );
}

// ── Object patterns ─────────────────────────────────────────────────

#[test]
fn object_spread() {
    assert_eval_int(
        r#"
        let a = {"x": 1, "y": 2};
        let b = {...a, "z": 3};
        b.x + b.y + b.z
    "#,
        6,
    );
}

#[test]
fn object_computed_key_variable() {
    assert_eval_int(
        r#"
        let key = "count";
        let obj = {};
        obj[key] = 42;
        obj.count
    "#,
        42,
    );
}

#[test]
fn in_operator() {
    assert_eval_bool(r#" "a" in {"a": 1, "b": 2} "#, true);
}

#[test]
fn in_operator_missing() {
    assert_eval_bool(r#" "c" in {"a": 1, "b": 2} "#, false);
}

// ── Recursion ───────────────────────────────────────────────────────

#[test]
fn recursion_factorial() {
    assert_eval_int(
        r#"
        function fact(n) {
            if (n <= 1) return 1;
            return n * fact(n - 1);
        }
        fact(10)
    "#,
        3628800,
    );
}

#[test]
fn recursion_fibonacci() {
    assert_eval_int(
        r#"
        function fib(n) {
            if (n <= 1) return n;
            return fib(n - 1) + fib(n - 2);
        }
        fib(10)
    "#,
        55,
    );
}

// ── Class patterns (additional) ─────────────────────────────────────

#[test]
fn class_method_chaining() {
    assert_eval_int(
        r#"
        class Builder {
            constructor() { this.val = 0; }
            add(n) { this.val += n; return this; }
            result() { return this.val; }
        }
        new Builder().add(1).add(2).add(3).result()
    "#,
        6,
    );
}

#[test]
fn class_field_initializer() {
    assert_eval_int(
        r#"
        class Foo {
            x = 10;
            y = 20;
            sum() { return this.x + this.y; }
        }
        new Foo().sum()
    "#,
        30,
    );
}

#[test]
fn class_private_convention() {
    assert_eval_int(
        r#"
        class Counter {
            constructor() { this._count = 0; }
            increment() { this._count++; return this; }
            get count() { return this._count; }
        }
        let c = new Counter();
        c.increment().increment().increment();
        c.count
    "#,
        3,
    );
}

// ── Map / Set additional ────────────────────────────────────────────

#[test]
fn map_has_delete_v1() {
    assert_eval_bool(
        r#"
        let m = new Map();
        m.set("a", 1);
        m.set("b", 2);
        m.delete("a");
        !m.has("a") && m.has("b")
    "#,
        true,
    );
}

#[test]
fn set_add_has_size() {
    assert_eval_int(
        r#"
        let s = new Set();
        s.add(1); s.add(2); s.add(2); s.add(3);
        s.size
    "#,
        3,
    );
}

// ── Error handling ──────────────────────────────────────────────────

#[test]
fn error_message_property() {
    assert_eval_str(
        r#"
        try {
            throw new Error("test message");
        } catch (e) {
            e.message
        }
    "#,
        "test message",
    );
}

#[test]
fn error_name_property() {
    assert_eval_str(
        r#"
        try {
            throw new Error("test");
        } catch (e) {
            e.name
        }
    "#,
        "Error",
    );
}

#[test]
fn throw_string() {
    assert_eval_str(
        r#"
        try {
            throw "custom error";
        } catch (e) {
            e
        }
    "#,
        "custom error",
    );
}

#[test]
fn throw_number() {
    assert_eval_int(
        r#"
        try { throw 42; } catch (e) { e }
    "#,
        42,
    );
}

// ── Misc patterns ───────────────────────────────────────────────────

#[test]
fn chained_ternary_assignment() {
    assert_eval_str(
        r#"
        let x = 5;
        let result = x > 10 ? "big" : x > 3 ? "medium" : "small";
        result
    "#,
        "medium",
    );
}

#[test]
fn short_circuit_and_v1() {
    assert_eval_int(r#" 0 && 42 "#, 0);
}

#[test]
fn short_circuit_or_v1() {
    assert_eval_int(r#" 0 || 42 "#, 42);
}

#[test]
fn nullish_coalescing_false() {
    assert_eval_bool(r#" false ?? true "#, false);
}

#[test]
fn string_template_with_method() {
    assert_eval_str(r#" let arr = [1,2,3]; `[${arr.join(",")}]` "#, "[1,2,3]");
}

#[test]
fn for_in_inherited_excluded() {
    assert_eval_str(
        r#"
        let obj = {"a": 1, "b": 2};
        let keys = [];
        for (let k in obj) { keys.push(k); }
        keys.sort().join(",")
    "#,
        "a,b",
    );
}

#[test]
fn typeof_function_v1() {
    assert_eval_str(r#" typeof function(){} "#, "function");
}

#[test]
fn infinity_check() {
    assert_eval_bool(r#" 1/0 === Infinity "#, true);
}

#[test]
fn negative_infinity() {
    assert_eval_bool(r#" -1/0 === -Infinity "#, true);
}

#[test]
fn isnan_check() {
    assert_eval_bool(r#" isNaN(NaN) "#, true);
}

#[test]
fn isfinite_check() {
    assert_eval_bool(r#" isFinite(42) "#, true);
}

#[test]
fn parseint_basic() {
    assert_eval_int(r#" parseInt("42") "#, 42);
}

#[test]
fn parsefloat_basic() {
    assert_eval_float(r#" parseFloat("3.14") "#, 3.14);
}

#[test]
fn number_isinteger() {
    assert_eval_bool(r#" Number.isInteger(42) "#, true);
}

#[test]
fn number_isinteger_float() {
    assert_eval_bool(r#" Number.isInteger(42.5) "#, false);
}

#[test]
fn array_destructure_swap() {
    assert_eval_str(
        r#"
        let a = 1;
        let b = 2;
        [a, b] = [b, a];
        a + "," + b
    "#,
        "2,1",
    );
}

// ── Edge cases from TypeScript reference ─────────────────────────────

#[test]
fn edge_division_by_zero_positive() {
    assert_eval_bool(r#" 1/0 === Infinity "#, true);
}

#[test]
fn edge_zero_div_zero_is_nan() {
    assert_eval_bool(r#" isNaN(0/0) "#, true);
}

#[test]
fn edge_array_oob_undefined() {
    assert_eval_undefined(r#" let arr = [1,2,3]; arr[10] "#);
}

#[test]
fn edge_negative_modulo() {
    assert_eval_int(r#" -5 % 3 "#, -2);
}

#[test]
fn edge_string_slice_basic() {
    assert_eval_str(r#" "hello world".slice(0, 5) "#, "hello");
}

#[test]
fn edge_string_slice_negative() {
    assert_eval_str(r#" "hello".slice(-3) "#, "llo");
}

#[test]
fn edge_trailing_comma_array() {
    assert_eval_int(r#" [1, 2, 3,].length "#, 3);
}

#[test]
fn edge_trailing_comma_call() {
    assert_eval_int(
        r#"
        function add(a, b) { return a + b; }
        add(3, 4,)
    "#,
        7,
    );
}

#[test]
fn edge_arrow_function_this() {
    assert_eval_int(
        r#"
        class A {
            constructor() {
                this.val = 10;
                this.arrow = (x) => this.val + x;
            }
        }
        let a = new A();
        a.arrow(5)
    "#,
        15,
    );
}

#[test]
fn edge_nested_template_literal() {
    assert_eval_str(r#" `A ${ `B ${10}` }` "#, "A B 10");
}

#[test]
fn edge_bracket_method_call() {
    assert_eval_int(
        r#"
        let obj = {};
        obj.add = function(a, b) { return a + b; };
        let method = "add";
        obj[method](3, 4)
    "#,
        7,
    );
}

#[test]
fn edge_bracket_array_call() {
    assert_eval_int(
        r#"
        let funcs = [() => 10, () => 20, () => 30];
        funcs[0]() + funcs[2]()
    "#,
        40,
    );
}

#[test]
fn edge_return_no_value() {
    assert_eval_undefined(
        r#"
        function f() { return; }
        f()
    "#,
    );
}

#[test]
fn edge_function_no_return() {
    assert_eval_undefined(
        r#"
        function noop() {}
        noop()
    "#,
    );
}

#[test]
fn edge_object_rest_destructuring() {
    assert_eval_str(
        r#"
        let {a, ...rest} = {"a": 1, "b": 2, "c": 3};
        a + "," + rest.b + "," + rest.c
    "#,
        "1,2,3",
    );
}

#[test]
fn edge_array_destructure_holes() {
    assert_eval_str(
        r#"
        let [a, , c] = [1, 2, 3];
        a + "," + c
    "#,
        "1,3",
    );
}

#[test]
fn edge_string_char_code_at() {
    assert_eval_int(r#" "A".charCodeAt(0) "#, 65);
}

#[test]
fn edge_string_from_char_code() {
    assert_eval_str(r#" String.fromCharCode(65, 66, 67) "#, "ABC");
}

#[test]
fn edge_negative_array_index() {
    assert_eval_undefined(r#" let arr = [1,2,3]; arr[-1] "#);
}

#[test]
fn edge_modulo_negative_divisor() {
    assert_eval_int(r#" 5 % -3 "#, 2);
}

#[test]
fn edge_loose_equality_false_zero() {
    assert_eval_bool(r#" false == 0 "#, true);
}

#[test]
fn edge_loose_equality_true_one() {
    assert_eval_bool(r#" true == 1 "#, true);
}

#[test]
fn edge_loose_equality_empty_string_zero() {
    assert_eval_bool(r#" "" == 0 "#, true);
}

#[test]
fn edge_exponent_operator() {
    assert_eval_int(r#" 2 ** 10 "#, 1024);
}

#[test]
fn edge_exponent_float() {
    assert_eval_int(r#" 4 ** 0.5 "#, 2);
}

#[test]
fn edge_unsigned_right_shift() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#" -1 >>> 0 "#).expect("eval");
    match &out {
        Object::Integer(v) => assert_eq!(*v, 4294967295),
        Object::Float(v) => assert!((v - 4294967295.0).abs() < 1.0),
        _ => panic!("expected numeric, got {out:?}"),
    }
}

// ── valueOf / toString on class instances ────────────────────────────

#[test]
fn class_valueof_in_arithmetic() {
    assert_eval_int(
        r#"
        class Money {
            constructor(v) { this.value = v; }
            valueOf() { return this.value; }
        }
        new Money(10) + new Money(20)
    "#,
        30,
    );
}

#[test]
fn class_tostring_in_concat() {
    assert_eval_str(
        r#"
        class Point {
            constructor(x, y) { this.x = x; this.y = y; }
            toString() { return "(" + this.x + "," + this.y + ")"; }
        }
        "Point: " + new Point(3, 4)
    "#,
        "Point: (3,4)",
    );
}

// ── Computed/dynamic property patterns ──────────────────────────────

#[test]
fn dynamic_method_name() {
    assert_eval_int(
        r#"
        let obj = {};
        obj["get" + "Value"] = function() { return 42; };
        obj.getValue()
    "#,
        42,
    );
}

#[test]
fn property_deletion_check() {
    assert_eval_bool(
        r#"
        let obj = {"a": 1, "b": 2};
        delete obj.a;
        "a" in obj
    "#,
        false,
    );
}

// ── Number parsing edge cases ───────────────────────────────────────

#[test]
fn parseint_hex() {
    assert_eval_int(r#" parseInt("0xff", 16) "#, 255);
}

#[test]
fn parseint_leading_zeros() {
    assert_eval_int(r#" parseInt("007") "#, 7);
}

#[test]
fn parseint_with_trailing() {
    assert_eval_int(r#" parseInt("42abc") "#, 42);
}

#[test]
fn parsefloat_scientific() {
    assert_eval_int(r#" parseFloat("1.5e2") "#, 150);
}

// ── JSON edge cases ─────────────────────────────────────────────────

#[test]
fn json_stringify_array() {
    assert_eval_str(r#" JSON.stringify([1,2,3]) "#, "[1,2,3]");
}

#[test]
fn json_stringify_null() {
    assert_eval_str(r#" JSON.stringify(null) "#, "null");
}

#[test]
fn json_stringify_nested() {
    assert_eval_str(
        r#" JSON.stringify({"a": {"b": 1}}) "#,
        r#"{"a":{"b":1}}"#,
    );
}

#[test]
fn json_parse_nested() {
    assert_eval_int(
        r#"
        let obj = JSON.parse('{"a":{"b":42}}');
        obj.a.b
    "#,
        42,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// valueOf / toString coercion tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn valueof_add_instances() {
    assert_eval_int(
        r#"
        class Money {
            constructor(amount) { this.amount = amount; }
            valueOf() { return this.amount; }
        }
        let a = new Money(10);
        let b = new Money(20);
        a + b;
    "#,
        30,
    );
}

#[test]
fn valueof_subtract_instances() {
    assert_eval_int(
        r#"
        class Money {
            constructor(amount) { this.amount = amount; }
            valueOf() { return this.amount; }
        }
        let a = new Money(30);
        let b = new Money(12);
        a - b;
    "#,
        18,
    );
}

#[test]
fn valueof_multiply_instances() {
    assert_eval_int(
        r#"
        class Vec {
            constructor(x) { this.x = x; }
            valueOf() { return this.x; }
        }
        let a = new Vec(6);
        let b = new Vec(7);
        a * b;
    "#,
        42,
    );
}

#[test]
fn valueof_divide_instances() {
    // Division always returns float in JS
    assert_eval_float(
        r#"
        class Num {
            constructor(v) { this.v = v; }
            valueOf() { return this.v; }
        }
        let a = new Num(100);
        let b = new Num(4);
        a / b;
    "#,
        25.0,
    );
}

#[test]
fn valueof_mod_instances() {
    assert_eval_int(
        r#"
        class Num {
            constructor(v) { this.v = v; }
            valueOf() { return this.v; }
        }
        let a = new Num(17);
        let b = new Num(5);
        a % b;
    "#,
        2,
    );
}

#[test]
fn valueof_mixed_instance_and_number() {
    assert_eval_int(
        r#"
        class Num {
            constructor(v) { this.v = v; }
            valueOf() { return this.v; }
        }
        let a = new Num(10);
        a + 5;
    "#,
        15,
    );
}

#[test]
fn valueof_number_plus_instance() {
    assert_eval_int(
        r#"
        class Num {
            constructor(v) { this.v = v; }
            valueOf() { return this.v; }
        }
        let a = new Num(7);
        3 + a;
    "#,
        10,
    );
}

#[test]
fn tostring_coercion_in_concat() {
    assert_eval_str(
        r#"
        class Name {
            constructor(n) { this.n = n; }
            toString() { return this.n; }
        }
        let a = new Name("hello");
        "say " + a;
    "#,
        "say hello",
    );
}

#[test]
fn valueof_comparison() {
    assert_eval_bool(
        r#"
        class Num {
            constructor(v) { this.v = v; }
            valueOf() { return this.v; }
        }
        let a = new Num(5);
        let b = new Num(3);
        a > b;
    "#,
        true,
    );
}

#[test]
fn class_static_field_direct_set() {
    assert_eval_int(
        r#"
        class Counter { static count = 0; }
        Counter.count = 42;
        Counter.count
    "#,
        42,
    );
}

#[test]
fn class_instanceof_basic() {
    assert_eval_bool(
        r#"
        class Point { constructor(x) { this.x = x; } }
        let p = new Point(1);
        p instanceof Point
    "#,
        true,
    );
}

#[test]
fn class_static_field_increment() {
    assert_eval_int(
        r#"
        class Counter { static count = 0; }
        Counter.count = Counter.count + 1;
        Counter.count = Counter.count + 1;
        Counter.count
    "#,
        2,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Arrow function this binding tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn arrow_this_in_method() {
    assert_eval_int(
        r#"
        class Obj {
            constructor() { this.x = 100; }
            getAdder() { return (n) => this.x + n; }
        }
        let o = new Obj();
        let adder = o.getAdder();
        adder(23)
    "#,
        123,
    );
}

#[test]
fn arrow_this_nested() {
    assert_eval_int(
        r#"
        class Obj {
            constructor() { this.x = 5; }
            test() {
                let f = () => {
                    let g = () => this.x * 2;
                    return g();
                };
                return f();
            }
        }
        let o = new Obj();
        o.test()
    "#,
        10,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// More class patterns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn class_getter_setter_pair() {
    assert_eval_int(
        r#"
        class Temperature {
            constructor(c) { this._celsius = c; }
            get fahrenheit() { return this._celsius * 9 / 5 + 32; }
            set fahrenheit(f) { this._celsius = (f - 32) * 5 / 9; }
        }
        let t = new Temperature(100);
        t.fahrenheit
    "#,
        212,
    );
}

#[test]
fn class_method_chaining_v2() {
    assert_eval_int(
        r#"
        class Builder {
            constructor() { this.val = 0; }
            add(n) { this.val = this.val + n; return this; }
            mul(n) { this.val = this.val * n; return this; }
            result() { return this.val; }
        }
        new Builder().add(3).mul(4).add(2).result()
    "#,
        14,
    );
}

#[test]
fn class_static_method_no_this() {
    assert_eval_int(
        r#"
        class MathHelper {
            static square(x) { return x * x; }
            static cube(x) { return x * x * x; }
        }
        MathHelper.square(3) + MathHelper.cube(2)
    "#,
        17,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// More closure tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn closure_counter_v2() {
    assert_eval_int(
        r#"
        function makeCounter() {
            let count = 0;
            return function() {
                count = count + 1;
                return count;
            };
        }
        let c = makeCounter();
        c(); c(); c()
    "#,
        3,
    );
}

#[test]
fn closure_shared_state() {
    assert_eval_int(
        r#"
        function makePair() {
            let val = 0;
            return {
                "get": function() { return val; },
                "set": function(v) { val = v; }
            };
        }
        let p = makePair();
        p.set(42);
        p.get()
    "#,
        42,
    );
}

#[test]
fn closure_iife() {
    assert_eval_int(
        r#"
        let result = (function(x) { return x * x; })(7);
        result
    "#,
        49,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// More array method tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_flatmap() {
    assert_eval_str(
        r#"
        let arr = [1, 2, 3];
        let result = arr.flatMap(function(x) { return [x, x * 2]; });
        result.join(",")
    "#,
        "1,2,2,4,3,6",
    );
}

#[test]
fn array_every_some() {
    assert_eval_bool(
        r#"
        let arr = [2, 4, 6, 8];
        arr.every(function(x) { return x % 2 === 0; })
    "#,
        true,
    );
}

#[test]
fn array_some_mixed() {
    assert_eval_bool(
        r#"
        let arr = [1, 3, 5, 6];
        arr.some(function(x) { return x % 2 === 0; })
    "#,
        true,
    );
}

#[test]
fn array_findindex() {
    assert_eval_int(
        r#"
        let arr = [10, 20, 30, 40];
        arr.findIndex(function(x) { return x > 25; })
    "#,
        2,
    );
}

#[test]
fn array_includes_v2() {
    assert_eval_bool(
        r#"
        [1, 2, 3, 4].includes(3)
    "#,
        true,
    );
}

#[test]
fn array_includes_false_v2() {
    assert_eval_bool(
        r#"
        [1, 2, 3, 4].includes(5)
    "#,
        false,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// More string method tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_padstart() {
    assert_eval_str(r#" "5".padStart(3, "0") "#, "005");
}

#[test]
fn string_padend() {
    assert_eval_str(r#" "hi".padEnd(5, "!") "#, "hi!!!");
}

#[test]
fn string_repeat_v2() {
    assert_eval_str(r#" "ab".repeat(3) "#, "ababab");
}

#[test]
fn string_startswith() {
    assert_eval_bool(r#" "hello world".startsWith("hello") "#, true);
}

#[test]
fn string_endswith() {
    assert_eval_bool(r#" "hello world".endsWith("world") "#, true);
}

#[test]
fn string_trimstart() {
    assert_eval_str(r#" "  hello  ".trimStart() "#, "hello  ");
}

#[test]
fn string_trimend() {
    assert_eval_str(r#" "  hello  ".trimEnd() "#, "  hello");
}

// ═══════════════════════════════════════════════════════════════════════
// Object static methods
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn object_keys_v2() {
    assert_eval_str(
        r#"
        let obj = {"a": 1, "b": 2, "c": 3};
        Object.keys(obj).join(",")
    "#,
        "a,b,c",
    );
}

#[test]
fn object_values_v2() {
    assert_eval_str(
        r#"
        let obj = {"x": 10, "y": 20};
        Object.values(obj).join(",")
    "#,
        "10,20",
    );
}

#[test]
fn object_entries_v2() {
    assert_eval_int(
        r#"
        let obj = {"a": 1, "b": 2};
        Object.entries(obj).length
    "#,
        2,
    );
}

#[test]
fn object_assign() {
    assert_eval_int(
        r#"
        let a = {"x": 1};
        let b = {"y": 2};
        let c = Object.assign({}, a, b);
        c.x + c.y
    "#,
        3,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Miscellaneous edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nullish_coalescing_chain() {
    assert_eval_int(
        r#"
        let a = null;
        let b = undefined;
        let c = 42;
        a ?? b ?? c
    "#,
        42,
    );
}

#[test]
fn optional_chaining_method() {
    assert_eval_str(
        r#"
        let obj = {"greet": function() { return "hi"; }};
        obj?.greet()
    "#,
        "hi",
    );
}

#[test]
fn optional_chaining_null_v2() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("let obj = null; obj?.foo").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn labeled_break_nested_loops() {
    assert_eval_int(
        r#"
        let sum = 0;
        outer: for (let i = 0; i < 5; i = i + 1) {
            for (let j = 0; j < 5; j = j + 1) {
                if (j === 3) break outer;
                sum = sum + 1;
            }
        }
        sum
    "#,
        3,
    );
}

#[test]
fn symbol_iterator_for_of() {
    assert_eval_int(
        r#"
        let sum = 0;
        for (let x of [10, 20, 30]) {
            sum = sum + x;
        }
        sum
    "#,
        60,
    );
}

#[test]
fn computed_property_name() {
    assert_eval_int(
        r#"
        let key = "value";
        let obj = { [key]: 42 };
        obj.value
    "#,
        42,
    );
}

#[test]
fn short_circuit_and_side_effects() {
    assert_eval_int(
        r#"
        let x = 0;
        false && (x = 10);
        x
    "#,
        0,
    );
}

#[test]
fn short_circuit_or_side_effects() {
    assert_eval_int(
        r#"
        let x = 0;
        true || (x = 10);
        x
    "#,
        0,
    );
}

#[test]
fn typeof_various() {
    assert_eval_str(r#" typeof 42 "#, "number");
}

#[test]
fn typeof_string_v2() {
    assert_eval_str(r#" typeof "hello" "#, "string");
}

#[test]
fn typeof_boolean_v2() {
    assert_eval_str(r#" typeof true "#, "boolean");
}

#[test]
fn typeof_undefined_v2() {
    assert_eval_str(r#" typeof undefined "#, "undefined");
}

#[test]
fn typeof_null_v2() {
    assert_eval_str(r#" typeof null "#, "object");
}

#[test]
fn typeof_function_v2() {
    assert_eval_str(r#" typeof function(){} "#, "function");
}

#[test]
fn typeof_object_v2() {
    assert_eval_str(r#" typeof {} "#, "object");
}

#[test]
fn typeof_array_v2() {
    assert_eval_str(r#" typeof [] "#, "object");
}

#[test]
fn string_raw_access() {
    assert_eval_str(r#" "hello"[1] "#, "e");
}

#[test]
fn array_destructure_swap_v2() {
    assert_eval_int(
        r#"
        let a = 1;
        let b = 2;
        [a, b] = [b, a];
        a * 10 + b
    "#,
        21,
    );
}

#[test]
fn rest_params_function() {
    assert_eval_int(
        r#"
        function sum(...args) {
            return args.reduce(function(a, b) { return a + b; }, 0);
        }
        sum(1, 2, 3, 4, 5)
    "#,
        15,
    );
}

#[test]
fn default_params_v2() {
    assert_eval_int(
        r#"
        function greet(x, y = 10) {
            return x + y;
        }
        greet(5)
    "#,
        15,
    );
}

#[test]
fn spread_in_call() {
    assert_eval_int(
        r#"
        function add(a, b, c) { return a + b + c; }
        let args = [1, 2, 3];
        add(...args)
    "#,
        6,
    );
}

#[test]
fn for_in_object_v2() {
    assert_eval_int(
        r#"
        let obj = {"a": 1, "b": 2, "c": 3};
        let count = 0;
        for (let k in obj) {
            count = count + 1;
        }
        count
    "#,
        3,
    );
}

#[test]
fn do_while_loop_v2() {
    assert_eval_int(
        r#"
        let i = 0;
        let sum = 0;
        do {
            sum = sum + i;
            i = i + 1;
        } while (i < 5);
        sum
    "#,
        10,
    );
}

#[test]
fn conditional_ternary() {
    assert_eval_int(r#" let x = true ? 10 : 20; x "#, 10);
}

#[test]
fn conditional_ternary_false() {
    assert_eval_int(r#" let x = false ? 10 : 20; x "#, 20);
}

#[test]
fn nested_ternary_v2() {
    assert_eval_int(
        r#" let x = 2; x === 1 ? 10 : x === 2 ? 20 : 30 "#,
        20,
    );
}

#[test]
fn chained_comparison() {
    assert_eval_bool(r#" 1 < 2 && 2 < 3 "#, true);
}

#[test]
fn logical_nullish_assignment() {
    assert_eval_int(
        r#"
        let x = null;
        x ??= 42;
        x
    "#,
        42,
    );
}

#[test]
fn logical_or_assignment() {
    assert_eval_int(
        r#"
        let x = 0;
        x ||= 42;
        x
    "#,
        42,
    );
}

#[test]
fn logical_and_assignment() {
    assert_eval_int(
        r#"
        let x = 1;
        x &&= 42;
        x
    "#,
        42,
    );
}

#[test]
fn number_methods_tofixed() {
    assert_eval_str(r#" (3.14159).toFixed(2) "#, "3.14");
}

#[test]
fn number_isnan() {
    assert_eval_bool(r#" Number.isNaN(NaN) "#, true);
}

#[test]
fn number_isfinite() {
    assert_eval_bool(r#" Number.isFinite(42) "#, true);
}

#[test]
fn number_isfinite_infinity() {
    assert_eval_bool(r#" Number.isFinite(Infinity) "#, false);
}

#[test]
fn math_max_v2() {
    assert_eval_int(r#" Math.max(1, 5, 3, 2) "#, 5);
}

#[test]
fn math_min_v2() {
    assert_eval_int(r#" Math.min(5, 1, 3, 2) "#, 1);
}

#[test]
fn math_abs_v2() {
    assert_eval_int(r#" Math.abs(-42) "#, 42);
}

#[test]
fn math_pow_v2() {
    assert_eval_int(r#" Math.pow(2, 10) "#, 1024);
}

#[test]
fn exponentiation_operator() {
    assert_eval_int(r#" 2 ** 10 "#, 1024);
}

#[test]
fn array_from_string_v2() {
    assert_eval_int(
        r#"
        let arr = Array.from("abc");
        arr.length
    "#,
        3,
    );
}

#[test]
fn array_isarray() {
    assert_eval_bool(r#" Array.isArray([1, 2, 3]) "#, true);
}

#[test]
fn array_isarray_false_v2() {
    assert_eval_bool(r#" Array.isArray("hello") "#, false);
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: scope & closures
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn scope_block_doesnt_leak() {
    assert_eval_int(
        r#"
        let x = 1;
        { let x = 2; }
        x
    "#,
        1,
    );
}

#[test]
fn scope_var_hoisting() {
    assert_eval_int(
        r#"
        function test() {
            var x = 10;
            if (true) { var x = 20; }
            return x;
        }
        test()
    "#,
        20,
    );
}

#[test]
fn closure_mutates_outer() {
    assert_eval_int(
        r#"
        let x = 0;
        function inc() { x = x + 1; }
        inc(); inc(); inc();
        x
    "#,
        3,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: type coercion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn coerce_string_plus_number_ts() {
    assert_eval_str(r#" "5" + 3 "#, "53");
}

#[test]
fn coerce_string_minus_number() {
    assert_eval_int(r#" "10" - 3 "#, 7);
}

#[test]
fn coerce_true_plus_true() {
    assert_eval_number(r#" true + true "#, 2.0);
}

#[test]
fn coerce_null_plus_one() {
    assert_eval_number(r#" null + 1 "#, 1.0);
}

#[test]
fn coerce_unary_plus_string() {
    assert_eval_number(r#" +"42" "#, 42.0);
}

#[test]
fn coerce_unary_plus_empty_string() {
    assert_eval_number(r#" +"" "#, 0.0);
}

#[test]
fn coerce_unary_plus_true() {
    assert_eval_number(r#" +true "#, 1.0);
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: arithmetic edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn division_by_zero_pos_inf() {
    assert_eval_float(r#" 1 / 0 "#, f64::INFINITY);
}

#[test]
fn division_by_zero_neg_inf() {
    assert_eval_float(r#" -1 / 0 "#, f64::NEG_INFINITY);
}

#[test]
fn modulo_negative() {
    assert_eval_int(r#" -5 % 3 "#, -2);
}

#[test]
fn nan_strict_not_equal_to_self() {
    assert_eval_bool(r#" NaN === NaN "#, false);
}

#[test]
fn infinity_minus_infinity() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Infinity - Infinity").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_nan(), "expected NaN, got {v}"),
        _ => panic!("expected Float(NaN), got {out:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: this binding
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn this_in_method_call() {
    assert_eval_int(
        r#"
        class Obj {
            constructor() { this.x = 42; }
            getX() { return this.x; }
        }
        let o = new Obj();
        o.getX()
    "#,
        42,
    );
}

#[test]
fn this_method_chaining_preserves_this() {
    assert_eval_int(
        r#"
        class Chain {
            constructor() { this.val = 0; }
            inc() { this.val = this.val + 1; return this; }
            get() { return this.val; }
        }
        new Chain().inc().inc().inc().get()
    "#,
        3,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: destructuring edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn destructure_array_with_defaults() {
    assert_eval_int(
        r#"
        let [a = 10, b = 20] = [1];
        a + b
    "#,
        21,
    );
}

#[test]
fn destructure_rest_element() {
    assert_eval_int(
        r#"
        let [first, ...rest] = [1, 2, 3, 4];
        first + rest.length
    "#,
        4,
    );
}

#[test]
fn destructure_object_with_rename() {
    assert_eval_int(
        r#"
        let obj = {"a": 42};
        let { a: x } = obj;
        x
    "#,
        42,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: exception handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn finally_always_runs() {
    assert_eval_int(
        r#"
        let result = 0;
        try {
            result = 1;
            throw "err";
        } catch(e) {
            result = 2;
        } finally {
            result = result + 10;
        }
        result
    "#,
        12,
    );
}

#[test]
fn nested_try_catch_rethrow() {
    assert_eval_str(
        r#"
        let log = "";
        try {
            try {
                throw "inner";
            } catch(e) {
                log = log + e;
                throw "outer";
            }
        } catch(e) {
            log = log + "-" + e;
        }
        log
    "#,
        "inner-outer",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: array method edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_map_with_index() {
    assert_eval_str(
        r#"
        [10, 20, 30].map(function(val, idx) { return val + idx; }).join(",")
    "#,
        "10,21,32",
    );
}

#[test]
fn array_reduce_no_initial() {
    assert_eval_int(
        r#"
        [1, 2, 3, 4].reduce(function(a, b) { return a + b; })
    "#,
        10,
    );
}

#[test]
fn array_empty_filter() {
    assert_eval_int(
        r#"
        [].filter(function(x) { return true; }).length
    "#,
        0,
    );
}

#[test]
fn array_lastindexof() {
    assert_eval_int(
        r#"
        [1, 2, 3, 2, 1].lastIndexOf(2)
    "#,
        3,
    );
}

#[test]
fn array_slice_negative() {
    assert_eval_str(
        r#"
        [1, 2, 3, 4, 5].slice(-2).join(",")
    "#,
        "4,5",
    );
}

#[test]
fn array_every_not_all_match() {
    assert_eval_bool(
        r#"
        [1, 2, 3].every(function(x) { return x > 1; })
    "#,
        false,
    );
}

#[test]
fn array_every_empty() {
    assert_eval_bool(
        r#"
        [].every(function(x) { return false; })
    "#,
        true,
    );
}

#[test]
fn array_flat_depth() {
    // flat(2) flattens 2 levels: [1, [2, [3, [4]]]] → [1, 2, 3, [4]]
    assert_eval_str(
        r#"
        [1, [2, [3, [4]]]].flat(2).join(",")
    "#,
        "1,2,3,4",
    );
}

#[test]
fn array_flat_depth3() {
    // flat(3) flattens all 3 levels
    assert_eval_str(
        r#"
        [1, [2, [3, [4]]]].flat(3).join(",")
    "#,
        "1,2,3,4",
    );
}

#[test]
fn array_method_chaining_filter_map_reduce() {
    assert_eval_int(
        r#"
        [1, 2, 3, 4, 5, 6]
            .filter(function(x) { return x % 2 === 0; })
            .map(function(x) { return x * x; })
            .reduce(function(a, b) { return a + b; }, 0)
    "#,
        56,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: operator edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn operator_and_returns_value() {
    assert_eval_int(r#" 0 && 5 "#, 0);
}

#[test]
fn operator_and_returns_second() {
    assert_eval_int(r#" 1 && 2 "#, 2);
}

#[test]
fn operator_or_returns_first_truthy() {
    assert_eval_int(r#" 0 || 5 "#, 5);
}

#[test]
fn operator_or_returns_first() {
    assert_eval_int(r#" 1 || 2 "#, 1);
}

#[test]
fn comma_operator_returns_last() {
    assert_eval_int(r#" (1, 2, 3) "#, 3);
}

#[test]
fn bitwise_not_five() {
    assert_eval_int(r#" ~5 "#, -6);
}

#[test]
fn unsigned_right_shift_negative() {
    // -1 >>> 0 should be 4294967295 (u32 max)
    assert_eval_number(r#" -1 >>> 0 "#, 4294967295.0);
}

#[test]
fn pre_increment() {
    assert_eval_int(
        r#"
        let i = 5;
        let x = ++i;
        x
    "#,
        6,
    );
}

#[test]
fn post_increment() {
    assert_eval_int(
        r#"
        let i = 5;
        let x = i++;
        x
    "#,
        5,
    );
}

#[test]
fn post_increment_side_effect() {
    assert_eval_int(
        r#"
        let i = 5;
        i++;
        i
    "#,
        6,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: numeric literals
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hex_literal() {
    assert_eval_int(r#" 0xFF "#, 255);
}

#[test]
fn octal_literal() {
    assert_eval_int(r#" 0o77 "#, 63);
}

#[test]
fn binary_literal() {
    assert_eval_int(r#" 0b1010 "#, 10);
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: truthiness
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn double_not_coercion() {
    assert_eval_bool(r#" !!1 "#, true);
}

#[test]
fn double_not_zero() {
    assert_eval_bool(r#" !!0 "#, false);
}

#[test]
fn double_not_empty_string() {
    assert_eval_bool(r#" !!"" "#, false);
}

#[test]
fn double_not_string() {
    assert_eval_bool(r#" !!"hello" "#, true);
}

#[test]
fn double_not_null() {
    assert_eval_bool(r#" !!null "#, false);
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: loose equality
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn loose_eq_zero_false() {
    assert_eval_bool(r#" 0 == false "#, true);
}

#[test]
fn loose_eq_empty_string_false() {
    assert_eval_bool(r#" "" == false "#, true);
}

#[test]
fn loose_eq_string_number() {
    assert_eval_bool(r#" "1" == 1 "#, true);
}

#[test]
fn strict_eq_string_number() {
    assert_eval_bool(r#" 1 === "1" "#, false);
}

#[test]
fn null_eq_undefined() {
    assert_eval_bool(r#" null == undefined "#, true);
}

#[test]
fn null_strict_neq_undefined() {
    assert_eval_bool(r#" null === undefined "#, false);
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: class patterns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn class_super_constructor_args() {
    assert_eval_int(
        r#"
        class Animal {
            constructor(legs) { this.legs = legs; }
        }
        class Dog extends Animal {
            constructor() { super(4); }
        }
        let d = new Dog();
        d.legs
    "#,
        4,
    );
}

#[test]
fn class_super_method_call() {
    assert_eval_str(
        r#"
        class Base {
            greet() { return "hello"; }
        }
        class Child extends Base {
            greet() { return super.greet() + " world"; }
        }
        new Child().greet()
    "#,
        "hello world",
    );
}

#[test]
fn class_instanceof_hierarchy() {
    assert_eval_bool(
        r#"
        class A {}
        class B extends A {}
        class C extends B {}
        let c = new C();
        c instanceof A
    "#,
        true,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: switch statement
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn switch_basic() {
    assert_eval_str(
        r#"
        let x = 2;
        let result;
        switch(x) {
            case 1: result = "one"; break;
            case 2: result = "two"; break;
            case 3: result = "three"; break;
            default: result = "other";
        }
        result
    "#,
        "two",
    );
}

#[test]
fn switch_default_case() {
    assert_eval_str(
        r#"
        let x = 99;
        let result;
        switch(x) {
            case 1: result = "one"; break;
            default: result = "default";
        }
        result
    "#,
        "default",
    );
}

#[test]
fn switch_fall_through_multi() {
    assert_eval_int(
        r#"
        let x = 1;
        let count = 0;
        switch(x) {
            case 1: count = count + 1;
            case 2: count = count + 1;
            case 3: count = count + 1; break;
            case 4: count = count + 1;
        }
        count
    "#,
        3,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: Map and Set
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn map_set_get_has() {
    assert_eval_int(
        r#"
        let m = new Map();
        m.set("a", 10);
        m.set("b", 20);
        m.get("a") + m.get("b")
    "#,
        30,
    );
}

#[test]
fn map_size() {
    assert_eval_int(
        r#"
        let m = new Map();
        m.set("x", 1);
        m.set("y", 2);
        m.set("z", 3);
        m.size
    "#,
        3,
    );
}

#[test]
fn map_has() {
    assert_eval_bool(
        r#"
        let m = new Map();
        m.set("key", "val");
        m.has("key")
    "#,
        true,
    );
}

#[test]
fn map_delete() {
    assert_eval_bool(
        r#"
        let m = new Map();
        m.set("key", "val");
        m.delete("key");
        !m.has("key")
    "#,
        true,
    );
}

#[test]
fn set_add_has() {
    assert_eval_bool(
        r#"
        let s = new Set();
        s.add(1);
        s.add(2);
        s.add(1);
        s.has(1) && s.size === 2
    "#,
        true,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: regex
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn regex_test_match() {
    assert_eval_bool(r#" /hello/.test("say hello world") "#, true);
}

#[test]
fn regex_test_no_match_v2() {
    assert_eval_bool(r#" /xyz/.test("hello world") "#, false);
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: JSON roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn json_stringify_parse_roundtrip() {
    assert_eval_int(
        r#"
        let obj = {"a": 1, "b": 2};
        let s = JSON.stringify(obj);
        let parsed = JSON.parse(s);
        parsed.a + parsed.b
    "#,
        3,
    );
}

#[test]
fn json_stringify_array_v2() {
    assert_eval_str(
        r#" JSON.stringify([1, 2, 3]) "#,
        "[1,2,3]",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: string edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_length_basic() {
    assert_eval_int(r#" "hello".length "#, 5);
}

#[test]
fn string_trim_basic() {
    assert_eval_str(r#" "  hello  ".trim() "#, "hello");
}

#[test]
fn string_charcodeat() {
    assert_eval_int(r#" "hello".charCodeAt(1) "#, 101);
}

#[test]
fn string_touppercase() {
    assert_eval_str(r#" "hello".toUpperCase() "#, "HELLO");
}

#[test]
fn string_tolowercase() {
    assert_eval_str(r#" "HELLO".toLowerCase() "#, "hello");
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: spread on strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn spread_string_in_array() {
    assert_eval_str(
        r#"
        let arr = [..."hi", "!"];
        arr.join("")
    "#,
        "hi!",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: for-of with destructuring
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn for_of_array_destructure() {
    assert_eval_int(
        r#"
        let pairs = [[1, 2], [3, 4], [5, 6]];
        let sum = 0;
        for (let [a, b] of pairs) {
            sum = sum + a + b;
        }
        sum
    "#,
        21,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: recursive functions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn recursive_fibonacci_v2() {
    assert_eval_int(
        r#"
        function fib(n) {
            if (n <= 1) return n;
            return fib(n - 1) + fib(n - 2);
        }
        fib(10)
    "#,
        55,
    );
}

#[test]
fn recursive_factorial() {
    assert_eval_int(
        r#"
        function fact(n) {
            if (n <= 1) return 1;
            return n * fact(n - 1);
        }
        fact(10)
    "#,
        3628800,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// String coercion: array and object toString in concatenation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_concat_array() {
    assert_eval_str(r#" '' + [1,2,3] "#, "1,2,3");
}

#[test]
fn string_concat_object() {
    assert_eval_str(r#" '' + {} "#, "[object Object]");
}

#[test]
fn string_concat_nested_array() {
    assert_eval_str(r#" '' + [1,[2,3],4] "#, "1,2,3,4");
}

#[test]
fn string_concat_null() {
    assert_eval_str(r#" '' + null "#, "null");
}

#[test]
fn string_concat_undefined() {
    assert_eval_str(r#" '' + undefined "#, "undefined");
}

#[test]
fn string_concat_bool() {
    assert_eval_str(r#" '' + true + false "#, "truefalse");
}

// ═══════════════════════════════════════════════════════════════════════
// do-while loops
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn do_while_basic() {
    assert_eval_int(r#" let i = 0; do { i++; } while(i < 5); i "#, 5);
}

#[test]
fn do_while_runs_once() {
    assert_eval_int(r#" let i = 0; do { i++; } while(false); i "#, 1);
}

// ═══════════════════════════════════════════════════════════════════════
// Assignment operators
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn assign_plus_eq() {
    assert_eval_int(r#" let x = 5; x += 3; x "#, 8);
}

#[test]
fn assign_minus_eq() {
    assert_eval_int(r#" let x = 10; x -= 3; x "#, 7);
}

#[test]
fn assign_mul_eq() {
    assert_eval_int(r#" let x = 4; x *= 3; x "#, 12);
}

#[test]
fn assign_div_eq() {
    assert_eval_int(r#" let x = 12; x /= 4; x "#, 3);
}

#[test]
fn assign_mod_eq() {
    assert_eval_int(r#" let x = 10; x %= 3; x "#, 1);
}

#[test]
fn assign_exp_eq() {
    assert_eval_int(r#" let x = 2; x **= 10; x "#, 1024);
}

#[test]
fn assign_and_eq() {
    assert_eval_int(r#" let x = 1; x &&= 0; x "#, 0);
}

#[test]
fn assign_or_eq() {
    assert_eval_int(r#" let x = 0; x ||= 42; x "#, 42);
}

#[test]
fn assign_nullish_eq() {
    assert_eval_int(r#" let x = null; x ??= 42; x "#, 42);
}

#[test]
fn assign_bitand_eq() {
    assert_eval_int(r#" let x = 7; x &= 3; x "#, 3);
}

#[test]
fn assign_bitor_eq() {
    assert_eval_int(r#" let x = 5; x |= 3; x "#, 7);
}

#[test]
fn assign_bitxor_eq() {
    assert_eval_int(r#" let x = 7; x ^= 3; x "#, 4);
}

#[test]
fn assign_shl_eq() {
    assert_eval_int(r#" let x = 1; x <<= 3; x "#, 8);
}

#[test]
fn assign_shr_eq() {
    assert_eval_int(r#" let x = 16; x >>= 2; x "#, 4);
}

#[test]
fn assign_obj_prop_plus_eq() {
    assert_eval_int(r#" let o = {x: 10}; o.x += 5; o.x "#, 15);
}

#[test]
fn assign_arr_idx_plus_eq() {
    assert_eval_int(r#" let a = [1,2,3]; a[1] += 10; a[1] "#, 12);
}

// ═══════════════════════════════════════════════════════════════════════
// Loose equality edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn loose_eq_null_undefined() {
    assert_eval_bool(r#" null == undefined "#, true);
}

#[test]
fn loose_eq_null_not_zero() {
    assert_eval_bool(r#" null == 0 "#, false);
}

#[test]
fn loose_eq_null_not_empty_str() {
    assert_eval_bool(r#" null == '' "#, false);
}

#[test]
fn loose_eq_zero_false_v2() {
    assert_eval_bool(r#" 0 == false "#, true);
}

#[test]
fn loose_eq_one_true() {
    assert_eval_bool(r#" 1 == true "#, true);
}

#[test]
fn loose_eq_str_num() {
    assert_eval_bool(r#" '42' == 42 "#, true);
}

#[test]
fn loose_eq_empty_str_zero() {
    assert_eval_bool(r#" '' == 0 "#, true);
}

// ═══════════════════════════════════════════════════════════════════════
// typeof additional
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn typeof_array_basic() {
    assert_eval_str(r#" typeof [] "#, "object");
}

#[test]
fn typeof_null_is_object_v2() {
    assert_eval_str(r#" typeof null "#, "object");
}

#[test]
fn typeof_regex() {
    assert_eval_str(r#" typeof /abc/ "#, "object");
}

// ═══════════════════════════════════════════════════════════════════════
// Getter/setter in object literals
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn getter_obj_literal() {
    assert_eval_int(
        r#"
        let obj = { get x() { return 42; } };
        obj.x
    "#,
        42,
    );
}

#[test]
fn setter_obj_literal_coerce() {
    assert_eval_int(
        r#"
        let obj = {
            _v: 0,
            get v() { return this._v; },
            set v(n) { this._v = n * 2; }
        };
        obj.v = 5;
        obj.v
    "#,
        10,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Method shorthand in object literal
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn method_shorthand_obj() {
    assert_eval_str(
        r#"
        let obj = { greet(name) { return "hello " + name; } };
        obj.greet("world")
    "#,
        "hello world",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Computed property names
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn computed_prop_name_concat() {
    assert_eval_int(
        r#"
        let key = "x";
        let obj = { [key]: 42, [key + "2"]: 99 };
        obj.x + obj.x2
    "#,
        141,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Object.create and Array.of
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn object_create_basic() {
    assert_eval_str(
        r#"
        let proto = { greet() { return "hello"; } };
        let obj = Object.create(proto);
        obj.greet()
    "#,
        "hello",
    );
}

#[test]
fn object_create_null() {
    assert_eval_str(r#" typeof Object.create(null) "#, "object");
}

#[test]
fn array_of_basic() {
    assert_eval_str(r#" Array.of(1, 2, 3).join(",") "#, "1,2,3");
}

// ═══════════════════════════════════════════════════════════════════════
// Rest params and default params
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rest_params_sum() {
    assert_eval_int(
        r#"
        function sum(...args) { return args.reduce((a,b) => a+b, 0); }
        sum(1, 2, 3, 4, 5)
    "#,
        15,
    );
}

#[test]
fn default_params_basic() {
    assert_eval_str(
        r#"
        function greet(name = "world") { return "hello " + name; }
        greet() + " | " + greet("rust")
    "#,
        "hello world | hello rust",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Number.toString with radix
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn number_to_string_base10() {
    assert_eval_str(r#" (42).toString() "#, "42");
}

#[test]
fn number_to_string_hex() {
    assert_eval_str(r#" (255).toString(16) "#, "ff");
}

#[test]
fn number_to_string_binary() {
    assert_eval_str(r#" (10).toString(2) "#, "1010");
}

// ═══════════════════════════════════════════════════════════════════════
// Array.splice returning removed elements
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_splice_return() {
    assert_eval_str(
        r#"
        let a = [1,2,3,4,5];
        let removed = a.splice(1, 2);
        removed.join(",") + "|" + a.join(",")
    "#,
        "2,3|1,4,5",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// String split with limit
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn string_split_with_limit() {
    assert_eval_str(r#" "a,b,c,d".split(",", 2).join("-") "#, "a-b");
}

// ═══════════════════════════════════════════════════════════════════════
// Infinity and NaN types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn typeof_infinity() {
    assert_eval_str(r#" typeof Infinity "#, "number");
}

#[test]
fn typeof_nan() {
    assert_eval_str(r#" typeof NaN "#, "number");
}

#[test]
fn negative_zero_division() {
    assert_eval_bool(r#" 1 / (-0) === -Infinity "#, true);
}

// ═══════════════════════════════════════════════════════════════════════
// Optional chaining deep
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn optional_chain_deep() {
    assert_eval_int(
        r#"
        let o = {a: {b: {c: 42}}};
        o?.a?.b?.c
    "#,
        42,
    );
}

#[test]
fn optional_chain_null_deep() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let o = {a: null}; o?.a?.b?.c")
        .expect("eval");
    assert!(matches!(out, Object::Undefined), "expected Undefined, got {out:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Nested ternary
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nested_ternary_classify() {
    assert_eval_str(
        r#"
        function classify(n) { return n > 0 ? "positive" : n < 0 ? "negative" : "zero"; }
        classify(5) + "," + classify(-3) + "," + classify(0)
    "#,
        "positive,negative,zero",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// IIFE (immediately invoked function expression)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn iife_arrow() {
    assert_eval_int(r#" ((x) => x * x)(7) "#, 49);
}

#[test]
fn iife_function() {
    assert_eval_int(r#" (function(x) { return x + 1; })(41) "#, 42);
}

// ═══════════════════════════════════════════════════════════════════════
// Chained array methods
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chained_filter_sort_map() {
    assert_eval_str(
        r#"
        [5,3,8,1,9,2].filter(x => x > 3).sort((a,b) => a-b).map(x => x*10).join(",")
    "#,
        "50,80,90",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Error handling: name and message
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_name_is_error() {
    assert_eval_str(
        r#" try { throw new Error("test"); } catch(e) { e.name } "#,
        "Error",
    );
}

#[test]
fn error_message_is_hello() {
    assert_eval_str(
        r#" try { throw new Error("hello"); } catch(e) { e.message } "#,
        "hello",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Closure mutual recursion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mutual_recursion() {
    assert_eval_str(
        r#"
        function isEven(n) { return n === 0 ? true : isOdd(n - 1); }
        function isOdd(n) { return n === 0 ? false : isEven(n - 1); }
        isEven(10) + "," + isOdd(7)
    "#,
        "true,true",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Nullish coalescing chain
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nullish_chain_zero() {
    assert_eval_str(
        r#"
        let a = null;
        let b = undefined;
        let c = 0;
        (a ?? "A") + "," + (b ?? "B") + "," + (c ?? "C")
    "#,
        "A,B,0",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// for-in on object
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn for_in_collects_keys() {
    assert_eval_str(
        r#"
        let result = [];
        for (let k in {a:1, b:2, c:3}) { result.push(k); }
        result.join(",")
    "#,
        "a,b,c",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Map/Set advanced
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn map_has_and_delete() {
    assert_eval_str(
        r#"
        let m = new Map();
        m.set("a", 1);
        let has = m.has("a");
        m.delete("a");
        has + "," + m.has("a") + "," + m.size
    "#,
        "true,false,0",
    );
}

#[test]
fn set_for_of_iterate() {
    assert_eval_str(
        r#"
        let s = new Set();
        s.add(10); s.add(20); s.add(30);
        let result = [];
        for (let v of s) { result.push(v); }
        result.join(",")
    "#,
        "10,20,30",
    );
}

#[test]
fn map_for_of_iterate() {
    assert_eval_str(
        r#"
        let m = new Map();
        m.set("a", 1); m.set("b", 2);
        let result = [];
        for (let [k,v] of m) { result.push(k + ":" + v); }
        result.join(",")
    "#,
        "a:1,b:2",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Generator advanced
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn generator_return_stops() {
    assert_eval_str(
        r#"
        function* gen() { yield 1; yield 2; yield 3; }
        let g = gen();
        let a = g.next().value;
        let b = g.return(99);
        let c = g.next();
        a + "," + b.value + "," + b.done + "," + c.done
    "#,
        "1,99,true,true",
    );
}

#[test]
fn generator_next_value_injection() {
    assert_eval_int(
        r#"
        function* gen() { let x = yield 1; yield x + 10; }
        let g = gen();
        g.next();
        g.next(5).value
    "#,
        15,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Template literal edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn template_with_method_call() {
    assert_eval_str(
        r#"
        let a = "hello";
        `${a.toUpperCase()} ${"world".length}`
    "#,
        "HELLO 5",
    );
}

#[test]
fn template_multiline_contains_newline() {
    assert_eval_bool(
        r#"
        let name = "world";
        let msg = `hello
${name}`;
        msg.includes("\n")
    "#,
        true,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Comma operator in for loop
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn comma_in_for_loop() {
    assert_eval_int(
        r#" let x = 0; for(let i = 0, j = 10; i < 3; i++, j--) { x = x + j; } x "#,
        27,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Object.is edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn object_is_nan_nan() {
    assert_eval_bool(r#" Object.is(NaN, NaN) "#, true);
}

#[test]
fn object_is_zero_negzero() {
    assert_eval_bool(r#" Object.is(0, -0) "#, false);
}

// ═══════════════════════════════════════════════════════════════════════
// Array unshift
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn array_unshift_basic() {
    assert_eval_str(
        r#"
        let a = [3, 4, 5];
        a.unshift(1, 2);
        a.join(",")
    "#,
        "1,2,3,4,5",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Comparison chain
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn comparison_logical_chain() {
    assert_eval_bool(r#" 1 < 2 && 2 < 3 && 3 < 4 "#, true);
}

// ═══════════════════════════════════════════════════════════════════════
// delete operator
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn delete_op_returns_true() {
    assert_eval_bool(r#" let o = {a:1}; delete o.a "#, true);
}

#[test]
fn delete_removes_key() {
    assert_eval_str(
        r#" let o = {a:1, b:2}; delete o.a; Object.keys(o).join(",") "#,
        "b",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// in operator
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn in_operator_on_object() {
    assert_eval_bool(r#" "a" in {a:1} "#, true);
}

#[test]
fn in_operator_array_index() {
    assert_eval_bool(r#" 1 in [10, 20, 30] "#, true);
}

// ═══════════════════════════════════════════════════════════════════════
// Edge cases from TypeScript test suite
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn zero_divided_by_zero_is_nan() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("0 / 0").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_nan(), "expected NaN, got {v}"),
        _ => panic!("expected NaN float, got {out:?}"),
    }
}

#[test]
fn array_out_of_bounds() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("let arr = [1,2,3]; arr[10]").expect("eval");
    assert!(matches!(out, Object::Undefined | Object::Null), "expected undefined/null, got {out:?}");
}

#[test]
fn array_negative_index() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("let arr = [1,2,3]; arr[-1]").expect("eval");
    assert!(matches!(out, Object::Undefined | Object::Null), "expected undefined/null, got {out:?}");
}

#[test]
fn string_slice_negative_ec() {
    assert_eval_str(r#" "hello".slice(-2) "#, "lo");
}

#[test]
fn string_slice_negative_range_ec() {
    assert_eval_str(r#" "hello".slice(-3, -1) "#, "ll");
}

#[test]
fn string_repeat_zero_ec() {
    assert_eval_str(r#" "hello".repeat(0) "#, "");
}

#[test]
fn string_padstart_default_space() {
    assert_eval_str(r#" "5".padStart(3) "#, "  5");
}

#[test]
fn string_concat_method_ec() {
    assert_eval_str(r#" "hello".concat(" ", "world") "#, "hello world");
}

#[test]
fn string_lastindex_not_found() {
    assert_eval_int(r#" "hello".lastIndexOf("x") "#, -1);
}

#[test]
fn array_lastindex_not_found() {
    assert_eval_int(r#" [1,2,3].lastIndexOf(5) "#, -1);
}

#[test]
fn modulo_negative_divisor_ec() {
    assert_eval_int(r#" 5 % -3 "#, 2);
}

#[test]
fn modulo_both_negative_ec() {
    assert_eval_int(r#" -5 % -3 "#, -2);
}

#[test]
fn strict_eq_null_null() {
    assert_eval_bool(r#" null === null "#, true);
}

#[test]
fn strict_eq_one_string_one() {
    assert_eval_bool(r#" 1 === "1" "#, false);
}

#[test]
fn loose_eq_empty_str_false() {
    assert_eval_bool(r#" "" == false "#, true);
}

#[test]
fn loose_eq_string_one_num_one() {
    assert_eval_bool(r#" "1" == 1 "#, true);
}

#[test]
fn prefix_increment_returns() {
    assert_eval_str(
        r#"
        let i = 5;
        let x = ++i;
        i + "," + x
    "#,
        "6,6",
    );
}

#[test]
fn prefix_decrement_returns() {
    assert_eval_str(
        r#"
        let i = 5;
        let x = --i;
        i + "," + x
    "#,
        "4,4",
    );
}

#[test]
fn post_increment_returns_old() {
    assert_eval_str(
        r#"
        let i = 5;
        let x = i++;
        i + "," + x
    "#,
        "6,5",
    );
}

#[test]
fn post_decrement_returns_old() {
    assert_eval_str(
        r#"
        let i = 5;
        let x = i--;
        i + "," + x
    "#,
        "4,5",
    );
}

#[test]
fn falsy_zero_ec() {
    assert_eval_str(r#" 0 ? "truthy" : "falsy" "#, "falsy");
}

#[test]
fn falsy_empty_string_ec() {
    assert_eval_str(r#" "" ? "truthy" : "falsy" "#, "falsy");
}

#[test]
fn falsy_null_ec() {
    assert_eval_str(r#" null ? "truthy" : "falsy" "#, "falsy");
}

#[test]
fn truthy_negative_one() {
    assert_eval_str(r#" -1 ? "truthy" : "falsy" "#, "truthy");
}

#[test]
fn truthy_empty_array_ec() {
    assert_eval_str(r#" [] ? "truthy" : "falsy" "#, "truthy");
}

#[test]
fn truthy_empty_object() {
    assert_eval_str(r#" ({}) ? "truthy" : "falsy" "#, "truthy");
}

#[test]
fn missing_property_undefined() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("let obj = {a:1}; obj.b").expect("eval");
    assert!(matches!(out, Object::Undefined), "expected Undefined, got {out:?}");
}

#[test]
fn empty_array_length() {
    assert_eval_int(r#" [].length "#, 0);
}

#[test]
fn empty_array_pop() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("[].pop()").expect("eval");
    assert!(matches!(out, Object::Undefined), "expected Undefined, got {out:?}");
}

#[test]
fn template_expr_arithmetic() {
    assert_eval_str(r#" `1 + 1 = ${1 + 1}` "#, "1 + 1 = 2");
}

#[test]
fn spread_in_array() {
    assert_eval_str(r#" [1, ...[2, 3], 4].join(",") "#, "1,2,3,4");
}

#[test]
fn spread_in_function_call_ec() {
    assert_eval_int(
        r#"
        let add = function(a, b, c) { return a + b + c; };
        add(...[1, 2, 3])
    "#,
        6,
    );
}

#[test]
fn number_tofixed_whole() {
    assert_eval_str(r#" (3).toFixed(2) "#, "3.00");
}

#[test]
fn string_replace_first_only() {
    assert_eval_str(r#" "aaa".replace("a", "b") "#, "baa");
}

#[test]
fn string_replace_all_ec() {
    assert_eval_str(r#" "aaa".replaceAll("a", "b") "#, "bbb");
}

#[test]
fn string_replace_regex_digits() {
    assert_eval_str(r#" "hello 123 world 456".replace(/\d+/, "NUM") "#, "hello NUM world 456");
}

#[test]
fn array_from_string_ec() {
    assert_eval_str(r#" Array.from("abc").join(",") "#, "a,b,c");
}

#[test]
fn array_from_with_mapfn() {
    assert_eval_str(r#" Array.from([1,2,3], x => x * 2).join(",") "#, "2,4,6");
}

#[test]
fn array_concat_multiple() {
    assert_eval_str(r#" [1,2].concat([3,4],[5]).join(",") "#, "1,2,3,4,5");
}

#[test]
fn array_sort_default_string() {
    assert_eval_str(r#" [3,1,4,1,5].sort().join(",") "#, "1,1,3,4,5");
}

#[test]
fn array_to_sorted_ec() {
    assert_eval_str(
        r#"
        let a = [3,1,2];
        let b = a.toSorted();
        a.join(",") + "|" + b.join(",")
    "#,
        "3,1,2|1,2,3",
    );
}

#[test]
fn array_with_method_ec() {
    assert_eval_str(r#" [1,2,3].with(1, 99).join(",") "#, "1,99,3");
}

#[test]
fn array_keys_iterator() {
    assert_eval_str(
        r#"
        let result = [];
        for (let k of [10,20,30].keys()) { result.push(k); }
        result.join(",")
    "#,
        "0,1,2",
    );
}

#[test]
fn array_values_iterator() {
    assert_eval_str(
        r#"
        let result = [];
        for (let v of [10,20,30].values()) { result.push(v); }
        result.join(",")
    "#,
        "10,20,30",
    );
}

#[test]
fn array_entries_iterator() {
    assert_eval_str(
        r#"
        let result = [];
        for (let [k,v] of [10,20,30].entries()) { result.push(k+":"+v); }
        result.join(",")
    "#,
        "0:10,1:20,2:30",
    );
}

#[test]
fn object_entries_map() {
    assert_eval_str(
        r#" Object.entries({a:1}).map(e=>e[0]+":"+e[1]).join(",") "#,
        "a:1",
    );
}

#[test]
fn object_assign_merge_ec() {
    assert_eval_str(
        r#"
        let t = {a:1};
        Object.assign(t, {b:2}, {c:3});
        t.a + "," + t.b + "," + t.c
    "#,
        "1,2,3",
    );
}

#[test]
fn object_freeze_prevents_mutation() {
    assert_eval_int(
        r#"
        let o = {a:1};
        Object.freeze(o);
        o.a = 99;
        o.a
    "#,
        1,
    );
}

#[test]
fn class_to_string_method() {
    assert_eval_str(
        r#"
        class Animal {
            constructor(name) { this.name = name; }
            toString() { return "Animal:" + this.name; }
        }
        "" + new Animal("cat")
    "#,
        "Animal:cat",
    );
}

#[test]
fn class_static_field_access() {
    assert_eval_int(
        r#"
        class C { static x = 42; }
        C.x
    "#,
        42,
    );
}

#[test]
fn class_instance_field_init() {
    assert_eval_int(
        r#"
        class Counter { count = 0; inc() { this.count++; } }
        let c = new Counter(); c.inc(); c.inc(); c.count
    "#,
        2,
    );
}

#[test]
fn async_returns_promise_like() {
    assert_eval_str(
        r#"
        async function f() { return 42; }
        typeof f()
    "#,
        "object",
    );
}

#[test]
fn regex_exec_captures() {
    assert_eval_str(
        r#"
        let m = /(\d+)/.exec("abc123def");
        m[0] + "," + m[1]
    "#,
        "123,123",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Bug fix parity: from formlogic-typescript bug_fixes.test.ts
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn arrow_this_in_class_constructor() {
    assert_eval_int(
        r#"
        class A {
            constructor() {
                this.val = 10;
                this.arrow = (x) => this.val + x;
            }
        }
        let a = new A();
        a.arrow(5)
    "#,
        15,
    );
}

#[test]
fn nested_template_literals_bf() {
    assert_eval_str(r#" `A ${ `B ${10}` }` "#, "A B 10");
}

#[test]
fn spread_on_string() {
    assert_eval_str(
        r#"
        let s = "hi";
        let arr = [...s, "!"];
        arr.join(",")
    "#,
        "h,i,!",
    );
}

#[test]
fn array_from_array_like_with_mapfn() {
    assert_eval_str(
        r#" Array.from({length: 3}, (_, i) => i * 10).join(",") "#,
        "0,10,20",
    );
}

#[test]
fn return_without_value_is_undefined() {
    assert_eval_str(
        r#"
        function f() { return; }
        typeof f()
    "#,
        "undefined",
    );
}

#[test]
fn function_no_return_is_undefined() {
    assert_eval_str(
        r#"
        function noop() {}
        typeof noop()
    "#,
        "undefined",
    );
}

#[test]
fn bracket_notation_method_call() {
    assert_eval_int(
        r#"
        let obj = { add(a,b) { return a+b; }, sub(a,b) { return a-b; } };
        let method = "add";
        obj[method](3, 4)
    "#,
        7,
    );
}

#[test]
fn array_bracket_function_call() {
    assert_eval_int(
        r#"
        let funcs = [() => 10, () => 20, () => 30];
        funcs[0]() + funcs[2]()
    "#,
        40,
    );
}

#[test]
fn string_constructor_array() {
    assert_eval_str(r#" String([1, 2]) "#, "1,2");
}

#[test]
fn string_constructor_empty_array() {
    assert_eval_str(r#" String([]) "#, "");
}

#[test]
fn string_constructor_object() {
    assert_eval_str(r#" String({}) "#, "[object Object]");
}

#[test]
fn string_constructor_null() {
    assert_eval_str(r#" String(null) "#, "null");
}

#[test]
fn string_constructor_undefined() {
    assert_eval_str(r#" String(undefined) "#, "undefined");
}

#[test]
fn trailing_comma_in_array_literal() {
    assert_eval_int(r#" [1, 2, 3,].length "#, 3);
}

#[test]
fn trailing_comma_in_fn_call() {
    assert_eval_int(
        r#"
        function add(a, b) { return a + b; }
        add(3, 4,)
    "#,
        7,
    );
}

#[test]
fn object_destr_rest_element() {
    assert_eval_str(
        r#"
        let {a, ...rest} = {a: 1, b: 2, c: 3};
        a + "," + rest.b + "," + rest.c
    "#,
        "1,2,3",
    );
}

#[test]
fn for_of_map_destructuring() {
    assert_eval_int(
        r#"
        let m = new Map();
        m.set("x", 10); m.set("y", 20);
        let sum = 0;
        for (let [k, v] of m) { sum += v; }
        sum
    "#,
        30,
    );
}

#[test]
fn destr_array_holes() {
    assert_eval_str(r#" let [a, , c] = [1, 2, 3]; a + "," + c "#, "1,3");
}

#[test]
fn destr_array_leading_hole() {
    assert_eval_str(r#" let [, b, , d] = [1, 2, 3, 4]; b + "," + d "#, "2,4");
}

#[test]
fn valueof_loose_equality() {
    assert_eval_str(
        r#"
        class N { constructor(v) { this.v = v; } valueOf() { return this.v; } }
        (new N(5) == 5) + "," + (new N(5) > new N(3)) + "," + (new N(5) === 5)
    "#,
        "true,true,false",
    );
}

#[test]
fn class_arrow_method_closure() {
    assert_eval_int(
        r#"
        class Counter {
            constructor() { this.count = 0; }
            makeInc() { return () => { this.count++; }; }
        }
        let c = new Counter();
        let inc = c.makeInc();
        inc(); inc(); inc();
        c.count
    "#,
        3,
    );
}

#[test]
fn array_reduce_initial_value() {
    assert_eval_int(r#" [1,2,3,4].reduce((a,b) => a + b, 10) "#, 20);
}

#[test]
fn array_reduce_no_initial_bf() {
    assert_eval_int(r#" [1,2,3,4].reduce((a,b) => a + b) "#, 10);
}

#[test]
fn string_search_regex() {
    assert_eval_int(r#" "hello123world".search(/[0-9]+/) "#, 5);
}

#[test]
fn string_match_first() {
    assert_eval_str(
        r#"
        let m = "abc123def456".match(/[0-9]+/);
        m[0]
    "#,
        "123",
    );
}

#[test]
fn map_constructor_from_array() {
    assert_eval_int(
        r#"
        let m = new Map([["a", 1], ["b", 2]]);
        m.get("a") + m.get("b")
    "#,
        3,
    );
}

#[test]
fn set_constructor_from_array() {
    assert_eval_int(
        r#"
        let s = new Set([1, 2, 3, 2, 1]);
        s.size
    "#,
        3,
    );
}

#[test]
fn closure_counter_factory() {
    // Use "value" instead of "get" to avoid parser getter keyword conflict
    assert_eval_str(
        r#"
        function makeCounter() {
            let count = 0;
            return {
                inc() { count++; },
                value() { return count; }
            };
        }
        let c = makeCounter();
        c.inc(); c.inc(); c.inc();
        "" + c.value()
    "#,
        "3",
    );
}

#[test]
fn promise_resolve_typeof() {
    assert_eval_str(r#" typeof Promise.resolve(42) "#, "object");
}

#[test]
fn math_random_in_range() {
    assert_eval_bool(
        r#"
        let r = Math.random();
        r >= 0 && r < 1
    "#,
        true,
    );
}

#[test]
fn math_pi_value() {
    assert_eval_int(r#" Math.round(Math.PI * 1000) "#, 3142);
}

#[test]
fn math_log_e_is_one() {
    assert_eval_int(r#" Math.round(Math.log(Math.E) * 100) / 100 "#, 1);
}

// ═══════════════════════════════════════════════════════════════════════
// TypeScript parity: additional uncovered patterns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parsefloat_no_args_is_nan() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("parseFloat()").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_nan(), "expected NaN, got {v}"),
        _ => panic!("expected NaN float, got {out:?}"),
    }
}

#[test]
fn unary_plus_empty_array() {
    assert_eval_number(r#" +[] "#, 0.0);
}

#[test]
fn unary_plus_single_array() {
    assert_eval_number(r#" +[5] "#, 5.0);
}

#[test]
fn unary_plus_multi_array_is_nan() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("+[1, 2]").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_nan(), "expected NaN, got {v}"),
        _ => panic!("expected NaN float, got {out:?}"),
    }
}

#[test]
fn math_max_no_args() {
    assert_eval_float(r#" Math.max() "#, f64::NEG_INFINITY);
}

#[test]
fn math_min_no_args() {
    assert_eval_float(r#" Math.min() "#, f64::INFINITY);
}

#[test]
fn math_e_constant() {
    assert_eval_bool(r#" Math.E > 2.718 && Math.E < 2.719 "#, true);
}

#[test]
fn number_max_safe_integer() {
    assert_eval_bool(r#" Number.MAX_SAFE_INTEGER === 9007199254740991 "#, true);
}

#[test]
fn number_epsilon_positive() {
    assert_eval_bool(r#" Number.EPSILON > 0 && Number.EPSILON < 1 "#, true);
}

#[test]
fn string_codepointat() {
    assert_eval_int(r#" "abc".codePointAt(0) "#, 97);
}

#[test]
fn string_at_negative_v2() {
    assert_eval_str(r#" "abc".at(-1) "#, "c");
}

#[test]
fn string_at_positive() {
    assert_eval_str(r#" "abc".at(0) "#, "a");
}

#[test]
fn optional_catch_binding() {
    assert_eval_bool(
        r#"
        let caught = false;
        try { throw "err"; } catch { caught = true; }
        caught
    "#,
        true,
    );
}

#[test]
fn spread_string_to_array() {
    assert_eval_str(r#" [..."hello"].join(",") "#, "h,e,l,l,o");
}

#[test]
fn let_redeclaration_in_function_scope() {
    assert_eval_int(
        r#"
        let x = 1;
        let f = function() { let x = 2; return x; };
        x + f()
    "#,
        3,
    );
}

#[test]
fn typeof_function_expression() {
    assert_eval_str(r#" typeof function(){} "#, "function");
}

#[test]
fn return_bare_is_undefined() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("function f() { return; } f()").expect("eval");
    assert!(matches!(out, Object::Undefined), "expected Undefined, got {out:?}");
}

#[test]
fn no_return_is_undefined() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("function noop() {} noop()").expect("eval");
    assert!(matches!(out, Object::Undefined), "expected Undefined, got {out:?}");
}

#[test]
fn array_isarray_new_array() {
    assert_eval_bool(r#" Array.isArray(new Array(3)) "#, true);
}

#[test]
fn number_is_safe_integer_max() {
    assert_eval_bool(r#" Number.isSafeInteger(Number.MAX_SAFE_INTEGER) "#, true);
}

#[test]
fn number_to_string_octal() {
    assert_eval_str(r#" (255).toString(8) "#, "377");
}

#[test]
fn string_from_charcode() {
    assert_eval_str(r#" String.fromCharCode(72, 105) "#, "Hi");
}

#[test]
fn array_flat_infinity() {
    assert_eval_str(
        r#" [1, [2, [3, [4]]]].flat(Infinity).join(",") "#,
        "1,2,3,4",
    );
}

#[test]
fn math_abs_negative() {
    assert_eval_int(r#" Math.abs(-42) "#, 42);
}

#[test]
fn math_floor_negative() {
    assert_eval_int(r#" Math.floor(-3.2) "#, -4);
}

#[test]
fn math_ceil_negative() {
    assert_eval_int(r#" Math.ceil(-3.8) "#, -3);
}

#[test]
fn exponentiation_float() {
    assert_eval_float(r#" 2.5 ** 2 "#, 6.25);
}

#[test]
fn array_fill_with_range_v2() {
    assert_eval_str(r#" [1,2,3,4,5].fill(0, 1, 3).join(",") "#, "1,0,0,4,5");
}

#[test]
fn string_normalize_basic() {
    assert_eval_str(r#" "hello".normalize() "#, "hello");
}

#[test]
fn array_flat_map_filter() {
    assert_eval_str(
        r#" [1,2,3,4].flatMap(x => x % 2 === 0 ? [x] : []).join(",") "#,
        "2,4",
    );
}

#[test]
fn string_split_empty() {
    assert_eval_str(r#" "abc".split("").join(",") "#, "a,b,c");
}

#[test]
fn array_index_of_not_found() {
    assert_eval_int(r#" [1,2,3].indexOf(5) "#, -1);
}

#[test]
fn object_has_own_property() {
    assert_eval_bool(r#" ({a:1}).hasOwnProperty("a") "#, true);
}

#[test]
fn object_has_own_property_missing() {
    assert_eval_bool(r#" ({a:1}).hasOwnProperty("b") "#, false);
}

// ── Comprehensive feature parity tests (batch 8) ──────────────────────

// --- Tagged template literals ---
#[test]
fn tagged_template_basic() {
    assert_eval_str(
        r#" function tag(s,...v) { return s[0] + v[0] + s[1]; } tag`a${1}b` "#,
        "a1b",
    );
}

// --- Map comprehensive ---
#[test]
fn map_basic_set_get() {
    assert_eval_int(r#" let m = new Map(); m.set("a",1); m.get("a") "#, 1);
}

#[test]
fn map_size_from_ctor() {
    assert_eval_int(r#" let m = new Map([["a",1],["b",2]]); m.size "#, 2);
}

#[test]
fn map_has_key() {
    assert_eval_bool(r#" let m = new Map([["a",1]]); m.has("a") "#, true);
}

#[test]
fn map_delete_reduces_size() {
    assert_eval_int(r#" let m = new Map([["a",1]]); m.delete("a"); m.size "#, 0);
}

#[test]
fn map_keys_iteration() {
    assert_eval_str(
        r#" let m = new Map([["a",1],["b",2]]); let r = []; for(let k of m.keys()) r.push(k); r.join(",") "#,
        "a,b",
    );
}

#[test]
fn map_values_iteration() {
    assert_eval_str(
        r#" let m = new Map([["a",1],["b",2]]); let r = []; for(let v of m.values()) r.push(v); r.join(",") "#,
        "1,2",
    );
}

#[test]
fn map_entries_iteration() {
    assert_eval_str(
        r#" let m = new Map([["a",1]]); let r = []; for(let [k,v] of m.entries()) r.push(k + "=" + v); r.join(",") "#,
        "a=1",
    );
}

// --- Set comprehensive ---
#[test]
fn set_dedup() {
    assert_eval_int(r#" let s = new Set([1,2,3,2]); s.size "#, 3);
}

#[test]
fn set_has_value() {
    assert_eval_bool(r#" let s = new Set([1,2,3]); s.has(2) "#, true);
}

#[test]
fn set_values_iteration() {
    assert_eval_str(
        r#" let s = new Set([1,2,3]); let r = []; for(let v of s.values()) r.push(v); r.join(",") "#,
        "1,2,3",
    );
}

// --- Class comprehensive ---
#[test]
fn class_constructor_and_method() {
    assert_eval_int(
        r#" class A { constructor(x) { this.x = x; } get() { return this.x; } } new A(5).get() "#,
        5,
    );
}

#[test]
fn class_extends_super() {
    assert_eval_int(
        r#" class A { constructor(x) { this.x = x; } } class B extends A { constructor(x) { super(x * 2); } } new B(3).x "#,
        6,
    );
}

#[test]
fn class_static_method_call() {
    assert_eval_int(r#" class A { static foo() { return 42; } } A.foo() "#, 42);
}

#[test]
fn class_getter_computed() {
    assert_eval_int(
        r#" class A { constructor(x) { this._x = x; } get x() { return this._x * 2; } } new A(5).x "#,
        10,
    );
}

#[test]
fn class_setter_with_getter() {
    assert_eval_int(
        r#" class A { constructor() { this._x = 0; } set x(v) { this._x = v * 2; } get x() { return this._x; } } let a = new A(); a.x = 5; a.x "#,
        10,
    );
}

// --- Error handling ---
#[test]
fn try_catch_string_throw_v2() {
    assert_eval_str(r#" let r; try { throw "err"; } catch(e) { r = e; } r "#, "err");
}

#[test]
fn try_finally_runs_always() {
    assert_eval_int(r#" let r = 0; try { r = 1; } finally { r += 10; } r "#, 11);
}

#[test]
fn try_catch_finally_combined() {
    assert_eval_int(
        r#" let r = 0; try { throw "e"; } catch(e) { r = 1; } finally { r += 10; } r "#,
        11,
    );
}

#[test]
fn error_message_new_error() {
    assert_eval_str(
        r#" try { throw new Error("boom"); } catch(e) { e.message } "#,
        "boom",
    );
}

// --- Regex ---
#[test]
fn regex_test_positive() {
    assert_eval_bool(r#" /abc/.test("xabcy") "#, true);
}

#[test]
fn regex_exec_capture_group() {
    assert_eval_str(r#" /(\d+)/.exec("abc123")[1] "#, "123");
}

#[test]
fn regex_replace_pattern() {
    assert_eval_str(r#" "hello world".replace(/world/, "rust") "#, "hello rust");
}

// --- Nullish coalescing and optional chaining ---
#[test]
fn nullish_coalescing_null() {
    assert_eval_int(r#" null ?? 42 "#, 42);
}

#[test]
fn nullish_coalescing_defined() {
    assert_eval_int(r#" 0 ?? 42 "#, 0);
}

#[test]
fn optional_chaining_defined() {
    assert_eval_int(r#" let o = {a: {b: 1}}; o.a?.b "#, 1);
}

#[test]
fn optional_chaining_null_b8() {
    assert_eval_undefined(r#" let o = null; o?.a "#);
}

#[test]
fn optional_chaining_call() {
    assert_eval_int(r#" let o = {f: () => 42}; o.f?.() "#, 42);
}

#[test]
fn optional_chaining_call_null() {
    assert_eval_undefined(r#" let o = {}; o.f?.() "#);
}

// --- Logical assignment ---
#[test]
fn logical_and_assign() {
    assert_eval_int(r#" let a = 1; a &&= 2; a "#, 2);
}

#[test]
fn logical_or_assign_b8() {
    assert_eval_int(r#" let a = 0; a ||= 5; a "#, 5);
}

#[test]
fn nullish_assign_b8() {
    assert_eval_int(r#" let a = null; a ??= 10; a "#, 10);
}

// --- Numeric builtins ---
#[test]
fn number_is_integer_b8() {
    assert_eval_bool(r#" Number.isInteger(5) "#, true);
}

#[test]
fn number_is_finite_inf() {
    assert_eval_bool(r#" Number.isFinite(Infinity) "#, false);
}

#[test]
fn number_is_nan_b8() {
    assert_eval_bool(r#" Number.isNaN(NaN) "#, true);
}

#[test]
fn number_parse_int_b8() {
    assert_eval_int(r#" Number.parseInt("42abc") "#, 42);
}

#[test]
fn number_parse_float_b8() {
    assert_eval_float(r#" Number.parseFloat("3.14abc") "#, 3.14);
}

#[test]
fn parse_int_hex() {
    assert_eval_int(r#" parseInt("0xff", 16) "#, 255);
}

#[test]
fn to_fixed_two_decimals() {
    assert_eval_str(r#" (3.14159).toFixed(2) "#, "3.14");
}

// --- String builtins ---
#[test]
fn string_from_char_code_multi() {
    assert_eval_str(r#" String.fromCharCode(65, 66, 67) "#, "ABC");
}

#[test]
fn string_from_code_point_b8() {
    assert_eval_str(r#" String.fromCodePoint(65) "#, "A");
}

// --- typeof comprehensive ---
#[test]
fn typeof_string_literal() {
    assert_eval_str(r#" typeof "hello" "#, "string");
}

#[test]
fn typeof_number_literal() {
    assert_eval_str(r#" typeof 42 "#, "number");
}

#[test]
fn typeof_boolean_literal() {
    assert_eval_str(r#" typeof true "#, "boolean");
}

#[test]
fn typeof_undefined_val() {
    assert_eval_str(r#" typeof undefined "#, "undefined");
}

#[test]
fn typeof_null_val() {
    assert_eval_str(r#" typeof null "#, "object");
}

#[test]
fn typeof_function_arrow() {
    assert_eval_str(r#" typeof (() => {}) "#, "function");
}

#[test]
fn typeof_object_literal() {
    assert_eval_str(r#" typeof ({}) "#, "object");
}

#[test]
fn typeof_array_val() {
    assert_eval_str(r#" typeof [] "#, "object");
}

// --- instanceof ---
#[test]
fn instanceof_class_instance() {
    assert_eval_bool(r#" class A {} new A() instanceof A "#, true);
}

// --- in operator ---
#[test]
fn in_operator_present() {
    assert_eval_bool(r#" "a" in {a:1} "#, true);
}

// --- delete ---
#[test]
fn delete_property_from_object() {
    assert_eval_str(r#" let o = {a:1,b:2}; delete o.a; Object.keys(o).join(",") "#, "b");
}

// --- comma operator ---
#[test]
fn comma_operator_returns_last_b8() {
    assert_eval_int(r#" (1, 2, 3) "#, 3);
}

// --- labeled break/continue ---
#[test]
fn labeled_break_outer() {
    assert_eval_int(
        r#" let r = 0; outer: for(let i=0;i<3;i++) { for(let j=0;j<3;j++) { if(j===1) break outer; r++; } } r "#,
        1,
    );
}

#[test]
fn labeled_continue_outer() {
    assert_eval_int(
        r#" let r = 0; outer: for(let i=0;i<3;i++) { for(let j=0;j<3;j++) { if(j===1) continue outer; r++; } } r "#,
        3,
    );
}

// --- switch ---
#[test]
fn switch_case_match() {
    assert_eval_str(
        r#" let r; switch(2) { case 1: r = "a"; break; case 2: r = "b"; break; default: r = "c"; } r "#,
        "b",
    );
}

#[test]
fn switch_fallthrough_v2() {
    assert_eval_str(
        r#" let r = ""; switch(1) { case 1: r += "a"; case 2: r += "b"; break; case 3: r += "c"; } r "#,
        "ab",
    );
}

// --- Date ---
#[test]
fn date_now_returns_number() {
    assert_eval_str(r#" typeof Date.now() "#, "number");
}

// --- JSON ---
#[test]
fn json_stringify_complex() {
    assert_eval_str(r#" JSON.stringify({a:1,b:[2,3]}) "#, r#"{"a":1,"b":[2,3]}"#);
}

#[test]
fn json_parse_returns_value() {
    assert_eval_int(r#" JSON.parse('{"a":1}').a "#, 1);
}

// --- globalThis ---
#[test]
fn global_this_type() {
    assert_eval_str(r#" typeof globalThis "#, "object");
}

// --- Generator ---
#[test]
fn generator_next_protocol() {
    assert_eval_int(
        r#" function* g() { yield 1; yield 2; yield 3; } let it = g(); it.next().value + it.next().value + it.next().value "#,
        6,
    );
}

// --- for-of ---
#[test]
fn for_of_array_sum_b8() {
    assert_eval_int(r#" let r = 0; for(let x of [1,2,3]) r += x; r "#, 6);
}

#[test]
fn for_of_string_concat() {
    assert_eval_str(r#" let r = ""; for(let c of "abc") r += c; r "#, "abc");
}

#[test]
fn for_of_array_destructuring() {
    assert_eval_int(r#" let r = 0; for(let [k,v] of [[1,2],[3,4]]) r += k + v; r "#, 10);
}

// --- for-in ---
#[test]
fn for_in_object_keys_b8() {
    assert_eval_str(r#" let r = ""; for(let k in {a:1,b:2}) r += k; r "#, "ab");
}

// --- Destructuring ---
#[test]
fn array_destructuring_basic() {
    assert_eval_int(r#" let [a,b] = [1,2]; a + b "#, 3);
}

#[test]
fn array_destructuring_rest() {
    assert_eval_str(r#" let [a,...b] = [1,2,3]; b.join(",") "#, "2,3");
}

#[test]
fn object_destructuring_basic() {
    assert_eval_int(r#" let {x,y} = {x:1,y:2}; x + y "#, 3);
}

#[test]
fn object_destructuring_rename() {
    assert_eval_int(r#" let {x:a, y:b} = {x:1, y:2}; a + b "#, 3);
}

#[test]
fn object_destructuring_default() {
    assert_eval_int(r#" let {x=10} = {}; x "#, 10);
}

#[test]
fn nested_destructuring() {
    assert_eval_int(r#" let {a: {b}} = {a: {b: 42}}; b "#, 42);
}

#[test]
fn param_destructuring() {
    assert_eval_int(r#" function f({a,b}) { return a + b; } f({a:1,b:2}) "#, 3);
}

// --- Spread ---
#[test]
fn spread_array_literal() {
    assert_eval_str(r#" let a = [1,2]; let b = [...a, 3]; b.join(",") "#, "1,2,3");
}

#[test]
fn spread_object_literal() {
    assert_eval_int(r#" let o = {a:1}; let p = {...o, b:2}; p.a + p.b "#, 3);
}

#[test]
fn spread_function_call() {
    assert_eval_int(r#" Math.max(...[1,5,3]) "#, 5);
}

// --- Array methods ---
#[test]
fn array_from_string_chars() {
    assert_eval_str(r#" Array.from("abc").join(",") "#, "a,b,c");
}

#[test]
fn array_find_index() {
    assert_eval_int(r#" [1,2,3].findIndex(x => x > 1) "#, 1);
}

#[test]
fn array_at_negative_index() {
    assert_eval_int(r#" [1,2,3].at(-1) "#, 3);
}

#[test]
fn array_flat_infinity_b8() {
    assert_eval_str(r#" [1,[2,[3]]].flat(Infinity).join(",") "#, "1,2,3");
}

#[test]
fn array_flatmap_expand() {
    assert_eval_str(r#" [1,2,3].flatMap(x => [x, x*2]).join(",") "#, "1,2,2,4,3,6");
}

#[test]
fn array_every_all_even() {
    assert_eval_bool(r#" [2,4,6].every(x => x % 2 === 0) "#, true);
}

#[test]
fn array_some_none_even() {
    assert_eval_bool(r#" [1,3,5].some(x => x % 2 === 0) "#, false);
}

#[test]
fn array_reduce_right_sum() {
    assert_eval_int(r#" [1,2,3].reduceRight((a,b) => a + b, 0) "#, 6);
}

#[test]
fn array_copy_within_shift() {
    assert_eval_str(r#" [1,2,3,4,5].copyWithin(0,3).join(",") "#, "4,5,3,4,5");
}

#[test]
fn array_to_sorted_new() {
    assert_eval_str(r#" [3,1,2].toSorted().join(",") "#, "1,2,3");
}

#[test]
fn array_with_replace() {
    assert_eval_str(r#" [1,2,3].with(1, 99).join(",") "#, "1,99,3");
}

#[test]
fn array_to_string_method() {
    assert_eval_str(r#" [1,2,3].toString() "#, "1,2,3");
}

// --- Object methods ---
#[test]
fn object_keys_list() {
    assert_eval_str(r#" Object.keys({a:1,b:2}).join(",") "#, "a,b");
}

#[test]
fn object_values_list() {
    assert_eval_str(r#" Object.values({a:1,b:2}).join(",") "#, "1,2");
}

#[test]
fn object_entries_mapped() {
    assert_eval_str(
        r#" Object.entries({a:1}).map(e => e[0] + "=" + e[1]).join(",") "#,
        "a=1",
    );
}

#[test]
fn object_assign_merge_b8() {
    assert_eval_int(r#" let o = Object.assign({}, {a:1}, {b:2}); o.a + o.b "#, 3);
}

#[test]
fn object_freeze_prevents_mutation_b8() {
    assert_eval_int(r#" let o = Object.freeze({x:1}); o.x = 2; o.x "#, 1);
}

#[test]
fn object_is_nan_nan_b8() {
    assert_eval_bool(r#" Object.is(NaN, NaN) "#, true);
}

#[test]
fn object_is_zero_neg_zero() {
    assert_eval_bool(r#" Object.is(0, -0) "#, false);
}

// --- Exponentiation ---
#[test]
fn exponentiation_large() {
    assert_eval_int(r#" 2 ** 10 "#, 1024);
}

// --- Computed properties ---
#[test]
fn computed_property_name_b8() {
    assert_eval_int(r#" let k = "x"; let o = {[k]: 42}; o.x "#, 42);
}

#[test]
fn computed_method_name() {
    assert_eval_int(r#" let k = "foo"; let o = {[k]() { return 1; }}; o.foo() "#, 1);
}

// --- Shorthand method ---
#[test]
fn shorthand_method_syntax() {
    assert_eval_str(r#" let o = { greet() { return "hi"; } }; o.greet() "#, "hi");
}

// --- Number constants ---
#[test]
fn number_max_safe_integer_b8() {
    assert_eval_number(r#" Number.MAX_SAFE_INTEGER "#, 9007199254740991.0);
}

#[test]
fn number_min_safe_integer() {
    assert_eval_number(r#" Number.MIN_SAFE_INTEGER "#, -9007199254740991.0);
}

#[test]
fn number_epsilon_small() {
    assert_eval_bool(r#" Number.EPSILON < 0.001 "#, true);
}

// --- Array.isArray ---
#[test]
fn array_isarray_true_b8() {
    assert_eval_bool(r#" Array.isArray([1,2,3]) "#, true);
}

#[test]
fn array_isarray_false_string() {
    assert_eval_bool(r#" Array.isArray("abc") "#, false);
}

// --- Bitwise ---
#[test]
fn bitwise_and_op() {
    assert_eval_int(r#" 5 & 3 "#, 1);
}

#[test]
fn bitwise_or_op() {
    assert_eval_int(r#" 5 | 3 "#, 7);
}

#[test]
fn bitwise_xor_op() {
    assert_eval_int(r#" 5 ^ 3 "#, 6);
}

#[test]
fn bitwise_not_op() {
    assert_eval_int(r#" ~5 "#, -6);
}

#[test]
fn left_shift_op() {
    assert_eval_int(r#" 1 << 3 "#, 8);
}

#[test]
fn right_shift_op() {
    assert_eval_int(r#" 16 >> 2 "#, 4);
}

#[test]
fn unsigned_right_shift_op() {
    assert_eval_float(r#" -1 >>> 0 "#, 4294967295.0);
}

// --- String methods ---
#[test]
fn string_repeat_method() {
    assert_eval_str(r#" "ab".repeat(3) "#, "ababab");
}

#[test]
fn string_pad_start_b8() {
    assert_eval_str(r#" "5".padStart(3, "0") "#, "005");
}

#[test]
fn string_pad_end_b8() {
    assert_eval_str(r#" "5".padEnd(3, "0") "#, "500");
}

#[test]
fn string_starts_with_b8() {
    assert_eval_bool(r#" "hello".startsWith("hel") "#, true);
}

#[test]
fn string_ends_with_b8() {
    assert_eval_bool(r#" "hello".endsWith("llo") "#, true);
}

#[test]
fn string_match_all() {
    assert_eval_str(
        r#" let m = "aaa".matchAll(/a/g); let r = []; for(let x of m) r.push(x[0]); r.join(",") "#,
        "a,a,a",
    );
}

#[test]
fn string_search_index() {
    assert_eval_int(r#" "hello world".search(/world/) "#, 6);
}

#[test]
fn string_trim_start_b8() {
    assert_eval_str(r#" "  hi  ".trimStart() "#, "hi  ");
}

#[test]
fn string_trim_end_b8() {
    assert_eval_str(r#" "  hi  ".trimEnd() "#, "  hi");
}

#[test]
fn string_replace_all_b8() {
    assert_eval_str(r#" "abab".replaceAll("a","x") "#, "xbxb");
}

#[test]
fn string_at_positive_v3() {
    assert_eval_str(r#" "abc".at(1) "#, "b");
}

#[test]
fn string_code_point_at_b8() {
    assert_eval_int(r#" "A".codePointAt(0) "#, 65);
}

// --- Math methods ---
#[test]
fn math_min_multi() {
    assert_eval_int(r#" Math.min(3, 1, 2) "#, 1);
}

#[test]
fn math_max_multi() {
    assert_eval_int(r#" Math.max(3, 1, 2) "#, 3);
}

#[test]
fn math_abs_negative_b8() {
    assert_eval_int(r#" Math.abs(-5) "#, 5);
}

#[test]
fn math_floor_decimal() {
    assert_eval_int(r#" Math.floor(3.7) "#, 3);
}

#[test]
fn math_ceil_decimal() {
    assert_eval_int(r#" Math.ceil(3.2) "#, 4);
}

#[test]
fn math_round_half() {
    assert_eval_int(r#" Math.round(3.5) "#, 4);
}

#[test]
fn math_trunc_decimal() {
    assert_eval_int(r#" Math.trunc(3.9) "#, 3);
}

#[test]
fn math_sign_negative() {
    assert_eval_int(r#" Math.sign(-5) "#, -1);
}

#[test]
fn math_sign_positive() {
    assert_eval_int(r#" Math.sign(5) "#, 1);
}

#[test]
fn math_sign_zero_b8() {
    assert_eval_int(r#" Math.sign(0) "#, 0);
}

#[test]
fn math_sqrt_perfect() {
    assert_eval_int(r#" Math.sqrt(9) "#, 3);
}

#[test]
fn math_cbrt_perfect() {
    assert_eval_int(r#" Math.cbrt(27) "#, 3);
}

#[test]
fn math_pow_power() {
    assert_eval_int(r#" Math.pow(2, 8) "#, 256);
}

#[test]
fn math_pi_range() {
    assert_eval_bool(r#" Math.PI > 3.14 && Math.PI < 3.15 "#, true);
}

#[test]
fn math_e_range() {
    assert_eval_bool(r#" Math.E > 2.71 && Math.E < 2.72 "#, true);
}

// --- Method chaining ---
#[test]
fn method_chain_sort_reverse_join() {
    assert_eval_str(r#" [3,1,2].sort().reverse().join("-") "#, "3-2-1");
}

#[test]
fn string_chain_operations() {
    assert_eval_str(
        r#" "  Hello World  ".trim().toLowerCase().split(" ").join("_") "#,
        "hello_world",
    );
}

// --- Closures ---
#[test]
fn closure_counter_increment() {
    assert_eval_int(
        r#" function counter() { let c = 0; return () => ++c; } let inc = counter(); inc(); inc(); inc() "#,
        3,
    );
}

#[test]
fn iife_basic() {
    assert_eval_int(r#" (function() { return 42; })() "#, 42);
}

#[test]
fn arrow_iife() {
    assert_eval_int(r#" (() => 99)() "#, 99);
}

// --- Recursion ---
#[test]
fn fibonacci_recursive() {
    assert_eval_int(
        r#" function fib(n) { return n <= 1 ? n : fib(n-1) + fib(n-2); } fib(10) "#,
        55,
    );
}

// --- Default/rest params ---
#[test]
fn default_param_simple() {
    assert_eval_int(r#" function f(a, b = 10) { return a + b; } f(5) "#, 15);
}

#[test]
fn default_param_expression() {
    assert_eval_int(r#" function f(a, b = a * 2) { return b; } f(5) "#, 10);
}

#[test]
fn rest_params_collect() {
    assert_eval_str(r#" function f(a, ...b) { return b.join(","); } f(1,2,3,4) "#, "2,3,4");
}

// --- Object literal getter/setter ---
#[test]
fn object_literal_getter() {
    assert_eval_int(r#" let o = { get x() { return 42; } }; o.x "#, 42);
}

#[test]
fn object_literal_setter_getter() {
    assert_eval_int(
        r#" let o = { _v: 0, get v() { return this._v; }, set v(x) { this._v = x * 2; } }; o.v = 5; o.v "#,
        10,
    );
}

// --- do-while ---
#[test]
fn do_while_loop_b8() {
    assert_eval_int(r#" let i = 0; do { i++; } while(i < 5); i "#, 5);
}

// --- Ternary ---
#[test]
fn ternary_nested_expression() {
    assert_eval_str(
        r#" let f = x => x > 0 ? "pos" : "neg"; f(1) + f(-1) "#,
        "posneg",
    );
}

// --- Number toString radix ---
#[test]
fn num_to_string_binary() {
    assert_eval_str(r#" (255).toString(2) "#, "11111111");
}

#[test]
fn num_to_string_hex() {
    assert_eval_str(r#" (255).toString(16) "#, "ff");
}

#[test]
fn num_to_string_octal() {
    assert_eval_str(r#" (255).toString(8) "#, "377");
}

// --- Array.from with mapFn ---
#[test]
fn array_from_with_map_fn() {
    assert_eval_str(r#" Array.from([1,2,3], x => x * 2).join(",") "#, "2,4,6");
}

#[test]
fn array_from_set() {
    assert_eval_str(r#" Array.from(new Set([1,2,3])).join(",") "#, "1,2,3");
}

#[test]
fn array_from_map() {
    assert_eval_str(
        r#" Array.from(new Map([["a",1]])).map(e => e[0] + e[1]).join(",") "#,
        "a1",
    );
}

// --- Features that are NOT yet implemented (ignored) ---

#[test]
fn string_raw_template() {
    assert_eval_str(r#" String.raw`hello\nworld` "#, "hello\\nworld");
}

#[test]
fn array_find_last() {
    assert_eval_int(r#" [1,2,3,4].findLast(x => x < 3) "#, 2);
}

#[test]
fn array_find_last_index() {
    assert_eval_int(r#" [1,2,3,4].findLastIndex(x => x < 3) "#, 1);
}

#[test]
fn array_to_reversed() {
    assert_eval_str(
        r#" let a = [1,2,3]; let b = a.toReversed(); a.join(",") + "|" + b.join(",") "#,
        "1,2,3|3,2,1",
    );
}

#[test]
fn generator_for_of_iteration() {
    assert_eval_int(
        r#" function* g() { yield 1; yield 2; } let r = 0; for(let v of g()) r += v; r "#,
        3,
    );
}

#[test]
fn custom_iterable_for_of() {
    assert_eval_int(
        r#"
        let obj = {
            [Symbol.iterator]() {
                let i = 0;
                return {
                    next() {
                        return i < 3 ? { value: i++, done: false } : { done: true };
                    }
                };
            }
        };
        let r = 0;
        for (let v of obj) r += v;
        r
    "#,
        3,
    );
}

#[test]
fn number_to_precision() {
    assert_eval_str(r#" (123.456).toPrecision(5) "#, "123.46");
}

#[test]
fn structured_clone_basic() {
    assert_eval_int(r#" let a = {x:1}; let b = structuredClone(a); b.x "#, 1);
}

#[test]
fn symbol_basic_type() {
    assert_eval_str(r#" let s = Symbol("foo"); typeof s "#, "symbol");
}

// ── Arrow expression body assignment fix (batch 9) ────────────────────

#[test]
fn arrow_body_assignment() {
    assert_eval_int(r#" let v; let f = x => v = x; f(42); v "#, 42);
}

#[test]
fn arrow_body_add_assign() {
    assert_eval_int(r#" let r = 0; [1,2,3].forEach(x => r += x); r "#, 6);
}

#[test]
fn arrow_body_concat_assign() {
    assert_eval_str(
        r#" let r = ""; ["a","b","c"].forEach(x => r += x); r "#,
        "abc",
    );
}

#[test]
fn map_foreach_arrow() {
    assert_eval_str(
        r#" let r = ""; let m = new Map([["a",1],["b",2]]); m.forEach((v,k) => { r += k + v; }); r "#,
        "a1b2",
    );
}

#[test]
fn map_foreach_function() {
    assert_eval_str(
        r#" let r = ""; let m = new Map([["a",1],["b",2]]); m.forEach(function(v,k) { r += k + v; }); r "#,
        "a1b2",
    );
}

#[test]
fn set_foreach_arrow() {
    assert_eval_int(
        r#" let r = 0; let s = new Set([1,2,3]); s.forEach(v => { r += v; }); r "#,
        6,
    );
}

#[test]
fn set_foreach_arrow_no_braces() {
    assert_eval_int(
        r#" let r = 0; let s = new Set([1,2,3]); s.forEach(v => r += v); r "#,
        6,
    );
}

// --- Array.from with generators and iterables ---
#[test]
fn array_from_map_entries() {
    assert_eval_str(
        r#" let m = new Map([["x",1],["y",2]]); Array.from(m).map(e => e[0]).join(",") "#,
        "x,y",
    );
}

// --- More complex destructuring tests ---
#[test]
fn destructure_array_skip() {
    assert_eval_int(r#" let [,b] = [1,2]; b "#, 2);
}

#[test]
fn destructure_nested_array() {
    assert_eval_int(r#" let [[a]] = [[42]]; a "#, 42);
}

#[test]
fn destructure_object_computed() {
    assert_eval_int(r#" let key = "x"; let {[key]: val} = {x: 42}; val "#, 42);
}

// --- Numeric edge cases ---
#[test]
fn negative_zero_string() {
    assert_eval_str(r#" String(0) "#, "0");
}

#[test]
fn infinity_typeof() {
    assert_eval_str(r#" typeof Infinity "#, "number");
}

#[test]
fn nan_typeof() {
    assert_eval_str(r#" typeof NaN "#, "number");
}

#[test]
fn nan_not_equal_self() {
    assert_eval_bool(r#" NaN === NaN "#, false);
}

#[test]
fn null_equals_undefined() {
    assert_eval_bool(r#" null == undefined "#, true);
}

#[test]
fn null_strict_not_undefined() {
    assert_eval_bool(r#" null === undefined "#, false);
}

// --- Complex class patterns ---
#[test]
fn class_method_returns_this() {
    assert_eval_int(
        r#" class Builder { constructor() { this.v = 0; } add(n) { this.v += n; return this; } } new Builder().add(1).add(2).add(3).v "#,
        6,
    );
}

#[test]
fn class_tostring_override() {
    assert_eval_str(
        r#" class Pt { constructor(x,y) { this.x = x; this.y = y; } toString() { return this.x + "," + this.y; } } "" + new Pt(1,2) "#,
        "1,2",
    );
}

// --- Closure patterns ---
#[test]
fn closure_over_loop_var_with_iife() {
    assert_eval_str(
        r#" let fns = []; for(let i=0;i<3;i++) { fns.push((i => () => i)(i)); } fns.map(f => f()).join(",") "#,
        "0,1,2",
    );
}

#[test]
fn closure_mutual_reference() {
    assert_eval_bool(
        r#"
        function isEven(n) { return n === 0 ? true : isOdd(n - 1); }
        function isOdd(n) { return n === 0 ? false : isEven(n - 1); }
        isEven(10) && isOdd(7)
        "#,
        true,
    );
}

// --- Regex patterns ---
#[test]
fn regex_global_match() {
    assert_eval_str(
        r#" "abcabc".match(/a/g).join(",") "#,
        "a,a",
    );
}

#[test]
fn regex_replace_with_function() {
    assert_eval_str(
        r#" "hello".replace(/[aeiou]/g, m => m.toUpperCase()) "#,
        "hEllO",
    );
}

// --- String template with complex expressions ---
#[test]
fn template_with_expression() {
    assert_eval_str(
        r#" let x = 10; `${x > 5 ? "big" : "small"}` "#,
        "big",
    );
}

#[test]
fn template_with_method_call_b9() {
    assert_eval_str(
        r#" let arr = [1,2,3]; `length is ${arr.length}` "#,
        "length is 3",
    );
}

// --- Chained optional access ---
#[test]
fn optional_chain_deep_b9() {
    assert_eval_undefined(r#" let o = {a: {b: null}}; o.a.b?.c?.d "#);
}

#[test]
fn optional_chain_with_default() {
    assert_eval_int(r#" let o = {}; o.x?.y ?? 42 "#, 42);
}

// --- Ternary and logical combinations ---
#[test]
fn short_circuit_and_b9() {
    assert_eval_int(r#" 0 && 42 "#, 0);
}

#[test]
fn short_circuit_or_b9() {
    assert_eval_int(r#" 0 || 42 "#, 42);
}

#[test]
fn logical_and_truthy() {
    assert_eval_int(r#" 1 && 2 && 3 "#, 3);
}

#[test]
fn logical_or_falsy() {
    assert_eval_int(r#" 0 || false || 5 "#, 5);
}

// --- Array higher-order patterns ---
#[test]
fn array_filter_map_chain() {
    assert_eval_str(
        r#" [1,2,3,4,5].filter(x => x % 2 === 1).map(x => x * 10).join(",") "#,
        "10,30,50",
    );
}

#[test]
fn array_reduce_to_object() {
    assert_eval_str(
        r#" let o = ["a","b","c"].reduce((acc,x,i) => { acc[x] = i; return acc; }, {}); o.a + "," + o.b + "," + o.c "#,
        "0,1,2",
    );
}

#[test]
fn array_sort_comparator_b9() {
    assert_eval_str(
        r#" [3,1,4,1,5].sort((a,b) => b - a).join(",") "#,
        "5,4,3,1,1",
    );
}

#[test]
fn array_includes_nan() {
    assert_eval_bool(r#" [1, NaN, 3].includes(NaN) "#, true);
}

// --- For loop patterns ---
#[test]
fn for_loop_no_init() {
    assert_eval_int(r#" let i = 0; for(; i < 5; i++) {} i "#, 5);
}

#[test]
fn for_loop_multiple_update() {
    assert_eval_str(
        r#" let r = ""; for(let i=0, j=10; i<3; i++, j--) r += i + "" + j + " "; r.trim() "#,
        "010 19 28",
    );
}

// --- Conditional assignment patterns ---
#[test]
fn assign_in_condition() {
    assert_eval_int(r#" let x; if(x = 5) { x } else { 0 } "#, 5);
}

// ── Multi-declaration and property shorthand (batch 10) ───────────────

#[test]
fn let_multi_declaration_with_init() {
    assert_eval_int(r#" let a = 1, b = 2; a + b "#, 3);
}

#[test]
fn let_multi_declaration_three() {
    assert_eval_int(r#" let a = 1, b = 2, c = 3; a + b + c "#, 6);
}

#[test]
fn let_multi_mixed_init() {
    assert_eval_int(r#" let a = 1, b; a "#, 1);
}

#[test]
fn const_multi_declaration() {
    assert_eval_int(r#" const a = 1, b = 2; a + b "#, 3);
}

#[test]
fn var_multi_declaration() {
    assert_eval_int(r#" var a = 1, b = 2; a + b "#, 3);
}

#[test]
fn swap_destructuring() {
    assert_eval_str(r#" let a = 1, b = 2; [a,b] = [b,a]; a + "," + b "#, "2,1");
}

#[test]
fn property_shorthand_syntax() {
    assert_eval_int(r#" let x = 1, y = 2; let o = {x, y}; o.x + o.y "#, 3);
}

// ── Comprehensive tests batch 11 ─────────────────────────────────────

#[test]
fn recursive_instance_method() {
    assert_eval_int(
        r#"
        class Node {
            constructor(val, next) { this.val = val; this.next = next; }
            sum() { return this.val + (this.next ? this.next.sum() : 0); }
        }
        let list = new Node(1, new Node(2, new Node(3, null)));
        list.sum()
    "#,
        6,
    );
}

#[test]
fn generator_return_value() {
    assert_eval_str(
        r#"
        function* g() { yield 1; return 2; yield 3; }
        let it = g();
        let a = it.next().value;
        let b = it.next().value;
        let c = it.next().done;
        a + "," + b + "," + c
    "#,
        "1,2,true",
    );
}

#[test]
fn map_chained_set() {
    assert_eval_int(r#" let m = new Map().set("a",1).set("b",2); m.get("b") "#, 2);
}

#[test]
fn set_spread_to_array() {
    assert_eval_str(r#" let s = new Set([1,2,3]); [...s].join(",") "#, "1,2,3");
}

#[test]
fn map_spread_to_array() {
    assert_eval_str(
        r#" let m = new Map([["a",1]]); [...m].map(e => e[0] + e[1]).join(",") "#,
        "a1",
    );
}

#[test]
fn empty_array_join() {
    assert_eval_str(r#" [].join(",") "#, "");
}

#[test]
fn single_element_join() {
    assert_eval_str(r#" [42].join(",") "#, "42");
}

#[test]
fn nested_empty_flat() {
    assert_eval_int(r#" [[],[]].flat().length "#, 0);
}

#[test]
fn string_ctor_null() {
    assert_eval_str(r#" String(null) "#, "null");
}

#[test]
fn string_ctor_undefined() {
    assert_eval_str(r#" String(undefined) "#, "undefined");
}

#[test]
fn string_ctor_bool() {
    assert_eval_str(r#" String(true) "#, "true");
}

#[test]
fn string_ctor_number() {
    assert_eval_str(r#" String(42) "#, "42");
}

#[test]
fn empty_object_keys() {
    assert_eval_int(r#" Object.keys({}).length "#, 0);
}

#[test]
fn immediately_destructured_iife() {
    assert_eval_int(r#" let {a, b} = (() => ({a: 1, b: 2}))(); a + b "#, 3);
}

#[test]
fn rest_in_object_destructuring() {
    assert_eval_str(
        r#" let {a, ...rest} = {a:1, b:2, c:3}; Object.keys(rest).join(",") "#,
        "b,c",
    );
}

#[test]
fn computed_bracket_access() {
    assert_eval_int(r#" let o = {x: 42}; let k = "x"; o[k] "#, 42);
}

#[test]
fn bracket_assignment() {
    assert_eval_int(r#" let o = {}; o["x"] = 42; o.x "#, 42);
}

#[test]
fn nested_template_literal() {
    assert_eval_str(
        r#" let a = "world"; `hello ${`dear ${a}`}` "#,
        "hello dear world",
    );
}

#[test]
fn chained_ternary_classify() {
    assert_eval_str(
        r#"
        function classify(n) {
            return n > 100 ? "big" : n > 10 ? "medium" : "small";
        }
        classify(5) + "," + classify(50) + "," + classify(500)
    "#,
        "small,medium,big",
    );
}

#[test]
fn split_with_regex() {
    assert_eval_str(r#" "a1b2c".split(/\d/).join(",") "#, "a,b,c");
}

#[test]
fn string_concat_method_b11() {
    assert_eval_str(r#" "hello".concat(" ", "world") "#, "hello world");
}

#[test]
fn generator_manual_iteration() {
    assert_eval_str(
        r#" function* g() { yield 1; yield 2; } let a = []; let it = g(); let n = it.next(); while(!n.done) { a.push(n.value); n = it.next(); } a.join(",") "#,
        "1,2",
    );
}

#[test]
fn spread_into_function() {
    assert_eval_int(r#" function sum(a,b,c) { return a+b+c; } sum(...[1,2,3]) "#, 6);
}

#[test]
fn class_getter_from_super() {
    assert_eval_int(
        r#"
        class Base {
            constructor() { this._v = 10; }
            get v() { return this._v; }
        }
        class Child extends Base { constructor() { super(); } }
        new Child().v
    "#,
        10,
    );
}

#[test]
fn class_in_function_scope() {
    assert_eval_int(
        r#"
        function createClass() {
            class Inner { constructor(x) { this.x = x; } value() { return this.x; } }
            return new Inner(42);
        }
        createClass().value()
    "#,
        42,
    );
}

#[test]
fn builder_pattern_chaining() {
    assert_eval_str(
        r#"
        class Builder {
            constructor() { this.items = []; }
            add(x) { this.items.push(x); return this; }
            build() { return this.items.join(","); }
        }
        new Builder().add("a").add("b").add("c").build()
    "#,
        "a,b,c",
    );
}

#[test]
fn closure_multiple_captures() {
    assert_eval_int(
        r#"
        function make(x, y) { return () => x + y; }
        make(10, 20)()
    "#,
        30,
    );
}

#[test]
fn array_from_length_object() {
    assert_eval_str(r#" Array.from({length: 3}, (_, i) => i).join(",") "#, "0,1,2");
}

#[test]
fn regex_capture_groups_match() {
    assert_eval_str(r#" "2024-01-15".match(/(\d{4})-(\d{2})-(\d{2})/)[1] "#, "2024");
}

#[test]
fn three_level_inheritance_fields() {
    assert_eval_int(
        r#"
        class A { constructor() { this.x = 1; } }
        class B extends A { constructor() { super(); this.y = 2; } }
        class C extends B { constructor() { super(); this.z = 3; } }
        let c = new C();
        c.x + c.y + c.z
    "#,
        6,
    );
}

#[test]
fn string_match_returns_null() {
    let engine = FormLogicEngine::default();
    let out = engine.eval(r#" "abc".match(/xyz/) "#).expect("eval");
    assert!(matches!(out, Object::Null), "expected Null, got {:?}", out);
}

#[test]
fn map_clear_empties() {
    assert_eval_int(r#" let m = new Map([["a",1]]); m.clear(); m.size "#, 0);
}

#[test]
fn set_delete_reduces_size() {
    assert_eval_int(r#" let s = new Set([1,2,3]); s.delete(2); s.size "#, 2);
}

#[test]
fn set_clear_empties() {
    assert_eval_int(r#" let s = new Set([1,2,3]); s.clear(); s.size "#, 0);
}

#[test]
fn complex_array_chain() {
    assert_eval_str(
        r#"
        [5,3,8,1,9,2,7,4,6]
            .filter(x => x > 3)
            .sort((a,b) => a - b)
            .map(x => x * 2)
            .join(",")
    "#,
        "8,10,12,14,16,18",
    );
}

#[test]
fn nested_function_scope() {
    assert_eval_int(
        r#"
        function outer() {
            let x = 10;
            function inner() { return x + 5; }
            return inner();
        }
        outer()
    "#,
        15,
    );
}

#[test]
fn try_catch_finally_with_error_message() {
    assert_eval_str(
        r#"
        let log = [];
        try {
            log.push("try");
            throw new Error("oops");
        } catch(e) {
            log.push("catch:" + e.message);
        } finally {
            log.push("finally");
        }
        log.join(",")
    "#,
        "try,catch:oops,finally",
    );
}

#[test]
fn destructure_skip_first() {
    assert_eval_int(r#" let [,b] = [1,2]; b "#, 2);
}

#[test]
fn destructure_nested_array_pattern() {
    assert_eval_int(r#" let [[a]] = [[42]]; a "#, 42);
}

#[test]
fn infinity_is_number_type() {
    assert_eval_str(r#" typeof Infinity "#, "number");
}

#[test]
fn nan_is_number_type() {
    assert_eval_str(r#" typeof NaN "#, "number");
}

#[test]
fn nan_strict_not_equal_self() {
    assert_eval_bool(r#" NaN === NaN "#, false);
}

#[test]
fn null_loose_equals_undefined() {
    assert_eval_bool(r#" null == undefined "#, true);
}

#[test]
fn null_strict_not_equals_undefined() {
    assert_eval_bool(r#" null === undefined "#, false);
}

#[test]
fn class_builder_returns_this() {
    assert_eval_int(
        r#" class Builder { constructor() { this.v = 0; } add(n) { this.v += n; return this; } } new Builder().add(1).add(2).add(3).v "#,
        6,
    );
}

#[test]
fn class_tostring_in_concat_b11() {
    assert_eval_str(
        r#" class Pt { constructor(x,y) { this.x = x; this.y = y; } toString() { return this.x + "," + this.y; } } "" + new Pt(1,2) "#,
        "1,2",
    );
}

#[test]
fn regex_replace_with_callback() {
    assert_eval_str(
        r#" "hello".replace(/[aeiou]/g, m => m.toUpperCase()) "#,
        "hEllO",
    );
}

#[test]
fn regex_global_match_array() {
    assert_eval_str(r#" "abcabc".match(/a/g).join(",") "#, "a,a");
}

#[test]
fn template_with_condition() {
    assert_eval_str(r#" let x = 10; `${x > 5 ? "big" : "small"}` "#, "big");
}

#[test]
fn template_with_arr_length() {
    assert_eval_str(r#" let arr = [1,2,3]; `length is ${arr.length}` "#, "length is 3");
}

#[test]
fn optional_chain_deep_null() {
    assert_eval_undefined(r#" let o = {a: {b: null}}; o.a.b?.c?.d "#);
}

#[test]
fn optional_chain_with_nullish_default() {
    assert_eval_int(r#" let o = {}; o.x?.y ?? 42 "#, 42);
}

#[test]
fn logical_and_returns_last_truthy() {
    assert_eval_int(r#" 1 && 2 && 3 "#, 3);
}

#[test]
fn logical_or_returns_first_truthy() {
    assert_eval_int(r#" 0 || false || 5 "#, 5);
}

#[test]
fn filter_map_chain() {
    assert_eval_str(
        r#" [1,2,3,4,5].filter(x => x % 2 === 1).map(x => x * 10).join(",") "#,
        "10,30,50",
    );
}

#[test]
fn reduce_to_object() {
    assert_eval_str(
        r#" let o = ["a","b","c"].reduce((acc,x,i) => { acc[x] = i; return acc; }, {}); o.a + "," + o.b + "," + o.c "#,
        "0,1,2",
    );
}

#[test]
fn sort_descending() {
    assert_eval_str(r#" [3,1,4,1,5].sort((a,b) => b - a).join(",") "#, "5,4,3,1,1");
}

#[test]
fn array_includes_nan_value() {
    assert_eval_bool(r#" [1, NaN, 3].includes(NaN) "#, true);
}

#[test]
fn for_loop_no_initializer() {
    assert_eval_int(r#" let i = 0; for(; i < 5; i++) {} i "#, 5);
}

#[test]
fn assign_in_if_condition() {
    assert_eval_int(r#" let x; if(x = 5) { x } else { 0 } "#, 5);
}

// ═══════════════════════════════════════════════════════════════════════
// Batch 12: Additional coverage
// ═══════════════════════════════════════════════════════════════════════

// --- new Array() constructor ---
#[test]
fn new_array_no_args() {
    assert_eval_int(r#" new Array().length "#, 0);
}

#[test]
fn new_array_single_number() {
    assert_eval_int(r#" new Array(3).length "#, 3);
}

#[test]
fn new_array_multiple_args() {
    assert_eval_str(r#" new Array(1,2,3).join(",") "#, "1,2,3");
}

// --- structuredClone ---
#[test]
fn structured_clone_object() {
    assert_eval_str(
        r#" let a = {x:1, y:2}; let b = structuredClone(a); b.x = 99; a.x + "," + b.x "#,
        "1,99",
    );
}

#[test]
fn structured_clone_array() {
    assert_eval_str(
        r#" let a = [1,2,3]; let b = structuredClone(a); b.push(4); a.length + "," + b.length "#,
        "3,4",
    );
}

#[test]
fn structured_clone_nested() {
    assert_eval_int(
        r#" let a = {inner: {v: 42}}; let b = structuredClone(a); b.inner.v = 0; a.inner.v "#,
        42,
    );
}

// --- String.normalize ---
#[test]
fn string_normalize_ascii() {
    assert_eval_str(r#" "test".normalize("NFC") "#, "test");
}

// --- toPrecision ---
#[test]
fn number_to_precision_integer() {
    assert_eval_str(r#" (123).toPrecision(5) "#, "123.00");
}

// --- String.raw ---
#[test]
fn string_raw_basic() {
    assert_eval_str(r#" String.raw`hello\nworld` "#, "hello\\nworld");
}

// --- Block scoping ---
#[test]
fn block_scope_if_body() {
    assert_eval_int(
        r#"
        let x = 1;
        if (true) { let x = 2; }
        x
    "#,
        1,
    );
}

#[test]
fn block_scope_nested() {
    assert_eval_str(
        r#"
        let x = "outer";
        { let x = "inner"; }
        x
    "#,
        "outer",
    );
}

#[test]
fn block_scope_var_leaks() {
    assert_eval_int(
        r#"
        function test() {
            var x = 1;
            { var x = 2; }
            return x;
        }
        test()
    "#,
        2,
    );
}

// --- Function hoisting ---
#[test]
fn function_hoisting_basic() {
    assert_eval_int(
        r#"
        function test() {
            let v = f();
            function f() { return 10; }
            return v;
        }
        test()
    "#,
        10,
    );
}

#[test]
fn function_hoisting_multiple() {
    assert_eval_int(
        r#"
        function test() {
            let result = a() + b();
            function a() { return 1; }
            function b() { return 2; }
            return result;
        }
        test()
    "#,
        3,
    );
}

// --- Optional catch binding ---
#[test]
fn optional_catch_binding_value() {
    assert_eval_str(
        r#"
        let result = "ok";
        try { throw "something"; } catch { result = "caught"; }
        result
    "#,
        "caught",
    );
}

// --- Generator for-of ---
#[test]
fn generator_for_of_basic() {
    assert_eval_str(
        r#"
        function* range(n) { for(let i=0; i<n; i++) yield i; }
        let r = [];
        for(let v of range(4)) r.push(v);
        r.join(",")
    "#,
        "0,1,2,3",
    );
}

#[test]
fn generator_for_of_with_return() {
    assert_eval_str(
        r#"
        function* items() { yield "a"; yield "b"; yield "c"; }
        let arr = [];
        for(let v of items()) arr.push(v);
        arr.join(",")
    "#,
        "a,b,c",
    );
}

#[test]
fn generator_spread_into_array() {
    assert_eval_str(
        r#"
        function* g() { yield 1; yield 2; yield 3; }
        [...g()].join(",")
    "#,
        "1,2,3",
    );
}

// --- While loop patterns ---
#[test]
fn while_loop_decrement() {
    assert_eval_int(
        r#" let n = 10; let sum = 0; while(n > 0) { sum += n; n--; } sum "#,
        55,
    );
}

// --- Do-while ---
#[test]
fn do_while_runs_once_b12() {
    assert_eval_int(r#" let x = 0; do { x++; } while(false); x "#, 1);
}

// --- Switch patterns ---
#[test]
fn switch_default_only() {
    assert_eval_str(
        r#"
        let r;
        switch(99) { default: r = "default"; }
        r
    "#,
        "default",
    );
}

#[test]
fn switch_multiple_cases_b12() {
    assert_eval_str(
        r#"
        function classify(n) {
            switch(n) {
                case 1: return "one";
                case 2: return "two";
                case 3: return "three";
                default: return "other";
            }
        }
        classify(2) + "," + classify(5)
    "#,
        "two,other",
    );
}

// --- typeof patterns ---
#[test]
fn typeof_null_is_object_b12() {
    assert_eval_str(r#" typeof null "#, "object");
}

#[test]
fn typeof_array_is_object() {
    assert_eval_str(r#" typeof [1,2,3] "#, "object");
}

#[test]
fn typeof_regex_is_object() {
    assert_eval_str(r#" typeof /abc/ "#, "object");
}

// --- Equality edge cases ---
#[test]
fn null_equals_undefined_b12() {
    assert_eval_bool(r#" null == undefined "#, true);
}

#[test]
fn null_not_strict_equals_undefined() {
    assert_eval_bool(r#" null === undefined "#, false);
}

#[test]
fn nan_not_equals_nan() {
    assert_eval_bool(r#" NaN === NaN "#, false);
}

#[test]
fn nan_not_equals_nan_loose() {
    assert_eval_bool(r#" NaN == NaN "#, false);
}

// --- Number methods ---
#[test]
fn number_is_integer_b12() {
    assert_eval_bool(r#" Number.isInteger(42) "#, true);
}

#[test]
fn number_is_integer_float() {
    assert_eval_bool(r#" Number.isInteger(42.5) "#, false);
}

#[test]
fn number_is_nan_b12() {
    assert_eval_bool(r#" Number.isNaN(NaN) "#, true);
}

#[test]
fn number_is_nan_string() {
    assert_eval_bool(r#" Number.isNaN("hello") "#, false);
}

#[test]
fn number_is_finite_b12() {
    assert_eval_bool(r#" Number.isFinite(42) "#, true);
}

#[test]
fn number_is_finite_infinity() {
    assert_eval_bool(r#" Number.isFinite(Infinity) "#, false);
}

// --- Math methods ---
#[test]
fn math_max_multiple() {
    assert_eval_int(r#" Math.max(1, 5, 3, 9, 2) "#, 9);
}

#[test]
fn math_min_multiple() {
    assert_eval_int(r#" Math.min(5, 1, 3, -2, 4) "#, -2);
}

#[test]
fn math_pow_b12() {
    assert_eval_int(r#" Math.pow(2, 10) "#, 1024);
}

#[test]
fn math_sqrt_b12() {
    assert_eval_int(r#" Math.sqrt(144) "#, 12);
}

#[test]
fn math_round_b12() {
    assert_eval_int(r#" Math.round(4.5) "#, 5);
}

#[test]
fn math_round_down() {
    assert_eval_int(r#" Math.round(4.4) "#, 4);
}

#[test]
fn math_trunc_b12() {
    assert_eval_int(r#" Math.trunc(4.9) "#, 4);
}

#[test]
fn math_trunc_negative() {
    assert_eval_int(r#" Math.trunc(-4.9) "#, -4);
}

#[test]
fn math_sign_positive_b12() {
    assert_eval_int(r#" Math.sign(42) "#, 1);
}

#[test]
fn math_sign_negative_b12() {
    assert_eval_int(r#" Math.sign(-42) "#, -1);
}

#[test]
fn math_sign_zero_b12() {
    assert_eval_int(r#" Math.sign(0) "#, 0);
}

// --- String methods ---
#[test]
fn string_char_at_b12() {
    assert_eval_str(r#" "hello".charAt(1) "#, "e");
}

#[test]
fn string_index_of_b12() {
    assert_eval_int(r#" "hello world".indexOf("world") "#, 6);
}

#[test]
fn string_index_of_not_found() {
    assert_eval_int(r#" "hello".indexOf("xyz") "#, -1);
}

#[test]
fn string_last_index_of_b12() {
    assert_eval_int(r#" "abcabc".lastIndexOf("abc") "#, 3);
}

#[test]
fn string_pad_start_b12() {
    assert_eval_str(r#" "5".padStart(3, "0") "#, "005");
}

#[test]
fn string_pad_end_b12() {
    assert_eval_str(r#" "hi".padEnd(5, ".") "#, "hi...");
}

#[test]
fn string_repeat_b12() {
    assert_eval_str(r#" "ab".repeat(3) "#, "ababab");
}

#[test]
fn string_trim_b12() {
    assert_eval_str(r#" "  hello  ".trim() "#, "hello");
}

#[test]
fn string_to_lower_case_b12() {
    assert_eval_str(r#" "HELLO".toLowerCase() "#, "hello");
}

#[test]
fn string_to_upper_case_b12() {
    assert_eval_str(r#" "hello".toUpperCase() "#, "HELLO");
}

#[test]
fn string_slice_negative_b12() {
    assert_eval_str(r#" "hello world".slice(-5) "#, "world");
}

#[test]
fn string_slice_range() {
    assert_eval_str(r#" "hello world".slice(0, 5) "#, "hello");
}

#[test]
fn string_substring_b12() {
    assert_eval_str(r#" "hello world".substring(6) "#, "world");
}

#[test]
fn string_includes_b12() {
    assert_eval_bool(r#" "hello world".includes("world") "#, true);
}

#[test]
fn string_includes_false() {
    assert_eval_bool(r#" "hello".includes("xyz") "#, false);
}

#[test]
fn string_starts_with_b12() {
    assert_eval_bool(r#" "hello".startsWith("hel") "#, true);
}

#[test]
fn string_ends_with_b12() {
    assert_eval_bool(r#" "hello".endsWith("llo") "#, true);
}

#[test]
fn string_replace_b12() {
    assert_eval_str(r#" "hello world".replace("world", "rust") "#, "hello rust");
}

#[test]
fn string_split_b12() {
    assert_eval_str(r#" "a,b,c".split(",").join("-") "#, "a-b-c");
}

#[test]
fn string_char_code_at_b12() {
    assert_eval_int(r#" "A".charCodeAt(0) "#, 65);
}

// --- Array methods ---
#[test]
fn array_push_pop() {
    assert_eval_str(
        r#" let a = [1,2]; a.push(3); let v = a.pop(); a.join(",") + "|" + v "#,
        "1,2|3",
    );
}

#[test]
fn array_shift_unshift() {
    assert_eval_str(
        r#" let a = [2,3]; a.unshift(1); let v = a.shift(); a.join(",") + "|" + v "#,
        "2,3|1",
    );
}

#[test]
fn array_splice_delete() {
    assert_eval_str(r#" let a = [1,2,3,4,5]; a.splice(1, 2); a.join(",") "#, "1,4,5");
}

#[test]
fn array_splice_insert_b12() {
    assert_eval_str(r#" let a = [1,4,5]; a.splice(1, 0, 2, 3); a.join(",") "#, "1,2,3,4,5");
}

#[test]
fn array_reverse_b12() {
    assert_eval_str(r#" [1,2,3].reverse().join(",") "#, "3,2,1");
}

#[test]
fn array_concat_b12() {
    assert_eval_str(r#" [1,2].concat([3,4]).join(",") "#, "1,2,3,4");
}

#[test]
fn array_index_of_b12() {
    assert_eval_int(r#" [10,20,30].indexOf(20) "#, 1);
}

#[test]
fn array_index_of_not_found_b12() {
    assert_eval_int(r#" [1,2,3].indexOf(99) "#, -1);
}

#[test]
fn array_last_index_of() {
    assert_eval_int(r#" [1,2,3,2,1].lastIndexOf(2) "#, 3);
}

#[test]
fn array_fill_basic_b12() {
    assert_eval_str(r#" [0,0,0].fill(7).join(",") "#, "7,7,7");
}

#[test]
fn array_every_true_b12() {
    assert_eval_bool(r#" [2,4,6,8].every(x => x % 2 === 0) "#, true);
}

#[test]
fn array_every_false_b12() {
    assert_eval_bool(r#" [2,4,5,8].every(x => x % 2 === 0) "#, false);
}

#[test]
fn array_some_true_b12() {
    assert_eval_bool(r#" [1,3,5,4].some(x => x % 2 === 0) "#, true);
}

#[test]
fn array_some_false_b12() {
    assert_eval_bool(r#" [1,3,5,7].some(x => x % 2 === 0) "#, false);
}

#[test]
fn array_find_b12() {
    assert_eval_int(r#" [1,2,3,4].find(x => x > 2) "#, 3);
}

#[test]
fn array_flat_depth_1() {
    assert_eval_str(r#" [1, [2, [3]]].flat().join(",") "#, "1,2,3");
}

#[test]
fn array_entries_b12() {
    assert_eval_str(
        r#" let a = ["a","b"]; let entries = [...a.entries()]; entries.map(e => e[0] + ":" + e[1]).join(",") "#,
        "0:a,1:b",
    );
}

// --- Object methods ---
#[test]
fn object_assign_merge_b12() {
    assert_eval_str(
        r#" let a = {x:1}; let b = {y:2}; let c = Object.assign({}, a, b); c.x + "," + c.y "#,
        "1,2",
    );
}

#[test]
fn object_assign_overwrite() {
    assert_eval_int(r#" let a = {x:1}; Object.assign(a, {x:2}); a.x "#, 2);
}

#[test]
fn object_freeze_prevents_mutation_b12() {
    assert_eval_int(r#" let o = {x:1}; Object.freeze(o); o.x = 99; o.x "#, 1);
}

// --- JSON ---
#[test]
fn json_stringify_object_b12() {
    assert_eval_str(r#" JSON.stringify({a:1, b:2}) "#, r#"{"a":1,"b":2}"#);
}

#[test]
fn json_stringify_array_b12() {
    assert_eval_str(r#" JSON.stringify([1,2,3]) "#, "[1,2,3]");
}

#[test]
fn json_parse_object_b12() {
    assert_eval_int(r#" JSON.parse('{"x":42}').x "#, 42);
}

#[test]
fn json_parse_array_b12() {
    assert_eval_str(r#" JSON.parse("[1,2,3]").join(",") "#, "1,2,3");
}

#[test]
fn json_roundtrip() {
    assert_eval_str(
        r#" let o = {a:1, b:"hello", c:true}; let s = JSON.stringify(o); let p = JSON.parse(s); p.a + "," + p.b + "," + p.c "#,
        "1,hello,true",
    );
}

// --- Error handling ---
#[test]
fn error_message_property_b12() {
    assert_eval_str(
        r#"
        try { throw new Error("oops"); }
        catch(e) { e.message }
    "#,
        "oops",
    );
}

#[test]
fn custom_error_throw() {
    assert_eval_str(
        r#"
        try { throw {code: 42, msg: "fail"}; }
        catch(e) { e.code + ":" + e.msg }
    "#,
        "42:fail",
    );
}

#[test]
fn nested_try_catch_b12() {
    assert_eval_str(
        r#"
        let log = [];
        try {
            try { throw "inner"; }
            catch(e) { log.push("caught:" + e); throw "outer"; }
        } catch(e) { log.push("caught:" + e); }
        log.join(",")
    "#,
        "caught:inner,caught:outer",
    );
}

#[test]
fn finally_always_runs_b12() {
    assert_eval_str(
        r#"
        let log = [];
        try { log.push("try"); }
        finally { log.push("finally"); }
        log.join(",")
    "#,
        "try,finally",
    );
}

// --- Regex ---
#[test]
fn regex_test_basic_b12() {
    assert_eval_bool(r#" /\d+/.test("abc123") "#, true);
}

#[test]
fn regex_test_no_match_b12() {
    assert_eval_bool(r#" /\d+/.test("abc") "#, false);
}

#[test]
fn regex_match_groups() {
    assert_eval_str(r#" "2024-01-15".match(/(\d{4})-(\d{2})-(\d{2})/)[2] "#, "01");
}

#[test]
fn regex_replace_simple() {
    assert_eval_str(r#" "foo bar".replace(/foo/, "baz") "#, "baz bar");
}

#[test]
fn regex_escaped_slash() {
    // Regex with escaped slash (\/) must not end the pattern prematurely
    assert_eval_str(
        r#" "<b>hello</b>".replace(/<\/b>/g, "") "#,
        "<b>hello",
    );
}

#[test]
fn regex_escaped_slash_in_complex_pattern() {
    // Complex regex like those used to strip HTML/thinking tags
    assert_eval_str(
        r#" "<think>secret</think> visible".replace(/<think[^>]*>[\s\S]*?<\/think[^>]*>/gi, "") "#,
        " visible",
    );
}

#[test]
fn regex_char_class_with_caret() {
    // Character class with negation [^>]
    assert_eval_str(
        r#" "<div class='x'>text</div>".replace(/<[^>]*>/g, "") "#,
        "text",
    );
}

// --- Map ---
#[test]
fn map_set_get_has_b12() {
    assert_eval_str(
        r#" let m = new Map(); m.set("a", 1); m.set("b", 2); m.has("a") + "," + m.get("b") + "," + m.has("c") "#,
        "true,2,false",
    );
}

#[test]
fn map_size_b12() {
    assert_eval_int(r#" let m = new Map([["a",1],["b",2],["c",3]]); m.size "#, 3);
}

#[test]
fn map_delete_b12() {
    assert_eval_int(r#" let m = new Map([["a",1],["b",2]]); m.delete("a"); m.size "#, 1);
}

// --- Set ---
#[test]
fn set_add_has_b12() {
    assert_eval_str(
        r#" let s = new Set(); s.add(1); s.add(2); s.add(1); s.has(1) + "," + s.has(3) + "," + s.size "#,
        "true,false,2",
    );
}

#[test]
fn set_from_array() {
    assert_eval_int(r#" new Set([1,1,2,2,3,3]).size "#, 3);
}

// --- Class patterns ---
#[test]
fn class_static_method_b12() {
    assert_eval_int(
        r#"
        class MathHelper {
            static add(a, b) { return a + b; }
            static mul(a, b) { return a * b; }
        }
        MathHelper.add(3, 4) + MathHelper.mul(2, 5)
    "#,
        17,
    );
}

#[test]
fn class_getter_setter_b12() {
    assert_eval_int(
        r#"
        class Temp {
            constructor(c) { this._c = c; }
            get fahrenheit() { return this._c * 9 / 5 + 32; }
            set celsius(v) { this._c = v; }
        }
        let t = new Temp(100);
        t.fahrenheit
    "#,
        212,
    );
}

#[test]
fn class_method_chaining_b12() {
    assert_eval_str(
        r#"
        class Query {
            constructor() { this.parts = []; }
            select(s) { this.parts.push("SELECT " + s); return this; }
            from(t) { this.parts.push("FROM " + t); return this; }
            build() { return this.parts.join(" "); }
        }
        new Query().select("*").from("users").build()
    "#,
        "SELECT * FROM users",
    );
}

#[test]
fn class_extends_override() {
    assert_eval_str(
        r#"
        class Animal {
            speak() { return "..."; }
        }
        class Dog extends Animal {
            speak() { return "Woof"; }
        }
        class Cat extends Animal {
            speak() { return "Meow"; }
        }
        new Dog().speak() + "," + new Cat().speak()
    "#,
        "Woof,Meow",
    );
}

// --- Closure patterns ---
#[test]
fn closure_counter_factory_b12() {
    assert_eval_str(
        r#"
        function makeCounter(start) {
            let count = start;
            return {
                inc() { return ++count; },
                dec() { return --count; },
                val() { return count; }
            };
        }
        let c = makeCounter(10);
        c.inc(); c.inc(); c.dec();
        "" + c.val()
    "#,
        "11",
    );
}

#[test]
fn closure_adder() {
    assert_eval_int(
        r#"
        function adder(x) { return y => x + y; }
        adder(5)(3)
    "#,
        8,
    );
}

#[test]
fn closure_compose() {
    assert_eval_int(
        r#"
        function compose(f, g) { return x => f(g(x)); }
        let double = x => x * 2;
        let inc = x => x + 1;
        compose(double, inc)(5)
    "#,
        12,
    );
}

// --- Complex algorithms ---
#[test]
fn fibonacci_recursive_b12() {
    assert_eval_int(
        r#"
        function fib(n) {
            if (n <= 1) return n;
            return fib(n-1) + fib(n-2);
        }
        fib(10)
    "#,
        55,
    );
}

#[test]
fn fibonacci_iterative() {
    assert_eval_int(
        r#"
        function fib(n) {
            let a = 0, b = 1;
            for(let i = 0; i < n; i++) {
                let temp = a + b;
                a = b;
                b = temp;
            }
            return a;
        }
        fib(10)
    "#,
        55,
    );
}

#[test]
fn gcd_recursive() {
    assert_eval_int(
        r#"
        function gcd(a, b) { return b === 0 ? a : gcd(b, a % b); }
        gcd(48, 18)
    "#,
        6,
    );
}

#[test]
fn binary_search() {
    assert_eval_int(
        r#"
        function bsearch(arr, target) {
            let lo = 0, hi = arr.length - 1;
            while(lo <= hi) {
                let mid = Math.floor((lo + hi) / 2);
                if (arr[mid] === target) return mid;
                if (arr[mid] < target) lo = mid + 1;
                else hi = mid - 1;
            }
            return -1;
        }
        bsearch([1,3,5,7,9,11,13], 7)
    "#,
        3,
    );
}

#[test]
fn quicksort() {
    assert_eval_str(
        r#"
        function qsort(arr) {
            if (arr.length <= 1) return arr;
            let pivot = arr[0];
            let left = [];
            let right = [];
            for(let i = 1; i < arr.length; i++) {
                if (arr[i] <= pivot) left.push(arr[i]);
                else right.push(arr[i]);
            }
            return [...qsort(left), pivot, ...qsort(right)];
        }
        qsort([3,6,8,10,1,2,1]).join(",")
    "#,
        "1,1,2,3,6,8,10",
    );
}

// --- Template literals ---
#[test]
fn template_multiline() {
    assert_eval_str(
        r#" let name = "World"; `Hello,
${name}!` "#,
        "Hello,\nWorld!",
    );
}

#[test]
fn template_with_arithmetic() {
    assert_eval_str(r#" let a = 3, b = 4; `${a} + ${b} = ${a+b}` "#, "3 + 4 = 7");
}

// --- Destructuring ---
#[test]
fn destructure_function_return() {
    assert_eval_str(
        r#"
        function getPoint() { return {x: 10, y: 20}; }
        let {x, y} = getPoint();
        x + "," + y
    "#,
        "10,20",
    );
}

#[test]
fn destructure_array_nested() {
    assert_eval_int(r#" let [[a]] = [[42]]; a "#, 42);
}

#[test]
fn destructure_default_in_object() {
    assert_eval_int(r#" let {a = 1, b = 2} = {a: 10}; a + b "#, 12);
}

// --- Spread/Rest ---
#[test]
fn rest_params_b12() {
    assert_eval_str(
        r#"
        function join(sep, ...items) { return items.join(sep); }
        join("-", "a", "b", "c")
    "#,
        "a-b-c",
    );
}

#[test]
fn spread_object_merge() {
    assert_eval_str(
        r#" let a = {x:1}; let b = {y:2}; let c = {...a, ...b}; c.x + "," + c.y "#,
        "1,2",
    );
}

#[test]
fn spread_object_override() {
    assert_eval_int(r#" let a = {x:1}; let b = {...a, x:2}; b.x "#, 2);
}

// --- Computed property names ---
#[test]
fn computed_property_b12() {
    assert_eval_int(r#" let key = "x"; let o = {[key]: 42}; o.x "#, 42);
}

#[test]
fn computed_property_expression() {
    assert_eval_int(r#" let o = {["a" + "b"]: 42}; o.ab "#, 42);
}

// --- for-in / for-of ---
#[test]
fn for_of_string() {
    assert_eval_str(
        r#" let r = []; for(let c of "abc") r.push(c); r.join(",") "#,
        "a,b,c",
    );
}

#[test]
fn for_in_object_b12() {
    assert_eval_str(
        r#" let o = {a:1, b:2, c:3}; let keys = []; for(let k in o) keys.push(k); keys.join(",") "#,
        "a,b,c",
    );
}

// --- Labeled statements ---
#[test]
fn labeled_break_nested_b12() {
    assert_eval_int(
        r#"
        let count = 0;
        outer: for(let i = 0; i < 3; i++) {
            for(let j = 0; j < 3; j++) {
                if (j === 1) break outer;
                count++;
            }
        }
        count
    "#,
        1,
    );
}

#[test]
fn labeled_continue_outer_b12() {
    assert_eval_int(
        r#"
        let count = 0;
        outer: for(let i = 0; i < 3; i++) {
            for(let j = 0; j < 3; j++) {
                if (j === 1) continue outer;
                count++;
            }
        }
        count
    "#,
        3,
    );
}

// --- Bitwise operators ---
#[test]
fn bitwise_and_b12() {
    assert_eval_int(r#" 0xFF & 0x0F "#, 15);
}

#[test]
fn bitwise_or_b12() {
    assert_eval_int(r#" 0xF0 | 0x0F "#, 255);
}

#[test]
fn bitwise_xor_b12() {
    assert_eval_int(r#" 0xFF ^ 0x0F "#, 240);
}

#[test]
fn bitwise_not_b12() {
    assert_eval_int(r#" ~0 "#, -1);
}

#[test]
fn bitwise_left_shift_b12() {
    assert_eval_int(r#" 1 << 10 "#, 1024);
}

#[test]
fn bitwise_right_shift_b12() {
    assert_eval_int(r#" 1024 >> 5 "#, 32);
}

// --- Unary operators ---
#[test]
fn unary_plus_string() {
    assert_eval_number(r#" +"42" "#, 42.0);
}

#[test]
fn unary_negation() {
    assert_eval_int(r#" -(-42) "#, 42);
}

#[test]
fn logical_not() {
    assert_eval_bool(r#" !false "#, true);
}

#[test]
fn double_not_truthy() {
    assert_eval_bool(r#" !!"hello" "#, true);
}

#[test]
fn double_not_falsy() {
    assert_eval_bool(r#" !!0 "#, false);
}

// --- Ternary complex ---
#[test]
fn nested_ternary_b12() {
    assert_eval_str(
        r#"
        function grade(score) {
            return score >= 90 ? "A" :
                   score >= 80 ? "B" :
                   score >= 70 ? "C" : "F";
        }
        grade(95) + grade(85) + grade(75) + grade(50)
    "#,
        "ABCF",
    );
}

// --- void and delete ---
#[test]
fn void_expression() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("void 0").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn delete_property_b12() {
    assert_eval_bool(
        r#" let o = {a:1, b:2}; delete o.a; o.a === undefined "#,
        true,
    );
}

// --- Comma operator ---
#[test]
fn comma_operator_b12() {
    assert_eval_int(r#" (1, 2, 3) "#, 3);
}

// --- Exponentiation ---
#[test]
fn exponentiation_basic() {
    assert_eval_int(r#" 2 ** 8 "#, 256);
}

#[test]
fn exponentiation_negative() {
    assert_eval_float(r#" 4 ** -1 "#, 0.25);
}

// --- globalThis ---
#[test]
fn global_this_exists() {
    assert_eval_str(r#" typeof globalThis "#, "object");
}

// --- Array.from patterns ---
#[test]
fn array_from_string_b12() {
    assert_eval_str(r#" Array.from("hello").join(",") "#, "h,e,l,l,o");
}

#[test]
fn array_from_set_b12() {
    assert_eval_str(r#" Array.from(new Set([3,1,2])).sort((a,b)=>a-b).join(",") "#, "1,2,3");
}

// --- Promise ---
// (Promise tests beyond basic are hard without async runtime)

// --- WeakRef-like patterns / edge cases ---
#[test]
fn nested_function_returning_function() {
    assert_eval_int(
        r#"
        function outer(x) {
            return function(y) {
                return function(z) {
                    return x + y + z;
                };
            };
        }
        outer(1)(2)(3)
    "#,
        6,
    );
}

#[test]
fn immediately_invoked_arrow_b12() {
    assert_eval_int(r#" (() => 42)() "#, 42);
}

#[test]
fn arrow_with_destructuring_param() {
    assert_eval_int(r#" (({x, y}) => x + y)({x: 10, y: 20}) "#, 30);
}

// --- Async/await basic ---
#[test]
fn async_function_returns_promise_like() {
    assert_eval_str(r#" typeof (async () => 42)() "#, "object");
}

// --- Object shorthand ---
#[test]
fn object_shorthand_method() {
    assert_eval_int(
        r#"
        let o = {
            x: 10,
            double() { return this.x * 2; }
        };
        o.double()
    "#,
        20,
    );
}

// --- Nullish coalescing ---
#[test]
fn nullish_coalescing_null_b12() {
    assert_eval_int(r#" null ?? 42 "#, 42);
}

#[test]
fn nullish_coalescing_undefined_b12() {
    assert_eval_int(r#" undefined ?? 42 "#, 42);
}

#[test]
fn nullish_coalescing_zero_b12() {
    assert_eval_int(r#" 0 ?? 42 "#, 0);
}

#[test]
fn nullish_coalescing_empty_string_b12() {
    assert_eval_str(r#" "" ?? "default" "#, "");
}

// --- Optional chaining ---
#[test]
fn optional_chain_method_call() {
    assert_eval_str(r#" "hello"?.toUpperCase() "#, "HELLO");
}

#[test]
fn optional_chain_null_method() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("let x = null; x?.toString()").expect("eval");
    assert!(matches!(out, Object::Undefined), "expected Undefined, got {:?}", out);
}

// --- in operator ---
#[test]
fn in_operator_true() {
    assert_eval_bool(r#" "x" in {x: 1, y: 2} "#, true);
}

#[test]
fn in_operator_false() {
    assert_eval_bool(r#" "z" in {x: 1, y: 2} "#, false);
}

// --- instanceof ---
#[test]
fn instanceof_class_b12() {
    assert_eval_bool(
        r#"
        class Foo {}
        let f = new Foo();
        f instanceof Foo
    "#,
        true,
    );
}

// --- for loop patterns ---
#[test]
fn for_loop_empty_body() {
    assert_eval_int(r#" let i; for(i = 0; i < 10; i++) {} i "#, 10);
}

#[test]
fn for_loop_break_with_value() {
    assert_eval_int(
        r#"
        let result = -1;
        for(let i = 0; i < 100; i++) {
            if (i * i > 50) { result = i; break; }
        }
        result
    "#,
        8,
    );
}

#[test]
fn for_loop_continue_b12() {
    assert_eval_int(
        r#" let sum = 0; for(let i = 0; i < 10; i++) { if(i % 2 === 0) continue; sum += i; } sum "#,
        25,
    );
}

