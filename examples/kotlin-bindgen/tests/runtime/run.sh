#!/usr/bin/env bash
# Builds probe.tw's exports in Kotlin through the bridge into out/, then
# runs test.mjs against them in node:
#   ts-gen --json -> kotlin-bindgen -> Kotlin/Wasm -> kotlin-bindgen link
#   -> ts-gen --bridge.
# TSGEN_KOTLIN_BUILT skips installing kotlin-bindgen.
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$DIR/../../../.." && pwd)"
B="$DIR/build"
OUT="$DIR/out"
KOUT="$B/compileSync/wasmJs/main/productionExecutable/kotlin"
BIN="$ROOT/target/debug"
PROFILE="all-host, results = single, throws = js"
KOTLIN="$ROOT/examples/kotlin-bindgen/build/install/kotlin-bindgen/bin/kotlin-bindgen"

(cd "$ROOT" && cargo build --quiet --bins -p ts-gen)
[ -n "${TSGEN_KOTLIN_BUILT:-}" ] || "$ROOT/examples/kotlin-bindgen/gradlew" --quiet -p "$ROOT/examples/kotlin-bindgen" installDist
mkdir -p "$B"
"$BIN/ts-gen" -i "$DIR/probe.tw" --json --profile "$PROFILE" > "$B/probe.json"
rm -rf "$B/bindings"
"$KOTLIN" \
  "$B/probe.json" "$B/bindings" --package probe --app-package app --runtime
"$ROOT/examples/kotlin-bindgen/gradlew" --quiet -p "$DIR" compileProductionExecutableKotlinWasmJs
rm -rf "$OUT"
mkdir -p "$OUT"
"$KOTLIN" link "$KOUT/probe.wasm" "$B/bindings/links.tsv" -o "$OUT/probe.wasm"
"$BIN/ts-gen" -i "$DIR/probe.tw" --bridge "$OUT/probe.wasm" --profile "$PROFILE" -o "$OUT"
cp "$DIR/test.mjs" "$KOUT/probe.import-object.mjs" "$KOUT/probe.js-builtins.mjs" "$OUT/"
node "$OUT/test.mjs"
