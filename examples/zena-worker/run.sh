#!/usr/bin/env bash
# Builds the Zena worker end to end into out/:
#   ts-gen --ir -> ts-gen-zena --entry -> zena build -t js
#   -> ts-gen-bridge generate.
# `./run.sh node` then runs one request in node; `./run.sh serve` serves
# it in workerd and curls it.
#
# Env: ZENA (a zena-cli, else PATH), ZENA_REPO_ROOT (its checkout, for
# the standard library), WORKERD (else PATH).
set -euo pipefail
EX="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$EX/../.." && pwd)"
B="$EX/build"
OUT="$EX/out"
BIN="$ROOT/target/debug"

(cd "$ROOT" && cargo build --quiet --bins -p ts-gen -p ts-gen-bridge -p ts-gen-zena)
rm -rf "$B" "$OUT"
mkdir -p "$B" "$OUT"

# 1. The IR, with what the module provides: the default export's fetch
#    and the Counter Durable Object (provides.d.ts).
"$BIN/ts-gen" -i "$ROOT/tests/fixtures/workers-types.d.ts" -i "$EX/env.d.ts" \
  --provides "$EX/provides.d.ts" --ir "$B/worker.ir" 2>/dev/null

# 2. The bindings, and the entry module exporting fetch under its key.
"$BIN/ts-gen-zena" "$B/worker.ir" -o "$B/bindings.zena" \
  --entry "$B/worker.zena" ./bindings.zena ./app.zena 2>/dev/null
cp "$EX/app.zena" "$B/"

# 3. The Zena module.
"${ZENA:-zena-cli}" build "$B/worker.zena" -t js -o "$OUT/worker.wasm"

# 4. The bridge, under the profile ts-gen-zena binds (GC, host strings).
"$BIN/ts-gen-bridge" generate "$B/worker.ir" "$OUT/worker.wasm" --profile "all-gc, str = host" -o "$OUT"
cp "$EX/loader.js" "$EX/config.capnp" "$EX/node.mjs" "$OUT/"
echo "built $OUT"

case "${1:-}" in
  node) node "$OUT/node.mjs" ;;
  serve)
    (cd "$OUT" && exec "${WORKERD:-workerd}" serve config.capnp --experimental) &
    pid=$!
    trap 'kill $pid 2>/dev/null' EXIT
    for _ in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:8788/ && break; sleep 0.2; done
    curl -s -H 'x-zena: hi' -d 'Hello!' http://127.0.0.1:8788/echo
    curl -s http://127.0.0.1:8788/count
    curl -s http://127.0.0.1:8788/count
    curl -s -w " %{http_code}\n" -o /dev/null http://127.0.0.1:8788/fail
    ;;
esac
