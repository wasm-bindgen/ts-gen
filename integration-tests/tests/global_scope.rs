#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::global_scope::{
    f, fx, xf, OnlyA, Proc, Win, CX, FX, SX, VX, XC, XF, XS, XV,
};

// The globals have no backing JavaScript, so this is compile-only coverage
// that an interface two input files declare is one type with the members
// of both, that an interface and a constructor `var` in different files
// are one class with the statics every file declares, and that a function
// doesn't take an interface's name.
#[allow(dead_code)]
fn global_scope_compiles(win: &Win, only_a: &OnlyA, process: &Proc) {
    win.a();
    win.b();
    let _: fn(&Win, &OnlyA) = f;
    let _: f64 = only_a.x();
    let _: f64 = process.pid();
}

#[allow(dead_code)]
fn constructor_vars_merge_across_files() -> Result<(), wasm_bindgen::JsValue> {
    XV::new()?.inst();
    XV::stat();
    VX::new()?.inst();
    VX::stat();
    XC::new()?.inst();
    XC::stat();
    CX::new()?.inst();
    CX::stat();
    XS::new(1.0)?.inst();
    XS::added();
    SX::new()?.inst();
    SX::added();
    let _: f64 = xf().a();
    let _: f64 = fx().a();
    let _: fn() -> XF = xf;
    let _: fn() -> FX = fx;
    Ok(())
}
