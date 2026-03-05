import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { WasmFormLogicEngine, wasm_engine_info } = require("../../../dist-wasm-node/formlogic_wasm.js");

const engine = new WasmFormLogicEngine();
const value = engine.eval("let x = 7; x * 6;");

console.log(wasm_engine_info());
console.log("result:", value);
