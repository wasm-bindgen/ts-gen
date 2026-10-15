#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::namespace_merge::{
    make, make_foo, Bar, Config, Foo, Kind, Mode, Options, Shape,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that `Name.Member` resolves through a namespace merged with a
// same-named class, function or interface. The function-pointer bindings pin
// the exact parameter types, which deref coercion to `&JsValue` would hide.
#[allow(dead_code)]
fn namespace_merge_compiles(bar: &Bar, shape: &Shape) {
    let _: fn(&Foo, &Options) = Foo::m;
    let _: fn(&Config) = make;
    let _: Foo = make_foo();
    let _: Bar = bar.mode(Mode::On);
    shape.kind(Kind::X);
    let _ = Options::new("a");
    let _ = Config::new("c");
}
