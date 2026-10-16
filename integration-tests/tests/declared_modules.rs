#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use ts_gen_integration_tests::declared_modules::{declared_modules, options};

// The `--lib-name` module's exports are at the top level, and each further
// module is reached through its own `pub mod`, so the two `constants` read
// from their own module.
#[wasm_bindgen_test]
fn declared_modules_are_separate_apis() {
    assert_eq!(options().depth(), 1.0);
    assert_eq!(declared_modules::fs::constants::o_rdonly.with(|v| *v), 1.0);
    assert_eq!(declared_modules::os::constants::o_rdonly.with(|v| *v), 10.0);
    assert_eq!(declared_modules::os::constants::signals::sigint.with(|v| *v), 2.0);
    assert_eq!(declared_modules::fs::open("abc").fd(), 3.0);
    assert_eq!(declared_modules::os::hostname(), "host");
    // The global namespace has no backing object, so it is only named.
    let _ = &declared_modules::version;
}
