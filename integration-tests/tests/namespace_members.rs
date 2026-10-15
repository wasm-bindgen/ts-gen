#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::namespace_members::{
    events, global_emitter, make, wasm, Emitter, Stream,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that namespace members resolve from their use site and are
// qualified from outside their `pub mod`. The function-pointer bindings pin
// the exact parameter types, which deref coercion to `&JsValue` would hide.
#[allow(dead_code)]
fn namespace_members_compile(
    stream: &Stream,
    options: &events::Options,
    handle: &events::inner::Handle,
) {
    let _: fn(&events::Emitter, &str) = events::Emitter::on;
    let _: fn(&str, &events::Options) -> events::Emitter = events::listen;
    let _: fn() -> events::Emitter = make;
    let _: fn() -> Emitter = global_emitter;
    let _: events::Emitter = options.emitter();
    let _: events::Emitter = handle.emitter();
    let _: events::Options = handle.options();
    let _: &events::Emitter = stream.as_ref();
    let _: fn() -> wasm::Global = wasm::current;
    let _: f64 = wasm::current().value();
}
