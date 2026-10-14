#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::prelude_collision::{
    label, take, take_with_d, try_take, Default_, Option_, Result_, Vec_,
};

// Compile-only: a local `Result` / `Option` / `Default` / `Vec` must not
// shadow the prelude types the other bindings spell bare.
#[allow(dead_code)]
fn prelude_collision_compiles(r: &Result_, o: &Option_) {
    let _: Vec_ = take(r, o);
    let _: Result<Vec_, wasm_bindgen::JsValue> = try_take(r, o);
    let _: Vec_ = take_with_d(r, o, &Default_::new());
    let _: Option<f64> = Default_::builder().limit(1.0).build().limit();
    let _: String = label("x");
    let _: Result<Result_, wasm_bindgen::JsValue> = Result_::new(true);
}
