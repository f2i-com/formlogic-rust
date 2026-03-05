$ErrorActionPreference = "Stop"

if (-not (Test-Path "./dist-wasm/formlogic_wasm.js")) {
  throw "Missing dist-wasm (web) artifacts. Run scripts/wasm-build.ps1 first."
}

if (-not (Test-Path "./dist-wasm-node/formlogic_wasm.js")) {
  throw "Missing dist-wasm-node artifacts. Run scripts/wasm-build-node.ps1 first."
}

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw "Node.js not found in PATH."
}

Write-Host "[formlogic-wasm] Running Node smoke test..."
node "./crates/formlogic-wasm/examples/node-smoke.mjs"
if ($LASTEXITCODE -ne 0) {
  throw "Node smoke test failed with exit code $LASTEXITCODE"
}

Write-Host "[formlogic-wasm] Smoke test completed."
