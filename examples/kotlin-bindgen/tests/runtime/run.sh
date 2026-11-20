#!/usr/bin/env bash
# Builds probe.ir's exports in Kotlin through the bridge into out/, then
# runs test.mjs against them in node:
#   ts-gen-abi lower -> kotlin-bindgen -> Kotlin/Wasm -> ts-gen-bridge link
#   -> ts-gen-bridge generate.
# TSGEN_KOTLIN_BUILT skips installing kotlin-bindgen.
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$DIR/../../../.." && pwd)"
B="$DIR/build"
OUT="$DIR/out"
KOUT="$B/compileSync/wasmJs/main/productionExecutable/kotlin"
BIN="$ROOT/target/debug"
PROFILE="all-host, results = single, throws = js"

(cd "$ROOT" && cargo build --quiet --bins -p ts-gen-abi -p ts-gen-bridge)
[ -n "${TSGEN_KOTLIN_BUILT:-}" ] || "$ROOT/examples/kotlin-bindgen/gradlew" --quiet -p "$ROOT/examples/kotlin-bindgen" installDist
mkdir -p "$B"
"$BIN/ts-gen-abi" lower "$DIR/probe.ir" --profile "$PROFILE" > "$B/probe.json"
rm -rf "$B/bindings"
"$ROOT/examples/kotlin-bindgen/build/install/kotlin-bindgen/bin/kotlin-bindgen" \
  "$B/probe.json" "$B/bindings" --package probe --app-package app --runtime
"$ROOT/examples/kotlin-bindgen/gradlew" --quiet -p "$DIR" compileProductionExecutableKotlinWasmJs
rm -rf "$OUT"
mkdir -p "$OUT"
"$BIN/ts-gen-bridge" link "$KOUT/probe.wasm" "$B/bindings/links.tsv" -o "$OUT/probe.wasm"
"$BIN/ts-gen-bridge" generate "$DIR/probe.ir" "$OUT/probe.wasm" --profile "$PROFILE" -o "$OUT"
cp "$DIR/test.mjs" "$KOUT/probe.import-object.mjs" "$KOUT/probe.js-builtins.mjs" "$OUT/"
node "$OUT/test.mjs"
