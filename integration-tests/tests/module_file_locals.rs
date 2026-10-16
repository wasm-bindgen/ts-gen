#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use ts_gen_integration_tests::module_file_locals::{
    _Foo, _Server, configure, ren, reveal, Alias, Foo, Hidden, Options,
};

// Unexported top-level declarations in a module file belong to the module:
// the renamed exports resolve to the local declarations, `configure` takes
// the unexported `Options`, and `reveal` returns the unexported `Hidden`.
// The module exports none of `helper`, `secret`, `Hidden`, so none of them
// is imported; this runs because `import { helper } from "module-file-locals"`
// would fail at link time, before any test.
#[wasm_bindgen_test]
fn module_file_locals_bind_only_through_exports() {
    let foo: Foo = _Foo::new().unwrap();
    foo.bar();
    let alias: Alias = configure(&Options::new(1.0));
    alias.bar();
    ren();
    let hidden: Hidden = reveal();
    hidden.m();
    let _: fn(&_Server) = _Server::listen;
}
