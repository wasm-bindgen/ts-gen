#![allow(non_snake_case, unused_imports, clippy::all)]

#[cfg(target_arch = "wasm32")]
include!(concat!(env!("OUT_DIR"), "/_lib.rs"));
