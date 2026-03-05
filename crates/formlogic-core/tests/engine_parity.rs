#![allow(clippy::approx_constant)]

use formlogic_core::engine::FormLogicEngine;
use formlogic_core::object::Object;

#[test]
fn engine_eval_basic_expressions_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("1 + 2;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = 10; let y = 20; x + y;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("\"5\" + 3;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "53"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("5 + \"3\";").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "53"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"5\" - 3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_prefix_and_comparison_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("!false;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("5 > 3;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("5 == 5;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("\"10\" > 9;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("null > 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("null >= 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("undefined > 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("undefined < 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("!!0;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("!!1;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("!!\"\";").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("!!\"hello\";").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("!!null;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("!!undefined;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_if_expression_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("if (1 < 2) { 10 } else { 20 };").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("if (1 > 2) { 10 } else { 20 };").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 20),
        Object::Float(v) => assert!((v - 20.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_array_and_hash_literals_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("[1, 2, 3];").expect("eval");
    match out {
        Object::Array(items) => assert_eq!(items.borrow().len(), 3),
        _ => panic!("expected array output"),
    }

    let out = engine
        .eval("{\"name\": \"John\", \"age\": 30};")
        .expect("eval");
    match out {
        Object::Hash(h) => assert_eq!(h.borrow().pairs.len(), 2),
        _ => panic!("expected hash output"),
    }
}

#[test]
fn engine_eval_function_call_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let add = function(a, b) { return a + b; }; add(2, 3);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let id = function(x) { x; }; id(42);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_index_and_property_access_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("[1, 2, 3][1];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("{\"name\": \"John\"}[\"name\"];")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "John"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("{\"name\": \"John\"}.name;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "John"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_assignment_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let x = 1; x = 7; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = 5; x += 3; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let arr = [1,2,3]; arr[1] = 9; arr[1];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_while_loop_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let i = 0; let sum = 0; while (i < 5) { i = i + 1; sum = sum + i; } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        Object::Float(v) => assert!((v - 15.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let i = 0; let sum = 0; while (i < 8) { i = i + 1; if (i == 5) { continue; } if (i == 7) { break; } sum = sum + i; } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 16),
        Object::Float(v) => assert!((v - 16.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_for_loop_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let sum = 0; for (let i = 0; i < 5; i = i + 1) { sum = sum + i; } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let i = 0; let sum = 0; for (; i < 8; i = i + 1) { if (i == 3) { continue; } if (i == 6) { break; } sum = sum + i; } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 12),
        Object::Float(v) => assert!((v - 12.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_function_declaration_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("function add(a, b) { return a + b; } add(4, 6);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("async function id(x) { return x; } id(12);")
        .expect("eval");
    assert!(matches!(out, Object::Promise(_)));
}

#[test]
fn engine_eval_for_of_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let sum = 0; for (let x of [1,2,3,4]) { sum = sum + x; } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_for_of_destructuring_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let sum = 0; for (let [n, s] of [[1,\"a\"],[2,\"b\"],[3,\"c\"]]) { sum = sum + n; } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_for_in_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let out = \"\"; for (let k in {\"a\":1,\"b\":2}) { out = out + k; } out;")
        .expect("eval");

    match out {
        Object::String(v) => {
            assert!(v.contains('a'));
            assert!(v.contains('b'));
            assert_eq!(v.len(), 2);
        }
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let out = \"\"; for (let k in {2:\"b\", 1:\"a\", 3:\"c\"}) { out = out + k; } out;")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "123"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_class_method_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("class Point { constructor(x, y) { this.x = x; this.y = y; } sum() { return this.x + this.y; } } let p = new Point(2, 3); p.sum();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_try_catch_finally_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let x = 0; try { throw 4; } catch (e) { x = e; } finally { x = x + 1; } x;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let err = engine
        .eval("throw 7;")
        .expect_err("expected uncaught throw error");
    assert!(err.contains("Uncaught throw"));
}

#[test]
fn engine_eval_in_and_instanceof_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("\"a\" in {\"a\":1};").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("1 in [10,20];").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("class Point { constructor(x) { this.x = x; } } let p = new Point(1); p instanceof Point;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[] instanceof Array;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("({}) instanceof Object;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("null instanceof Object;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_class_static_getter_setter_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("class Counter { static make(v) { return v + 1; } } Counter.make(4);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("class Box { constructor(v) { this._v = v; } get value() { return this._v; } set value(v) { this._v = v; } } let b = new Box(2); b.value = 9; b.value;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_class_extends_super_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("class A { value() { return 1; } } class B extends A { value() { return super.value() + 1; } } let b = new B(); b.value();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("class A {} class B extends A {} let b = new B(); b instanceof A;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn engine_eval_await_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("await 5;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("async function id(x) { return await x; } await id(9);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let err = engine
        .eval("async function bad() { throw 3; } await bad();")
        .expect_err("expected await rejection error");
    assert!(err.contains("Await rejected"));
}

#[test]
fn engine_eval_typeof_and_delete_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("typeof 1;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "number"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let o = {\"a\":1, \"b\":2}; delete o.a; \"a\" in o;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine
        .eval("let a = [1,2,3]; delete a[1]; a[1];")
        .expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn engine_eval_void_and_bitwise_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("void 123;").expect("eval");
    assert!(matches!(out, Object::Undefined));

    let out = engine.eval("(5 | 2) ^ 1;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("8 >>> 1;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("1 === 1.0;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn engine_eval_logical_and_nullish_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let x = 0; false && (x = 1); x;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 0; true || (x = 1); x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("null ?? 7;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("0 ?? 7;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("0 && \"hello\";").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("1 && \"hello\";").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("0 || \"hello\";").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("0 || \"\";").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, ""),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"\" || \"default\";").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "default"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("null || \"default\";").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "default"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("undefined ?? \"default\";").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "default"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("false ?? \"default\";").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("\"hello\" ?? \"default\";").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_optional_chain_and_logical_assign_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let o = {\"name\":\"n\"}; o?.name;")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "n"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("let o = null; o?.name;").expect("eval");
    assert!(matches!(out, Object::Undefined));

    let out = engine
        .eval("let f = function(x) { return x + 1; }; f?.(4);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 0; x &&= 5; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 0; x ||= 5; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 1; x ||= 5; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 1; x &&= 5; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 0; x ??= 5; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = null; x ??= 7; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        _ => panic!("expected integer output"),
    }
}

#[test]
fn engine_eval_unary_plus_and_bitwise_not_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("+\"42\";").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+true;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("~5;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -6),
        _ => panic!("expected integer output"),
    }
}

#[test]
fn engine_eval_bitwise_shift_assign_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let x = 6; x &= 3; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 2; x <<= 2; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 16; x >>>= 2; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        _ => panic!("expected integer output"),
    }
}

#[test]
fn engine_eval_index_compound_assign_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let arr = [1,2,3]; arr[1] += 5; arr[1];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let obj = {\"a\":6}; obj[\"a\"] &= 3; obj[\"a\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        _ => panic!("expected integer output"),
    }

    let out = engine
        .eval("let obj = {\"x\":16}; obj[\"x\"] >>>= 2; obj[\"x\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        _ => panic!("expected integer output"),
    }

    let out = engine
        .eval("class C { constructor() { this.x = 1; } inc() { this.x += 2; return this.x; } } let c = new C(); c.inc();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_labeled_break_continue_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            "let result = 0; outer: for (let i = 0; i < 3; i = i + 1) { for (let j = 0; j < 3; j = j + 1) { if (i === 1 && j === 1) { break outer; } result += 1; } } result;",
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval(
            "let result = 0; outer: for (let i = 0; i < 3; i = i + 1) { for (let j = 0; j < 3; j = j + 1) { if (j === 1) { continue outer; } result += 1; } } result;",
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval(
            "let result = 0; let i = 0; outer: while (i < 3) { let j = 0; while (j < 3) { if (i === 1 && j === 1) { break outer; } result += 1; j += 1; } i += 1; } result;",
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_exponent_and_assign_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("2 ** 3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = 3; x **= 2; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let arr = [2, 4]; arr[0] **= 3; arr[0];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_basic_destructuring_let_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let [a, b, c] = [1,2,3]; a + b + c;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let [a, , c] = [1,2,3]; a + c;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let {x, y: b} = {\"x\":10, \"y\":20}; x + b;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_basic_destructuring_assignment_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let a = 1; let b = 2; [a, b] = [b, a]; a * 10 + b;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 21),
        Object::Float(v) => assert!((v - 21.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let a = 0; let b = 0; {\"x\": a, \"y\": b} = {\"x\": 10, \"y\": 20}; a + b;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_object_shorthand_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let x = 10; let o = {x}; o[\"x\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let x = 0; let src = {\"x\": 7}; {x} = src; x;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_destructuring_defaults_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let [a = 1, b = 2, c = 3] = [10]; [a, b, c];")
        .expect("eval");
    match out {
        Object::Array(items) => {
            let items = items.borrow();
            assert_eq!(items.len(), 3);
            assert!(items[0].is_i32() && unsafe { items[0].as_i32_unchecked() } == 10);
            assert!(items[1].is_i32() && unsafe { items[1].as_i32_unchecked() } == 2);
            assert!(items[2].is_i32() && unsafe { items[2].as_i32_unchecked() } == 3);
        }
        _ => panic!("expected array output"),
    }

    let out = engine
        .eval("let {x = 5, y = 10} = {\"x\": 42}; x + y;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 52),
        Object::Float(v) => assert!((v - 52.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_destructuring_assignment_defaults_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let a = 0; let b = 0; [a = 1, b = 2] = [10]; a + b;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 12),
        Object::Float(v) => assert!((v - 12.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let x = 0; let y = 0; {\"x\": x = 5, \"y\": y = 10} = {\"x\": 42}; x + y;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 52),
        Object::Float(v) => assert!((v - 52.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_array_rest_binding_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let [head, ...rest] = [1,2,3,4]; head + rest[0] + rest[2];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval(
            "let sum = 0; for (let [h, ...t] of [[1,2,3],[4,5,6]]) { sum = sum + h + t[0]; } sum;",
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 12),
        Object::Float(v) => assert!((v - 12.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_object_rest_binding_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let {a, ...rest} = {\"a\":1,\"b\":2,\"c\":3}; rest[\"b\"] + rest[\"c\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_object_rest_for_of_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let sum = 0; for (let {a, ...rest} of [{\"a\":1,\"b\":2},{\"a\":3,\"b\":4}]) { sum = sum + a + rest[\"b\"]; } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let {a = 7, ...rest} = {\"b\":2}; a + rest[\"b\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_object_rest_assignment_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let a = 0; let rest = {}; {\"a\": a, ...rest} = {\"a\":1,\"b\":2,\"c\":3}; a + rest[\"b\"] + rest[\"c\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_object_rest_assignment_variants_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let a = 0; let bb = 0; let rest = {}; {a, b: bb = 9, ...rest} = {\"a\":1,\"c\":3}; a + bb + rest[\"c\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 13),
        Object::Float(v) => assert!((v - 13.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let x = 0; let rest = {}; {x, ...rest} = {\"x\": 7, \"y\": 8}; x + rest[\"y\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        Object::Float(v) => assert!((v - 15.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_computed_object_binding_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let k = \"x\"; let {[k]: v, ...rest} = {\"x\":1,\"y\":2,\"z\":3}; v + rest[\"y\"] + rest[\"z\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_computed_object_rest_assignment_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let k = \"x\"; let v = 0; let rest = {}; {[k]: v, ...rest} = {\"x\": 4, \"y\": 5}; v + rest[\"y\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_expression_computed_object_rest_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let prefix = \"my\"; let {[prefix + \"Key\"]: v, ...rest} = {\"myKey\": 10, \"other\": 20}; v + rest[\"other\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let k1 = \"a\"; let k2 = \"b\"; let a = 0; let b = 0; let rest = {}; {[k1]: a, [k2]: b, ...rest} = {\"a\": 1, \"b\": 2, \"c\": 3, \"d\": 4}; a + b + rest[\"c\"] + rest[\"d\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_nested_destructuring_assignment_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let x = 0; let a = 0; let b = 0; {\"p\": {\"q\": x}, \"arr\": [a, b]} = {\"p\": {\"q\": 9}, \"arr\": [3, 4]}; x + a + b;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 16),
        Object::Float(v) => assert!((v - 16.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let y = 0; let z = 0; [y, {\"k\": z = 5}] = [1, {}]; y + z;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_nested_destructuring_binding_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let {\"p\": {\"q\": x}, \"arr\": [a, b]} = {\"p\": {\"q\": 5}, \"arr\": [6, 7]}; x + a + b;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 18),
        Object::Float(v) => assert!((v - 18.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let sum = 0; for (let {\"n\": [u, v]} of [{\"n\":[1,2]}, {\"n\":[3,4]}]) { sum = sum + u + v; } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_nested_destructuring_defaults_binding_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let {\"p\": {\"q\": x = 3}, \"arr\": [a = 4, b = 5]} = {\"p\": {}, \"arr\": [1]}; x + a + b;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let sum = 0; for (let {\"n\": [u = 10, v = 20]} of [{\"n\": [1]}, {\"n\": []}]) { sum = sum + u + v; } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 51),
        Object::Float(v) => assert!((v - 51.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_nested_defaults_and_rest_binding_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let {\"outer\": {\"x\": x = 1, ...innerRest}, \"arr\": [h = 2, ...tail]} = {\"outer\": {\"y\": 5}, \"arr\": [9,8,7]}; x + innerRest[\"y\"] + h + tail[0] + tail[1];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let sum = 0; for (let {\"o\": {\"a\": a = 1, ...r}, \"p\": [u = 2, ...t]} of [{\"o\": {\"b\":3}, \"p\":[4,5]}, {\"o\": {}, \"p\": []}]) { sum = sum + a + (r[\"b\"] ?? 0) + u + (t[0] ?? 0); } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 16),
        Object::Float(v) => assert!((v - 16.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_nested_computed_default_binding_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let key = \"inner\"; let {\"o\": {[key]: v = 7, ...rest}} = {\"o\": {\"x\": 3}}; v + rest[\"x\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_nested_computed_rest_assignment_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let k = \"outer\"; let x = 0; let innerRest = {}; let outerRest = {}; {[k]: {\"a\": x = 1, ...innerRest}, ...outerRest} = {\"outer\": {\"b\": 4}, \"z\": 6}; x + innerRest[\"b\"] + outerRest[\"z\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 11),
        Object::Float(v) => assert!((v - 11.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_numeric_computed_key_rest_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let k = 1; let {[k]: v, ...rest} = {1: 10, 2: 20, 3: 30}; v + rest[2] + rest[3];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 60),
        Object::Float(v) => assert!((v - 60.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let k = 2; let v = 0; let rest = {}; {[k]: v, ...rest} = {1: 11, 2: 22, 3: 33}; v + rest[1] + rest[3];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 66),
        Object::Float(v) => assert!((v - 66.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_rejects_computed_key_shorthand_without_alias_subset() {
    let engine = FormLogicEngine::default();
    let err = engine
        .eval("let k = \"x\"; {[k]} = {\"x\": 1};")
        .unwrap_err();
    assert!(
        err.contains("computed object key requires ':'") || err.contains("Parser errors"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn engine_rejects_nested_computed_key_shorthand_without_alias_subset() {
    let engine = FormLogicEngine::default();
    let err = engine
        .eval("let k = \"x\"; let src = {\"o\": {\"x\": 1}}; let {o: {[k]}} = src;")
        .unwrap_err();
    assert!(
        err.contains("computed object key requires ':'") || err.contains("Parser errors"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn engine_eval_nested_computed_key_with_alias_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let k = \"inner\"; let {\"o\": {[k]: v, ...r}} = {\"o\": {\"inner\": 2, \"x\": 3}}; v + r[\"x\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_for_of_nested_computed_key_with_alias_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let k = \"id\"; let sum = 0; for (let {o: {[k]: v, ...r}} of [{o:{id:1,x:2}}, {o:{id:3,x:4}}]) { sum = sum + v + r[\"x\"]; } sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_computed_key_coercion_rest_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let k = true; let {[k]: v, ...r} = {true: 7, false: 2}; v + r[false];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let k = 1.5; let {[k]: v, ...r} = {1.5: 4, 2.5: 6}; v + r[2.5];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_rejects_default_on_rest_bindings_subset() {
    let engine = FormLogicEngine::default();

    let err = engine.eval("let [...tail = []] = [1,2,3];").unwrap_err();
    assert!(
        err.contains("rest element in array binding cannot have default"),
        "unexpected error: {}",
        err
    );

    let err = engine.eval("let {...rest = {}} = {\"a\": 1};").unwrap_err();
    assert!(
        err.contains("rest property in object binding cannot have default"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn engine_rejects_default_on_rest_assignments_subset() {
    let engine = FormLogicEngine::default();

    let err = engine.eval("[...tail = []] = [1,2,3];").unwrap_err();
    assert!(
        err.contains("rest element in array binding cannot have default"),
        "unexpected error: {}",
        err
    );

    let err = engine.eval("{...rest = {}} = {\"a\":1};").unwrap_err();
    assert!(
        err.contains("rest property in object pattern cannot have default"),
        "unexpected error: {}",
        err
    );

    let err = engine
        .eval("{x: {...inner = {}}} = {\"x\": {\"a\": 1}};")
        .unwrap_err();
    assert!(
        err.contains("rest property in object pattern cannot have default"),
        "unexpected error: {}",
        err
    );

    let out = engine.eval("[...tail] = [1,2,3]; tail[1];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_division_by_zero_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("1 / 0;").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_infinite() && v.is_sign_positive()),
        _ => panic!("expected +infinity float"),
    }

    let out = engine.eval("-1 / 0;").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_infinite() && v.is_sign_negative()),
        _ => panic!("expected -infinity float"),
    }

    let out = engine.eval("0 / 0;").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_nan()),
        _ => panic!("expected NaN float"),
    }
}

#[test]
fn engine_eval_array_out_of_bounds_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let arr = [1,2,3]; arr[10];").expect("eval");
    assert!(matches!(out, Object::Undefined));

    let out = engine.eval("let arr = [1,2,3]; arr[-1];").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn engine_eval_modulo_negative_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("-5 % 3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -2),
        Object::Float(v) => assert!((v + 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("5 % -3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_exponent_right_associative_subset() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("2 ** 3 ** 2;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 512),
        Object::Float(v) => assert!((v - 512.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_nullish_optional_combined_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {\"user\": {\"name\": \"Alice\"}}; obj?.user?.name ?? \"Unknown\";")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "Alice"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let obj = {\"user\": null}; obj?.user?.name ?? \"Unknown\";")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "Unknown"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_typeof_and_equality_edge_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("typeof null;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "object"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("1 == 1;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("1 === \"1\";").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("\"1\" == 1;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("\"\" == 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("\"\" == false;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("true == 1;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("false == 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("undefined == 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("null == 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("null == false;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("null == \"\";").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("\"0\" == false;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("NaN == NaN;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("NaN === NaN;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("NaN !== NaN;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[] == false;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[] == 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[1] == 1;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn engine_eval_truthy_falsy_conditional_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let x = 0; let out = \"\"; if (x) { out = \"truthy\"; } else { out = \"falsy\"; } out;")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "falsy"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let x = \"hello\"; let out = \"\"; if (x) { out = \"truthy\"; } else { out = \"falsy\"; } out;")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "truthy"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let x = []; let out = \"\"; if (x) { out = \"truthy\"; } else { out = \"falsy\"; } out;")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "truthy"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let x = NaN; x ? \"truthy\" : \"falsy\";")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "falsy"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let x = -0; x ? \"truthy\" : \"falsy\";")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "falsy"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_unary_plus_and_void_edge_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("+null;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = 5; void (x = 10); x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_in_operator_array_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("0 in [10,20,30];").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("3 in [10,20,30];").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_delete_operator_bracket_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {\"a\":1,\"b\":2}; delete obj[\"a\"]; \"a\" in obj;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine
        .eval("let obj = {\"a\":1}; delete obj[\"a\"];")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn engine_eval_optional_chain_index_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let arr = [1,2,3]; arr?.[1];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let arr = null; arr?.[1];").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn engine_eval_exponent_negative_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("2 ** -1;").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 0.5).abs() < 1e-9),
        Object::Integer(v) => assert_eq!(v, 0),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_unsigned_right_shift_negative_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("-1 >>> 0;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4_294_967_295),
        Object::Float(v) => assert!((v - 4_294_967_295.0).abs() < 1e-3),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_nullish_chain_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("null ?? null ?? 5;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("null ?? 3 ?? 5;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_optional_call_null_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let f = null; f?.(1,2,3);").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn engine_eval_delete_preserves_other_properties_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {\"a\":1,\"b\":2,\"c\":3}; delete obj[\"b\"]; obj[\"a\"] + obj[\"c\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_typeof_function_subset() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("typeof function(x){ return x; };")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "function"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_closure_capture_subset() {
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
fn engine_eval_nested_closure_scope_subset() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let outer = function() { let x = 10; return function() { let y = 5; return function() { return x + y; }; }; }; let middle = outer(); let inner = middle(); inner();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        Object::Float(v) => assert!((v - 15.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_recursive_function_subset() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let fib = function(n) { if (n <= 1) { return n; } return fib(n - 1) + fib(n - 2); }; fib(10);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 55),
        Object::Float(v) => assert!((v - 55.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_this_in_object_method_subset() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let obj = {\"value\": 42, \"getValue\": function() { return this.value; }}; obj.getValue();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_super_call_in_method_subset() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("class Animal { constructor(name) { this.name = name; } speak() { return \"sound\"; } } class Dog extends Animal { constructor(name, breed) { super(name); this.breed = breed; } speak() { return super.speak() + \" bark\"; } } let dog = new Dog(\"Rex\", \"Labrador\"); dog.speak();")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "sound bark"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_exponent_precedence_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("2 + 3 ** 2;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 11),
        Object::Float(v) => assert!((v - 11.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("(2 + 3) ** 2;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 25),
        Object::Float(v) => assert!((v - 25.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_exponent_float_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("2.5 ** 2;").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 6.25).abs() < 1e-9),
        Object::Integer(v) => assert_eq!(v, 6),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("4 ** 0.5;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_nullish_falsy_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("0 ?? 10;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("false ?? true;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_void_returns_undefined_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("void 0;").expect("eval");
    assert!(matches!(out, Object::Undefined));

    let out = engine.eval("void (1 + 2);").expect("eval");
    assert!(matches!(out, Object::Undefined));

    let out = engine.eval("void \"hello\";").expect("eval");
    assert!(matches!(out, Object::Undefined));

    let out = engine.eval("void true;").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn engine_eval_typeof_matrix_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("typeof undefined;").expect("eval");
    assert!(matches!(out, Object::String(v) if &*v == "undefined"));

    let out = engine.eval("typeof undeclaredVar;").expect("eval");
    assert!(matches!(out, Object::String(v) if &*v == "undefined"));

    let out = engine.eval("void 0 === undefined;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("undefined ?? 'default';").expect("eval");
    assert!(matches!(out, Object::String(v) if &*v == "default"));

    let out = engine.eval("typeof 42;").expect("eval");
    assert!(matches!(out, Object::String(v) if &*v == "number"));

    let out = engine.eval("typeof 3.14;").expect("eval");
    assert!(matches!(out, Object::String(v) if &*v == "number"));

    let out = engine.eval("typeof \"hello\";").expect("eval");
    assert!(matches!(out, Object::String(v) if &*v == "string"));

    let out = engine.eval("typeof true;").expect("eval");
    assert!(matches!(out, Object::String(v) if &*v == "boolean"));

    let out = engine.eval("typeof [];").expect("eval");
    assert!(matches!(out, Object::String(v) if &*v == "object"));

    let out = engine.eval("typeof {\"a\":1};").expect("eval");
    assert!(matches!(out, Object::String(v) if &*v == "object"));

    let out = engine.eval("typeof NaN;").expect("eval");
    assert!(matches!(out, Object::String(v) if &*v == "number"));
}

#[test]
fn engine_eval_delete_missing_property_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {\"a\":1}; delete obj[\"missing\"];")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn engine_eval_in_operator_object_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {\"a\":1,\"b\":2}; \"a\" in obj;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("let obj = {\"a\":1,\"b\":2}; \"c\" in obj;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_nullish_string_and_false_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("\"\" ?? \"default\";").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, ""),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("false ?? true;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_optional_chain_deep_missing_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {\"a\":1}; obj?.b?.c;")
        .expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn engine_eval_exponent_variable_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let x = 5; x ** 2;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 25),
        Object::Float(v) => assert!((v - 25.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = 3; x **= 2; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_short_circuit_side_effect_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let called = false; let side = function() { called = true; return true; }; false && side(); called;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine
        .eval("let called = false; let side = function() { called = true; return false; }; true || side(); called;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_missing_property_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let obj = {\"a\":1}; obj.b;").expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn engine_eval_sparse_array_write_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let arr = [1,2,3]; arr[10] = 42; arr.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 11),
        Object::Float(v) => assert!((v - 11.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let arr = [1,2,3]; arr[10] = 42; arr[10];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_equality_basics_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("null == null;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("null === null;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("null == undefined;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("null === undefined;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("1 === 1;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("1 === 1.0;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("1 === \"1\";").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("[] == false;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[] == 0;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[] == \"\";").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[0] == false;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[1] == 1;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("\"1,2\" == [1,2];").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[null] == \"\";").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[undefined] == \"\";").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[] !== false;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[] !== \"\";").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("undefined == false;").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("undefined == \"\";").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("\"1\" == true;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("0 === \"\";").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("undefined === undefined;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));
}

#[test]
fn engine_eval_optional_chain_missing_method_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {}; obj?.missing?.();")
        .expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn engine_eval_unsigned_shift_positive_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("8 >>> 1;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = 16; x >>>= 2; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_bitwise_compound_assign_full_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let x = 6; x &= 3; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 2; x |= 4; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 7; x ^= 3; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 3; x <<= 2; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 12),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("let x = 16; x >>= 2; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        _ => panic!("expected integer output"),
    }
}

#[test]
fn engine_eval_unary_plus_string_numeric_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("+\"3.14\";").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 3.14).abs() < 1e-9),
        Object::Integer(v) => assert_eq!(v, 3),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = \"10\"; +x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+\"\";").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+\"0x10\";").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 16),
        Object::Float(v) => assert!((v - 16.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+\"0b11\";").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+\"0o10\";").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+true;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+false;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+null;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+undefined;").expect("eval");
    assert!(matches!(out, Object::Float(v) if v.is_nan()));

    let out = engine.eval("+[];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+[5];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+[1,2];").expect("eval");
    assert!(matches!(out, Object::Float(v) if v.is_nan()));

    let out = engine.eval("+\"hello\";").expect("eval");
    assert!(matches!(out, Object::Float(v) if v.is_nan()));
}

#[test]
fn engine_eval_in_operator_variable_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {\"x\":1,\"y\":2}; \"x\" in obj;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("let obj = {\"x\":1,\"y\":2}; \"z\" in obj;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_delete_dot_notation_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {\"a\":1,\"b\":2}; delete obj.a;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("let obj = {\"a\":1,\"b\":2}; delete obj.a; \"a\" in obj;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_modulo_both_negative_subset() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("-5 % -3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -2),
        Object::Float(v) => assert!((v + 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_optional_method_call_existing_subset() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let obj = {\"g\": function() { return \"ok\"; }}; obj?.g?.();")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "ok"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_logical_assignment_defined_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let x = 2; x ||= 5; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = 2; x &&= 5; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = 0; x ??= 7; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_delete_bracket_variable_key_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {\"a\":1,\"b\":2}; let k = \"a\"; delete obj[k]; \"a\" in obj;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_in_operator_array_length_and_string_index_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("\"length\" in [10,20,30];").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("\"1\" in [10,20,30];").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("\"3\" in [10,20,30];").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_delete_array_integration_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let arr = [10,20,30]; delete arr[1]; 1 in arr;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("let arr = [10,20,30]; delete arr[1]; arr[1];")
        .expect("eval");
    assert!(matches!(out, Object::Undefined));

    let out = engine
        .eval("let arr = [10,20,30]; delete arr[1]; arr.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_nullish_assignment_null_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let x = null; x ??= 7; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_template_literal_basics_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("`hello world`;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello world"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("`hello\\nworld`;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello\nworld"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_template_interpolation_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("`1 + 1 = ${1 + 1}`;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1 + 1 = 2"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("`v=${1}${2}`;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "v=12"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("`outer ${`inner ${1 + 1}`}`;").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "outer inner 2"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_exponent_zero_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("10 ** 0;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_unsigned_shift_steps_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("8 >>> 2;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("8 >>> 3;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_unary_plus_number_noop_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("+42;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("+(-5);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -5),
        Object::Float(v) => assert!((v + 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_bitwise_not_edge_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("~0;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -1),
        _ => panic!("expected integer output"),
    }

    let out = engine.eval("~-1;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        _ => panic!("expected integer output"),
    }
}

#[test]
fn engine_eval_delete_nonexistent_object_key_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let obj = {\"a\":1,\"b\":2}; delete obj[\"z\"];")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("let obj = {\"a\":1,\"b\":2}; delete obj[\"z\"]; obj[\"a\"] + obj[\"b\"];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_async_await_basic_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let asyncAdd = async function(a, b) { return a + b; }; await asyncAdd(5, 3);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_ternary_operator_subset() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("true ? 1 : 2;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("false ? 1 : 2;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_comma_operator_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("(1, 2, 3);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("(\"a\", \"b\", \"c\");").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "c"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let x = 0; (x = 1, x = 2, x = 3); x;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = 0; (x = 5, 10); x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let x = 0; (1, x = 2, 3); x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let x = 0; function inc() { x++; return x; } (inc(), inc(), inc()); x;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_increment_decrement_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("let i = 1; ++i;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let i = 1; i++;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let i = 3; --i;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let i = 3; i--;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let obj = {\"x\": 4}; obj.x++; obj.x;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let arr = [2]; ++arr[0];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_spread_in_array_literal_subset() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("[1, ...[2, 3], 4];").expect("eval");
    match out {
        Object::Array(items) => {
            let items = items.borrow();
            assert_eq!(items.len(), 4);
            assert!(items[0].is_i32() && unsafe { items[0].as_i32_unchecked() } == 1);
            assert!(items[1].is_i32() && unsafe { items[1].as_i32_unchecked() } == 2);
            assert!(items[2].is_i32() && unsafe { items[2].as_i32_unchecked() } == 3);
            assert!(items[3].is_i32() && unsafe { items[3].as_i32_unchecked() } == 4);
        }
        _ => panic!("expected array output"),
    }

    let out = engine
        .eval("let src = [1,2,3,4]; let head = 0; [head, ...rest] = src; rest[2];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_spread_in_function_call_subset() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("let add = function(a,b,c){ return a+b+c; }; add(...[1,2,3]);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval(
            "let tail = [3,4]; let add4 = function(a,b,c,d){ return a+b+c+d; }; add4(1,2,...tail);",
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let maybe = null; maybe?.(...[1,2,3]);")
        .expect("eval");
    assert!(matches!(out, Object::Undefined));

    let out = engine
        .eval("let f = function(a,b){ return a*b; }; f?.(...[6,7]);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("class Pair { constructor(a,b){ this.v = a+b; } } let p = new Pair(...[20,22]); p.v;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_object_spread_literal_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let src = {\"b\": 2, \"c\": 3}; let out = {\"a\": 1, ...src}; out.b + out.c;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let src = {\"a\": 9}; let out = {\"a\": 1, ...src}; out.a;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let more = {\"c\": 3}; let out = {...{\"a\": 1}, \"b\": 2, ...more}; out.a + out.b + out.c;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_async_chained_calls_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let step1 = async function() { return 10; }; let step2 = async function(x) { return x * 2; }; let step3 = async function(x) { return x + 5; }; let main = async function() { let a = await step1(); let b = await step2(a); let c = await step3(b); return c; }; await main();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 25),
        Object::Float(v) => assert!((v - 25.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_map_set_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let m = Map(); m = m[\"set\"](\"a\", 3); m[\"get\"](\"a\");")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let s = Set(); s = s[\"add\"](\"x\"); s[\"has\"](\"x\") ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let m = Map().set(\"a\", 3); m.get(\"a\");")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let m = Map(); m.set(\"a\", 7); m.get(\"a\");")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let m = Map([[\"a\",1],[\"b\",2]]); m.size;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let s = Set([1,2,2,3]); s.size;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let s = Set(); s.add(\"x\"); s.has(\"x\") ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let m = Map([[\"a\",1],[\"b\",2]]); m.delete(\"a\"); m.size;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let s = Set([1,2,3]); s.delete(2); s.has(2) ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!((v - 0.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let s = Set([1,2]); s.clear(); s.size;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!((v - 0.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Array.from(Set([1,2,2,3]))[2];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("Array.from(Map([[\"a\",1],[\"b\",2]]))[1][0];")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "b"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("[...Set([1,2,2,3])][2];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let m = Map([[\"a\",1],[\"b\",2]]); m.keys()[0] + m.keys()[1];")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "ab"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let m = Map([[\"a\",1],[\"b\",2]]); m.values()[0] + m.values()[1];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let m = Map([[\"a\",1],[\"b\",2]]); m.entries()[1][0] + m.entries()[1][1];")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "b2"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let s = Set([3,1,2]); s.keys()[0] + s.values()[1] + s.entries()[2][1];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let m = Map([[\"x\",10],[\"y\",20]]); let sum = 0; m.forEach((v,k)=>{ sum += v; }); sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let s = Set([10,20,30]); let sum = 0; s.forEach(v=>{ sum += v; }); sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 60),
        Object::Float(v) => assert!((v - 60.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let a = Map([[\"x\",1],[\"y\",2]]); let b = Map(a); b.get(\"y\");")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let s = Set(Set([1,2,2,3])); s.size;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Set(\"aba\").size;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Array.from(Set([1.5]))[0];").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 1.5).abs() < 1e-9),
        _ => panic!("expected float output"),
    }

    let out = engine.eval("Set([1, 1.0]).size;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let m = Map(); m.set(null, 7); m.keys()[0];")
        .expect("eval");
    match out {
        Object::Null => {}
        _ => panic!("expected null output"),
    }
}

#[test]
fn engine_eval_global_this_subset() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("globalThis.Math.abs(-5);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_numeric_separators_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("1_000_000;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1_000_000),
        Object::Float(v) => assert!((v - 1_000_000.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("1_000.50;").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 1000.50).abs() < 1e-9),
        Object::Integer(v) => assert_eq!(v, 1000),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("1e3 + 2E-2;").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 1000.02).abs() < 1e-9),
        Object::Integer(v) => assert_eq!(v, 1000),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("0xff + 0b10 + 0o10;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 265),
        Object::Float(v) => assert!((v - 265.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_promise_namespace_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("await Promise.resolve(42);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let err = engine.eval("await Promise.reject(42);").unwrap_err();
    assert!(
        err.contains("Await rejected") || err.contains("42"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn engine_eval_math_namespace_subset() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Math.abs(-5);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.pow(2, 10);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1024),
        Object::Float(v) => assert!((v - 1024.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("[Math.round(2.5), Math.ceil(2.1), Math.floor(2.9)][2];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.min(5, 2, 8, 1, 9);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.max(5, 2, 8, 1, 9);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.min();").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_infinite() && v.is_sign_positive()),
        _ => panic!("expected +Infinity"),
    }

    let out = engine.eval("Math.max();").expect("eval");
    match out {
        Object::Float(v) => assert!(v.is_infinite() && v.is_sign_negative()),
        _ => panic!("expected -Infinity"),
    }

    let out = engine.eval("Math.sqrt(144);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 12),
        Object::Float(v) => assert!((v - 12.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("[Math.trunc(3.7), Math.trunc(-3.7)][1];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -3),
        Object::Float(v) => assert!((v + 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("[Math.sign(-5), Math.sign(0), Math.sign(5)][2];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.random();").expect("eval");
    match out {
        Object::Float(v) => assert!((0.0..1.0).contains(&v)),
        Object::Integer(v) => assert!((0..1).contains(&v)),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.log(1);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.log2(8);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.cbrt(27);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.sin(Math.PI / 2);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.cos(0);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.tan(0);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.exp(1);").expect("eval");
    match out {
        Object::Float(v) => assert!((v - std::f64::consts::E).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.log10(1000);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("Math.PI > 3 && Math.E > 2 && Math.LN2 > 0 && Math.LN10 > 2 && Math.SQRT2 > 1;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("Math.atan2(0, -1);").expect("eval");
    match out {
        Object::Float(v) => assert!((v - std::f64::consts::PI).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.hypot(3, 4);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.imul(-1, 5);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -5),
        Object::Float(v) => assert!((v + 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.imul(0xffffffff, 5);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, -5),
        Object::Float(v) => assert!((v + 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.clz32(1);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 31),
        Object::Float(v) => assert!((v - 31.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.clz32(0);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 32),
        Object::Float(v) => assert!((v - 32.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.clz32(1000);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 22),
        Object::Float(v) => assert!((v - 22.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.fround(1 / 3);").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 0.33333334).abs() < 1e-7),
        Object::Integer(v) => assert_eq!(v, 0),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.fround(5.5);").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 5.5).abs() < 1e-9),
        Object::Integer(v) => assert_eq!(v, 5),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.fround(5.05) !== 5.05;").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("Math.imul(2, 4);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Math.imul();").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_json_namespace_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("JSON.stringify({\"a\":1});").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "{\"a\":1}"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let parsed = JSON.parse(\"{}\"); Object.keys(parsed).length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!((v - 0.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let parsed = JSON.parse('{\"a\":{\"b\":[1,2,3]}}'); parsed.a.b[2];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("JSON.stringify({\"a\": [1, true, null], \"b\": {\"x\": 2}});")
        .expect("eval");
    match out {
        Object::String(v) => {
            assert!(v.contains("\"a\":[1,true,null]"));
            assert!(v.contains("\"b\":{\"x\":2}"));
        }
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_object_namespace_subset() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("Object.keys({\"a\":1})[0];").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "a"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("Object.is(NaN, NaN);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("Object.is(0, -0);").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("Object.is(1, 1);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("Object.is(null, void 0);").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_string_length_current_subset() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("\"hello\".length;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_string_method_calls_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("\"hello\".charAt(1);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "e"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"a,b,c\".split(\",\")[2];").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "c"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"hello world\".includes(\"world\");")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("\"hello\".slice(-3);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "llo"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"Hello\".toLowerCase();").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"hello\".toUpperCase();").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "HELLO"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"  hi  \".trim();").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hi"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("[\"hello\".startsWith(\"hel\"), \"hello\".endsWith(\"llo\")][1];")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("\"hello world\".replace(\"world\", \"there\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello there"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"a1b2c3\".replace(/[0-9]/g, \"X\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "aXbXcX"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"hello world\".replace(\"world\", (m) => m.toUpperCase());")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "hello WORLD"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"a1b2c3\".replace(/[0-9]/g, (m) => \"[\" + m + \"]\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "a[1]b[2]c[3]"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"aabbcc\".replaceAll(\"b\", \"X\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "aaXXcc"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"2024-01-15\".replace(/(\\d{4})-(\\d{2})-(\\d{2})/, \"$2/$3/$1\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "01/15/2024"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"foo\".replace(\"o\", \"[$&]-[$`]-[$']-[$$]\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "f[o]-[f]-[o]-[$]o"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"abc\".replace(/b/, \"[$&]-[$`]-[$']-[$$]\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "a[b]-[a]-[c]-[$]c"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"ab\".replace(/(a)/, \"$1-$2-$10\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "a-$2-a0b"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"aba\".replaceAll(\"a\", \"$$\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "$b$"),
        _ => panic!("expected string output"),
    }

    let err = engine
        .eval("\"aba\".replaceAll(/a/, \"x\");")
        .expect_err("expected non-global regex error");
    assert!(err.contains("non-global RegExp"));

    let out = engine
        .eval("\"2024-01-15\".replace(/(\\d{4})-(\\d{2})-(\\d{2})/, (m, y, mo, d) => mo + \"/\" + d + \"/\" + y);")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "01/15/2024"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"hello\".indexOf(\"ll\");").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("\"hello hello\".lastIndexOf(\"hello\");")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("\"hello\".substring(3,1);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "el"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"ab\".repeat(3);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "ababab"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"5\".padStart(3, \"0\");").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "005"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"5\".padEnd(3, \"0\");").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "500"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("\"A\".charCodeAt(0);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 65),
        Object::Float(v) => assert!((v - 65.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_array_method_calls_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("[1,2,3].map(function(x){ return x * 2; })[2];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[].pop();").expect("eval");
    assert!(matches!(out, Object::Undefined));

    let out = engine.eval("let a = [1,2]; a.push(3);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[10,20,30].keys()[1];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[10,20,30].values()[2];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        Object::Float(v) => assert!((v - 30.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[\"a\",\"b\"].entries()[1][0];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let sum = 0; [10,20,30].forEach(v => { sum += v; }); sum;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 60),
        Object::Float(v) => assert!((v - 60.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("[1,2,3].flatMap(x => [x, x * 2])[5];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[1,2,3].filter(x => x > 1)[1];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[1,2,3,4].find(x => x > 2);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("[1,2,3,4].reduce((a,b)=>a+b, 0);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[0/0, 1, 2].includes(0/0);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[1,2,3].join(\"-\");").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1-2-3"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("[1,2,3].toString();").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1,2,3"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("[].toString();").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, ""),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("[[1,2],[3]].toString();").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1,2,3"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let a = [1,2]; a.valueOf()[0] + a.valueOf()[1];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let a = [1,2]; a.valueOf() === a;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[1,2,3,4,5].slice(1,-1)[2];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("[1, [2, 3], [4, [5]]].flat(2)[4];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[3,1,2].sort()[0];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[3,1,2].sort((a,b)=>b-a)[0];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("Array.from({\"length\": 3}, (_, i) => i * i)[2];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("Array.from({\"0\":\"x\",\"1\":\"y\",\"length\": 2})[1];")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "y"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("[1,2,3].reverse()[0];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[1,2,3].some(x => x > 2);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("[1,2,3].every(x => x > 1);").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine
        .eval("[5,12,8,130].findIndex(x => x > 10);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[1,2,3,1,2,3].indexOf(2,3);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("[1,2,3,1,2,3].lastIndexOf(2,2);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_arrow_function_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let double = x => x * 2; double(21);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let addOne = async x => x + 1; await addOne(9);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let add = (a, b) => a + b; add(20, 22);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let one = () => 1; one();").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let addOne = async (x) => x + 1; await addOne(41);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_rest_parameter_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let f = function(...args) { return args[0] + args[2]; }; f(1,2,3,4);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 4),
        Object::Float(v) => assert!((v - 4.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let f = (...xs) => xs[1]; f(10,20,30);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 20),
        Object::Float(v) => assert!((v - 20.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let f = function(a, ...rest) { return a + rest[0] + rest[1]; }; f(1,2,3);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_default_parameters_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let f = function(a = 1, b = 2) { return a + b; }; f();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let f = function(a = 1, b = 2) { return a + b; }; f(5);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let f = function(a = 1, b = 2) { return a + b; }; f(void 0, 9);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("let g = (x = 3) => x * 2; g();").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_destructured_parameters_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let f = function({a, b: c}, [x, y]) { return a + c + x + y; }; f({\"a\":1, \"b\":2}, [3,4]);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 10),
        Object::Float(v) => assert!((v - 10.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let f = function({a} = {\"a\": 5}) { return a; }; f();")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let g = ({m}, [n]) => m * n; g({\"m\":6}, [7]);")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_regexp_constructor_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let re = new RegExp(\"x\"); re.test(\"x\") ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let re = RegExp(\"z\"); re.test(\"abc\") ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!((v - 0.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let base = /hello/i; let copy = RegExp(base); copy.test(\"HeLLo\") ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_string_regex_match_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("\"hello\".match(new RegExp(\"h\"))[0];")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "h"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("\"hello\".match(new RegExp(\"z\")) === null ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let matches = [...(\"test1 test2 test3\").matchAll(/test(\\d)/g)]; matches.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let matches = [...(\"abc123def456\").matchAll(/(\\d+)/g)]; matches[0][1] + \",\" + matches[1][1];")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "123,456"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let matches = [...(\"aaa\").matchAll(\"a\")]; matches.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let matches = [...(\"hello\").matchAll(/xyz/g)]; matches.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let err = engine
        .eval("[...(\"hello\").matchAll(/h/)];")
        .expect_err("expected non-global regex error");
    assert!(err.contains("non-global RegExp"));
}

#[test]
fn engine_eval_number_method_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("(3.14159).toFixed(2);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "3.14"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("(1.005).toFixed(2);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1.00"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("(255).toString(16);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "ff"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("(8).toString(8);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "10"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("(35).toString(36);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "z"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("(3).toFixed(0);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "3"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_parse_and_number_namespace_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("parseInt(\"42\");").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("parseFloat(\"3.14\");").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 3.14).abs() < 1e-9),
        Object::Integer(v) => assert_eq!(v, 3),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("isNaN(0/0);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("isFinite(42);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("Number.isNaN(\"hello\");").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("Number(\"\");").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number(\"  \");").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number();").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number(null);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number(true);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number(false);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 0),
        Object::Float(v) => assert!(v.abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number(undefined);").expect("eval");
    assert!(matches!(out, Object::Float(v) if v.is_nan()));

    let out = engine.eval("Number(\"abc\");").expect("eval");
    assert!(matches!(out, Object::Float(v) if v.is_nan()));

    let out = engine.eval("Number(\"0b101\");").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number(\"0o10\");").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 8),
        Object::Float(v) => assert!((v - 8.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number.isFinite(0/0);").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("parseInt(\"ff\", 16);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 255),
        Object::Float(v) => assert!((v - 255.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("parseInt(\"111\", 2);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("parseInt(\"17\", 8);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        Object::Float(v) => assert!((v - 15.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("parseInt(\"0xff\");").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 255),
        Object::Float(v) => assert!((v - 255.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number.parseInt(\"42\");").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number.parseFloat(\"3.14\");").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 3.14).abs() < 1e-9),
        Object::Integer(v) => assert_eq!(v, 3),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("parseFloat();").expect("eval");
    assert!(matches!(out, Object::Float(v) if v.is_nan()));

    let out = engine.eval("Number.isInteger(3.0);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("Number.isSafeInteger(9007199254740991);")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("Number.isSafeInteger(9007199254740992);")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine
        .eval("Number.isSafeInteger(Number.MAX_SAFE_INTEGER);")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("Number.EPSILON > 0 && Number.EPSILON < 0.001;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("Number.MAX_VALUE > 1000000 && Number.MAX_VALUE < Infinity;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("Number.MIN_VALUE > 0 && Number.MIN_VALUE < Number.EPSILON;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine
        .eval("Number.POSITIVE_INFINITY === Infinity && Number.NEGATIVE_INFINITY === -Infinity;")
        .expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("parseInt(\"111\", 2);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        Object::Float(v) => assert!((v - 7.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("parseInt(\"17\", 8);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        Object::Float(v) => assert!((v - 15.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("isNaN();").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("Number.isNaN();").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("Number.parseInt(\"42\");").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        Object::Float(v) => assert!((v - 42.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number.parseFloat(\"3.5\");").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 3.5).abs() < 1e-9),
        Object::Integer(v) => assert_eq!(v, 3),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("Number.isFinite(Infinity);").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("Number.isFinite(-Infinity);").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));

    let out = engine.eval("Number.isNaN(NaN);").expect("eval");
    assert!(matches!(out, Object::Boolean(true)));

    let out = engine.eval("isFinite(Infinity);").expect("eval");
    assert!(matches!(out, Object::Boolean(false)));
}

#[test]
fn engine_eval_string_namespace_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("String.fromCharCode(65);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "A"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("String(123);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "123"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("String({});").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "[object Object]"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("String([1,2,3]);").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1,2,3"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_object_utility_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let e = Object.entries({\"a\":1}); e[0][0] + e[0][1];")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "a1"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("let obj = Object.fromEntries([[\"a\",1],[\"b\",2]]); obj.a + obj.b;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("Object.values({\"a\":1,\"b\":2,\"c\":3})[1];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        Object::Float(v) => assert!((v - 2.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("Object.keys({\"z\":1,\"a\":2,\"m\":3})[0] + Object.keys({\"z\":1,\"a\":2,\"m\":3})[1] + Object.keys({\"z\":1,\"a\":2,\"m\":3})[2];")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "zam"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("Object.keys({2:\"b\", 1:\"a\", 3:\"c\"}).join(\",\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1,2,3"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("Object.keys({b:2, 1:\"a\", a:1, 0:\"z\"}).join(\",\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "0,1,b,a"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("Object.keys({c:3, a:1, b:2}).join(\",\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "c,a,b"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("Object.values({2:\"b\", 1:\"a\", 3:\"c\"}).join(\",\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "a,b,c"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("Object.entries({2:\"b\", 1:\"a\"}).map(e => e[0]).join(\",\");")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1,2"),
        _ => panic!("expected string output"),
    }

    let out = engine
        .eval("Object.values({\"z\":1,\"a\":2,\"m\":3})[0] + Object.values({\"z\":1,\"a\":2,\"m\":3})[1] + Object.values({\"z\":1,\"a\":2,\"m\":3})[2];")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        Object::Float(v) => assert!((v - 6.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let out = Object.assign({\"a\":1}, {\"b\":2}, {\"a\":3}); out.a + out.b;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        Object::Float(v) => assert!((v - 5.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("let out = Object.assign({}, \"ab\"); out[\"0\"] + out[\"1\"]; ")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "ab"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("Object.keys(\"ab\")[1];").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "1"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("Object.values(\"ab\")[0];").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "a"),
        _ => panic!("expected string output"),
    }

    let out = engine.eval("Object.entries(\"ab\")[1][1];").expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "b"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_object_has_own_subset() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("Object.hasOwn({\"a\":1}, \"a\") ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("Object.hasOwn(\"ab\", \"1\") ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_array_modern_methods_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("[1,2,3].at(-1);").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[3,1,2].toSorted()[0];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine.eval("[1,2,3].with(1, 9)[1];").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 9),
        Object::Float(v) => assert!((v - 9.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_regex_literal_subset() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval("/hello/.test(\"hello\") ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("/hello/i.test(\"HeLLo\") ? 1 : 0;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 1),
        Object::Float(v) => assert!((v - 1.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }

    let out = engine
        .eval("\"ab12cd34\".match(/\\d+/g)[1];")
        .expect("eval");
    match out {
        Object::String(v) => assert_eq!(&*v, "34"),
        _ => panic!("expected string output"),
    }
}

#[test]
fn engine_eval_delete_array_index_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let arr = [10,20,30]; delete arr[1]; arr[1];")
        .expect("eval");
    assert!(matches!(out, Object::Undefined));

    let out = engine
        .eval("let arr = [10,20,30]; delete arr[1]; arr.length;")
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
        _ => panic!("expected numeric output"),
    }
}

#[test]
fn engine_eval_sparse_array_hole_read_subset() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval("let arr = [1,2,3]; arr[10] = 42; arr[5];")
        .expect("eval");
    assert!(matches!(out, Object::Undefined));
}

#[test]
fn engine_eval_exponent_more_negative_subset() {
    let engine = FormLogicEngine::default();

    let out = engine.eval("4 ** -2;").expect("eval");
    match out {
        Object::Float(v) => assert!((v - 0.0625).abs() < 1e-9),
        _ => panic!("expected float output"),
    }
}

#[test]
fn engine_eval_while_conditionals_bench_exact() {
    let engine = FormLogicEngine::default();
    let source = r#"
        let count = 0;
        let i = 0;
        while (i < 10000) {
            if (i % 3 === 0) {
                count = count + 1;
            } else {
                if (i % 3 === 1) {
                    count = count + 2;
                } else {
                    count = count + 3;
                }
            }
            i = i + 1;
        }
        count;
    "#;
    let out = engine.eval(source).expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 19999, "while+conditionals returned wrong value"),
        _ => panic!("expected integer output, got {:?}", out),
    }
}

// ── Variable semantics: const, let, var ───────────────────────────────

#[test]
fn engine_eval_const_assignment_rejected_subset() {
    let engine = FormLogicEngine::default();

    // Direct assignment to const should fail
    let result = engine.eval("const x = 10; x = 20; x;");
    assert!(
        result.is_err(),
        "expected compile error for const reassignment"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("constant variable"),
        "error should mention constant variable, got: {}",
        err
    );
}

#[test]
fn engine_eval_const_compound_assignment_rejected_subset() {
    let engine = FormLogicEngine::default();

    let result = engine.eval("const x = 10; x += 5; x;");
    assert!(result.is_err(), "expected compile error for const += ");
    assert!(result.unwrap_err().contains("constant variable"));
}

#[test]
fn engine_eval_const_increment_rejected_subset() {
    let engine = FormLogicEngine::default();

    let result = engine.eval("const x = 10; x++; x;");
    assert!(result.is_err(), "expected compile error for const ++");
    assert!(result.unwrap_err().contains("constant variable"));
}

#[test]
fn engine_eval_const_prefix_decrement_rejected_subset() {
    let engine = FormLogicEngine::default();

    let result = engine.eval("const x = 10; --x; x;");
    assert!(result.is_err(), "expected compile error for const --x");
    assert!(result.unwrap_err().contains("constant variable"));
}

#[test]
fn engine_eval_const_read_only_works_subset() {
    let engine = FormLogicEngine::default();

    // const should work fine for read-only usage
    let out = engine.eval("const x = 42; x;").expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected integer 42, got {:?}", out),
    }
}

#[test]
fn engine_eval_var_hoisting_subset() {
    let engine = FormLogicEngine::default();

    // var should be hoisted — accessible before declaration as undefined
    let out = engine
        .eval(
            r#"
        let result = typeof x;
        var x = 10;
        result;
    "#,
        )
        .expect("eval");
    match out {
        Object::String(ref s) => assert_eq!(s.as_ref(), "undefined"),
        _ => panic!(
            "expected 'undefined' from typeof hoisted var, got {:?}",
            out
        ),
    }
}

#[test]
fn engine_eval_var_function_scope_hoisting_subset() {
    let engine = FormLogicEngine::default();

    // var inside if-block should be hoisted to function scope
    let out = engine
        .eval(
            r#"
        function test() {
            if (true) {
                var x = 42;
            }
            return x;
        }
        test();
    "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!(
            "expected 42 from var hoisted out of if block, got {:?}",
            out
        ),
    }
}

#[test]
fn engine_eval_var_in_for_loop_hoisted_subset() {
    let engine = FormLogicEngine::default();

    // var in for-loop should be accessible after loop
    let out = engine
        .eval(
            r#"
        function test() {
            for (var i = 0; i < 5; i++) {}
            return i;
        }
        test();
    "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 5),
        _ => panic!("expected 5 from var hoisted from for loop, got {:?}", out),
    }
}

#[test]
fn engine_eval_let_and_const_basic_usage_subset() {
    let engine = FormLogicEngine::default();

    // let and const work for basic declarations
    let out = engine
        .eval(
            r#"
        let a = 1;
        const b = 2;
        var c = 3;
        a + b + c;
    "#,
        )
        .expect("eval");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        _ => panic!("expected 6, got {:?}", out),
    }
}

#[test]
fn engine_eval_const_destructuring_rejected_subset() {
    let engine = FormLogicEngine::default();

    // const with destructuring — reassignment to any name should fail
    let result = engine.eval("const [a, b] = [1, 2]; a = 10; a;");
    assert!(
        result.is_err(),
        "expected compile error for const destructuring reassignment"
    );
    assert!(result.unwrap_err().contains("constant variable"));
}

// ── WP3: Object literal enhancements ────────────────────────────────────

#[test]
fn engine_eval_object_getter() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        let obj = {
            _x: 10,
            get x() {
                return this._x * 2;
            }
        };
        obj.x;
    "#,
        )
        .expect("eval getter");
    match out {
        Object::Integer(v) => assert_eq!(v, 20),
        _ => panic!("expected 20 from getter, got {:?}", out),
    }
}

#[test]
fn engine_eval_object_setter() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        let obj = {
            _x: 0,
            get x() {
                return this._x;
            },
            set x(val) {
                this._x = val * 3;
            }
        };
        obj.x = 5;
        obj.x;
    "#,
        )
        .expect("eval setter");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        _ => panic!("expected 15 from setter, got {:?}", out),
    }
}

#[test]
fn engine_eval_object_method_shorthand() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        let obj = {
            value: 42,
            getValue() {
                return this.value;
            }
        };
        obj.getValue();
    "#,
        )
        .expect("eval method shorthand");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected 42 from method shorthand, got {:?}", out),
    }
}

#[test]
fn engine_eval_object_getter_no_data_property() {
    let engine = FormLogicEngine::default();

    // Getter should be invoked even when no data property exists
    let out = engine
        .eval(
            r#"
        let obj = {
            get name() {
                return "hello";
            }
        };
        obj.name;
    "#,
        )
        .expect("eval getter no data");
    match out {
        Object::String(s) => assert_eq!(&*s, "hello"),
        _ => panic!("expected 'hello' from getter, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_expression() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        let MyClass = class {
            constructor(val) {
                this.val = val;
            }
            getVal() {
                return this.val;
            }
        };
        let inst = new MyClass(99);
        inst.getVal();
    "#,
        )
        .expect("eval class expression");
    match out {
        Object::Integer(v) => assert_eq!(v, 99),
        _ => panic!("expected 99 from class expression, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_extends_expression() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        class Base {
            constructor(x) {
                this.x = x;
            }
            getX() {
                return this.x;
            }
        }
        class Derived extends Base {
            constructor(x, y) {
                super(x);
                this.y = y;
            }
            sum() {
                return this.x + this.y;
            }
        }
        let d = new Derived(10, 20);
        d.sum();
    "#,
        )
        .expect("eval class extends");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        _ => panic!("expected 30 from class extends, got {:?}", out),
    }
}

#[test]
fn engine_eval_named_class_expression() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        let Foo = class Bar {
            constructor() {
                this.val = 7;
            }
        };
        let f = new Foo();
        f.val;
    "#,
        )
        .expect("eval named class expression");
    match out {
        Object::Integer(v) => assert_eq!(v, 7),
        _ => panic!("expected 7 from named class expression, got {:?}", out),
    }
}

#[test]
fn engine_eval_getter_setter_with_bracket_access() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        let obj = {
            _v: 100,
            get v() { return this._v; },
            set v(x) { this._v = x + 1; }
        };
        obj["v"] = 10;
        obj["v"];
    "#,
        )
        .expect("eval getter/setter bracket access");
    match out {
        Object::Integer(v) => assert_eq!(v, 11),
        _ => panic!("expected 11 from bracket getter/setter, got {:?}", out),
    }
}

// ── WP4: Class fields, static blocks, private fields ─────────────────────

#[test]
fn engine_eval_class_instance_fields_with_initializer() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        class Point {
            x = 10;
            y = 20;
            sum() { return this.x + this.y; }
        }
        let p = new Point();
        p.sum();
    "#,
        )
        .expect("eval instance fields with initializer");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        _ => panic!("expected 30, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_instance_fields_without_initializer() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        class Foo {
            x;
            check() { return this.x; }
        }
        let f = new Foo();
        f.check();
    "#,
        )
        .expect("eval instance field without initializer");
    match out {
        Object::Undefined => {}
        _ => panic!("expected undefined, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_instance_fields_with_constructor() {
    let engine = FormLogicEngine::default();

    // Fields are initialized before constructor runs.
    // Constructor can override field values.
    let out = engine
        .eval(
            r#"
        class Counter {
            count = 0;
            constructor(start) { this.count = start; }
            get() { return this.count; }
        }
        let c = new Counter(42);
        c.get();
    "#,
        )
        .expect("eval instance fields with constructor");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected 42, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_static_field() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        class Config {
            static defaultValue = 99;
        }
        Config.defaultValue;
    "#,
        )
        .expect("eval static field");
    match out {
        Object::Integer(v) => assert_eq!(v, 99),
        _ => panic!("expected 99, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_static_field_expression() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        class Math2 {
            static PI_TIMES_2 = 3 * 2;
        }
        Math2.PI_TIMES_2;
    "#,
        )
        .expect("eval static field expression");
    match out {
        Object::Integer(v) => assert_eq!(v, 6),
        _ => panic!("expected 6, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_static_block() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        let captured = 0;
        class Init {
            static x = 10;
            static {
                captured = this.x + 5;
            }
        }
        captured;
    "#,
        )
        .expect("eval static block");
    match out {
        Object::Integer(v) => assert_eq!(v, 15),
        _ => panic!("expected 15, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_multiple_static_blocks() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        let log = 0;
        class Multi {
            static a = 1;
            static { log = log + this.a; }
            static b = 2;
            static { log = log + this.b; }
        }
        log;
    "#,
        )
        .expect("eval multiple static blocks");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        _ => panic!("expected 3, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_private_field() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        class Secret {
            #value = 42;
            reveal() { return this.#value; }
        }
        let s = new Secret();
        s.reveal();
    "#,
        )
        .expect("eval private field");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected 42, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_private_field_set() {
    let engine = FormLogicEngine::default();

    // Test private field set via constructor (receiver_after propagation)
    let out = engine
        .eval(
            r#"
        class Box {
            #content = 0;
            constructor(v) { this.#content = v; }
            get() { return this.#content; }
        }
        let b = new Box(77);
        b.get();
    "#,
        )
        .expect("eval private field set");
    match out {
        Object::Integer(v) => assert_eq!(v, 77),
        _ => panic!("expected 77, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_inherited_fields() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        class Base {
            x = 10;
        }
        class Child extends Base {
            y = 20;
            sum() { return this.x + this.y; }
        }
        let c = new Child();
        c.sum();
    "#,
        )
        .expect("eval inherited fields");
    match out {
        Object::Integer(v) => assert_eq!(v, 30),
        _ => panic!("expected 30, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_field_default_before_constructor() {
    let engine = FormLogicEngine::default();

    // Instance field initializers run before the constructor body.
    // So if the constructor does NOT set count, it keeps its default.
    let out = engine
        .eval(
            r#"
        class Counter {
            count = 100;
            constructor() { }
            get() { return this.count; }
        }
        let c = new Counter();
        c.get();
    "#,
        )
        .expect("eval field default preserved when constructor doesn't set it");
    match out {
        Object::Integer(v) => assert_eq!(v, 100),
        _ => panic!("expected 100, got {:?}", out),
    }
}

#[test]
fn engine_eval_class_static_and_instance_fields() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        class Dual {
            static shared = 1000;
            own = 1;
            total() { return Dual.shared + this.own; }
        }
        let d = new Dual();
        d.total();
    "#,
        )
        .expect("eval static + instance fields together");
    match out {
        Object::Integer(v) => assert_eq!(v, 1001),
        _ => panic!("expected 1001, got {:?}", out),
    }
}

// ─── WP5: Debugger & Tagged Templates ───────────────────────────────

#[test]
fn engine_eval_debugger_statement() {
    let engine = FormLogicEngine::default();

    // debugger should be a no-op that doesn't error
    let out = engine
        .eval(
            r#"
        let x = 1;
        debugger;
        x + 2;
    "#,
        )
        .expect("debugger statement should be a no-op");
    match out {
        Object::Integer(v) => assert_eq!(v, 3),
        _ => panic!("expected 3, got {:?}", out),
    }
}

#[test]
fn engine_eval_debugger_in_function() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        function f() {
            debugger;
            return 42;
        }
        f();
    "#,
        )
        .expect("debugger inside function should be a no-op");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected 42, got {:?}", out),
    }
}

#[test]
fn engine_eval_tagged_template_no_interpolation() {
    let engine = FormLogicEngine::default();

    // Tag function receives (strings_obj). strings_obj[0] is the cooked string.
    let out = engine
        .eval(
            r#"
        function tag(strings) {
            return strings[0];
        }
        tag`hello world`;
    "#,
        )
        .expect("tagged template with no interpolation");
    match out {
        Object::String(s) => assert_eq!(&*s, "hello world"),
        _ => panic!("expected 'hello world', got {:?}", out),
    }
}

#[test]
fn engine_eval_tagged_template_with_interpolation() {
    let engine = FormLogicEngine::default();

    // Tag function joins strings and values together
    let out = engine
        .eval(
            r#"
        function tag(strings, val) {
            return strings[0] + val + strings[1];
        }
        let name = "world";
        tag`hello ${name}!`;
    "#,
        )
        .expect("tagged template with interpolation");
    match out {
        Object::String(s) => assert_eq!(&*s, "hello world!"),
        _ => panic!("expected 'hello world!', got {:?}", out),
    }
}

#[test]
fn engine_eval_tagged_template_multiple_interpolations() {
    let engine = FormLogicEngine::default();

    let out = engine
        .eval(
            r#"
        function tag(strings, a, b, c) {
            return strings[0] + a + strings[1] + b + strings[2] + c + strings[3];
        }
        let x = 10;
        let y = 20;
        tag`sum of ${x} and ${y} is ${x + y}`;
    "#,
        )
        .expect("tagged template with multiple interpolations");
    match out {
        Object::String(s) => assert_eq!(&*s, "sum of 10 and 20 is 30"),
        _ => panic!("expected 'sum of 10 and 20 is 30', got {:?}", out),
    }
}

#[test]
fn engine_eval_tagged_template_raw_access() {
    let engine = FormLogicEngine::default();

    // Access the raw property of the strings object
    let out = engine
        .eval(
            r#"
        function tag(strings) {
            return strings.raw[0];
        }
        tag`hello`;
    "#,
        )
        .expect("tagged template raw access");
    match out {
        Object::String(s) => assert_eq!(&*s, "hello"),
        _ => panic!("expected 'hello', got {:?}", out),
    }
}

#[test]
fn engine_eval_tagged_template_length() {
    let engine = FormLogicEngine::default();

    // The strings object should have a length property
    let out = engine
        .eval(
            r#"
        function tag(strings, a) {
            return strings.length;
        }
        tag`before ${42} after`;
    "#,
        )
        .expect("tagged template strings length");
    match out {
        Object::Integer(v) => assert_eq!(v, 2),
        _ => panic!("expected 2, got {:?}", out),
    }
}

#[test]
fn engine_eval_tagged_template_returns_non_string() {
    let engine = FormLogicEngine::default();

    // Tag functions can return anything, not just strings
    let out = engine
        .eval(
            r#"
        function tag(strings, val) {
            return val * 2;
        }
        tag`double: ${21}`;
    "#,
        )
        .expect("tagged template returning non-string");
    match out {
        Object::Integer(v) => assert_eq!(v, 42),
        _ => panic!("expected 42, got {:?}", out),
    }
}

#[test]
fn engine_eval_tagged_template_expression_tag() {
    let engine = FormLogicEngine::default();

    // Tag can be any expression, e.g. object method
    let out = engine
        .eval(
            r#"
        let obj = {
            tag: function(strings, val) {
                return strings[0] + val;
            }
        };
        obj.tag`value: ${99}`;
    "#,
        )
        .expect("tagged template with expression as tag");
    match out {
        Object::String(s) => assert_eq!(&*s, "value: 99"),
        _ => panic!("expected 'value: 99', got {:?}", out),
    }
}

// ===== WP6: Meta properties (new.target, import.meta) =====

#[test]
fn engine_new_target_in_class_constructor() {
    let engine = FormLogicEngine::default();

    // new.target inside a class constructor should be the class itself.
    // We test by checking that new.target is not undefined (truthy).
    let out = engine
        .eval(
            r#"
        class Foo {
            constructor() {
                this.hasNewTarget = new.target !== undefined;
            }
        }
        let f = new Foo();
        f.hasNewTarget;
    "#,
        )
        .expect("new.target in constructor");
    match out {
        Object::Boolean(b) => assert!(b, "new.target should be defined in constructor"),
        _ => panic!("expected boolean, got {:?}", out),
    }
}

#[test]
fn engine_new_target_outside_constructor() {
    let engine = FormLogicEngine::default();

    // new.target outside a constructor should be undefined.
    let out = engine
        .eval(
            r#"
        function foo() {
            return new.target;
        }
        foo();
    "#,
        )
        .expect("new.target outside constructor");
    match out {
        Object::Undefined | Object::Null => {}
        _ => panic!("expected undefined, got {:?}", out),
    }
}

#[test]
fn engine_new_target_in_subclass_constructor() {
    let engine = FormLogicEngine::default();

    // When constructing a subclass with `new Child()`, new.target in
    // the parent constructor should still refer to the child class.
    // We detect this by storing new.target's name (via a side-channel)
    // in the parent constructor.
    let out = engine
        .eval(
            r#"
        let captured = "none";
        class Parent {
            constructor() {
                this.parentSawNewTarget = new.target !== undefined;
            }
        }
        class Child extends Parent {
            constructor() {
                super();
                this.childSawNewTarget = new.target !== undefined;
            }
        }
        let c = new Child();
        c.childSawNewTarget;
    "#,
        )
        .expect("new.target in subclass constructor");
    match out {
        Object::Boolean(b) => assert!(b, "new.target should be defined in subclass constructor"),
        _ => panic!("expected boolean, got {:?}", out),
    }
}

#[test]
fn engine_import_meta_returns_object() {
    let engine = FormLogicEngine::default();

    // import.meta should return an object (even if stub/empty).
    let out = engine
        .eval(
            r#"
        let meta = import.meta;
        typeof meta;
    "#,
        )
        .expect("import.meta returns object");
    match out {
        Object::String(s) => assert_eq!(&*s, "object", "import.meta should be an object"),
        _ => panic!("expected string 'object', got {:?}", out),
    }
}

// ── WP7: Generators ────────────────────────────────────────────────────

#[test]
fn engine_generator_basic_yield() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
        function* gen() {
            yield 1;
            yield 2;
            yield 3;
        }
        let g = gen();
        let a = g.next();
        a.value;
    "#,
        )
        .expect("basic generator yield");
    match out {
        Object::Integer(n) => assert_eq!(n, 1),
        _ => panic!("expected Integer(1), got {:?}", out),
    }
}

#[test]
fn engine_generator_multiple_yields() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
        function* gen() {
            yield 10;
            yield 20;
            yield 30;
        }
        let g = gen();
        g.next();
        g.next();
        let third = g.next();
        third.value;
    "#,
        )
        .expect("multiple yields");
    match out {
        Object::Integer(n) => assert_eq!(n, 30),
        _ => panic!("expected Integer(30), got {:?}", out),
    }
}

#[test]
fn engine_generator_done_flag() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
        function* gen() {
            yield 1;
        }
        let g = gen();
        let first = g.next();
        let second = g.next();
        let result = "" + first.done + "," + second.done;
        result;
    "#,
        )
        .expect("done flag");
    match out {
        Object::String(s) => assert_eq!(&*s, "false,true"),
        _ => panic!("expected 'false,true', got {:?}", out),
    }
}

#[test]
fn engine_generator_return_value() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
        function* gen() {
            yield 1;
            return 42;
        }
        let g = gen();
        g.next();
        let result = g.next();
        result.value;
    "#,
        )
        .expect("generator return value");
    match out {
        Object::Integer(n) => assert_eq!(n, 42),
        _ => panic!("expected Integer(42), got {:?}", out),
    }
}

#[test]
fn engine_generator_resume_value() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
        function* gen() {
            let x = yield 1;
            yield x + 10;
        }
        let g = gen();
        g.next();
        let result = g.next(5);
        result.value;
    "#,
        )
        .expect("resume value");
    match out {
        Object::Integer(n) => assert_eq!(n, 15),
        _ => panic!("expected Integer(15), got {:?}", out),
    }
}

#[test]
fn engine_generator_return_method() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
        function* gen() {
            yield 1;
            yield 2;
            yield 3;
        }
        let g = gen();
        g.next();
        let result = g.return(99);
        let done_str = "" + result.value + "," + result.done;
        done_str;
    "#,
        )
        .expect("generator .return()");
    match out {
        Object::String(s) => assert_eq!(&*s, "99,true"),
        _ => panic!("expected '99,true', got {:?}", out),
    }
}

#[test]
fn engine_generator_exhausted_after_return() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
        function* gen() {
            yield 1;
            yield 2;
        }
        let g = gen();
        g.next();
        g.return(99);
        let result = g.next();
        "" + result.value + "," + result.done;
    "#,
        )
        .expect("exhausted after return");
    match out {
        Object::String(s) => assert_eq!(&*s, "undefined,true"),
        _ => panic!("expected 'undefined,true', got {:?}", out),
    }
}

#[test]
fn engine_generator_no_yields() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
        function* gen() {
            return 42;
        }
        let g = gen();
        let result = g.next();
        "" + result.value + "," + result.done;
    "#,
        )
        .expect("generator with no yields");
    match out {
        Object::String(s) => assert_eq!(&*s, "42,true"),
        _ => panic!("expected '42,true', got {:?}", out),
    }
}

#[test]
fn engine_generator_yield_undefined() {
    let engine = FormLogicEngine::default();
    let out = engine
        .eval(
            r#"
        function* gen() {
            yield;
        }
        let g = gen();
        let result = g.next();
        "" + result.value + "," + result.done;
    "#,
        )
        .expect("yield undefined");
    match out {
        Object::String(s) => assert_eq!(&*s, "undefined,false"),
        _ => panic!("expected 'undefined,false', got {:?}", out),
    }
}

#[test]
fn engine_generator_accumulator_pattern() {
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
        let c = counter();
        c.next().value;
        c.next().value;
        c.next().value;
        c.next().value;
        c.next().value;
    "#,
        )
        .expect("accumulator pattern");
    match out {
        Object::Integer(n) => assert_eq!(n, 5),
        _ => panic!("expected Integer(5), got {:?}", out),
    }
}

// ── Benchmark issue reproduction tests ─────────────────────────────

#[test]
fn engine_bench_obj_property_loop() {
    let engine = FormLogicEngine::default();
    let source = r#"
        let obj = { x: 0, y: 0, z: 0 };
        for (let i = 0; i < 5000; i = i + 1) {
            obj.x = obj.x + 1;
            obj.y = obj.y + 2;
            obj.z = obj.x + obj.y;
        }
        obj.z;
    "#;
    // Run twice to test VM reuse (benchmark runs warmup + multiple runs)
    let _ = engine.eval(source).expect("obj prop loop run 1");
    let out = engine.eval(source).expect("obj prop loop run 2");
    match out {
        Object::Integer(n) => assert_eq!(n, 15000),
        _ => panic!("expected Integer(15000), got {:?}", out),
    }
}

#[test]
fn engine_bench_map_set_get() {
    let engine = FormLogicEngine::default();
    let source = r#"
        let m = new Map();
        for (let i = 0; i < 100; i = i + 1) {
            m.set("key" + i, i * 3);
        }
        let total = 0;
        for (let i = 0; i < 100; i = i + 1) {
            total = total + m.get("key" + i);
        }
        total;
    "#;
    let _ = engine.eval(source).expect("map run 1");
    let out = engine.eval(source).expect("map run 2");
    match out {
        Object::Integer(n) => assert_eq!(n, 14850),
        _ => panic!("expected Integer(14850), got {:?}", out),
    }
}

#[test]
fn engine_bench_array_index_write_sum() {
    let engine = FormLogicEngine::default();
    let source = r#"
        let arr = [];
        for (let i = 0; i < 2000; i = i + 1) {
            arr[i] = i * 2;
        }
        let total = 0;
        for (let i = 0; i < 2000; i = i + 1) {
            total = total + arr[i];
        }
        total;
    "#;
    let _ = engine.eval(source).expect("arr run 1");
    let out = engine.eval(source).expect("arr run 2");
    match out {
        Object::Integer(n) => assert_eq!(n, 3998000),
        _ => panic!("expected Integer(3998000), got {:?}", out),
    }
}

#[test]
fn engine_bench_string_concat_length() {
    let engine = FormLogicEngine::default();
    let source = r#"
        let s = "";
        for (let i = 0; i < 1000; i = i + 1) {
            s = s + "a";
        }
        s.length;
    "#;
    let _ = engine.eval(source).expect("str run 1");
    let out = engine.eval(source).expect("str run 2");
    match out {
        Object::Integer(n) => assert_eq!(n, 1000),
        _ => panic!("expected Integer(1000), got {:?}", out),
    }
}
