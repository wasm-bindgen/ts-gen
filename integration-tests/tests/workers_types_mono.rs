//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

// Compile-only, like `workers_types`, under per-monomorphization lowering.
use ts_gen_integration_tests::workers_types_mono as _;
