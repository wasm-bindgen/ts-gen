//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, Function, JsNullable, Number, Promise};

use ts_gen_integration_tests::rust_tags_mono::*;

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that every `@rust` scalar has a wasm-bindgen ABI. Under
// per-monomorphization an async member's `Promise<T>` and a nullable list
// read carry the scalar; a nested array and a written nullable list keep the
// JS `Number` their handles need.
#[allow(dead_code)]
async fn rust_tag_mono_signatures_compile(
    response: &Response,
    callback: &Function<fn()>,
    ready: &Promise<Number>,
    codes: &[JsNullable<Number>],
) {
    let _: u16 = response.status();
    let _: Option<u32> = response.content_length();
    let _: u32 = response.with_status(200);
    let _: u32 = response.with_status_with_reason(200, "OK");
    let _: Promise<u32> = response.count();
    let _: u32 = response.count().await.unwrap();
    let _: Option<u32> = response.maybe_count().await.unwrap();
    response.upload_part(1);
    let _: Vec<u8> = response.bytes();
    response.listen(&[80, 443]);
    let _: u64 = response.offset();

    let _: Promise<u32> = response.closed();
    let _: u32 = response.closed().await.unwrap();
    let _: u64 = response.total().await.unwrap();
    response.when_ready(ready);
    let matrix: Vec<Array<Number>> = response.matrix();
    response.set_matrix(&matrix);
    let _: Vec<Option<u16>> = response.codes();
    response.set_codes(codes);

    let init = ResponseInit::builder().status(204).build();
    let _: Option<u16> = init.status();
    let _: i32 = set_timeout(callback);
    let _: i32 = set_timeout_with_ms(callback, 10u32);
}
