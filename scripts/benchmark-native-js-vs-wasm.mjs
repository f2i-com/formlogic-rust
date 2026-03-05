import { performance } from "node:perf_hooks";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { WasmFormLogicEngine, wasm_engine_info } = require("../dist-wasm-node/formlogic_wasm.js");

const SOURCE = `
let sum = 0;
for (let i = 0; i < 400; i = i + 1) {
  sum = sum + (i * 3) - 2;
}
sum;
`;

const WARMUP = 200;
const RUNS = 5000;

function nativeJsCase() {
  let sum = 0;
  for (let i = 0; i < 400; i += 1) {
    sum = sum + i * 3 - 2;
  }
  return sum;
}

function benchNativeJs(warmup, runs) {
  for (let i = 0; i < warmup; i += 1) {
    nativeJsCase();
  }
  const start = performance.now();
  let last;
  for (let i = 0; i < runs; i += 1) {
    last = nativeJsCase();
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

function main() {
  console.log("Benchmark: Native JavaScript (Node) vs FormLogic Rust Wasm");
  console.log(`Wasm module: ${wasm_engine_info()}`);
  console.log(`Warmup: ${WARMUP}, Runs: ${RUNS}`);

  const native = benchNativeJs(WARMUP, RUNS);
  const wasm = benchWasmEngine(SOURCE, WARMUP, RUNS);

  const nativeOps = opsPerSec(RUNS, native.elapsedMs);
  const wasmOps = opsPerSec(RUNS, wasm.elapsedMs);
  const wasmVsNative = wasmOps / nativeOps;
  const nativeVsWasm = nativeOps / wasmOps;

  console.log("\nResults:");
  console.log(`- Native JS (Node): ${fmt(native.elapsedMs)} ms total, ${fmt(nativeOps)} ops/sec`);
  console.log(`- FormLogic Wasm:   ${fmt(wasm.elapsedMs)} ms total, ${fmt(wasmOps)} ops/sec`);
  console.log(`- Relative speed (Wasm/Native): ${wasmVsNative.toFixed(6)}x`);
  console.log(`- Relative speed (Native/Wasm): ${fmt(nativeVsWasm)}x`);
  console.log(`- Last result check: Native=${native.last} WASM=${wasm.last}`);
}

try {
  main();
} catch (err) {
  console.error("Benchmark failed:", err);
  process.exitCode = 1;
}
