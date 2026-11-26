//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::iterables_mono::{lines, KeyValueStore, Pairs, Rows};

// The fixture has no backing module, so the globals are installed before
// the bindings first read them.
fn install() {
    js_sys::eval(
        "globalThis.Pairs = class extends Map {};
        globalThis.Rows = class extends Array {};
        globalThis.lines = () => new Set(['one', 'two']);",
    )
    .unwrap();
}

#[wasm_bindgen_test]
fn a_reference_iterates_what_the_iterator_yields() {
    install();
    let pairs = Pairs::new().unwrap();
    pairs.set("a", 1.0);
    pairs.set("b", 2.0);
    let mut seen = Vec::new();
    for (key, value) in &pairs {
        seen.push((String::from(key), value.value_of()));
    }
    assert_eq!(seen, [("a".to_string(), 1.0), ("b".to_string(), 2.0)]);

    let rows = Rows::new(JsString::from("x"), JsString::from("y")).unwrap();
    let rows: Vec<JsString> = (&rows).into_iter().collect();
    assert_eq!(rows, ["x", "y"]);

    let lines: Vec<String> = lines().into_iter().map(String::from).collect();
    assert_eq!(lines, ["one", "two"]);
}

// Compile-only: a synthesized `Iterable<T>` wrapper iterates the same way.
#[allow(dead_code)]
fn wrappers_iterate(store: &KeyValueStore) {
    for (key, value) in &store.list() {
        let _: (JsString, wasm_bindgen::JsValue) = (key, value);
    }
}
