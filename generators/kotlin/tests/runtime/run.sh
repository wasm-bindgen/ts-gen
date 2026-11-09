#!/usr/bin/env bash
# Builds probe.ir's exports in Kotlin through the bridge into out/, then
# runs test.mjs against them in node:
#   ts-gen-abi lower -> ts-gen-kotlin -> Kotlin/Wasm -> ts-gen-bridge link
#   -> ts-gen-bridge generate.
# TSGEN_KOTLIN_BUILT skips installing ts-gen-kotlin.
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$DIR/../../../.." && pwd)"
B="$DIR/build"
OUT="$DIR/out"
KOUT="$B/compileSync/wasmJs/main/productionExecutable/kotlin"
BIN="$ROOT/target/debug"
PROFILE="all-host, results = single, throws = js"

(cd "$ROOT" && cargo build --quiet --bins -p ts-gen-abi -p ts-gen-bridge)
[ -n "${TSGEN_KOTLIN_BUILT:-}" ] || "$ROOT/generators/kotlin/gradlew" --quiet -p "$ROOT/generators/kotlin" installDist
mkdir -p "$B"
"$BIN/ts-gen-abi" lower "$DIR/probe.ir" --profile "$PROFILE" > "$B/probe.json"
rm -rf "$B/bindings"
"$ROOT/generators/kotlin/build/install/ts-gen-kotlin/bin/ts-gen-kotlin" \
  "$B/probe.json" "$B/bindings" --package probe --app-package app --runtime
"$ROOT/generators/kotlin/gradlew" --quiet -p "$DIR" compileProductionExecutableKotlinWasmJs
rm -rf "$OUT"
mkdir -p "$OUT"
"$BIN/ts-gen-bridge" link "$KOUT/probe.wasm" "$B/bindings/links.tsv" -o "$OUT/probe.wasm"
"$BIN/ts-gen-bridge" generate "$DIR/probe.ir" "$OUT/probe.wasm" --profile "$PROFILE" -o "$OUT"
cp "$DIR/test.mjs" "$KOUT/probe.import-object.mjs" "$KOUT/probe.js-builtins.mjs" "$OUT/"
node "$OUT/test.mjs"
