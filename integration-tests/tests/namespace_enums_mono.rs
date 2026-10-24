//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::Intl::RoundingMode;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::namespace_enums_mono::{calendar, clock};

// `namespace_enums.rs` against the per-monomorphization bindings, which
// place and name synthesised enums the same way.
#[wasm_bindgen_test]
fn synthesised_enums_round_trip_from_their_modules() {
    let to_string = clock::ToStringOptions::builder()
        .rounding_mode(RoundingMode::HalfEven)
        .unit(clock::UnitKind::Week)
        .style(clock::StyleKind::Short)
        .precision(clock::PrecisionKind::Auto)
        .build();
    assert_eq!(to_string.rounding_mode(), Some(RoundingMode::HalfEven));

    let from = clock::FromOptions::builder()
        .display(to_string.style().unwrap())
        .overflow(clock::OverflowKind::Constrain)
        .build();
    assert!(matches!(from.display(), Some(clock::StyleKind::Short)));

    let zone = clock::zone::Options::builder()
        .overflow(from.overflow().unwrap())
        .limit_with_f64(10.0)
        .build();
    assert!(matches!(
        zone.overflow(),
        Some(clock::OverflowKind::Constrain)
    ));
    assert!(matches!(
        zone.limit(),
        Some(clock::zone::LimitKind::F64(l)) if l == 10.0
    ));

    let calendar = calendar::Options::new();
    calendar.set_unit(to_string.unit().unwrap());
    assert!(matches!(calendar.unit(), Some(clock::UnitKind::Week)));
}

// `"foo-bar" | "foo_bar"` and `"foo_bar" | "foo-bar"` are one enum with one
// numbering, so each literal factory sends the literal it is named for.
#[wasm_bindgen_test]
fn literal_sets_in_either_order_send_their_own_literal() {
    // The getter reads back the JS string, and the variant converts to it.
    let sent = |kind: calendar::SepKind| wasm_bindgen::JsValue::from(kind).as_string().unwrap();
    let reversed = calendar::ReversedSeparator::new_foo_bar();
    assert_eq!(sent(reversed.sep()), "foo_bar");
    let reversed = calendar::ReversedSeparator::new_foo_bar_2();
    assert_eq!(sent(reversed.sep()), "foo-bar");

    let separator = calendar::Separator::builder().sep(reversed.sep()).build();
    assert_eq!(sent(separator.sep().unwrap()), "foo-bar");
}
