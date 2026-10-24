//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use ts_gen_integration_tests::setter_matches_getter_mono::{
    DigitsKind, DigitsOptions, FractionalSecondDigitsKind, ModeKind, RequiredDigits, RoundingModeKind, Toggle,
};

fn is_f64(kind: Option<FractionalSecondDigitsKind>, expected: f64) -> bool {
    matches!(kind, Some(FractionalSecondDigitsKind::F64(n)) if n == expected)
}

// Dictionaries need no backing module, so this writes the mixed
// string/number enum through wasm-bindgen and reads it back.
#[wasm_bindgen_test]
fn mixed_literal_enum_round_trips() {
    let options = DigitsOptions::builder()
        .fractional_second_digits(FractionalSecondDigitsKind::F64(3.0))
        .build();
    assert!(is_f64(options.fractional_second_digits(), 3.0));

    options.set_fractional_second_digits(FractionalSecondDigitsKind::Auto);
    assert!(matches!(
        options.fractional_second_digits(),
        Some(FractionalSecondDigitsKind::Auto)
    ));

    options.set_fractional_second_digits_with_f64(9.0);
    let read = options.fractional_second_digits().unwrap();
    let copy = DigitsOptions::new();
    copy.set_fractional_second_digits(read);
    assert!(is_f64(copy.fractional_second_digits(), 9.0));

    assert!(matches!(
        RequiredDigits::new_auto().digits(),
        DigitsKind::Auto
    ));
    assert!(matches!(RequiredDigits::new_2().digits(), DigitsKind::F64(n) if n == 2.0));

    let toggle = Toggle::new_true();
    assert!(matches!(toggle.mode(), ModeKind::Bool(true)));
    toggle.set_mode(ModeKind::Y);
    toggle.set_mode(toggle.mode());
    assert!(matches!(toggle.mode(), ModeKind::Y));
}

// A synthesised enum is a value like any declared one: a literal-only read
// compares, prints and copies, and one with a payload clones and prints.
#[wasm_bindgen_test]
fn synthesised_enum_derives() {
    let options = DigitsOptions::builder()
        .rounding_mode(RoundingModeKind::Floor)
        .build();
    let read = options.rounding_mode();
    assert_eq!(read, Some(RoundingModeKind::Floor));
    assert_ne!(read, Some(RoundingModeKind::Ceil));
    assert_eq!(format!("{read:?}"), "Some(Floor)");
    let mode = read.unwrap();
    options.set_rounding_mode(mode);
    assert_eq!(options.rounding_mode(), Some(mode));

    let toggle = Toggle::new_x();
    let mode = toggle.mode();
    toggle.set_mode(mode.clone());
    assert_eq!(format!("{mode:?}"), "X");
    assert!(matches!(toggle.mode(), ModeKind::X));
}
