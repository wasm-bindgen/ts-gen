#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Reflect};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::alias_constructors::{
    pt, unwrap_text, wrap2, Counter, Label, MultiA, MultiB, Point, Stub, Wrap,
};

// The `const` constructs the type its alias names, so the stub passes
// straight to an API that takes `Wrap<string>`.
#[wasm_bindgen_test]
fn alias_constructors_construct_the_aliased_type() {
    let stub: Wrap<JsString> = Wrap::new(&JsString::from("abc")).unwrap();
    assert_eq!(unwrap_text(&stub), "abc");
    assert_eq!(stub.get(), "abc");
    assert_eq!(Label::new("a").unwrap().text(), "a");
    assert_eq!(Label::new_with_n_and_unit(2.0, "px").unwrap().text(), "2px");
    assert_eq!(Counter::new(3.0).unwrap().count(), 3.0);
    assert_eq!(MultiA::new().unwrap().a(), "a");
    assert_eq!(MultiB::new(2.0).unwrap().b(), 2.0);
}

#[wasm_bindgen_test]
fn colliding_alias_constructors_bind_as_values() {
    let _: Point = Point::new();
    let _: Stub<JsString> = Stub::new(&JsString::from("x")).unwrap();
    assert!(Reflect::get(&pt.with(Clone::clone), &"name".into()).unwrap() == "Pt");
    assert!(Reflect::get(&wrap2.with(Clone::clone), &"name".into()).unwrap() == "Wrap");
}
