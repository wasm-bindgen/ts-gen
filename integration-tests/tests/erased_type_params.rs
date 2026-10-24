#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::erased_type_params::{Nested, NestedMapped, V};

// Compile-only: the fixture has no backing JavaScript module. An argument
// given for an erased parameter is dropped inside `keyof`, an indexed
// access, a mapped type and an object literal too, so `Slot<"a", V>`
// binds `V` rather than `"a"` and each field takes `V`'s type.
#[allow(dead_code)]
fn erased_type_params_nested_compile(nested: &Nested) {
    let _: f64 = nested.indexed();
    let _: V = nested.keys();
    nested.set_keys(V::X);
    let mapped: NestedMapped = nested.mapped();
    let _: bool = mapped.x();
    let _: bool = mapped.y();
    let _: Vec<String> = nested.literal();
    let _ = Nested::new(
        1.0,
        V::X,
        &NestedMapped::new(true, false),
        &["s".to_string()],
    );
}
