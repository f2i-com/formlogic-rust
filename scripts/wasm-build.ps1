Param(
  [string]$OutDir = "dist-wasm"
)

$ErrorActionPreference = "Stop"

Write-Host "[formlogic-wasm] Ensuring wasm target is installed..."
rustup target add wasm32-unknown-unknown | Out-Null

Write-Host "[formlogic-wasm] Building wasm crate..."
cargo build -p formlogic-wasm --target wasm32-unknown-unknown --release

if (-not (Get-Command wasm-bindgen -ErrorAction SilentlyContinue)) {
  throw "wasm-bindgen CLI not found. Install with: cargo install wasm-bindgen-cli"
}

Write-Host "[formlogic-wasm] Generating JS bindings into '$OutDir'..."
wasm-bindgen --target web --out-dir "$OutDir" "./target/wasm32-unknown-unknown/release/formlogic_wasm.wasm"

Write-Host "[formlogic-wasm] Done. Artifacts in '$OutDir'."
