#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::renamed_constructor_var::{
    _Bar, _Foo, _Late, AlsoPlain, Bar, Foo, Late, Plain,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that a renamed interface + constructor variable is one class:
// the constructor returns the type the methods are on.
#[allow(dead_code)]
fn renamed_constructor_var_compiles() -> Result<(), wasm_bindgen::JsValue> {
    let foo: Foo = _Foo::new()?;
    foo.bar();
    let _: Foo = _Foo::from(1.0);
    let bar: Bar = _Bar::new(2.0)?;
    let _: String = bar.baz();
    let _: String = _Bar::version();
    let late: Late = _Late::new()?;
    late.go();
    let _: AlsoPlain = Plain::new()?;
    Ok(())
}
