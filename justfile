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

# Run ts-gen unit + snapshot tests
test *ARGS="":
    RUST_BACKTRACE=1 cargo +stable test -p ts-gen -p ts-gen-abi {{ARGS}}

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
    cargo +stable build -p ts-gen-integration-tests --target wasm32-unknown-unknown
    node integration-tests/provides_namespace.mjs \
        {{justfile_directory()}}/target/wasm32-unknown-unknown/debug/ts_gen_integration_tests.wasm \
        $(which wasm-bindgen)
