#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use ts_gen_integration_tests::module_namespace_merge::constants;

// `constants` of both modules and the global `constants` share one module,
// so this only compiles when they are merged, and only passes when each
// member keeps its own module.
#[wasm_bindgen_test]
fn same_named_namespaces_share_a_module() {
    assert_eq!(constants::o_rdonly.with(|v| *v), 1.0);
    assert_eq!(constants::o_rdonly_2.with(|v| *v), 10.0);
    assert_eq!(constants::flags::excl.with(|v| *v), 2.0);
    assert_eq!(constants::flags::nofollow.with(|v| *v), 20.0);
    assert_eq!(constants::signals::sigint.with(|v| *v), 30.0);
    // The global namespace has no backing object, so it is only named.
    let _ = &constants::version;
}
