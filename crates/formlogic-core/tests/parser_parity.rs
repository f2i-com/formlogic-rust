use formlogic_core::ast::{
    ArrayBindingItem, BindingPattern, BindingTarget, ClassMember, Expression, ForBinding,
    HashEntry, ObjectBindingItem, Statement, VariableKind,
};
use formlogic_core::parser::parse_program_from_source;

#[test]
fn parser_let_statements_parity_subset() {
    let (program, errors) =
        parse_program_from_source("let x = 5; let y = 10; let foobar = 838383;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 3);

    let names = ["x", "y", "foobar"];
    for (i, expected) in names.iter().enumerate() {
        match &program.statements[i] {
            Statement::Let { name, .. } => assert_eq!(name, expected),
            _ => panic!("expected let statement"),
        }
    }
}

#[test]
fn parser_return_statements_parity_subset() {
    let (program, errors) = parse_program_from_source("return 5; return 10; return 993322;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 3);
    for stmt in &program.statements {
        assert!(matches!(stmt, Statement::Return { .. }));
    }
}

#[test]
fn parser_prefix_and_infix_parity_subset() {
    let (program, errors) = parse_program_from_source("!-5; 5 + 5 * 2;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Expression(Expression::Prefix { operator, .. }) => assert_eq!(operator, "!"),
        _ => panic!("expected prefix expression"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Infix { operator, .. }) => assert_eq!(operator, "+"),
        _ => panic!("expected infix expression"),
    }
}

#[test]
fn parser_parses_literals_and_collections_subset() {
    let (program, errors) =
        parse_program_from_source("null; undefined; [1,2,3]; {\"name\":\"John\", \"age\":30};");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 4);
    assert!(matches!(
        program.statements[0],
        Statement::Expression(Expression::Null)
    ));

    assert!(matches!(
        &program.statements[1],
        Statement::Expression(Expression::Identifier(name)) if name == "undefined"
    ));

    match &program.statements[2] {
        Statement::Expression(Expression::Array(items)) => assert_eq!(items.len(), 3),
        _ => panic!("expected array literal"),
    }

    match &program.statements[3] {
        Statement::Expression(Expression::Hash(pairs)) => assert_eq!(pairs.len(), 2),
        _ => panic!("expected hash literal"),
    }
}

#[test]
fn parser_parses_if_function_and_call_subset() {
    let (program, errors) = parse_program_from_source(
        "if (x > 5) { return true; } else { return false; } function(a,b){ return a+b; } add(1,2);",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 3);

    match &program.statements[0] {
        Statement::Expression(Expression::If {
            condition,
            consequence,
            alternative,
        }) => {
            assert!(matches!(**condition, Expression::Infix { .. }));
            assert_eq!(consequence.len(), 1);
            assert!(alternative.is_some());
        }
        _ => panic!("expected if expression"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Function {
            parameters, body, ..
        }) => {
            assert_eq!(parameters.len(), 2);
            assert_eq!(body.len(), 1);
        }
        _ => panic!("expected function literal"),
    }

    match &program.statements[2] {
        Statement::Expression(Expression::Call { arguments, .. }) => assert_eq!(arguments.len(), 2),
        _ => panic!("expected call expression"),
    }
}

#[test]
fn parser_parses_index_and_dot_access_subset() {
    let (program, errors) = parse_program_from_source("arr[1]; obj.name;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Expression(Expression::Index { .. }) => {}
        _ => panic!("expected index expression"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Index { .. }) => {}
        _ => panic!("expected dot/index expression"),
    }
}

#[test]
fn parser_parses_assignment_subset() {
    let (program, errors) = parse_program_from_source("x = 3; x += 2; arr[0] = 9;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 3);

    match &program.statements[0] {
        Statement::Expression(Expression::Assign { operator, .. }) => assert_eq!(operator, "="),
        _ => panic!("expected assign expression"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Assign { operator, .. }) => assert_eq!(operator, "+="),
        _ => panic!("expected compound assign expression"),
    }

    match &program.statements[2] {
        Statement::Expression(Expression::Assign { .. }) => {}
        _ => panic!("expected index assign expression"),
    }
}

#[test]
fn parser_parses_while_break_continue_subset() {
    let (program, errors) = parse_program_from_source(
        "while (x < 10) { x = x + 1; if (x == 5) { continue; } if (x == 8) { break; } }",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0] {
        Statement::While { body, .. } => {
            assert!(!body.is_empty());
        }
        _ => panic!("expected while statement"),
    }
}

#[test]
fn parser_parses_for_loop_subset() {
    let (program, errors) =
        parse_program_from_source("for (let i = 0; i < 3; i = i + 1) { x = x + i; }");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0] {
        Statement::For {
            init,
            condition,
            update,
            body,
        } => {
            assert!(init.is_some());
            assert!(condition.is_some());
            assert!(update.is_some());
            assert!(!body.is_empty());
        }
        _ => panic!("expected for statement"),
    }
}

#[test]
fn parser_parses_function_declaration_subset() {
    let (program, errors) = parse_program_from_source(
        "function add(a, b) { return a + b; } async function id(x) { return x; }",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::FunctionDecl {
            name,
            parameters,
            is_async,
            ..
        } => {
            assert_eq!(name, "add");
            assert_eq!(parameters.len(), 2);
            assert!(!is_async);
        }
        _ => panic!("expected function declaration"),
    }

    match &program.statements[1] {
        Statement::FunctionDecl {
            name,
            parameters,
            is_async,
            ..
        } => {
            assert_eq!(name, "id");
            assert_eq!(parameters.len(), 1);
            assert!(*is_async);
        }
        _ => panic!("expected async function declaration"),
    }
}

#[test]
fn parser_parses_for_of_subset() {
    let (program, errors) = parse_program_from_source("for (let x of [1,2,3]) { sum = sum + x; }");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::ForOf {
            binding,
            iterable,
            body,
        } => {
            assert!(matches!(binding, ForBinding::Identifier(n) if n == "x"));
            assert!(matches!(iterable, Expression::Array(_)));
            assert!(!body.is_empty());
        }
        _ => panic!("expected for-of statement"),
    }
}

#[test]
fn parser_parses_for_in_subset() {
    let (program, errors) =
        parse_program_from_source("for (let k in {\"a\":1, \"b\":2}) { acc = acc + k; }");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::ForIn {
            var_name,
            iterable,
            body,
        } => {
            assert_eq!(var_name, "k");
            assert!(matches!(iterable, Expression::Hash(_)));
            assert!(!body.is_empty());
        }
        _ => panic!("expected for-in statement"),
    }
}

#[test]
fn parser_parses_for_of_destructuring_subset() {
    let (program, errors) =
        parse_program_from_source("for (let [n, s] of [[1,\"a\"],[2,\"b\"]]) { out = n; }");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::ForOf {
            binding,
            iterable,
            body,
        } => {
            match binding {
                ForBinding::Pattern(BindingPattern::Array(items)) => {
                    assert_eq!(items.len(), 2);
                }
                _ => panic!("expected array binding pattern in for-of"),
            }
            assert!(matches!(iterable, Expression::Array(_)));
            assert!(!body.is_empty());
        }
        _ => panic!("expected for-of statement"),
    }
}

#[test]
fn parser_parses_for_of_object_rest_subset() {
    let (program, errors) =
        parse_program_from_source("for (let {a, ...rest} of [{\"a\":1,\"b\":2}]) { out = rest; }");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::ForOf {
            binding,
            iterable,
            body,
        } => {
            match binding {
                ForBinding::Pattern(BindingPattern::Object(items)) => {
                    assert_eq!(items.len(), 2);
                    assert!(
                        matches!(&items[0], ObjectBindingItem { key: Expression::String(key), target: BindingTarget::Identifier(name), is_rest: false, .. } if key == "a" && name == "a")
                    );
                    assert!(
                        matches!(&items[1], ObjectBindingItem { target: BindingTarget::Identifier(name), is_rest: true, .. } if name == "rest")
                    );
                }
                _ => panic!("expected object binding pattern in for-of"),
            }
            assert!(matches!(iterable, Expression::Array(_)));
            assert!(!body.is_empty());
        }
        _ => panic!("expected for-of statement"),
    }
}

#[test]
fn parser_parses_class_and_new_subset() {
    let (program, errors) = parse_program_from_source(
        "class Point { constructor(x, y) { this.x = x; this.y = y; } sum() { return this.x + this.y; } } let p = new Point(2, 3); p.sum();",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 3);

    match &program.statements[0] {
        Statement::ClassDecl {
            name,
            members,
            extends,
        } => {
            assert_eq!(name.as_deref(), Some("Point"));
            assert!(extends.is_none());
            // 2 methods: constructor and sum
            let method_count = members
                .iter()
                .filter(|m| matches!(m, ClassMember::Method(_)))
                .count();
            assert_eq!(method_count, 2);
        }
        _ => panic!("expected class declaration"),
    }

    match &program.statements[1] {
        Statement::Let { value, .. } => match value {
            Expression::New { .. } => {}
            _ => panic!("expected new expression"),
        },
        _ => panic!("expected let statement for new expression"),
    }
}

#[test]
fn parser_parses_try_catch_finally_subset() {
    let (program, errors) =
        parse_program_from_source("try { throw 1; } catch (e) { x = e; } finally { x = x + 1; }");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::Try {
            try_block,
            catch_param,
            catch_block,
            finally_block,
        } => {
            assert!(!try_block.is_empty());
            assert_eq!(catch_param.as_deref(), Some("e"));
            assert!(catch_block.is_some());
            assert!(finally_block.is_some());
        }
        _ => panic!("expected try statement"),
    }
}

#[test]
fn parser_parses_in_and_instanceof_subset() {
    let (program, errors) = parse_program_from_source("\"a\" in {\"a\":1}; x instanceof Point;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Expression(Expression::Infix { operator, .. }) => assert_eq!(operator, "in"),
        _ => panic!("expected 'in' infix expression"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Infix { operator, .. }) => {
            assert_eq!(operator, "instanceof")
        }
        _ => panic!("expected 'instanceof' infix expression"),
    }
}

#[test]
fn parser_parses_class_method_kinds_subset() {
    let (program, errors) = parse_program_from_source(
        "class Counter { static create(v) { return v; } get value() { return this._v; } set value(v) { this._v = v; } inc() { this._v = this._v + 1; } }",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::ClassDecl { members, .. } => {
            let methods: Vec<_> = members
                .iter()
                .filter_map(|m| match m {
                    ClassMember::Method(cm) => Some(cm),
                    _ => None,
                })
                .collect();
            assert_eq!(methods.len(), 4);
            assert!(methods.iter().any(|m| m.name == "create" && m.is_static));
            assert!(methods.iter().any(|m| m.name == "value" && m.is_getter));
            assert!(methods.iter().any(|m| m.name == "value" && m.is_setter));
            assert!(methods.iter().any(|m| m.name == "inc"));
        }
        _ => panic!("expected class declaration"),
    }
}

#[test]
fn parser_parses_class_extends_and_super_subset() {
    let (program, errors) = parse_program_from_source(
        "class A { value() { return 1; } } class B extends A { value() { return super.value() + 1; } }",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[1] {
        Statement::ClassDecl {
            name,
            extends,
            members,
        } => {
            assert_eq!(name.as_deref(), Some("B"));
            match extends.as_deref() {
                Some(Expression::Identifier(s)) => assert_eq!(s, "A"),
                other => panic!("expected extends Identifier(\"A\"), got {:?}", other),
            }
            let method_count = members
                .iter()
                .filter(|m| matches!(m, ClassMember::Method(_)))
                .count();
            assert_eq!(method_count, 1);
        }
        _ => panic!("expected subclass declaration"),
    }
}

#[test]
fn parser_parses_await_subset() {
    let (program, errors) =
        parse_program_from_source("await 1; async function f(x) { return await x; }");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Expression(Expression::Await { .. }) => {}
        _ => panic!("expected await expression"),
    }

    match &program.statements[1] {
        Statement::FunctionDecl { is_async, .. } => assert!(*is_async),
        _ => panic!("expected async function declaration"),
    }
}

#[test]
fn parser_parses_typeof_and_delete_subset() {
    let (program, errors) = parse_program_from_source("typeof x; delete obj.a;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Expression(Expression::Typeof { .. }) => {}
        _ => panic!("expected typeof expression"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Delete { .. }) => {}
        _ => panic!("expected delete expression"),
    }
}

#[test]
fn parser_parses_void_and_bitwise_subset() {
    let (program, errors) = parse_program_from_source("void x; (1 | 2) ^ 3; 8 >>> 1;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 3);

    match &program.statements[0] {
        Statement::Expression(Expression::Void { .. }) => {}
        _ => panic!("expected void expression"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Infix { .. }) => {}
        _ => panic!("expected bitwise infix expression"),
    }

    match &program.statements[2] {
        Statement::Expression(Expression::Infix { operator, .. }) => assert_eq!(operator, ">>>"),
        _ => panic!("expected unsigned right shift expression"),
    }
}

#[test]
fn parser_parses_logical_and_nullish_subset() {
    let (program, errors) = parse_program_from_source("a && b; a || b; a ?? b;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 3);

    match &program.statements[0] {
        Statement::Expression(Expression::Infix { operator, .. }) => assert_eq!(operator, "&&"),
        _ => panic!("expected && infix"),
    }
    match &program.statements[1] {
        Statement::Expression(Expression::Infix { operator, .. }) => assert_eq!(operator, "||"),
        _ => panic!("expected || infix"),
    }
    match &program.statements[2] {
        Statement::Expression(Expression::Infix { operator, .. }) => assert_eq!(operator, "??"),
        _ => panic!("expected ?? infix"),
    }
}

#[test]
fn parser_parses_optional_chain_and_logical_assign_subset() {
    let (program, errors) =
        parse_program_from_source("obj?.name; maybe?.(1); x &&= y; x ||= y; x ??= y;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 5);

    match &program.statements[0] {
        Statement::Expression(Expression::OptionalIndex { .. }) => {}
        _ => panic!("expected optional index expression"),
    }
    match &program.statements[1] {
        Statement::Expression(Expression::OptionalCall { .. }) => {}
        _ => panic!("expected optional call expression"),
    }
    match &program.statements[2] {
        Statement::Expression(Expression::Assign { operator, .. }) => assert_eq!(operator, "&&="),
        _ => panic!("expected &&= assignment"),
    }
    match &program.statements[3] {
        Statement::Expression(Expression::Assign { operator, .. }) => assert_eq!(operator, "||="),
        _ => panic!("expected ||= assignment"),
    }
    match &program.statements[4] {
        Statement::Expression(Expression::Assign { operator, .. }) => assert_eq!(operator, "??="),
        _ => panic!("expected ??= assignment"),
    }
}

#[test]
fn parser_parses_unary_plus_and_bitwise_not_subset() {
    let (program, errors) = parse_program_from_source("+x; ~5;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Expression(Expression::Prefix { operator, .. }) => assert_eq!(operator, "+"),
        _ => panic!("expected unary plus prefix expression"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Prefix { operator, .. }) => assert_eq!(operator, "~"),
        _ => panic!("expected bitwise not prefix expression"),
    }
}

#[test]
fn parser_parses_bitwise_shift_assign_subset() {
    let (program, errors) =
        parse_program_from_source("x &= y; x |= y; x ^= y; x <<= 1; x >>= 1; x >>>= 1;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 6);

    let expected = ["&=", "|=", "^=", "<<=", ">>=", ">>>="];
    for (i, op) in expected.iter().enumerate() {
        match &program.statements[i] {
            Statement::Expression(Expression::Assign { operator, .. }) => assert_eq!(operator, op),
            _ => panic!("expected assignment expression"),
        }
    }
}

#[test]
fn parser_parses_index_compound_assign_subset() {
    let (program, errors) =
        parse_program_from_source("arr[1] += 2; obj.a &= 3; obj[\"x\"] >>>= 1;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 3);

    let expected = ["+=", "&=", ">>>="];
    for (i, op) in expected.iter().enumerate() {
        match &program.statements[i] {
            Statement::Expression(Expression::Assign { left, operator, .. }) => {
                assert_eq!(operator, op);
                assert!(matches!(**left, Expression::Index { .. }));
            }
            _ => panic!("expected index assignment expression"),
        }
    }
}

#[test]
fn parser_parses_labeled_loop_control_subset() {
    let (program, errors) = parse_program_from_source(
        "outer: for (let i = 0; i < 3; i = i + 1) { if (i == 1) { break outer; } } inner: while (x < 3) { continue inner; }",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Labeled { label, statement } => {
            assert_eq!(label, "outer");
            assert!(matches!(**statement, Statement::For { .. }));
        }
        _ => panic!("expected labeled for statement"),
    }

    match &program.statements[1] {
        Statement::Labeled { label, statement } => {
            assert_eq!(label, "inner");
            assert!(matches!(**statement, Statement::While { .. }));
        }
        _ => panic!("expected labeled while statement"),
    }
}

#[test]
fn parser_parses_exponent_and_assign_subset() {
    let (program, errors) = parse_program_from_source("2 ** 3; x **= 2;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Expression(Expression::Infix { operator, .. }) => assert_eq!(operator, "**"),
        _ => panic!("expected exponent infix expression"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Assign { operator, .. }) => assert_eq!(operator, "**="),
        _ => panic!("expected exponent assignment expression"),
    }
}

#[test]
fn parser_parses_exponent_right_associative_subset() {
    let (program, errors) = parse_program_from_source("2 ** 3 ** 2;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::Expression(Expression::Infix {
            operator,
            left,
            right,
        }) => {
            assert_eq!(operator, "**");
            assert!(matches!(&**left, Expression::Integer(2)));
            assert!(matches!(&**right, Expression::Infix { operator, .. } if operator == "**"));
        }
        _ => panic!("expected exponent infix expression"),
    }
}

#[test]
fn parser_parses_basic_destructuring_let_subset() {
    let (program, errors) =
        parse_program_from_source("let [a, , c] = [1,2,3]; let {x, y: b} = {x:10, y:20};");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Array(items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(
                    &items[0],
                    ArrayBindingItem::Binding {
                        target: BindingTarget::Identifier(name),
                        default_value: None
                    } if name == "a"
                ));
                assert!(matches!(&items[1], ArrayBindingItem::Hole));
                assert!(matches!(
                    &items[2],
                    ArrayBindingItem::Binding {
                        target: BindingTarget::Identifier(name),
                        default_value: None
                    } if name == "c"
                ));
            }
            _ => panic!("expected array binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }

    match &program.statements[1] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Object(pairs) => {
                assert_eq!(pairs.len(), 2);
                assert_eq!(
                    pairs[0],
                    ObjectBindingItem {
                        key: Expression::String("x".to_string()),
                        target: BindingTarget::Identifier("x".to_string()),
                        default_value: None,
                        is_rest: false,
                    }
                );
                assert_eq!(
                    pairs[1],
                    ObjectBindingItem {
                        key: Expression::String("y".to_string()),
                        target: BindingTarget::Identifier("b".to_string()),
                        default_value: None,
                        is_rest: false,
                    }
                );
            }
            _ => panic!("expected object binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }
}

#[test]
fn parser_parses_basic_destructuring_assignment_subset() {
    let (program, errors) =
        parse_program_from_source("[a, b] = [1, 2]; {\"x\": a, \"y\": b} = obj;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Expression(Expression::Assign { left, operator, .. }) => {
            assert_eq!(operator, "=");
            assert!(matches!(**left, Expression::Array(_)));
        }
        _ => panic!("expected array destructuring assignment"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Assign { left, operator, .. }) => {
            assert_eq!(operator, "=");
            assert!(matches!(**left, Expression::Hash(_)));
        }
        _ => panic!("expected object destructuring assignment"),
    }
}

#[test]
fn parser_parses_object_shorthand_subset() {
    let (program, errors) = parse_program_from_source("let x = 10; let o = {x}; {x} = o;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 3);

    match &program.statements[1] {
        Statement::Let {
            value: Expression::Hash(pairs),
            ..
        } => {
            assert_eq!(pairs.len(), 1);
            assert!(
                matches!(&pairs[0], HashEntry::KeyValue { key: Expression::String(k), value: Expression::Identifier(v) } if k == "x" && v == "x")
            );
        }
        _ => panic!("expected shorthand object literal"),
    }

    match &program.statements[2] {
        Statement::Expression(Expression::Assign { left, .. }) => {
            assert!(matches!(**left, Expression::Hash(_)));
        }
        _ => panic!("expected object destructuring assignment"),
    }
}

#[test]
fn parser_parses_destructuring_defaults_subset() {
    let (program, errors) =
        parse_program_from_source("let [a = 1, b] = arr; let {x = 5, y: z = 9} = obj;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Array(items) => {
                assert!(
                    matches!(&items[0], ArrayBindingItem::Binding { target: BindingTarget::Identifier(name), default_value: Some(_)} if name == "a")
                );
                assert!(
                    matches!(&items[1], ArrayBindingItem::Binding { target: BindingTarget::Identifier(name), default_value: None} if name == "b")
                );
            }
            _ => panic!("expected array binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }

    match &program.statements[1] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Object(items) => {
                assert!(
                    matches!(&items[0], ObjectBindingItem { key: Expression::String(key), target: BindingTarget::Identifier(name), default_value: Some(_), is_rest: false} if key == "x" && name == "x")
                );
                assert!(
                    matches!(&items[1], ObjectBindingItem { key: Expression::String(key), target: BindingTarget::Identifier(name), default_value: Some(_), is_rest: false} if key == "y" && name == "z")
                );
            }
            _ => panic!("expected object binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }
}

#[test]
fn parser_parses_array_rest_binding_subset() {
    let (program, errors) = parse_program_from_source("let [head, ...rest] = arr;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Array(items) => {
                assert_eq!(items.len(), 2);
                assert!(
                    matches!(&items[0], ArrayBindingItem::Binding { target: BindingTarget::Identifier(name), .. } if name == "head")
                );
                assert!(matches!(&items[1], ArrayBindingItem::Rest { name } if name == "rest"));
            }
            _ => panic!("expected array binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }
}

#[test]
fn parser_parses_object_rest_binding_subset() {
    let (program, errors) = parse_program_from_source("let {a, ...rest} = obj;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Object(items) => {
                assert_eq!(items.len(), 2);
                assert!(
                    matches!(&items[0], ObjectBindingItem { key: Expression::String(key), target: BindingTarget::Identifier(name), is_rest: false, .. } if key == "a" && name == "a")
                );
                assert!(
                    matches!(&items[1], ObjectBindingItem { target: BindingTarget::Identifier(name), is_rest: true, .. } if name == "rest")
                );
            }
            _ => panic!("expected object binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }
}

#[test]
fn parser_parses_computed_object_binding_subset() {
    let (program, errors) = parse_program_from_source("let {[k]: v, ...rest} = obj;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Object(items) => {
                assert_eq!(items.len(), 2);
                assert!(
                    matches!(&items[0], ObjectBindingItem { key: Expression::Identifier(k), target: BindingTarget::Identifier(name), is_rest: false, .. } if k == "k" && name == "v")
                );
                assert!(
                    matches!(&items[1], ObjectBindingItem { target: BindingTarget::Identifier(name), is_rest: true, .. } if name == "rest")
                );
            }
            _ => panic!("expected object binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }
}

#[test]
fn parser_parses_destructuring_assignment_defaults_subset() {
    let (program, errors) =
        parse_program_from_source("[a = 1, b] = arr; {\"x\": a = 5, \"y\": b} = obj;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Expression(Expression::Assign { left, operator, .. }) => {
            assert_eq!(operator, "=");
            match &**left {
                Expression::Array(items) => {
                    assert!(matches!(
                        &items[0],
                        Expression::Assign {
                            left,
                            operator,
                            right: _
                        } if operator == "=" && matches!(&**left, Expression::Identifier(n) if n == "a")
                    ));
                    assert!(matches!(&items[1], Expression::Identifier(n) if n == "b"));
                }
                _ => panic!("expected array destructuring target"),
            }
        }
        _ => panic!("expected destructuring assignment"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Assign { left, operator, .. }) => {
            assert_eq!(operator, "=");
            match &**left {
                Expression::Hash(pairs) => {
                    assert_eq!(pairs.len(), 2);
                    assert!(matches!(&pairs[0],
                        HashEntry::KeyValue { key: Expression::String(k), value: Expression::Assign { left, operator, right: _ } }
                        if k == "x" && operator == "=" && matches!(&**left, Expression::Identifier(n) if n == "a")
                    ));
                    assert!(matches!(&pairs[1],
                        HashEntry::KeyValue { key: Expression::String(k), value: Expression::Identifier(n) } if k == "y" && n == "b"
                    ));
                }
                _ => panic!("expected object destructuring target"),
            }
        }
        _ => panic!("expected destructuring assignment"),
    }
}

#[test]
fn parser_parses_object_rest_assignment_subset() {
    let (program, errors) = parse_program_from_source("{\"a\": a, ...rest} = src;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::Expression(Expression::Assign { left, operator, .. }) => {
            assert_eq!(operator, "=");
            match &**left {
                Expression::Hash(pairs) => {
                    assert_eq!(pairs.len(), 2);
                    assert!(matches!(&pairs[0],
                        HashEntry::KeyValue { key: Expression::String(k), value: Expression::Identifier(n) } if k == "a" && n == "a"));
                    assert!(matches!(&pairs[1],
                        HashEntry::Spread(Expression::Identifier(n)) if n == "rest"));
                }
                _ => panic!("expected object destructuring hash target"),
            }
        }
        _ => panic!("expected assignment expression"),
    }
}

#[test]
fn parser_parses_object_rest_assignment_variants_subset() {
    let (program, errors) = parse_program_from_source("{a, b: bb = 9, ...rest} = src;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::Expression(Expression::Assign { left, operator, .. }) => {
            assert_eq!(operator, "=");
            match &**left {
                Expression::Hash(pairs) => {
                    assert_eq!(pairs.len(), 3);
                    assert!(matches!(&pairs[0],
                        HashEntry::KeyValue { key: Expression::String(k), value: Expression::Identifier(n) } if k == "a" && n == "a"));
                    assert!(
                        matches!(&pairs[1],
                            HashEntry::KeyValue { key: Expression::String(k), value: Expression::Assign { left, operator, .. } }
                            if k == "b" && operator == "=" && matches!(&**left, Expression::Identifier(n) if n == "bb")
                        ) || matches!(&pairs[1],
                            HashEntry::KeyValue { key: Expression::Identifier(k), value: Expression::Assign { left, operator, .. } }
                            if k == "b" && operator == "=" && matches!(&**left, Expression::Identifier(n) if n == "bb")
                        )
                    );
                    assert!(matches!(&pairs[2],
                        HashEntry::Spread(Expression::Identifier(n)) if n == "rest"));
                }
                _ => panic!("expected object destructuring hash target"),
            }
        }
        _ => panic!("expected assignment expression"),
    }
}

#[test]
fn parser_parses_computed_object_rest_assignment_subset() {
    let (program, errors) = parse_program_from_source("{[k]: v, ...rest} = src;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::Expression(Expression::Assign { left, operator, .. }) => {
            assert_eq!(operator, "=");
            match &**left {
                Expression::Hash(pairs) => {
                    assert_eq!(pairs.len(), 2);
                    assert!(matches!(&pairs[0],
                        HashEntry::KeyValue { key: Expression::Identifier(k), value: Expression::Identifier(v) } if k == "k" && v == "v"));
                    assert!(matches!(&pairs[1],
                        HashEntry::Spread(Expression::Identifier(n)) if n == "rest"));
                }
                _ => panic!("expected object destructuring hash target"),
            }
        }
        _ => panic!("expected assignment expression"),
    }
}

#[test]
fn parser_parses_expression_computed_object_patterns_subset() {
    let (program, errors) = parse_program_from_source(
        "let {[prefix + \"Key\"]: v, ...rest} = obj; {[k1]: a, [k2]: b, ...rest2} = src;",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Object(items) => {
                assert_eq!(items.len(), 2);
                assert!(
                    matches!(&items[0], ObjectBindingItem { key: Expression::Infix { .. }, target: BindingTarget::Identifier(name), is_rest: false, .. } if name == "v")
                );
                assert!(
                    matches!(&items[1], ObjectBindingItem { target: BindingTarget::Identifier(name), is_rest: true, .. } if name == "rest")
                );
            }
            _ => panic!("expected object binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::Assign { left, .. }) => match &**left {
            Expression::Hash(pairs) => {
                assert_eq!(pairs.len(), 3);
                assert!(
                    matches!(&pairs[0], HashEntry::KeyValue { key: Expression::Identifier(k), .. } if k == "k1")
                );
                assert!(
                    matches!(&pairs[1], HashEntry::KeyValue { key: Expression::Identifier(k), .. } if k == "k2")
                );
                assert!(matches!(&pairs[2], HashEntry::Spread(_)));
            }
            _ => panic!("expected object hash assignment target"),
        },
        _ => panic!("expected assignment statement"),
    }
}

#[test]
fn parser_parses_nested_destructuring_assignment_subset() {
    let (program, errors) =
        parse_program_from_source("{\"p\": {\"q\": x}, \"arr\": [a, b]} = src;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::Expression(Expression::Assign { left, operator, .. }) => {
            assert_eq!(operator, "=");
            match &**left {
                Expression::Hash(pairs) => {
                    assert_eq!(pairs.len(), 2);
                    assert!(matches!(&pairs[0],
                        HashEntry::KeyValue { key: Expression::String(k), value: Expression::Hash(_) } if k == "p"));
                    assert!(matches!(&pairs[1],
                        HashEntry::KeyValue { key: Expression::String(k), value: Expression::Array(_) } if k == "arr"));
                }
                _ => panic!("expected object hash assignment target"),
            }
        }
        _ => panic!("expected assignment statement"),
    }
}

#[test]
fn parser_parses_nested_destructuring_binding_subset() {
    let (program, errors) = parse_program_from_source(
        "let {\"p\": {\"q\": x}, \"arr\": [a, b]} = src; for (let {\"n\": [u, v]} of seq) { out = u; }",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Object(items) => {
                assert_eq!(items.len(), 2);
                assert!(
                    matches!(&items[0], ObjectBindingItem { key: Expression::String(k), target: BindingTarget::Pattern(_), .. } if k == "p")
                );
                assert!(
                    matches!(&items[1], ObjectBindingItem { key: Expression::String(k), target: BindingTarget::Pattern(_), .. } if k == "arr")
                );
            }
            _ => panic!("expected object binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }

    match &program.statements[1] {
        Statement::ForOf { binding, .. } => {
            assert!(matches!(
                binding,
                ForBinding::Pattern(BindingPattern::Object(_))
            ));
        }
        _ => panic!("expected for-of statement"),
    }
}

#[test]
fn parser_parses_nested_destructuring_defaults_binding_subset() {
    let (program, errors) =
        parse_program_from_source("let {\"p\": {\"q\": x = 3}, \"arr\": [a = 4, b]} = src;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Object(items) => {
                assert_eq!(items.len(), 2);
                assert!(
                    matches!(&items[0], ObjectBindingItem { key: Expression::String(k), target: BindingTarget::Pattern(_), .. } if k == "p")
                );
                assert!(
                    matches!(&items[1], ObjectBindingItem { key: Expression::String(k), target: BindingTarget::Pattern(_), .. } if k == "arr")
                );
            }
            _ => panic!("expected object binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }
}

#[test]
fn parser_parses_nested_defaults_and_rest_binding_subset() {
    let (program, errors) = parse_program_from_source(
        "let {\"outer\": {\"x\": x = 1, ...innerRest}, \"arr\": [h = 2, ...tail]} = src;",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Object(items) => {
                assert_eq!(items.len(), 2);
                assert!(
                    matches!(&items[0], ObjectBindingItem { key: Expression::String(k), target: BindingTarget::Pattern(_), .. } if k == "outer")
                );
                assert!(
                    matches!(&items[1], ObjectBindingItem { key: Expression::String(k), target: BindingTarget::Pattern(_), .. } if k == "arr")
                );
            }
            _ => panic!("expected object binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }
}

#[test]
fn parser_rejects_non_terminal_rest_in_object_binding_subset() {
    let (_program, errors) = parse_program_from_source("let {...rest, a} = obj;");
    assert!(!errors.is_empty(), "expected parser error");
}

#[test]
fn parser_rejects_non_terminal_rest_in_array_binding_subset() {
    let (_program, errors) = parse_program_from_source("let [...tail, last] = arr;");
    assert!(!errors.is_empty(), "expected parser error");
}

#[test]
fn parser_rejects_nested_non_terminal_rest_in_object_binding_subset() {
    let (_program, errors) = parse_program_from_source("let {x: {...innerRest, y}} = src;");
    assert!(!errors.is_empty(), "expected parser error");
}

#[test]
fn parser_parses_ternary_operator_subset() {
    let (program, errors) = parse_program_from_source("true ? 1 : 2;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0] {
        Statement::Expression(Expression::If {
            condition,
            consequence,
            alternative,
        }) => {
            assert!(matches!(&**condition, Expression::Boolean(true)));
            assert_eq!(consequence.len(), 1);
            assert!(matches!(
                consequence[0],
                Statement::Expression(Expression::Integer(1))
            ));
            let alt = alternative.as_ref().expect("alternative");
            assert_eq!(alt.len(), 1);
            assert!(matches!(
                alt[0],
                Statement::Expression(Expression::Integer(2))
            ));
        }
        _ => panic!("expected ternary lowered to if expression"),
    }
}

#[test]
fn parser_parses_comma_operator_subset() {
    let (program, errors) = parse_program_from_source("(x = 1, x = 2, x = 3);");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(
        &program.statements[0],
        Statement::Expression(Expression::Infix { operator, .. }) if operator == ","
    ));
}

#[test]
fn parser_parses_increment_decrement_subset() {
    let (program, errors) = parse_program_from_source("let i = 1; ++i; i++; --i; i--;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 5);

    assert!(
        matches!(&program.statements[1], Statement::Expression(Expression::Update { operator, prefix, .. }) if operator == "++" && *prefix)
    );
    assert!(
        matches!(&program.statements[2], Statement::Expression(Expression::Update { operator, prefix, .. }) if operator == "++" && !*prefix)
    );
    assert!(
        matches!(&program.statements[3], Statement::Expression(Expression::Update { operator, prefix, .. }) if operator == "--" && *prefix)
    );
    assert!(
        matches!(&program.statements[4], Statement::Expression(Expression::Update { operator, prefix, .. }) if operator == "--" && !*prefix)
    );
}

#[test]
fn parser_parses_numeric_separator_int_subset() {
    let (program, errors) = parse_program_from_source("1_000_000;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(
        program.statements[0],
        Statement::Expression(Expression::Integer(1_000_000))
    ));
}

#[test]
fn parser_parses_arrow_function_subset() {
    let (program, errors) = parse_program_from_source("[1,2,3].map(x => x * 2);");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::Expression(Expression::Call { arguments, .. }) => {
            assert_eq!(arguments.len(), 1);
            assert!(
                matches!(&arguments[0], Expression::Function { parameters, .. } if parameters == &vec!["x".to_string()])
            );
        }
        _ => panic!("expected call expression with arrow argument"),
    }
}

#[test]
fn parser_parses_parenthesized_arrow_function_subset() {
    let (program, errors) =
        parse_program_from_source("let add = (a, b) => a + b; let one = () => 1;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Let {
            value: Expression::Function { parameters, .. },
            ..
        } => assert_eq!(parameters, &vec!["a".to_string(), "b".to_string()]),
        _ => panic!("expected let with arrow function value"),
    }

    match &program.statements[1] {
        Statement::Let {
            value: Expression::Function { parameters, .. },
            ..
        } => assert!(parameters.is_empty()),
        _ => panic!("expected let with zero-arg arrow function value"),
    }
}

#[test]
fn parser_parses_rest_parameter_subset() {
    let (program, errors) = parse_program_from_source(
        "let f = function(...args) { return args; }; let g = (...xs) => xs;",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Let {
            value: Expression::Function { parameters, .. },
            ..
        } => assert_eq!(parameters, &vec!["...args".to_string()]),
        _ => panic!("expected function with rest parameter"),
    }

    match &program.statements[1] {
        Statement::Let {
            value: Expression::Function { parameters, .. },
            ..
        } => assert_eq!(parameters, &vec!["...xs".to_string()]),
        _ => panic!("expected arrow function with rest parameter"),
    }
}

#[test]
fn parser_rejects_non_terminal_rest_parameter_subset() {
    let (_program, errors) =
        parse_program_from_source("let f = function(a, ...rest, b) { return a; };");
    assert!(!errors.is_empty(), "expected parser error");
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rest parameter must be last")),
        "unexpected errors: {:?}",
        errors
    );
}

#[test]
fn parser_parses_default_parameters_subset() {
    let (program, errors) = parse_program_from_source(
        "let f = function(a = 1, b = 2) { return a + b; }; let g = (x = 3) => x;",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Let {
            value: Expression::Function {
                parameters, body, ..
            },
            ..
        } => {
            assert_eq!(parameters, &vec!["a".to_string(), "b".to_string()]);
            assert!(!body.is_empty());
        }
        _ => panic!("expected function with default params"),
    }

    match &program.statements[1] {
        Statement::Let {
            value: Expression::Function { parameters, .. },
            ..
        } => assert_eq!(parameters, &vec!["x".to_string()]),
        _ => panic!("expected arrow function with default param"),
    }
}

#[test]
fn parser_parses_destructured_parameters_subset() {
    let (program, errors) = parse_program_from_source(
        "let f = function({a, b: c}, [x, y] = [1, 2]) { return a + c + x + y; }; let g = ({m}, [n]) => m + n;",
    );
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Let {
            value: Expression::Function {
                parameters, body, ..
            },
            ..
        } => {
            assert_eq!(parameters.len(), 2);
            assert!(matches!(&body[0], Statement::LetPattern { .. }));
            assert!(matches!(
                &body[1],
                Statement::Expression(Expression::Assign { .. })
            ));
            assert!(matches!(&body[2], Statement::LetPattern { .. }));
        }
        _ => panic!("expected function with destructured parameters"),
    }

    match &program.statements[1] {
        Statement::Let {
            value: Expression::Function {
                parameters, body, ..
            },
            ..
        } => {
            assert_eq!(parameters.len(), 2);
            assert!(matches!(&body[0], Statement::LetPattern { .. }));
            assert!(matches!(&body[1], Statement::LetPattern { .. }));
        }
        _ => panic!("expected arrow with destructured parameters"),
    }
}

#[test]
fn parser_parses_template_interpolation_subset() {
    let (program, errors) = parse_program_from_source("`1 + 1 = ${1 + 1}`;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0] {
        Statement::Expression(Expression::Infix { operator, .. }) => assert_eq!(operator, "+"),
        _ => panic!("expected concatenated template expression"),
    }
}

#[test]
fn parser_parses_regex_literal_subset() {
    let (program, errors) = parse_program_from_source("/hello/i.test(\"hello\");");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0] {
        Statement::Expression(Expression::Call { function, .. }) => match &**function {
            Expression::Index { left, index } => {
                assert!(matches!(
                    &**left,
                    Expression::RegExp { pattern, flags } if pattern == "hello" && flags == "i"
                ));
                assert!(matches!(&**index, Expression::String(name) if name == "test"));
            }
            _ => panic!("expected index call on regex"),
        },
        _ => panic!("expected call expression"),
    }
}

#[test]
fn parser_parses_dot_property_keyword_names_subset() {
    let (program, errors) = parse_program_from_source("Map().set(\"a\", 1); Set().add(1);");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);
}

#[test]
fn parser_parses_numeric_separator_float_subset() {
    let (program, errors) = parse_program_from_source("1_000.50;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(
        program.statements[0],
        Statement::Expression(Expression::Float(v)) if (v - 1000.50).abs() < 1e-9
    ));
}

#[test]
fn parser_parses_scientific_notation_subset() {
    let (program, errors) = parse_program_from_source("1e3;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(
        program.statements[0],
        Statement::Expression(Expression::Float(v)) if (v - 1000.0).abs() < 1e-9
    ));

    let (program, errors) = parse_program_from_source("2E-2;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(
        program.statements[0],
        Statement::Expression(Expression::Float(v)) if (v - 0.02).abs() < 1e-9
    ));
}

#[test]
fn parser_parses_radix_integer_literals_subset() {
    let (program, errors) = parse_program_from_source("0xff;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert!(matches!(
        program.statements[0],
        Statement::Expression(Expression::Integer(v)) if v == 255
    ));

    let (program, errors) = parse_program_from_source("0b101;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert!(matches!(
        program.statements[0],
        Statement::Expression(Expression::Integer(v)) if v == 5
    ));

    let (program, errors) = parse_program_from_source("0o10;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert!(matches!(
        program.statements[0],
        Statement::Expression(Expression::Integer(v)) if v == 8
    ));
}

#[test]
fn parser_rejects_default_on_array_rest_binding_subset() {
    let (_program, errors) = parse_program_from_source("let [...tail = []] = arr;");
    assert!(!errors.is_empty(), "expected parser error");
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rest element in array binding cannot have default")),
        "unexpected errors: {:?}",
        errors
    );
}

#[test]
fn parser_rejects_default_on_object_rest_binding_subset() {
    let (_program, errors) = parse_program_from_source("let {...rest = {}} = obj;");
    assert!(!errors.is_empty(), "expected parser error");
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rest property in object binding cannot have default")),
        "unexpected errors: {:?}",
        errors
    );
}

#[test]
fn parser_rejects_default_on_array_rest_assignment_subset() {
    let (_program, errors) = parse_program_from_source("[...tail = []] = arr;");
    assert!(!errors.is_empty(), "expected parser error");
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rest element in array binding cannot have default")),
        "unexpected errors: {:?}",
        errors
    );
}

#[test]
fn parser_parses_spread_like_array_assignment_subset() {
    let (program, errors) = parse_program_from_source("[head, ...tail] = arr;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0] {
        Statement::Expression(Expression::Assign { left, .. }) => match &**left {
            Expression::Array(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(&items[0], Expression::Identifier(n) if n == "head"));
                assert!(
                    matches!(&items[1], Expression::Spread { value } if matches!(&**value, Expression::Identifier(n) if n == "tail"))
                );
            }
            _ => panic!("expected array assignment target"),
        },
        _ => panic!("expected assignment statement"),
    }
}

#[test]
fn parser_parses_spread_in_array_literal_subset() {
    let (program, errors) = parse_program_from_source("[1, ...[2, 3], 4];");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0] {
        Statement::Expression(Expression::Array(items)) => {
            assert_eq!(items.len(), 3);
            assert!(matches!(&items[0], Expression::Integer(1)));
            assert!(matches!(&items[1], Expression::Spread { .. }));
            assert!(matches!(&items[2], Expression::Integer(4)));
        }
        _ => panic!("expected array literal"),
    }
}

#[test]
fn parser_parses_spread_in_function_call_subset() {
    let (program, errors) = parse_program_from_source("fn(...arr, 3);");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0] {
        Statement::Expression(Expression::Call { arguments, .. }) => {
            assert_eq!(arguments.len(), 2);
            assert!(
                matches!(&arguments[0], Expression::Spread { value } if matches!(&**value, Expression::Identifier(n) if n == "arr"))
            );
            assert!(matches!(&arguments[1], Expression::Integer(3)));
        }
        _ => panic!("expected call expression"),
    }
}

#[test]
fn parser_parses_spread_in_new_and_optional_call_subset() {
    let (program, errors) = parse_program_from_source("new C(...args); fn?.(...args);");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 2);

    match &program.statements[0] {
        Statement::Expression(Expression::New { arguments, .. }) => {
            assert_eq!(arguments.len(), 1);
            assert!(matches!(&arguments[0], Expression::Spread { .. }));
        }
        _ => panic!("expected new expression"),
    }

    match &program.statements[1] {
        Statement::Expression(Expression::OptionalCall { arguments, .. }) => {
            assert_eq!(arguments.len(), 1);
            assert!(matches!(&arguments[0], Expression::Spread { .. }));
        }
        _ => panic!("expected optional call expression"),
    }
}

#[test]
fn parser_parses_object_spread_literal_subset() {
    let (program, errors) = parse_program_from_source("let obj = {a: 1, ...src};");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::Let {
            value: Expression::Hash(pairs),
            ..
        } => {
            assert_eq!(pairs.len(), 2);
            assert!(matches!(&pairs[0],
                HashEntry::KeyValue { key: Expression::String(k), value: Expression::Integer(1) } if k == "a"));
            assert!(matches!(&pairs[1],
                HashEntry::Spread(Expression::Identifier(v)) if v == "src"));
        }
        _ => panic!("expected let hash literal"),
    }
}

#[test]
fn parser_parses_object_spread_literal_variants_subset() {
    let (program, errors) = parse_program_from_source("let obj = {...{a: 1}, b: 2, ...more};");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::Let {
            value: Expression::Hash(pairs),
            ..
        } => {
            assert_eq!(pairs.len(), 3);
            assert!(matches!(&pairs[0], HashEntry::Spread(Expression::Hash(_))));
            assert!(matches!(&pairs[1],
                HashEntry::KeyValue { key: Expression::String(k), value: Expression::Integer(2) } if k == "b"));
            assert!(matches!(&pairs[2],
                HashEntry::Spread(Expression::Identifier(v)) if v == "more"));
        }
        _ => panic!("expected let hash literal"),
    }
}

#[test]
fn parser_rejects_default_on_object_rest_assignment_subset() {
    let (_program, errors) = parse_program_from_source("{...rest = {}} = obj;");
    assert!(!errors.is_empty(), "expected parser error");
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rest property in object pattern cannot have default")),
        "unexpected errors: {:?}",
        errors
    );
}

#[test]
fn parser_rejects_nested_default_on_rest_assignment_subset() {
    let (_program, errors) = parse_program_from_source("{x: {...inner = {}}} = src;");
    assert!(!errors.is_empty(), "expected parser error");
    assert!(
        errors
            .iter()
            .any(|e| e.contains("rest property in object pattern cannot have default")),
        "unexpected errors: {:?}",
        errors
    );
}

#[test]
fn parser_rejects_non_terminal_rest_in_object_assignment_subset() {
    let (_program, errors) = parse_program_from_source("{...rest, a} = src;");
    assert!(!errors.is_empty(), "expected parser error");
}

#[test]
fn parser_parses_nested_computed_rest_assignment_subset() {
    let (program, errors) =
        parse_program_from_source("{[k]: {\"a\": x = 1, ...innerRest}, ...outerRest} = src;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::Expression(Expression::Assign { left, .. }) => match &**left {
            Expression::Hash(pairs) => {
                assert_eq!(pairs.len(), 2);
                assert!(matches!(&pairs[0],
                    HashEntry::KeyValue { key: Expression::Identifier(k), value: Expression::Hash(_) } if k == "k"));
                assert!(matches!(&pairs[1],
                    HashEntry::Spread(Expression::Identifier(n)) if n == "outerRest"));
            }
            _ => panic!("expected object hash assignment target"),
        },
        _ => panic!("expected assignment statement"),
    }
}

#[test]
fn parser_rejects_computed_key_without_alias_in_binding_subset() {
    let (_program, errors) = parse_program_from_source("let {[k]} = obj;");
    assert!(!errors.is_empty(), "expected parser error");
}

#[test]
fn parser_rejects_computed_key_without_alias_in_assignment_subset() {
    let (_program, errors) = parse_program_from_source("{[k]} = obj;");
    assert!(!errors.is_empty(), "expected parser error");
}

#[test]
fn parser_rejects_nested_computed_key_without_alias_in_binding_subset() {
    let (_program, errors) = parse_program_from_source("let {x: {[k]}} = src;");
    assert!(!errors.is_empty(), "expected parser error");
}

#[test]
fn parser_rejects_nested_computed_key_without_alias_in_assignment_subset() {
    let (_program, errors) = parse_program_from_source("{x: {[k]}} = src;");
    assert!(!errors.is_empty(), "expected parser error");
}

#[test]
fn parser_parses_nested_computed_key_with_alias_subset() {
    let (program, errors) = parse_program_from_source("let {x: {[k]: v, ...r}} = src;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::LetPattern { pattern, .. } => match pattern {
            BindingPattern::Object(items) => {
                assert_eq!(items.len(), 1);
                assert!(
                    matches!(&items[0], ObjectBindingItem { key: Expression::String(k), target: BindingTarget::Pattern(_), .. } if k == "x")
                );
            }
            _ => panic!("expected object binding pattern"),
        },
        _ => panic!("expected let pattern statement"),
    }
}

#[test]
fn parser_parses_for_of_nested_computed_key_with_alias_subset() {
    let (program, errors) =
        parse_program_from_source("for (let {o: {[k]: v, ...r}} of seq) { out = v; }");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0] {
        Statement::ForOf { binding, .. } => {
            assert!(
                matches!(binding, ForBinding::Pattern(BindingPattern::Object(items)) if items.len() == 1)
            );
        }
        _ => panic!("expected for-of statement"),
    }
}

#[test]
fn parser_parses_variable_kinds_subset() {
    let (program, errors) = parse_program_from_source("let a = 1; const b = 2; var c = 3;");
    assert!(errors.is_empty(), "parser errors: {:?}", errors);
    assert_eq!(program.statements.len(), 3);

    match &program.statements[0] {
        Statement::Let { kind, .. } => {
            assert_eq!(*kind, VariableKind::Let, "first declaration should be let");
        }
        other => panic!("expected Let statement, got {:?}", other),
    }
    match &program.statements[1] {
        Statement::Let { kind, .. } => {
            assert_eq!(
                *kind,
                VariableKind::Const,
                "second declaration should be const"
            );
        }
        other => panic!("expected Let statement for const, got {:?}", other),
    }
    match &program.statements[2] {
        Statement::Let { kind, .. } => {
            assert_eq!(*kind, VariableKind::Var, "third declaration should be var");
        }
        other => panic!("expected Let statement for var, got {:?}", other),
    }
}
