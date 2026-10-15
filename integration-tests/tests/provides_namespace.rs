#![cfg(target_arch = "wasm32")]

// Compile-only here; the JS namespace shape these exports publish
// (`m.version()`, `m.api.math.add`, `m.default.issue`) is checked in node
// by `integration-tests/provides_namespace.mjs` over the built cdylib.
#[allow(unused_imports)]
use ts_gen_integration_tests::provides_namespace_provides;

#[wasm_bindgen_test::wasm_bindgen_test]
fn wrapper_compiles() {}
