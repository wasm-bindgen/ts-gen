//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, Number, Promise};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::inline_wrappers_mono::{
    now, Box_ as JsBox, R2Bucket, R2Object, R2PutOptions,
};

// No backing JavaScript module; compile-only coverage that under
// per-monomorphization the `@rust u64` wrappers compile, including over a
// generic receiver, whose wrappers carry the extern's ABI bounds. An async
// member is handed out as its `Promise`, so a `u64` on a `number` leaf keeps
// the JS number there (`Promise<f64>`) and gets no wrapper.
#[allow(dead_code)]
async fn mono_wrappers_compile(obj: &R2Object, boxed: &JsBox<R2Object>) -> Result<(), JsValue> {
    let _: u64 = obj.size();
    let _: Option<u64> = obj.uploaded_at();
    let _: Vec<R2Object> = obj.list(10).await?;
    let _: Vec<R2Object> = obj.list_with_prefix(10, "p").await?;
    let _: Promise<f64> = obj.count();
    let _: f64 = obj.count().await?;
    let _: Vec<u64> = obj.sizes();
    obj.upload(&[1, 2, 3]);
    obj.upload_maybe(&[1, 2, 3]);
    obj.upload_maybe_with_null();
    obj.upload_unset(Some(&[1, 2, 3]), "t");
    obj.upload_unset(None, "t");
    let _: i64 = obj.seek(-1);
    let _: Promise<f64> = obj.settled();
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
