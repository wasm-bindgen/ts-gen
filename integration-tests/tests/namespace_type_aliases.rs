#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Number};
use ts_gen_integration_tests::namespace_type_aliases::{emit, events, on_event};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that namespace aliases are emitted and that generic instantiations
// name them, from inside and outside their `pub mod`.
#[allow(dead_code)]
fn namespace_type_aliases_compile() {
    let _: events::Name = JsString::from("a");
    let _: events::Label = JsString::from("a");
    let _: events::inner::Id = JsString::from("a");
    let _: fn(&str, &events::Opts) -> String = emit;
    let _: fn(&events::Listener<JsString>) = on_event;
    let _: fn(&str, &events::Options) -> String = events::listen;
    let _: fn(&events::Pair<JsString, Number>) -> events::Result_<Number> = events::pair;
    let _: fn(&str) -> events::Options = events::inner::find;
}
