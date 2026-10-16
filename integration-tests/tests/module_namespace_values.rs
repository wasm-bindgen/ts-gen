#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::module_namespace_values::{inspect, outer, InspectOptions};

// Value members of a namespace inside a module bind through the namespace
// object (`inspect.defaultOptions`), not as module exports, so these only
// pass when the bindings carry `module` and `js_namespace` together.
#[wasm_bindgen_test]
fn module_namespace_values_bind_through_the_namespace() {
    // `inspect` names both the function and the module.
    assert_eq!(inspect(&JsValue::from(1)), "inspect(1)");
    let options: InspectOptions = inspect::default_options.with(Clone::clone);
    assert_eq!(options.depth(), 2.0);
    let formatter: inspect::Formatter<JsString> = inspect::with_depth(3.0);
    assert_eq!(formatter.format(&JsString::from("x")), "x@3");
    assert_eq!(outer::inner::limit.with(|v| *v), 7.0);
    assert_eq!(outer::inner::run(inspect::Style::Ansi), "run:ansi");
}
