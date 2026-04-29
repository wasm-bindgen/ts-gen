default:
    @just --list

# Check formatting (CI uses this)
fmt:
    cargo fmt --all -- --check

# Apply formatting
fmt-fix:
    cargo fmt --all

# Lint with clippy (excludes integration-tests; that crate only builds for wasm32)
clippy:
    cargo clippy --no-deps --workspace --all-features --exclude ts-gen-integration-tests -- -D warnings

# Run ts-gen unit + snapshot tests
test *ARGS="":
    RUST_BACKTRACE=1 cargo test -p ts-gen {{ARGS}}

# Re-bless all snapshots
test-overwrite:
    RUST_BACKTRACE=1 BLESS=1 cargo test -p ts-gen

# Run wasm-bindgen integration tests (requires wasm32-unknown-unknown target + Node)
# package.json must live alongside the crate's Cargo.toml: wasm-bindgen-test-runner
# reads `<CARGO_MANIFEST_DIR>/package.json` to discover the npm dependencies that
# generated bindings reference via `#[wasm_bindgen(module = "...")]`.
test-integration *ARGS="":
    npm install --prefix integration-tests
    RUST_BACKTRACE=1 \
    NODE_PATH={{justfile_directory()}}/integration-tests/node_modules \
    CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
        cargo test -p ts-gen-integration-tests --target wasm32-unknown-unknown {{ARGS}}
