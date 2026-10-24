#![cfg(target_arch = "wasm32")]

use js_sys::{BigInt, JsString, Symbol};

use ts_gen_integration_tests::primitive_unions_mono::{
    has_key, has_key_explicit_with_symbol, has_key_with_f64, lookup, lookup_with_f64, send,
    send_with_f64, send_with_str, set_any, set_any_with_bool, set_any_with_f64, set_any_with_str,
    set_any_with_symbol, set_flag, set_flag_with_big_int, set_keyed, set_keyed_with_symbol,
    set_level, set_level_with_str, set_maybe, set_maybe_with_f64, set_maybe_with_str, set_nullable,
    set_nullable_with_f64, set_nullable_with_null, set_value, set_value_with_f64, LevelKind,
    Settings, Store, Target,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that each primitive union expands per member, with strings taking
// any `JsStringLike`.
#[allow(dead_code)]
fn primitive_union_signatures_compile(js_string: &JsString, symbol: &Symbol, big: &BigInt) {
    set_value("borrowed");
    set_value(String::from("owned"));
    set_value(js_string);
    set_value_with_f64(1.5);

    set_flag(true);
    set_flag_with_big_int(big);

    set_any(big);
    set_any_with_bool(false);
    set_any_with_f64(2.0);
    set_any_with_str("s");
    set_any_with_symbol(symbol);

    set_keyed("key");
    set_keyed_with_symbol(symbol);

    set_level(1.0);
    set_level_with_str("auto");
    // `string | number | null`: `null` is its own arm.
    set_nullable("s");
    set_nullable(js_string);
    set_nullable_with_f64(3.0);
    set_nullable_with_null();
    // `value?: string | number`: omission is the shorter overload.
    set_maybe();
    set_maybe_with_str("value");
    set_maybe_with_f64(1.0);
    lookup("id");
    lookup_with_f64(7.0);

    let _: bool = has_key("key");
    let _: bool = has_key_with_f64(0.0);
    let _: bool = has_key_explicit_with_symbol(symbol);

    let target = Target::new("name");
    send(&target);
    send_with_str("to");
    send_with_f64(42.0);

    let store = Store::new("key").unwrap();
    let _: String = store.get_with_f64(3.0);
    let _: Store = Store::of(true);

    let _: Settings = Settings::new_low("id", &target);
    let _: Settings = Settings::new_with_f64_and_bool(2.0, 1.0, false);
    let _: Settings = Settings::builder_high_with_str_and_str("id", js_string)
        .label_with_bool(true)
        .build();
    let settings = Settings::new(2.0, "id", &target);
    settings.set_level(LevelKind::Low);
    settings.set_level_with_f64(3.0);
    settings.set_id_with_f64(4.0);
}
