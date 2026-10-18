#![cfg(target_arch = "wasm32")]

use js_sys::{AggregateError, Error, UriError};

use ts_gen_integration_tests::builtin_errors::Codec;

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that js_sys error classes resolve as `@throws` types.
#[allow(dead_code)]
async fn builtin_errors_compile(codec: &Codec) {
    let _: Result<String, UriError> = codec.try_decode("%");
    let _: Result<String, AggregateError> = codec.try_decode_all("%");
    let _: Result<String, Error> = codec.parse("%").await;
}
