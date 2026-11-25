#!/usr/bin/env bash
# Builds the Zena worker end to end into out/:
#   ts-gen --ir -> zena-bindgen --entry -> zena build -t js
#   -> ts-gen-bridge generate.
# `./run.sh node` then runs the routes node can serve; `./run.sh serve`
# serves the worker in workerd and curls every route.
#
# Env: ZENA (a zena-cli, else PATH), ZENA_REPO_ROOT (its checkout, for
# the standard library), WORKERD (else PATH).
set -euo pipefail
EX="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$EX/../../.." && pwd)"
B="$EX/build"
OUT="$EX/out"
BIN="$ROOT/target/debug"
STD="$ROOT/std/std.tw"
PROFILE="all-gc, str = host"

(cd "$ROOT" && cargo build --quiet --bins -p ts-gen -p ts-gen-bridge -p zena-bindgen)
rm -rf "$B" "$OUT"
mkdir -p "$B" "$OUT"

# 1. The IR: workers-types and the worker's own declarations (env.d.ts)
#    against the JS standard library, the ABI choices of annotate.d.ts,
#    and what the module provides (provides.d.ts).
"$BIN/ts-gen" -i "$ROOT/tests/fixtures/workers-types.d.ts" -i "$EX/env.d.ts" \
  --annotate "$EX/annotate.d.ts" --provides "$EX/provides.d.ts" \
  --with "$STD" --ir "$B/worker.tw" 2>"$B/ts-gen.log"

# 2. The bindings, the standard library's, and the entry module
#    exporting what the module provides under its keys.
"$BIN/zena-bindgen" "$STD" -o "$B/std.zena" 2>/dev/null
"$BIN/zena-bindgen" "$B/worker.tw" --with "$STD" ./std.zena -o "$B/bindings.zena" \
  --entry "$B/worker.zena" ./bindings.zena ./app.zena 2>"$B/zena-bindgen.log"
cp -r "$EX/app.zena" "$EX/app" "$B/"

# 3. The Zena module.
"${ZENA:-zena-cli}" build "$B/worker.zena" -t js -o "$OUT/worker.wasm"

# 4. The bridge, under the profile zena-bindgen binds (GC, host strings).
"$BIN/ts-gen-bridge" generate "$B/worker.tw" "$OUT/worker.wasm" --with "$STD" \
  --profile "$PROFILE" -o "$OUT"
cp "$EX/loader.js" "$EX/config.capnp" "$EX/node.mjs" "$EX/helper.js" "$OUT/"
# The Durable Objects' storage, empty for each build.
mkdir "$OUT/storage"
echo "built $OUT"

case "${1:-}" in
  node) node "$OUT/node.mjs" ;;
  serve)
    # workerd's own log, kept out of the tour.
    (cd "$OUT" && exec "${WORKERD:-workerd}" serve config.capnp --experimental 2>workerd.log) &
    pid=$!
    trap 'kill $pid 2>/dev/null' EXIT
    for _ in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:8788/ && break; sleep 0.2; done
    "$EX/tour.sh" http://127.0.0.1:8788
    ;;
esac
