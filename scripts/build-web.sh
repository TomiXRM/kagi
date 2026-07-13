#!/bin/bash
# Build the kagi-web WASM harness into crates/kagi-web/dist/.
#
# Requires: nightly toolchain with wasm32-unknown-unknown (wasm_thread uses
# #![feature]), wasm-bindgen-cli.
#   rustup toolchain install nightly
#   rustup target add wasm32-unknown-unknown --toolchain nightly
#   cargo install wasm-bindgen-cli
#
# Usage: scripts/build-web.sh [--release]
set -euo pipefail
cd "$(dirname "$0")/.."

MODE=debug
FLAG=""
if [[ "${1:-}" == "--release" ]]; then MODE=release; FLAG=--release; fi

cargo +nightly build -p kagi-web --target wasm32-unknown-unknown $FLAG

DIST=crates/kagi-web/dist
rm -rf "$DIST" && mkdir -p "$DIST"
wasm-bindgen --target web --no-typescript \
  --out-dir "$DIST" \
  "target/wasm32-unknown-unknown/$MODE/kagi_web.wasm"
cp crates/kagi-web/web/index.html "$DIST/"
echo "built: $DIST (mode: $MODE)"
