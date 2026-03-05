import { performance } from "node:perf_hooks";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { WasmFormLogicEngine, wasm_engine_info } = require("../dist-wasm-node/formlogic_wasm.js");

const WARMUP = Number.parseInt(process.env.BENCH_WARMUP ?? "1", 10);
const RUNS = Number.parseInt(process.env.BENCH_RUNS ?? "5", 10);

function benchNative(fn, warmup, runs) {
  for (let i = 0; i < warmup; i += 1) fn();
  const start = performance.now();
  let last;
  for (let i = 0; i < runs; i += 1) last = fn();
  return { elapsedMs: performance.now() - start, last };
}

function benchWasm(source, warmup, runs) {
  const engine = new WasmFormLogicEngine();
  for (let i = 0; i < warmup; i += 1) {
    engine.eval(source);
  }
  const start = performance.now();
  let last;
  for (let i = 0; i < runs; i += 1) {
    last = engine.eval(source);
  }
  return { elapsedMs: performance.now() - start, last };
}

function opsPerSec(runs, ms) {
  return (runs * 1000) / ms;
}

function fmt(n) {
  return Number.isFinite(n)
    ? n.toLocaleString("en-US", { maximumFractionDigits: 2 })
    : "NaN";
}

const CASES = [
  {
    name: "Arithmetic loop (5k)",
    source: `
      let sum = 0;
      for (let i = 1; i <= 5000; i = i + 1) {
        sum = sum + i;
      }
      sum;
    `,
    native: () => {
      let sum = 0;
      for (let i = 1; i <= 5000; i += 1) sum += i;
      return sum;
    },
  },
  {
    name: "Fibonacci recursive (n=12)",
    source: `
      function fib(n) {
        if (n <= 1) { return n; }
        return fib(n - 1) + fib(n - 2);
      }
      fib(12);
    `,
    native: () => {
      function fib(n) {
        if (n <= 1) return n;
        return fib(n - 1) + fib(n - 2);
      }
      return fib(12);
    },
  },
  {
    name: "Array index write + sum (2k)",
    source: `
      let arr = [];
      for (let i = 0; i < 2000; i = i + 1) {
        arr[i] = i * 2;
      }
      let total = 0;
      for (let i = 0; i < 2000; i = i + 1) {
        total = total + arr[i];
      }
      total;
    `,
    native: () => {
      const arr = [];
      for (let i = 0; i < 2000; i += 1) arr.push(i * 2);
      let total = 0;
      for (let i = 0; i < 2000; i += 1) total += arr[i];
      return total;
    },
  },
  {
    name: "Object property loop (5k)",
    source: `
      let obj = { x: 0, y: 0, z: 0 };
      for (let i = 0; i < 5000; i = i + 1) {
        obj.x = obj.x + 1;
        obj.y = obj.y + 2;
        obj.z = obj.x + obj.y;
      }
      obj.z;
    `,
    native: () => {
      const obj = { x: 0, y: 0, z: 0 };
      for (let i = 0; i < 5000; i += 1) {
        obj.x = obj.x + 1;
        obj.y = obj.y + 2;
        obj.z = obj.x + obj.y;
      }
      return obj.z;
    },
  },
  {
    name: "String concatenation (1k)",
    source: `
      let s = "";
      for (let i = 0; i < 1000; i = i + 1) {
        s = s + "a";
      }
      s.length;
    `,
    native: () => {
      let s = "";
      for (let i = 0; i < 1000; i += 1) s += "a";
      return s.length;
    },
  },
  {
    name: "Function calls (1k)",
    source: `
      function add(a, b) { return a + b; }
      let result = 0;
      for (let i = 0; i < 1000; i = i + 1) {
        result = add(result, 1);
      }
      result;
    `,
    native: () => {
      function add(a, b) { return a + b; }
      let result = 0;
      for (let i = 0; i < 1000; i += 1) result = add(result, 1);
      return result;
    },
  },
  {
    name: "While + conditionals (10k)",
    source: `
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
    `,
    native: () => {
      let count = 0;
      let i = 0;
      while (i < 10000) {
        if (i % 3 === 0) count += 1;
        else if (i % 3 === 1) count += 2;
        else count += 3;
        i += 1;
      }
      return count;
    },
  },
  {
    name: "Map set/get (2k)",
    source: `
      let m = new Map();
      for (let i = 0; i < 2000; i = i + 1) {
        m.set("key" + i, i * 3);
      }
      let total = 0;
      for (let i = 0; i < 2000; i = i + 1) {
        total = total + m.get("key" + i);
      }
      total;
    `,
    native: () => {
      const m = new Map();
      for (let i = 0; i < 2000; i += 1) m.set(`key${i}`, i * 3);
      let total = 0;
      for (let i = 0; i < 2000; i += 1) total += m.get(`key${i}`);
      return total;
    },
  },
];

function main() {
  console.log("Benchmark suite: Native JavaScript (Node) vs FormLogic Rust Wasm");
  console.log(`Wasm module: ${wasm_engine_info()}`);
  console.log(`Warmup: ${WARMUP}, Runs per case: ${RUNS}\n`);

  const rows = [];
  for (const c of CASES) {
    try {
      const native = benchNative(c.native, WARMUP, RUNS);
      const wasm = benchWasm(c.source, WARMUP, RUNS);
      const nativeOps = opsPerSec(RUNS, native.elapsedMs);
      const wasmOps = opsPerSec(RUNS, wasm.elapsedMs);
      const nativeOverWasm = nativeOps / wasmOps;
      const same = String(native.last) === String(wasm.last);

      rows.push({
        name: c.name,
        nativeMs: native.elapsedMs,
        wasmMs: wasm.elapsedMs,
        nativeOps,
        wasmOps,
        nativeOverWasm,
        same,
        nativeLast: native.last,
        wasmLast: wasm.last,
      });

      console.log(
        `- ${c.name}: Native ${fmt(nativeOps)} ops/s | Wasm ${fmt(wasmOps)} ops/s | Native/Wasm ${fmt(nativeOverWasm)}x | match=${same}`
      );
    } catch (err) {
      console.log(`- ${c.name}: ERROR ${String(err)}`);
    }
  }

  const valid = rows.filter((r) => Number.isFinite(r.nativeMs) && Number.isFinite(r.wasmMs));
  const totalNativeMs = valid.reduce((s, r) => s + r.nativeMs, 0);
  const totalWasmMs = valid.reduce((s, r) => s + r.wasmMs, 0);
  const totalNativeOps = (valid.length * RUNS * 1000) / totalNativeMs;
  const totalWasmOps = (valid.length * RUNS * 1000) / totalWasmMs;

  console.log("\nSummary:");
  console.log(`- Cases: ${valid.length}/${CASES.length} successful`);
  console.log(`- Aggregate Native: ${fmt(totalNativeMs)} ms, ${fmt(totalNativeOps)} ops/s`);
  console.log(`- Aggregate Wasm:   ${fmt(totalWasmMs)} ms, ${fmt(totalWasmOps)} ops/s`);
  console.log(`- Aggregate Native/Wasm: ${fmt(totalNativeOps / totalWasmOps)}x`);

  const mismatches = valid.filter((r) => !r.same);
  if (mismatches.length > 0) {
    console.log("\nResult mismatches:");
    for (const m of mismatches) {
      console.log(`- ${m.name}: native=${m.nativeLast} wasm=${m.wasmLast}`);
    }
  }
}

try {
  main();
} catch (err) {
  console.error("Benchmark failed:", err);
  process.exitCode = 1;
}
