//! Benchmark comparing stack-based VM vs register-based VM.
//! Run with: cargo test -p formlogic-core --release --test bench_register_vs_stack -- --nocapture

use std::time::Instant;

use formlogic_core::compiler::Compiler;
use formlogic_core::config::FormLogicConfig;
use formlogic_core::object::Object;
use formlogic_core::parser::parse_program_from_source;
use formlogic_core::rcompiler::RCompiler;
use formlogic_core::value::{val_to_obj, Value};
use formlogic_core::vm::VM;

const WARMUP: usize = 5;
const RUNS: usize = 100;

use formlogic_core::bytecode::Bytecode;

fn compile_stack(source: &str) -> Result<Bytecode, String> {
    let (program, errors) = parse_program_from_source(source);
    if !errors.is_empty() {
        return Err(format!("Parser errors: {}", errors.join(", ")));
    }
    Compiler::new().compile_program(&program)
}

fn compile_register(source: &str) -> Result<Bytecode, String> {
    let (program, errors) = parse_program_from_source(source);
    if !errors.is_empty() {
        return Err(format!("Parser errors: {}", errors.join(", ")));
    }
    RCompiler::new().compile_program(&program)
}

fn run_stack(bytecode: &Bytecode) -> Result<Object, String> {
    let config = FormLogicConfig::default();
    let mut vm = VM::new(bytecode.clone(), config);
    vm.run().map_err(|e| format!("VM error: {:?}", e))?;
    let last = vm.last_popped.take().unwrap_or(Value::UNDEFINED);
    Ok(val_to_obj(last, &vm.heap))
}

fn run_register(bytecode: &Bytecode) -> Result<Object, String> {
    let config = FormLogicConfig::default();
    let mut vm = VM::new(bytecode.clone(), config);
    vm.run_register().map_err(|e| format!("VM error: {:?}", e))?;
    let last = vm.last_popped.take().unwrap_or(Value::UNDEFINED);
    Ok(val_to_obj(last, &vm.heap))
}

fn eval_stack(source: &str) -> Result<Object, String> {
    run_stack(&compile_stack(source)?)
}

fn eval_register(source: &str) -> Result<Object, String> {
    run_register(&compile_register(source)?)
}

struct BenchCase {
    name: &'static str,
    source: &'static str,
}

// All cases wrapped in a function call so both VMs benefit from function-scope
// locals (register-based locals for the register VM, indexed locals for the stack VM).
// This matches real FormLogic usage where code runs inside callFunction().
// NOTE: Use `let name = function(...){...};` (not `function name(...){}`) inside
// function bodies because FunctionDecl inside functions doesn't support recursive
// self-references — the callee is compiled before the binding is created.
const CASES: &[BenchCase] = &[
    BenchCase {
        name: "Arithmetic loop (5k)",
        source: r#"
            function bench() {
                let sum = 0;
                for (let i = 1; i <= 5000; i = i + 1) {
                    sum = sum + i;
                }
                return sum;
            }
            bench();
        "#,
    },
    BenchCase {
        name: "Fibonacci recursive (n=20)",
        source: r#"
            function bench() {
                let fib = function(n) {
                    if (n <= 1) { return n; }
                    return fib(n - 1) + fib(n - 2);
                };
                return fib(20);
            }
            bench();
        "#,
    },
    BenchCase {
        name: "Array index write + sum (2k)",
        source: r#"
            function bench() {
                let arr = [];
                for (let i = 0; i < 2000; i = i + 1) {
                    arr[i] = i * 2;
                }
                let total = 0;
                for (let i = 0; i < 2000; i = i + 1) {
                    total = total + arr[i];
                }
                return total;
            }
            bench();
        "#,
    },
    BenchCase {
        name: "Object property loop (5k)",
        source: r#"
            function bench() {
                let obj = { x: 0, y: 0, z: 0 };
                for (let i = 0; i < 5000; i = i + 1) {
                    obj.x = obj.x + 1;
                    obj.y = obj.y + 2;
                    obj.z = obj.x + obj.y;
                }
                return obj.z;
            }
            bench();
        "#,
    },
    BenchCase {
        name: "String concatenation (1k)",
        source: r#"
            function bench() {
                let s = "";
                for (let i = 0; i < 1000; i = i + 1) {
                    s = s + "a";
                }
                return s.length;
            }
            bench();
        "#,
    },
    BenchCase {
        name: "Function calls (1k)",
        source: r#"
            function bench() {
                let add = function(a, b) { return a + b; };
                let result = 0;
                for (let i = 0; i < 1000; i = i + 1) {
                    result = add(result, 1);
                }
                return result;
            }
            bench();
        "#,
    },
    BenchCase {
        name: "While + conditionals (10k)",
        source: r#"
            function bench() {
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
                return count;
            }
            bench();
        "#,
    },
    BenchCase {
        name: "Map set/get (2k)",
        source: r#"
            function bench() {
                let m = new Map();
                for (let i = 0; i < 2000; i = i + 1) {
                    m.set("key" + i, i * 3);
                }
                let total = 0;
                for (let i = 0; i < 2000; i = i + 1) {
                    total = total + m.get("key" + i);
                }
                return total;
            }
            bench();
        "#,
    },
];

fn bench_precompiled<F: Fn(&Bytecode) -> Result<Object, String>>(
    f: &F,
    bytecode: &Bytecode,
    warmup: usize,
    runs: usize,
) -> (f64, Object) {
    // Warmup
    for _ in 0..warmup {
        let _ = f(bytecode);
    }
    // Timed runs (VM execution only, no parse/compile)
    let start = Instant::now();
    let mut last = Object::Undefined;
    for _ in 0..runs {
        last = f(bytecode).expect("bench eval failed");
    }
    let elapsed = start.elapsed().as_secs_f64() * 1000.0; // ms
    (elapsed, last)
}

#[test]
fn benchmark_stack_vs_register() {
    eprintln!("\n{}", "=".repeat(90));
    eprintln!("  Stack-based VM vs Register-based VM Benchmark");
    eprintln!("  Warmup: {WARMUP}, Runs: {RUNS} (VM execution only, pre-compiled)");
    eprintln!("{}", "=".repeat(90));

    eprintln!(
        "\n{:<30}  {:>12}  {:>12}  {:>10}  {:>7}",
        "Benchmark", "Stack (ms)", "Reg (ms)", "Speedup", "Match"
    );
    eprintln!("{}", "-".repeat(80));

    let mut total_stack = 0.0f64;
    let mut total_reg = 0.0f64;

    for case in CASES {
        // Pre-compile once, then only time VM execution
        let stack_bytecode = compile_stack(case.source).expect("stack compile failed");
        let reg_bytecode = compile_register(case.source).expect("register compile failed");

        let (stack_ms, stack_result) = bench_precompiled(&run_stack, &stack_bytecode, WARMUP, RUNS);
        let (reg_ms, reg_result) = bench_precompiled(&run_register, &reg_bytecode, WARMUP, RUNS);

        let speedup = stack_ms / reg_ms;
        let results_match = format!("{:?}", stack_result) == format!("{:?}", reg_result);

        total_stack += stack_ms;
        total_reg += reg_ms;

        eprintln!(
            "{:<30}  {:>12.3}  {:>12.3}  {:>9.2}x  {:>7}",
            case.name,
            stack_ms,
            reg_ms,
            speedup,
            if results_match { "yes" } else { "NO" }
        );
    }

    eprintln!("{}", "-".repeat(80));
    eprintln!(
        "{:<30}  {:>12.3}  {:>12.3}  {:>9.2}x",
        "TOTAL", total_stack, total_reg, total_stack / total_reg
    );
    eprintln!("{}\n", "=".repeat(90));
}

#[test]
fn debug_function_calls_mismatch() {
    let source = r#"
        function bench() {
            let add = function(a, b) { return a + b; };
            let result = 0;
            for (let i = 0; i < 1000; i = i + 1) {
                result = add(result, 1);
            }
            return result;
        }
        bench();
    "#;
    let stack = eval_stack(source).expect("stack");
    let reg = eval_register(source).expect("register");
    eprintln!("Stack:    {:?}", stack);
    eprintln!("Register: {:?}", reg);
}
