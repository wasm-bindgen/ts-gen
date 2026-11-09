#![cfg(target_arch = "wasm32")]

// Compile-only: the bindings of the largest fixture compile, which checks
// what each setter, builder and constructor passes to another.
use ts_gen_integration_tests::workers_types as _;
