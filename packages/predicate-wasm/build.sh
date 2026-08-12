#!/usr/bin/env bash
#
# Regenerates pkg/ from crates/market-wasm.
#
# The output is committed, so the front end builds without a Rust toolchain --
# the same arrangement as the IDL. Run this whenever the verifier changes.
#
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

# Must match the `wasm-bindgen` version pinned in crates/market-wasm/Cargo.toml,
# or the generated glue will not match the module it is generated from.
command -v wasm-bindgen >/dev/null || {
  echo "wasm-bindgen not found: cargo install wasm-bindgen-cli --version 0.2.104" >&2
  exit 1
}

cargo build -p market-wasm --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/market_wasm.wasm \
  --out-dir packages/predicate-wasm/pkg \
  --target web

echo "wasm rebuilt: $(du -h packages/predicate-wasm/pkg/market_wasm_bg.wasm | cut -f1)"
