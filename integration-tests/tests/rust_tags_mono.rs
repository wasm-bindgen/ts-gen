//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, BigInt, Function, JsNullable, Nullable, Number, Promise, Undefined};

use ts_gen_integration_tests::rust_tags_mono::*;

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that every `@rust` scalar has a wasm-bindgen ABI, including a
// tagged leaf inside a generic handle, which stays a JS `Number`.
#[allow(dead_code)]
async fn rust_tag_mono_signatures_compile(
    response: &Response,
    callback: &Function<fn() -> Undefined>,
) {
    let _: u16 = response.status();
    let _: Nullable<u32> = response.content_length();
    let _: u32 = response.with_status(200);
    let _: u32 = response.with_status_with_reason(200, "OK");
    let _: u32 = response.count().await.unwrap();
    let _: Nullable<u32> = response.maybe_count().await.unwrap();
    response.upload_part(1);
    let _: Vec<u8> = response.bytes();
    response.listen(&[80, 443]);
    let _: u64 = response.offset();

    let closed: Promise<Number> = response.closed();
    let _: Number = response.closed().await.unwrap();
    let _: BigInt = response.total().await.unwrap();
    response.when_ready(&closed);
    let matrix: Vec<Array<Number>> = response.matrix();
    response.set_matrix(&matrix);
    let codes: Vec<JsNullable<Number>> = response.codes();
    response.set_codes(&codes);

    let init = ResponseInit::builder().status(204).build();
    let _: Option<u16> = init.status();
    let _: i32 = set_timeout(callback);
    let _: i32 = set_timeout_with_ms(callback, 10u32);
}
