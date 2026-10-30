#!/usr/bin/env bash
# Builds the Kotlin worker end to end into out/:
#   ts-gen --target wat -> generator (Gradle) -> Kotlin/Wasm -> ts-gen embed.
# `./run.sh serve` then runs it in workerd and curls it.
#
# Env: TS_GEN (default: the repo's target/release/ts-gen, built here),
# WASM_MERGE (binaryen's wasm-merge, else PATH), WORKERD (else PATH).
set -euo pipefail
EX="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$EX/../.." && pwd)"
OUT="$EX/out"
KOUT="$EX/build/compileSync/wasmJs/main/productionExecutable/kotlin"
ABI="$EX/build/worker.abi.wat"

if [ -z "${TS_GEN:-}" ]; then
  (cd "$ROOT" && cargo build --release --quiet)
  TS_GEN="$ROOT/target/release/ts-gen"
fi

# 1. The ABI template. `--provides` names the handler the module exports.
mkdir -p "$EX/build"
"$TS_GEN" -i "$ROOT/tests/fixtures/cloudflare-worker.d.ts" -i "$EX/env.d.ts" \
  --target wat --provides ExportedHandler.fetch -o "$ABI"

# 2. Bindings (the :generator subproject, run by the generateBindings task
#    on build/worker.abi.wat) and the Kotlin/Wasm module.
(cd "$EX" && ./gradlew --quiet compileProductionExecutableKotlinWasmJs)

# 3. Embed: links the "js_code" imports named like ABI functions to the
#    bindings and leaves Kotlin's own to its import object. The bindings
#    share Kotlin's "'" string constants namespace.
rm -rf "$OUT"
"$TS_GEN" embed --abi "$ABI" --bindings-module js_code --imported-string-constants "'" \
  -m "$KOUT/worker.wasm" -o "$OUT"
cp "$EX/loader.js" "$EX/config.capnp" "$KOUT/worker.import-object.mjs" "$KOUT/worker.js-builtins.mjs" "$OUT/"
echo "built $OUT"

if [ "${1:-}" = serve ]; then
  (cd "$OUT" && exec "${WORKERD:-workerd}" serve config.capnp --experimental) &
  pid=$!
  trap 'kill $pid 2>/dev/null' EXIT
  for _ in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:8789/ && break; sleep 0.2; done
  curl -s -H 'x-kotlin: hi' -d 'Hello!' http://127.0.0.1:8789/echo
fi
