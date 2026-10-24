#![cfg(target_arch = "wasm32")]

use js_sys::{Array, BigInt, Boolean, Function, JsNullable, JsString, Nullable, Number, Undefined};

use ts_gen_integration_tests::nested_arrays::*;

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that every slice / `Vec` element has a wasm-bindgen ABI.
#[allow(dead_code)]
async fn nested_array_signatures_compile(
    grid: &Grid,
    callback: &Function<fn(Array<Array<JsString>>) -> Undefined>,
) {
    let _: Array<Array<JsString>> = grid.rows().await.unwrap();
    grid.put_rows(&[Array::<Number>::new_typed()]);
    let _: Vec<String> = grid.names();
    let tags: Vec<Array<Boolean>> = grid.tags();
    grid.set_tags(&tags);

    let deep_rows: Vec<Array<Array<JsString>>> = deep();
    put_deep(&deep_rows);

    let rows: Vec<Array<JsString>> = readonly_rows(&[]);
    let _: Vec<Array<JsString>> = generic_rows(&rows);
    let _: Vec<Row> = alias_rows(&rows);
    let _: Nullable<Vec<Array<JsString>>> = maybe_rows(&rows);
    let _: Nullable<Vec<Array<JsString>>> = maybe_rows_with_null();
    for_each_row(callback);

    let strings: Vec<JsNullable<JsString>> = nullable_strings(&[]);
    let _ = nullable_strings(&strings);
    let nullable: Vec<JsNullable<Array<JsString>>> = nullable_rows(&[]);
    let _ = nullable_rows(&nullable);

    let bools: Vec<bool> = flags(&[]);
    let _ = flags(&bools);
    let _: Vec<String> = ids(&["a".to_string()]);
    let _: Vec<String> = modes(&["a".to_string()]);
    let _: Vec<f64> = counts(&[1.0]);
    let big: Vec<BigInt> = bigints(&[]);
    let _ = bigints(&big);
    let _: Vec<Grid> = grids(&[grid.clone()]);
}
