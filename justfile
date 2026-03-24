default:
    @just --list

# Run ts-gen unit + snapshot tests
test *ARGS="":
    RUST_BACKTRACE=1 cargo test -p ts-gen {{ARGS}}

# Re-bless all snapshots
test-overwrite:
    RUST_BACKTRACE=1 BLESS=1 cargo test -p ts-gen

# Run wasm-bindgen integration tests (requires wasm32-unknown-unknown target + Node)
test-wasm-bindgen *ARGS="":
    npm install --prefix crates/wasm-bindgen-tests
    RUST_BACKTRACE=1 \
    NODE_PATH={{justfile_directory()}}/crates/wasm-bindgen-tests/node_modules \
        cargo test -p ts-gen-wasm-bindgen-tests --target wasm32-unknown-unknown {{ARGS}}
