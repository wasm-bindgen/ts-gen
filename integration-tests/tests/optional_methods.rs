#![cfg(target_arch = "wasm32")]

// An interface of optional methods alone is a dictionary: built with the
// functions a caller has, each read back as a function that may be absent.
// Beside a required method an optional one stays a method.
use js_sys::{Function, JsOption, Promise, Reflect, Undefined};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::optional_methods::*;

#[wasm_bindgen_test]
fn handlers_build_from_the_functions_set() {
    let end: Function<fn() -> Undefined> =
        Function::new_no_args("globalThis.ended = true").unchecked_into();
    let handlers = Handlers::builder().end(&end).label("p").build();
    assert!(handlers.chunk().is_none());
    assert_eq!(handlers.label().as_deref(), Some("p"));

    let read: Function = handlers.end().unwrap().unchecked_into();
    read.call0(&handlers).unwrap();
    let ended = Reflect::get(&js_sys::global(), &JsValue::from_str("ended")).unwrap();
    assert_eq!(ended, JsValue::TRUE);

    assert!(Handlers::new().end().is_none());
}

#[allow(dead_code)]
fn signatures_compile(
    rewriter: &Rewriter,
    chunk: &Function<fn(Chunk) -> JsOption<Promise<Undefined>>>,
) {
    let handlers = Handlers::builder().chunk(chunk).build();
    handlers.set_chunk(chunk);
    let _: Rewriter = rewriter.on("p", &handlers);
    rewriter.flush();
}
