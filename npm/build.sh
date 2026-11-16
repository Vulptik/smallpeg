#!/usr/bin/env bash
# Build the npm package: the bindings crate in wasm/ compiled to
# WebAssembly, with JavaScript glue for Node (npm/node) and for browsers
# and bundlers (npm/web).
set -euo pipefail
cd "$(dirname "$0")/.."

WANT=$(grep '^wasm-bindgen = ' wasm/Cargo.toml | cut -d'"' -f2 | tr -d '=')
HAVE=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [[ "$HAVE" != "$WANT" ]]; then
  echo "wasm-bindgen CLI $WANT is required (found: ${HAVE:-none})." >&2
  echo "Install it with: cargo install wasm-bindgen-cli --version $WANT --locked" >&2
  exit 1
fi

CRATE=$(grep '^version = ' Cargo.toml | head -1 | cut -d'"' -f2)
PKG=$(node -p "require('./npm/package.json').version")
if [[ "$CRATE" != "$PKG" ]]; then
  echo "npm/package.json version $PKG does not match Cargo.toml $CRATE" >&2
  exit 1
fi

cargo build --manifest-path wasm/Cargo.toml --release --target wasm32-unknown-unknown
WASM=wasm/target/wasm32-unknown-unknown/release/smallpeg_wasm.wasm

rm -rf npm/node npm/web
wasm-bindgen --target nodejs --out-dir npm/node "$WASM"
wasm-bindgen --target web --out-dir npm/web "$WASM"
# The web glue is an ES module; say so, or Node reparses it with a warning.
echo '{ "type": "module" }' > npm/web/package.json

# Shrink further when Binaryen's wasm-opt is installed (optional).
if command -v wasm-opt >/dev/null; then
  for f in npm/node/smallpeg_wasm_bg.wasm npm/web/smallpeg_wasm_bg.wasm; do
    wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext -o "$f" "$f"
  done
fi

cp LICENSE-MIT LICENSE-APACHE THIRD_PARTY.md npm/
ls -l npm/node/*.wasm npm/web/*.wasm
echo "npm package ready in npm/"
