#![cfg(target_arch = "wasm32")]

use js_sys::{BigInt, JsString, Symbol};

use ts_gen_integration_tests::primitive_unions_mono::{
    has_key, has_key_explicit, lookup, send, send_with_number_or_string, set_any, set_flag,
    set_keyed, set_level, set_maybe_with_value, set_nullable, set_value, Settings, Store, Target,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that each primitive-union bound accepts the Rust representations
// of all its member categories.
#[allow(dead_code)]
fn primitive_union_signatures_compile(js_string: &JsString, symbol: &Symbol, big: &BigInt) {
    set_value("borrowed");
    set_value(String::from("owned"));
    set_value(js_string);
    set_value(1u8);
    set_value(1i32);
    set_value(1.5f64);

    set_flag(true);
    set_flag(1i64);
    set_flag(big);

    set_any(1u128);
    set_any(false);
    set_any(2u32);
    set_any("s");
    set_any(symbol);

    set_keyed("key");
    set_keyed(symbol.clone());

    set_level(1i32);
    set_level("auto");
    set_nullable(3.0f32);
    set_maybe_with_value("value");
    lookup(7usize);

    let _: bool = has_key("key");
    let _: bool = has_key(0u32);
    let _: bool = has_key(symbol);
    let _: bool = has_key_explicit(js_string.clone());

    let target = Target::new("name");
    send(&target);
    send_with_number_or_string(42i32);

    let store = Store::new("key").unwrap();
    let _: String = store.get(3i32);
    let _: Store = Store::of(true);

    let _: Settings = Settings::new_low(1i32, &target);
    let _: Settings = Settings::new(2.0, "id", &target);
    let _: Settings = Settings::builder_high_with_boolean_or_string("id", false)
        .label(true)
        .build();
    let settings = Settings::new_with_boolean_or_string(2.0, 0u8, "from");
    settings.set_level("low");
    settings.set_level(3i32);
}
