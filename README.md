# FormLogic

A high-performance, sandboxed scripting language and execution engine, implemented in Rust and compiled to WebAssembly.

## Overview

FormLogic is a secure, sandboxed execution engine designed for embedding in applications, workflows, and AI agents. It provides a familiar JavaScript-like syntax while offering complete isolation, deterministic execution limits, and seamless bridging to host environments (like Node.js or the browser).

By compiling to WebAssembly, FormLogic achieves incredible speed—running up to **20x faster** than pure TypeScript interpreters—while maintaining strict memory safety and preventing malicious code from accessing native OS APIs.

**Key Features:**

- **Blazing Fast WebAssembly** - Rust-powered bytecode VM compiled to WASM.
- **JavaScript-like syntax** - Familiar syntax for easy adoption.
- **Secure Sandbox** - Completely isolated from the host OS, preventing unauthorized access.
- **Yield-Based Async Bridge** - Securely pause the WASM VM (`yield`), run native JS host callbacks, and resume the VM seamlessly.
- **Modern features** - Arrow functions, classes, optional chaining (`?.`), nullish coalescing (`??`), destructuring, spread/rest.
- **Built-in modules** - `Math`, `String`, `Array`, `Object`, `Map`, `Set`, `JSON`, `Promise`.
- **Generators** - `function*` with `yield` support.
- **Exception handling** - `try`/`catch`/`finally` with `throw`.

## What It Can Run

The engine supports a broad JavaScript-like subset, including:

- **Core language:** variables, expressions, conditionals, loops, functions, closures, recursion
- **Modern syntax:** classes/inheritance/super, arrow functions, optional chaining, nullish coalescing, logical assignment
- **Destructuring/spread/rest:** arrays/objects, parameter destructuring/defaults, object rest assignment
- **Operators:** arithmetic/bitwise/relational/equality, comma operator, unary operators, `in`, `instanceof`, `typeof`, `void`, `delete`
- **Regex support:** literals, `RegExp`, `replace`, `replaceAll`, `match`, `matchAll`

## Installation (Node.js & Browser)

The Rust codebase compiles to a highly optimized WebAssembly package for use in JS/TS environments.

```bash
npm install formlogic-lang
```

*Note: The NPM package name is `formlogic-lang`, which maps to the WebAssembly build output.*

## Quick Start (JavaScript / WebAssembly)

```javascript
import { WasmFormLogicEngine } from 'formlogic-lang';

const engine = new WasmFormLogicEngine();

// Evaluate simple expressions
const value = engine.eval('Math.sqrt(16) + 2');
console.log(value); // 6

// Initialize and execute a persistent script
engine.initScript(`
  let count = 0;
  function increment() {
    count++;
    return count;
  }
`);

// The engine is stateful between calls
console.log(engine.eval('increment()')); // 1
console.log(engine.eval('increment()')); // 2
```

## Bridging Host Functions (Async Yielding)

FormLogic handles asynchronous external operations (like network requests or file system access) by safely yielding control back to the JavaScript host. The WebAssembly VM suspends execution, asks the host to resolve an operation, and then steps forward.

```javascript
import { WasmFormLogicEngine } from 'formlogic-lang';

const engine = new WasmFormLogicEngine();

const script = `
  function* run() {
    // Yield a host command
    let response = yield { _kind: "Http.get", _args: ["https://api.example.com"] };
    return response;
  }
  let gen = run();
`;

engine.initScript(script);

// Drain pending host calls and resolve them
const calls = engine.drainPendingHostCalls();
for (const call of calls) {
  if (call.kind === "Http.get") {
    // Perform actual native OS operation
    fetch(call.args[0])
      .then(res => res.text())
      .then(data => {
        // Feed the result back into the WASM VM!
        engine.resolveHostCallback(call.id, data);
      });
  }
}
```

## Quick Start (Rust)

You can also embed the engine directly into Rust binaries:

```rust
use formlogic_core::engine::FormLogicEngine;

fn main() {
    let engine = FormLogicEngine::default();
    let out = engine.eval("let x = 7; x * 6;").unwrap();
    println!("{}", out.inspect()); // 42
}
```

## Building WebAssembly Locally

If you are developing FormLogic and want to build the WebAssembly targets manually:

```bash
# Add WebAssembly target
rustup target add wasm32-unknown-unknown

# Build for Web/Browser
cargo build -p formlogic-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir dist-wasm ./target/wasm32-unknown-unknown/release/formlogic_wasm.wasm

# Build for Node.js
wasm-bindgen --target nodejs --out-dir dist-wasm-node ./target/wasm32-unknown-unknown/release/formlogic_wasm.wasm
```

There are also helper scripts provided in the `scripts/` directory:
- `scripts/wasm-build.ps1` (web target)
- `scripts/wasm-build-node.ps1` (node target)
- `scripts/wasm-smoke.ps1` (node smoke tests)

## Workspace Layout

- `crates/formlogic-core`: Core lexer, parser, compiler, and Register-based VM runtime.
- `crates/formlogic-wasm`: WebAssembly (`wasm-bindgen`) wrapper bridging the VM to JS.
- `scripts/`: Build helpers, smoke tests, and benchmark scripts.

## Architecture

FormLogic in Rust utilizes a highly optimized Register VM:

```
Source Code
    |
    v
+---------+
|  Lexer  |  Tokenizes source into tokens
+----+----+
     |
     v
+---------+
| Parser  |  Pratt parser builds AST
+----+----+
     |
     v
+----------+
| Compiler |  Generates register-based bytecode instructions
+----+-----+
     |
     v
+---------+
|   VM    |  High-performance Register VM Execution
+---------+
```

## Benchmarks

The Rust WebAssembly implementation is drastically faster than equivalent TypeScript-based interpreters.

Sample benchmark results (`BENCH_WARMUP=10`, `BENCH_RUNS=50`):

| Benchmark | JS Native (V8) | FormLogic Rust (WASM) | FormLogic TS (Node) |
|---|---|---|---|
| Arithmetic loop (5k) | 169,607 ops/s | 9,855 ops/s | ~545 ops/s |
| Fibonacci recursive (n=12) | 297,974 ops/s | 27,156 ops/s | ~1,400 ops/s |
| Array index write + sum (2k) | 70,992 ops/s | 8,650 ops/s | ~200 ops/s |
| Object property loop (5k) | 201,939 ops/s | 7,751 ops/s | ~300 ops/s |
| String concatenation (1k) | 193,498 ops/s | 14,994 ops/s | ~550 ops/s |
| Function calls (1k) | 259,740 ops/s | 21,128 ops/s | ~1,100 ops/s |
| While + conditionals (10k) | 90,351 ops/s | 4,407 ops/s | ~220 ops/s |
| Map set/get (2k) | 7,186 ops/s | 3,039 ops/s | ~150 ops/s |
| **Aggregate** | **42,661 ops/s** | **7,598 ops/s** | **~380 ops/s** |

* FormLogic Rust Wasm is **~20x faster** than the previous FormLogic TypeScript implementation.
* The internal `Register VM` implementation provided a **3.1x speedup** over the legacy Stack VM.

| Benchmark | Stack VM (ms) | Register VM (ms) | Speedup |
|---|---|---|---|
| Arithmetic loop (5k) | 21.3 | 15.0 | 1.43x |
| Fibonacci recursive (n=20) | 169.5 | 49.2 | 3.45x |
| Array index write + sum (2k) | 41.8 | 9.2 | 4.56x |
| Object property loop (5k) | 29.8 | 8.5 | 3.51x |
| String concatenation (1k) | 61.3 | 39.1 | 1.57x |
| Function calls (1k) | 13.4 | 3.5 | 3.87x |
| While + conditionals (10k) | 56.5 | 17.2 | 3.28x |
| Map set/get (2k) | 142.2 | 30.7 | 4.63x |
| **TOTAL** | **535.9** | **172.3** | **3.11x** |

Three-way comparison (Native JS vs FormLogic TypeScript vs FormLogic Rust Wasm):

- FormLogic Rust Wasm: **~20x faster** than FormLogic TypeScript
- Native JS / Rust Wasm: `~5.6x`
- Native JS / FormLogic TS: `250.55x`

Note: absolute numbers vary by machine, Node version, and workload mix. The WASM benchmark
includes parse+compile overhead per iteration; the internal Register vs Stack benchmark
measures pre-compiled VM execution only.

## License

Apache License 2.0
