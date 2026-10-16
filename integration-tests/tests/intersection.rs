#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::intersection::{
    Both, Conditional, Derived, Fetcher, GenericStub, GetOptions, Provider, Refined, Store,
    StoreOptions, Stub, Unrelated,
};
use wasm_bindgen::{JsCast, JsValue};

// Compile-only coverage that the `extends` chains synthesised from
// intersections are accepted by wasm-bindgen and deref to their parents.
#[allow(dead_code)]
async fn intersection_bindings_compile(
    store: &Store,
    stub: &Stub,
    generic_stub: &GenericStub<JsValue>,
    derived: &Derived,
) {
    let refined = StoreOptions::new(&Conditional::new());
    let _: Option<String> = refined.range();
    let _ = store.get_with_store_options("k", &refined).await;
    let _ = store.get_with_get_options("k", &refined).await;
    let _: &GetOptions = &refined;

    let alias = Refined::new(&Conditional::new());
    let _: Conditional = alias.only_if();

    store.refine(derived);
    store.same(derived);
    store.generic(derived);
    store.primitive("x");
    store.anything(&JsValue::NULL);

    // Deref reaches the first parent; the rest through AsRef.
    let both: &Both = &JsValue::NULL.unchecked_into();
    let _: &Derived = both;
    let unrelated: &Unrelated = both.as_ref();
    let _: String = unrelated.other();

    let _: String = stub.id();
    let fetcher: &Fetcher = stub;
    let _ = fetcher.fetch("x").await;

    let _: String = generic_stub.id();
    let provider: &Provider<JsValue> = generic_stub;
    let _ = provider.call();
    let fetcher: &Fetcher = generic_stub.as_ref();
    let _ = fetcher.fetch("x").await;
}
