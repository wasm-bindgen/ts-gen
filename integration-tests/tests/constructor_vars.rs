#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::constructor_vars::{m, n, q, r, Multi, W};

// Compile-only: each constructor var and its interface are one class in the
// var's container, with the constructor and statics its type gives.
#[allow(dead_code)]
fn constructor_vars_compile() {
    let foo: n::Foo = n::Foo::new().unwrap();
    foo.go();
    let _: n::Foo = n::Foo::create();
    n::Before::new(1.0).unwrap().b();
    r::Q::new().unwrap().q();
    r::P::new().unwrap().p();
    let w: W = W::new().unwrap();
    w.w();
    let _: W = W::make();
    let _: m::W = m::W::new().unwrap();
    let _: W = m::W::make();
    Multi::new().unwrap().m();
    let _: Multi = Multi::new_with_n(1.0).unwrap();
    let _: Multi = Multi::new_with_s_and_n("s", 1.0).unwrap();
    q::Qual::new().unwrap().q();
    let _: q::Qual = q::Qual::make();
}
