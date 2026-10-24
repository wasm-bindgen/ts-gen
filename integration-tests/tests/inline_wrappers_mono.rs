//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, Nullable, Number, Promise};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::inline_wrappers_mono::{
    now, Box as JsBox, R2Bucket, R2Object, R2PutOptions,
};

// No backing JavaScript module; compile-only coverage that under
// per-monomorphization the `@rust u64` wrappers compile, including over a
// generic receiver, whose wrappers carry the extern's ABI bounds.
#[allow(dead_code)]
async fn mono_wrappers_compile(obj: &R2Object, boxed: &JsBox<R2Object>) -> Result<(), JsValue> {
    let _: u64 = obj.size();
    let _: Nullable<u64> = obj.uploaded_at();
    let _: Array<R2Object> = obj.list(10).await?;
    let _: Array<R2Object> = obj.list_with_prefix(10, "p").await?;
    let _: u64 = obj.count().await?;
    let _: Vec<u64> = obj.sizes();
    obj.upload(&[1, 2, 3]);
    let _: i64 = obj.seek(-1);
    let _: Promise<Number> = obj.settled();
    let _: Vec<Array<Number>> = obj.matrix();
    let _: u64 = R2Bucket::max_size();
    let _: Result<R2Bucket, JsValue> = R2Bucket::new(1);
    let _: u64 = now();
    let options = R2PutOptions::builder().size(5).build();
    let _: Option<u64> = options.size();
    options.set_size(6);
    let _: u64 = boxed.length();
    let _: R2Object = boxed.take(0);
    Ok(())
}
