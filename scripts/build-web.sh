#!/bin/bash
# Build the kagi-web WASM harness into crates/kagi-web/dist/.
#
# Requires the pinned nightly (wasm_thread uses #![feature]) and wasm-bindgen:
#   rustup toolchain install nightly-2026-10-18 --profile minimal --target wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.123 --locked
#
# Usage: scripts/build-web.sh [--release]
set -euo pipefail
cd "$(dirname "$0")/.."

MODE=debug
FLAG=""
if [[ "${1:-}" == "--release" ]]; then MODE=release; FLAG=--release; fi

cargo +nightly-2026-10-18 build --locked -p kagi-web --target wasm32-unknown-unknown $FLAG

DIST=crates/kagi-web/dist
rm -rf "$DIST" && mkdir -p "$DIST"
wasm-bindgen --target web --no-typescript \
  --out-dir "$DIST" \
  "target/wasm32-unknown-unknown/$MODE/kagi_web.wasm"
cp crates/kagi-web/web/index.html "$DIST/"
echo "built: $DIST (mode: $MODE)"
