#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::global_scope_export::{f, OnlyA, Proc, Win};

// Compile-only: with `--export b.d.ts`, `Win` still has a.d.ts's members
// and `OnlyA`, which only a.d.ts declares, is emitted because `f` takes it.
#[allow(dead_code)]
fn global_scope_export_compiles(win: &Win, only_a: &OnlyA, process: &Proc) {
    win.a();
    win.b();
    let _: fn(&Win, &OnlyA) = f;
    let _: OnlyA = OnlyA::new(1.0);
    let _: f64 = only_a.x();
    let _: f64 = process.pid();
}
