#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::module_file_locals::{
    _Foo, _Server, configure, Alias, Foo, Options, Server,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that unexported top-level declarations in a module file are
// emitted with the module: the renamed exports resolve and `configure`
// takes the unexported `Options`.
#[allow(dead_code)]
fn module_file_locals_compile(server: &Server) -> Result<(), wasm_bindgen::JsValue> {
    let foo: Foo = _Foo::new()?;
    foo.bar();
    let _: &_Server = server;
    server.listen();
    let alias: Alias = configure(&Options::new(1.0));
    alias.bar();
    Ok(())
}
