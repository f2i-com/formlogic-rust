# formlogic-wasm

WebAssembly wrapper around `formlogic-core` using `wasm-bindgen`.

## API

- `new WasmFormLogicEngine()`
- `engine.eval(source: string): any`
- `engine.evalInspect(source: string): string`
- `wasm_engine_info(): string`

## Rust Smoke Tests

Host-side smoke tests for the wrapper live in:

- `crates/formlogic-wasm/tests/api_smoke.rs`

Run with:

```bash
cargo test -p formlogic-wasm
```

`eval` converts Rust runtime values to JavaScript values:

- numbers -> JS number
- booleans -> JS boolean
- null/undefined -> JS null/undefined
- strings -> JS string
- arrays -> JS array
- hashes/objects -> plain JS object
- promises/errors -> plain JS object with status/name/message fields
- other internal runtime objects -> `inspect()` string

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo build -p formlogic-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir ./dist-wasm ./target/wasm32-unknown-unknown/release/formlogic_wasm.wasm
```

## Browser Usage

```js
import init, { WasmFormLogicEngine, wasm_engine_info } from "./dist-wasm/formlogic_wasm.js";

await init();
console.log(wasm_engine_info());

const engine = new WasmFormLogicEngine();
const value = engine.eval("let x = 7; x * 6;");
console.log(value); // 42
```

## Smoke Test Files

This crate includes simple examples you can run after generating `dist-wasm`:

- Browser: `crates/formlogic-wasm/examples/browser-smoke.html`
- Node ESM: `crates/formlogic-wasm/examples/node-smoke.mjs`

Node example:

```bash
node ./crates/formlogic-wasm/examples/node-smoke.mjs
```

On Windows, you can use helper scripts from the workspace root:

```powershell
./scripts/wasm-build.ps1
./scripts/wasm-smoke.ps1
```
