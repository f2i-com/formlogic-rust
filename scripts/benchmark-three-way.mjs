/**
 * Three-way benchmark: Native JavaScript vs FormLogic TypeScript vs FormLogic Rust (Wasm)
 *
 * Usage:
 *   node scripts/benchmark-three-way.mjs
 *   BENCH_WARMUP=2 BENCH_RUNS=10 node scripts/benchmark-three-way.mjs
 */

import { performance } from "node:perf_hooks";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

// Rust Wasm engine (sync)
const { WasmFormLogicEngine, wasm_engine_info } = require("../dist-wasm-node/formlogic_wasm.js");

// TypeScript engine (async)
const { FormLogicEngine: TSFormLogicEngine } = await import(
  "../../formlogic-typescript/dist/engine.js"
);

const WARMUP = Number.parseInt(process.env.BENCH_WARMUP ?? "1", 10);
const RUNS = Number.parseInt(process.env.BENCH_RUNS ?? "5", 10);

// ---------------------------------------------------------------------------
// Bench helpers
// ---------------------------------------------------------------------------

function benchNativeSync(fn, warmup, runs) {
  for (let i = 0; i < warmup; i += 1) fn();
  const start = performance.now();
  let last;
  for (let i = 0; i < runs; i += 1) last = fn();
  return { elapsedMs: performance.now() - start, last };
}

function benchRustWasm(source, warmup, runs) {
  const engine = new WasmFormLogicEngine();
  for (let i = 0; i < warmup; i += 1) engine.eval(source);
  const start = performance.now();
  let last;
  for (let i = 0; i < runs; i += 1) last = engine.eval(source);
  return { elapsedMs: performance.now() - start, last };
}

async function benchTypeScript(source, warmup, runs) {
  const engine = new TSFormLogicEngine();
  for (let i = 0; i < warmup; i += 1) await engine.eval(source);
  const start = performance.now();
  let last;
  for (let i = 0; i < runs; i += 1) last = await engine.eval(source);
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

function fmtMs(n) {
  return Number.isFinite(n)
    ? n.toLocaleString("en-US", { maximumFractionDigits: 3 })
    : "NaN";
}

// ---------------------------------------------------------------------------
// Test cases (shared source for both FL engines, native JS equivalent)
// ---------------------------------------------------------------------------

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
      function add(a, b) {
        return a + b;
      }
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

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  console.log(
    "Three-way benchmark: Native JS  vs  FormLogic TypeScript  vs  FormLogic Rust (Wasm)"
  );
  console.log(`Rust Wasm module: ${wasm_engine_info()}`);
  console.log(`Warmup: ${WARMUP}, Runs per case: ${RUNS}\n`);

  // Table header
  const colName = 30;
  const colOps = 18;
  const colRatio = 12;
  const colMatch = 7;
  const header = [
    "Benchmark".padEnd(colName),
    "Native ops/s".padStart(colOps),
    "TS ops/s".padStart(colOps),
    "Rust ops/s".padStart(colOps),
    "Rust/TS".padStart(colRatio),
    "Match".padStart(colMatch),
  ].join("  ");
  console.log(header);
  console.log("-".repeat(header.length));

  const rows = [];
  for (const c of CASES) {
    try {
      const native = benchNativeSync(c.native, WARMUP, RUNS);
      const ts = await benchTypeScript(c.source, WARMUP, RUNS);
      const rust = benchRustWasm(c.source, WARMUP, RUNS);

      const nativeOps = opsPerSec(RUNS, native.elapsedMs);
      const tsOps = opsPerSec(RUNS, ts.elapsedMs);
      const rustOps = opsPerSec(RUNS, rust.elapsedMs);
      const rustOverTs = rustOps / tsOps;

      const matchNativeTs = String(native.last) === String(ts.last);
      const matchNativeRust = String(native.last) === String(rust.last);
      const allMatch = matchNativeTs && matchNativeRust;

      rows.push({
        name: c.name,
        nativeMs: native.elapsedMs,
        tsMs: ts.elapsedMs,
        rustMs: rust.elapsedMs,
        nativeOps,
        tsOps,
        rustOps,
        rustOverTs,
        allMatch,
        nativeLast: native.last,
        tsLast: ts.last,
        rustLast: rust.last,
      });

      const line = [
        c.name.padEnd(colName),
        fmt(nativeOps).padStart(colOps),
        fmt(tsOps).padStart(colOps),
        fmt(rustOps).padStart(colOps),
        (fmt(rustOverTs) + "x").padStart(colRatio),
        (allMatch ? "yes" : "NO").padStart(colMatch),
      ].join("  ");
      console.log(line);
    } catch (err) {
      console.log(`${c.name.padEnd(colName)}  ERROR: ${String(err)}`);
    }
  }

  // Summary
  const valid = rows.filter(
    (r) =>
      Number.isFinite(r.nativeMs) &&
      Number.isFinite(r.tsMs) &&
      Number.isFinite(r.rustMs)
  );

  const totalNativeMs = valid.reduce((s, r) => s + r.nativeMs, 0);
  const totalTsMs = valid.reduce((s, r) => s + r.tsMs, 0);
  const totalRustMs = valid.reduce((s, r) => s + r.rustMs, 0);

  const totalNativeOps = (valid.length * RUNS * 1000) / totalNativeMs;
  const totalTsOps = (valid.length * RUNS * 1000) / totalTsMs;
  const totalRustOps = (valid.length * RUNS * 1000) / totalRustMs;

  console.log("\nSummary:");
  console.log(`  Cases: ${valid.length}/${CASES.length} successful`);
  console.log(
    `  Native JS:          ${fmtMs(totalNativeMs).padStart(10)} ms   ${fmt(totalNativeOps).padStart(12)} ops/s`
  );
  console.log(
    `  FormLogic TS:       ${fmtMs(totalTsMs).padStart(10)} ms   ${fmt(totalTsOps).padStart(12)} ops/s`
  );
  console.log(
    `  FormLogic Rust Wasm:${fmtMs(totalRustMs).padStart(10)} ms   ${fmt(totalRustOps).padStart(12)} ops/s`
  );
  console.log(
    `  Rust/TS speedup:    ${fmt(totalRustOps / totalTsOps)}x faster`
  );
  console.log(
    `  Native/Rust:        ${fmt(totalNativeOps / totalRustOps)}x`
  );
  console.log(
    `  Native/TS:          ${fmt(totalNativeOps / totalTsOps)}x`
  );

  const mismatches = valid.filter((r) => !r.allMatch);
  if (mismatches.length > 0) {
    console.log("\nResult mismatches:");
    for (const m of mismatches) {
      console.log(
        `  ${m.name}: native=${m.nativeLast} ts=${m.tsLast} rust=${m.rustLast}`
      );
    }
  }
}

try {
  await main();
} catch (err) {
  console.error("Benchmark failed:", err);
  process.exitCode = 1;
}
