#!/usr/bin/env bash
# Builds apps/desktop/src/lib/web-host/ from crates/txtodo-web (ADR 0042): the browser build's
# Worker host. Generated and gitignored (about 3 MB), unlike src/lib/wasm-core/.
#
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <matching crates/txtodo-web's wasm-bindgen dep>
#        a clang that targets wasm32, for ring and SQLite's C: Apple's cannot. On macOS
#        `brew install llvm`; this script finds Homebrew's on its own. CC_/AR_ overrides win.
# Refs: https://wasm-bindgen.github.io/wasm-bindgen/reference/cli.html ·
#       https://docs.rs/cc/latest/cc/#external-configuration-via-environment-variables
set -euo pipefail
cd "$(dirname "$0")/../../.."

if [ -z "${CC_wasm32_unknown_unknown:-}" ]; then
  for llvm in /opt/homebrew/opt/llvm@21 /opt/homebrew/opt/llvm /usr/local/opt/llvm; do
    if [ -x "$llvm/bin/clang" ]; then
      export CC_wasm32_unknown_unknown="$llvm/bin/clang"
      export AR_wasm32_unknown_unknown="$llvm/bin/llvm-ar"
      break
    fi
  done
fi

cargo build -p txtodo-web --target wasm32-unknown-unknown --release

wasm-bindgen \
  target/wasm32-unknown-unknown/release/txtodo_web.wasm \
  --out-dir apps/desktop/src/lib/web-host \
  --out-name txtodo_web \
  --target web \
  --typescript
