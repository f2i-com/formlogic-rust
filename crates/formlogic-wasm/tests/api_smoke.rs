use formlogic_wasm::{wasm_engine_info, WasmFormLogicEngine};

#[test]
fn wasm_engine_info_smoke() {
    let info = wasm_engine_info();
    assert!(info.contains("formlogic-wasm"));
}

#[test]
fn wasm_eval_inspect_smoke() {
    let engine = WasmFormLogicEngine::new();
    let out = engine
        .eval_inspect("let x = 7; x * 6;")
        .expect("eval_inspect should succeed");
    assert_eq!(out, "42");
}
