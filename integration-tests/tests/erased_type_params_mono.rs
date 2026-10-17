//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, JsString, Nullable};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::erased_type_params_mono::{
    ListKey, ListResult, Nested, NestedMapped, Store, V,
};

// Compile-only: the fixture has no backing JavaScript module. An argument
// given for an erased parameter (`ListResult<Metadata, Key>`) is dropped
// before per-monomorphization lowering looks for string leaves, so `list`
// returns the one-parameter `ListResult` and has no `_js_string` variant.
// No argument infers `list`'s own `Metadata`, so it erases to `unknown`.
#[allow(dead_code)]
async fn erased_type_params_mono_compile(store: &Store) -> Result<(), JsValue> {
    let _: Nullable<String> = store.get("k").await?;
    let _: Nullable<JsString> = store.get_js_string("k").await?;
    let result: ListResult = store.list().await?;
    let keys: Vec<ListKey> = result.keys();
    for key in &keys {
        let _: String = key.name();
        let _: Option<JsValue> = key.metadata();
    }
    let _: String = result.cursor();
    store.tag("info", 1.0);
    let _: f64 = store.bump(2.0);
    Ok(())
}

// The argument is also dropped inside `keyof`, an indexed access, a mapped
// type and an object literal, so `Slot<"a", V>` binds `V`, not `"a"`.
#[allow(dead_code)]
fn erased_type_params_mono_nested_compile(nested: &Nested) {
    let _: f64 = nested.indexed();
    let _: V = nested.keys();
    let mapped: NestedMapped = nested.mapped();
    let _: bool = mapped.x();
    let _: bool = mapped.y();
    let _: Vec<String> = nested.literal();
    let _: Array<JsString> = nested.literal_js_string();
    nested.set_literal(&["s"]);
}
