#!/usr/bin/env bash
# Builds the Kotlin worker end to end into out/:
#   ts-gen --ir -> ts-gen-abi lower -> kotlin-bindgen -> Kotlin/Wasm
#   -> kotlin-bindgen link -> ts-gen-bridge generate.
# `./run.sh serve` then serves it in workerd and curls every route;
# `./run.sh node` runs the routes node can serve.
#
# Env: WORKERD (else `workerd` on PATH); TSGEN_KOTLIN_BUILT when
# kotlin-bindgen is already installed.
set -euo pipefail
EX="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$EX/../../.." && pwd)"
B="$EX/build"
OUT="$EX/out"
KOUT="$B/compileSync/wasmJs/main/productionExecutable/kotlin"
BIN="$ROOT/target/debug"
STD="$ROOT/std/std.tw"
PROFILE="all-host, results = single, throws = js"
KOTLIN="$ROOT/examples/kotlin-bindgen/build/install/kotlin-bindgen/bin/kotlin-bindgen"

(cd "$ROOT" && cargo build --quiet --bins -p ts-gen -p ts-gen-abi -p ts-gen-bridge)
[ -n "${TSGEN_KOTLIN_BUILT:-}" ] || "$ROOT/examples/kotlin-bindgen/gradlew" --quiet -p "$ROOT/examples/kotlin-bindgen" installDist
mkdir -p "$B"

# 1. The IR: workers-types and the bindings of env.d.ts, with the ABI
#    choices of annotate.d.ts, the JS standard library resolved against
#    std.tw, and what the module provides (provides.d.ts).
"$BIN/ts-gen" -i "$ROOT/tests/fixtures/workers-types.d.ts" -i "$EX/env.d.ts" \
  --annotate "$EX/annotate.d.ts" --with "$STD" \
  --provides "$EX/provides.d.ts" --ir "$B/worker.tw" 2>/dev/null

# 2. The bindings: std's (with the runtime: futures and the loop), then
#    the worker's over them.
"$BIN/ts-gen-abi" lower "$STD" --profile "$PROFILE" > "$B/std.json"
"$BIN/ts-gen-abi" lower "$B/worker.tw" --with "$STD" --profile "$PROFILE" > "$B/worker.json"
"$KOTLIN" "$B/std.json" "$B/std" --package std --runtime
"$KOTLIN" "$B/worker.json" "$B/bindings" --package workers --app-package app --lib std=std

# 3. The Kotlin/Wasm module.
"$ROOT/examples/kotlin-bindgen/gradlew" --quiet -p "$EX" compileProductionExecutableKotlinWasmJs

# 4. Link: the js_code imports the links.tsv files list become their tw
#    keys; the bridge is generated for the linked module.
rm -rf "$OUT"
mkdir -p "$OUT"
"$KOTLIN" link "$KOUT/worker.wasm" "$B/std/links.tsv" "$B/bindings/links.tsv" -o "$OUT/worker.wasm"
"$BIN/ts-gen-bridge" generate "$B/worker.tw" "$OUT/worker.wasm" --with "$STD" --profile "$PROFILE" -o "$OUT"
cp "$EX/loader.js" "$EX/backend.js" "$EX/config.capnp" "$EX/node.mjs" "$KOUT/worker.import-object.mjs" "$KOUT/worker.js-builtins.mjs" "$OUT/"
echo "built $OUT"

case "${1:-}" in
  node) node "$OUT/node.mjs" ;;
  serve)
    mkdir -p "$OUT/storage"
    (cd "$OUT" && exec "${WORKERD:-workerd}" serve config.capnp --experimental 2> workerd.log) &
    pid=$!
    trap 'kill $pid 2>/dev/null' EXIT
    U=http://127.0.0.1:8789
    for _ in $(seq 1 50); do curl -s -o /dev/null $U/ && break; sleep 0.2; done
    curl -s -o /dev/null $U/ || { cat "$OUT/workerd.log" >&2; exit 1; }

    # The tour: each route's reply, under the request that got it.
    get() { printf '\n== %s\n' "$1"; shift; curl -s -A tour "$@"; }
    status() { printf '\n== %s\n' "$1"; curl -s -o /dev/null -w 'status %{http_code}\n' "$2"; }
    ws() {
      printf '\n== WebSocket %s\n' "$1"
      node -e 'const ws = new WebSocket(process.argv[1]);
        ws.onopen = () => ws.send("hello");
        ws.onmessage = (e) => console.log(e.data);
        ws.onclose = (e) => console.log(`closed: ${e.code} ${e.reason}`);' "ws://127.0.0.1:8789$1"
    }
    printf 'file contents' > "$OUT/note.txt"
    get 'GET /' $U/
    get 'POST /echo' -H 'x-kotlin: hi' -d 'Hello!' $U/echo
    get 'GET /url?tag=a&tag=b&q=kotlin' "$U/url?tag=a&tag=b&q=kotlin"
    get 'POST /form (a string and a file)' --form-string 'title=Kotlin' -F "upload=@$OUT/note.txt;type=text/plain" $U/form
    get 'POST /json' -H 'content-type: application/json' -d '{"item":"tea","quantity":2,"tags":["hot","green"]}' $U/json
    echo
    get 'POST /json (not JSON)' -d '{nope' $U/json
    get 'GET /crypto' $U/crypto
    get 'GET /stream' $U/stream
    get 'GET /fetch' $U/fetch
    get 'GET /timers' $U/timers
    get 'GET /timers (again)' $U/timers
    get 'GET /rewrite' $U/rewrite
    get 'GET /counter/increment' $U/counter/increment
    get 'GET /counter/increment (again)' $U/counter/increment
    get 'GET /counter/list' $U/counter/list
    get 'GET /counter/sql' $U/counter/sql
    get 'GET /counter/sql (again)' $U/counter/sql
    get 'GET /counter/alarm' $U/counter/alarm
    get 'GET /counter/reset' $U/counter/reset
    ws /ws
    ws /counter/ws
    get 'GET /rpc' $U/rpc
    get 'GET /errors' $U/errors
    status 'GET /fail' $U/fail
    status 'GET /fail/plain' $U/fail/plain
    get 'GET /scheduled' $U/scheduled
    get 'GET /queue' $U/queue
    ;;
esac
