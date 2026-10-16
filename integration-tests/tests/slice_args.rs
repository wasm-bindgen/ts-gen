#![cfg(target_arch = "wasm32")]

use js_sys::{BigInt, Boolean, JsString};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::slice_args::*;

#[wasm_bindgen_test]
fn slice_arguments_arrive_as_arrays() {
    assert_eq!(numbers(&[1.0, 2.5]), "Array(1,2.5)");
    assert_eq!(counts(&[3.0]), "Array(3)");
    assert_eq!(literals(&[1.0, 2.0]), "Array(1,2)");
    assert_eq!(levels(&[2.0]), "Array(2)");
    assert_eq!(strings(&["a".to_string(), "b".to_string()]), "Array(a,b)");
    assert_eq!(
        flags(&[Boolean::from(true), Boolean::from(false)]),
        "Array(true,false)"
    );
    assert_eq!(bigints(&[BigInt::from(7)]), "Array(7)");
    assert_eq!(
        items(&[Item::new("x").unwrap(), Item::new("y").unwrap()]),
        "Array(x,y)"
    );
    assert_eq!(generic(&[JsString::from("g")]), "Array(g)");
}
