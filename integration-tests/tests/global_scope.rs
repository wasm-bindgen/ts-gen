#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::global_scope::{f, OnlyA, Proc, Win};

// The globals have no backing JavaScript, so this is compile-only coverage
// that an interface two input files declare is one type with the members
// of both.
#[allow(dead_code)]
fn global_scope_compiles(win: &Win, only_a: &OnlyA, process: &Proc) {
    win.a();
    win.b();
    let _: fn(&Win, &OnlyA) = f;
    let _: f64 = only_a.x();
    let _: f64 = process.pid();
}
