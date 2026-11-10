#!/usr/bin/env bash
# Builds the Kotlin worker end to end into out/:
#   ts-gen --ir -> ts-gen-abi lower -> ts-gen-kotlin -> Kotlin/Wasm
#   -> ts-gen-bridge link -> ts-gen-bridge generate.
# `./run.sh node` then runs one request in node; `./run.sh serve` serves
# it in workerd and curls it.
#
# Env: WORKERD (else PATH); TSGEN_KOTLIN_BUILT when ts-gen-kotlin is
# already installed.
set -euo pipefail
EX="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$EX/../.." && pwd)"
B="$EX/build"
OUT="$EX/out"
KOUT="$B/compileSync/wasmJs/main/productionExecutable/kotlin"
BIN="$ROOT/target/debug"

(cd "$ROOT" && cargo build --quiet --bins -p ts-gen -p ts-gen-abi -p ts-gen-bridge)
[ -n "${TSGEN_KOTLIN_BUILT:-}" ] || "$ROOT/generators/kotlin/gradlew" --quiet -p "$ROOT/generators/kotlin" installDist
mkdir -p "$B"

# 1. The IR, with the default export's fetch as what the module provides.
"$BIN/ts-gen" -i "$ROOT/tests/fixtures/workers-types.d.ts" -i "$EX/env.d.ts" \
  --provides ExportedHandler.fetch --ir "$B/worker.ir" 2>/dev/null

# 2. The bindings, with the runtime (futures and the loop).
"$BIN/ts-gen-abi" lower "$B/worker.ir" --profile "all-host, results = single, throws = js" > "$B/worker.json"
"$ROOT/generators/kotlin/build/install/ts-gen-kotlin/bin/ts-gen-kotlin" \
  "$B/worker.json" "$B/bindings" --package workers --app-package app --runtime

# 3. The Kotlin/Wasm module.
"$ROOT/generators/kotlin/gradlew" --quiet -p "$EX" compileProductionExecutableKotlinWasmJs

# 4. Link: the js_code imports links.tsv lists become their tw keys; the
#    bridge is generated for the linked module.
rm -rf "$OUT"
mkdir -p "$OUT"
"$BIN/ts-gen-bridge" link "$KOUT/worker.wasm" "$B/bindings/links.tsv" -o "$OUT/worker.wasm"
"$BIN/ts-gen-bridge" generate "$B/worker.ir" "$OUT/worker.wasm" --profile "all-host, results = single, throws = js" -o "$OUT"
cp "$EX/loader.js" "$EX/config.capnp" "$EX/node.mjs" "$KOUT/worker.import-object.mjs" "$KOUT/worker.js-builtins.mjs" "$OUT/"
echo "built $OUT"

case "${1:-}" in
  node) node "$OUT/node.mjs" ;;
  serve)
    (cd "$OUT" && exec "${WORKERD:-workerd}" serve config.capnp --experimental) &
    pid=$!
    trap 'kill $pid 2>/dev/null' EXIT
    for _ in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:8789/ && break; sleep 0.2; done
    curl -s -H 'x-kotlin: hi' -d 'Hello!' http://127.0.0.1:8789/echo
    ;;
esac
