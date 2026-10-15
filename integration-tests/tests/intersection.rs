#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::intersection::{
    Conditional, Derived, Fetcher, GetOptions, Refined, Store, StoreOptions, StoreValue, Stub,
};
use wasm_bindgen::{JsCast, JsValue};

// Compile-only coverage that the `extends` chains synthesised from
// intersections are accepted by wasm-bindgen and deref to their parents.
#[allow(dead_code)]
async fn intersection_bindings_compile(store: &Store, stub: &Stub) {
    let refined = StoreOptions::new(&Conditional::new());
    let _: Option<String> = refined.range();
    let _ = store.get_with_store_options("k", &refined).await;
    let _ = store.get_with_get_options("k", &refined).await;
    let _: &GetOptions = &refined;

    let alias = Refined::new(&Conditional::new());
    let _: Conditional = alias.only_if();

    let value: &StoreValue = &JsValue::NULL.unchecked_into();
    let _: &Derived = value;
    let _: String = value.id();

    let _: String = stub.id();
    let fetcher: &Fetcher = stub;
    let _ = fetcher.fetch("x").await;
}
