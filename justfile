default:
    @just --list

# Install wasm-bindgen-cli (test-integration's runner) at the wasm-bindgen commit Cargo.lock pins
install-wasm-bindgen-cli:
    cargo install wasm-bindgen-cli --locked \
        --git https://github.com/wasm-bindgen/wasm-bindgen \
        --rev "$(grep -m1 'source = "git+https://github.com/wasm-bindgen/wasm-bindgen' {{justfile_directory()}}/Cargo.lock | grep -o '#[0-9a-f]\{40\}' | tr -d '#')"

# Apply formatting
fmt:
    cargo +stable fmt --all

# Check formatting (CI uses this)
fmt-check:
    cargo +stable fmt --all -- --check

# Lint with clippy (excludes integration-tests; that crate only builds for wasm32)
clippy:
    cargo +stable clippy --no-deps --workspace --all-features --exclude ts-gen-integration-tests -- -D warnings

# Run the unit, snapshot and bridge runtime tests (Node 26)
test *ARGS="":
    RUST_BACKTRACE=1 cargo +stable test -p ts-gen -p ts-gen-abi -p ts-gen-bridge {{ARGS}}

# Run the bridge microbenchmarks (Node 26; BENCH=<dir> filters) and print a ns/op table
bench:
    rm -f target/tmp/bench/results.jsonl
    cargo +stable test -q -p ts-gen-bridge --test bridge -- --ignored bench --nocapture
    python3 bridge/benches/table.py target/tmp/bench/results.jsonl

# List, per fixture and configuration, the types the IR zip reads from the parse, into target/zip-diffs
zip-diffs:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo +stable build -q -p ts-gen
    out={{justfile_directory()}}/target/zip-diffs
    rm -rf "$out"
    for f in {{justfile_directory()}}/tests/fixtures/*.d.ts; do
        name=$(basename "$f" .d.ts)
        args=$(sed -n 's|^//! @ts-gen ||p' "$f" | tr '\n' ' ')
        for config in "default:" "mono:--experimental-generic-mono --fallible"; do
            mkdir -p "$out/$name"
            {{justfile_directory()}}/target/debug/ts-gen --input "$f" --output /dev/null $args ${config#*:} --zip-diffs 2>&1 \
                | sed -n 's|^\[ts-gen info\]: zip: ||p' > "$out/$name/${config%%:*}.zip" || true
        done
    done
    find "$out" -empty -delete
    cat "$out"/*/*.zip 2>/dev/null | wc -l

# Re-bless all snapshots
test-overwrite:
    RUST_BACKTRACE=1 BLESS=1 cargo +stable test -p ts-gen

# Run wasm-bindgen integration tests (requires wasm32-unknown-unknown target + Node)
# package.json must live alongside the crate's Cargo.toml: wasm-bindgen-test-runner
# reads `<CARGO_MANIFEST_DIR>/package.json` to discover the npm dependencies that
# generated bindings reference via `#[wasm_bindgen(module = "...")]`.
test-integration *ARGS="":
    npm install --prefix integration-tests
    RUST_BACKTRACE=1 \
    NODE_PATH={{justfile_directory()}}/integration-tests/node_modules \
    CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
        cargo +stable test -p ts-gen-integration-tests --target wasm32-unknown-unknown {{ARGS}}
