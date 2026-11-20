#![cfg(target_arch = "wasm32")]

use js_sys::{Function, JsString, Map, Number, Object};
use wasm_bindgen::{JsCast, JsValue};

use ts_gen_integration_tests::generic_defaults::{
    enter_span, enter_span_typed, lookup, lookup_typed, lookup_with_key, loose_box, loose_holder,
    make_holder, make_holder_typed, mixed_holder, of, string_holder, Box_, Cursor, Holder, Options,
    Row, Store,
};

// Compile-only: the defaults make the bare names well-formed Rust types
// while explicit arguments still work.
#[allow(dead_code)]
fn defaults_compile(holder: Holder, boxed: Box_, options: Options) {
    let _: JsValue = holder.get();
    let _: Box_<JsValue> = holder.boxed();
    let _: JsValue = boxed.value();
    let _: Option<Box_<JsString>> = options.initial();
    let _: Holder<JsString, Box_<JsString>> = JsValue::UNDEFINED.unchecked_into();
    let _: Options<Box_<JsString>> = Options::builder().build();
    let _ = Options::<JsString>::builder_typed().build();
}

// Arguments left to the declared defaults name the same types as the
// spelled-out instantiations.
#[allow(dead_code)]
fn trimmed_defaults_compile() {
    let _: Box_<JsValue> = loose_box();
    let _: Holder<JsValue, Box_<JsValue>> = loose_holder();
    let _: Holder<JsString, Box_<JsString>> = string_holder();
    let _: Holder<JsString, Box_<JsValue>> = mixed_holder();
}

// An uninferrable `V` in `Holder<string, V>` skips `U`'s default `Box<T>`,
// which names Holder's own `T`, and erases to `any`.
#[allow(dead_code)]
fn erased_past_dependent_default_compile() {
    let _: Holder<JsString, JsValue> = make_holder();
    let _: Holder<JsString, Number> = make_holder_typed::<Number>();
    let _: Holder<JsString, JsValue> = Holder::make();
}

// A method type parameter no argument infers reads at its default under
// the plain name: the default of the container it is nested in (`Row` is
// `Row<Object>`), else its own, else its bound. The `_typed` twin takes
// the caller's choice, and one the return never mentions has no twin.
#[allow(dead_code)]
async fn uninferrable_type_params_compile(
    store: &Store,
    callback: &Function,
) -> Result<(), JsValue> {
    store.put("k", &JsString::from("v")).await?;
    let _: JsValue = store.get("k").await?;
    let _: Option<JsString> = store.get_typed::<JsString>("k").await?;
    let _: JsValue = store.first().await?;
    let _: Option<Number> = store.first_typed::<Number>().await?;
    let _: JsValue = store.json().await?;
    let _: Object = store.json_typed::<Object>().await?;
    let _: JsString = store
        .transaction::<JsString>(callback.unchecked_ref())
        .await?;
    let _: Map<JsString, JsValue> = store.collect(callback.unchecked_ref()).await?;
    let _: Map<JsString, JsString> = store.each::<JsString>(callback.unchecked_ref()).await?;
    let _: Map<JsString, JsValue> = store.each_rest(callback).await?;
    let _: Map<JsString, Number> = store.each_rest_typed::<Number>(callback).await?;
    let _: Row<Object> = store.run().await?;
    let _: Row<Number> = store.run_typed::<Number>().await?;
    let _: Map<JsString, JsValue> = store.list().await?;
    let _: Map<JsString, Number> = store.list_typed::<Number>().await?;
    let _: Cursor<JsString> = store.scan();
    let _: Cursor<Number> = store.scan_typed::<Number>();
    let _: Cursor = store.pick();
    let _: Cursor<Number> = store.pick_typed::<Cursor<Number>>();
    let _: f64 = store.schedule(callback, &[]);
    let _: Row = store.append(&[JsValue::from(1)]);
    let _: Row<Number> = store.append_typed::<Number>(&[JsValue::from(1)]);
    let _: Row = of(&[JsValue::from(1)]);
    let _: JsValue = enter_span("span", callback, &[]);
    let _: JsString = enter_span_typed::<JsString>("span", callback, &[]);
    let _: Map<JsString, Number> = lookup();
    let _: Map<JsString, JsString> = lookup_typed::<JsString>();
    lookup_with_key("k");
    Ok(())
}
