#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::mixed_literal_factory::{
    BoolOrStrings, Level, LevelKind, Mixed, ModeKind, ModeKind2, ModeKind3, ModeKind5,
    NumberOrStrings, Open, Optional,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that setters taking the getter's enum lower to Rust wasm-bindgen
// accepts, and round-trip the getter's value.
#[allow(dead_code)]
fn mixed_literal_factory_compile() {
    let bool_or_strings = BoolOrStrings::new_y();
    bool_or_strings.set_mode_with_bool(true);
    bool_or_strings.set_mode(ModeKind::X);
    bool_or_strings.set_mode(bool_or_strings.mode());

    let number_or_strings = NumberOrStrings::new_x();
    number_or_strings.set_mode_with_f64(0.0);
    number_or_strings.set_mode(ModeKind2::Y);

    let mixed = Mixed::new_2();
    mixed.set_mode(ModeKind3::X);
    mixed.set_mode_with_f64(1.0);
    mixed.set_mode_with_bool(false);
    mixed.set_mode(mixed.mode());

    let level = Level::new_0();
    level.set_level(LevelKind::N2);

    let _: Open = Open::new_x();
    let _: Open = Open::new("z");

    let optional = Optional::builder().mode(ModeKind5::X).build();
    if let Some(mode) = optional.mode() {
        optional.set_mode(mode);
    }
}
