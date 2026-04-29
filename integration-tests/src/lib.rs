#![allow(non_snake_case, non_upper_case_globals, unused_imports, clippy::all)]

#[cfg(target_arch = "wasm32")]
include!(concat!(env!("OUT_DIR"), "/_lib.rs"));
