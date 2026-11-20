#!/usr/bin/env bash
# Curls every route of the worker at $1 and prints what each replies.
set -euo pipefail
U="$1"
EX="$(cd "$(dirname "$0")" && pwd)"

show() {
  echo "== $1"
  shift
  curl -s -m 20 -w '[%{http_code}]\n\n' "$@"
}

show "GET /" "$U/"
show "POST /echo" -A tour -H "x-tour: 1" -H "x-zena: hi" -d "Hello!" "$U/echo"
show "GET /url?b=2&a=1" "$U/url?b=2&a=1"
show "POST /form (a field and a file)" -F name=zena -F "upload=@$EX/upload.txt;type=text/plain" "$U/form"
show "POST /json" -d '{"item":"widget","quantity":2,"tags":["a","b"]}' "$U/json"
show "GET /crypto" "$U/crypto"
show "GET /stream" "$U/stream"
show "GET /timers" "$U/timers"
# Its waitUntil finishes after the response.
sleep 0.3
show "GET /fetch" "$U/fetch"
show "GET /rewrite" "$U/rewrite"
show "GET /cache" "$U/cache"
show "GET /counter/kv" "$U/counter/kv"
show "GET /counter/transaction" "$U/counter/transaction"
show "GET /counter/sql" "$U/counter/sql"
show "GET /counter/alarm" "$U/counter/alarm"
show "GET /ws" "$U/ws"
show "GET /rpc" "$U/rpc"
show "GET /fail" "$U/fail"
show "GET /scheduled" "$U/scheduled"
show "GET /queue" "$U/queue"
show "GET /nowhere" "$U/nowhere"
