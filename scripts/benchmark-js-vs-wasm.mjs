import { performance } from "node:perf_hooks";
import { createRequire } from "node:module";
import { FormLogicEngine as JsFormLogicEngine } from "../../formlogic-typescript/dist/index.js";

const require = createRequire(import.meta.url);
const {
  WasmFormLogicEngine,
  wasm_engine_info,
} = require("../dist-wasm-node/formlogic_wasm.js");

const SOURCE = `
let sum = 0;
for (let i = 0; i < 400; i = i + 1) {
  sum = sum + (i * 3) - 2;
}
sum;
`;

const WARMUP = 50;
const RUNS = 600;

async function benchJsEngine(source, warmup, runs) {
  const engine = new JsFormLogicEngine();
  for (let i = 0; i < warmup; i += 1) {
    await engine.eval(source);
  }
  const start = performance.now();
  let last;
  for (let i = 0; i < runs; i += 1) {
    last = await engine.eval(source);
  }
  const elapsedMs = performance.now() - start;
  return { elapsedMs, last };
}

function benchWasmEngine(source, warmup, runs) {
  const engine = new WasmFormLogicEngine();
  for (let i = 0; i < warmup; i += 1) {
    engine.eval(source);
  }
  const start = performance.now();
  let last;
  for (let i = 0; i < runs; i += 1) {
    last = engine.eval(source);
  }
  const elapsedMs = performance.now() - start;
  return { elapsedMs, last };
}

function opsPerSec(runs, elapsedMs) {
  return (runs * 1000) / elapsedMs;
}

function fmt(n) {
  return n.toLocaleString("en-US", { maximumFractionDigits: 2 });
}

async function main() {
  console.log("Benchmark: npm TypeScript engine vs Rust Wasm engine");
  console.log(`Wasm module: ${wasm_engine_info()}`);
  console.log(`Warmup: ${WARMUP}, Runs: ${RUNS}`);

  const js = await benchJsEngine(SOURCE, WARMUP, RUNS);
  const wasm = benchWasmEngine(SOURCE, WARMUP, RUNS);

  const jsOps = opsPerSec(RUNS, js.elapsedMs);
  const wasmOps = opsPerSec(RUNS, wasm.elapsedMs);
  const speedup = wasmOps / jsOps;

  console.log("\nResults:");
  console.log(`- JS (npm/TypeScript): ${fmt(js.elapsedMs)} ms total, ${fmt(jsOps)} ops/sec`);
  console.log(`- Rust Wasm:          ${fmt(wasm.elapsedMs)} ms total, ${fmt(wasmOps)} ops/sec`);
  console.log(`- Speedup (Wasm/JS):  ${fmt(speedup)}x`);
  console.log(`- Last result check:  JS=${js.last} WASM=${wasm.last}`);
}

main().catch((err) => {
  console.error("Benchmark failed:", err);
  process.exitCode = 1;
});
