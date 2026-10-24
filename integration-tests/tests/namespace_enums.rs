#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use ts_gen_integration_tests::namespace_enums::{
    calendar, clock, timing, DurationUnit, Precision, TimingDurationUnit,
};

// Same-named enums at the top level and inside a namespace only clash when
// wasm-bindgen processes the linked module, so this has to run, not just
// compile.
#[wasm_bindgen_test]
fn namespace_enums_coexist() {
    assert_eq!(
        wasm_bindgen::JsValue::from(DurationUnit::Minute).as_string(),
        Some("minute".to_string())
    );
    assert_eq!(
        wasm_bindgen::JsValue::from(timing::DurationUnit::Hour).as_string(),
        Some("hour".to_string())
    );
    assert_eq!(
        wasm_bindgen::JsValue::from(TimingDurationUnit::Week).as_string(),
        Some("week".to_string())
    );
    assert_ne!(Precision::High as u32, timing::Precision::Coarse as u32);
}

// Each synthesised enum is reached at the module of its first use, and
// setters and getters on every owner share it. Dictionaries need no backing
// module, so this runs.
#[wasm_bindgen_test]
fn synthesised_enums_round_trip_from_their_modules() {
    let to_string = clock::ToStringOptions::builder()
        .offset(clock::OffsetKind::Never)
        .unit(clock::UnitKind::Week)
        .build();
    assert!(matches!(to_string.offset(), Some(clock::OffsetKind::Never)));

    let from = clock::FromOptions::new();
    from.set_offset(clock::OffsetKind2::Prefer);
    from.set_overflow(clock::OverflowKind::Reject);
    assert!(matches!(from.offset(), Some(clock::OffsetKind2::Prefer)));

    let zone = clock::zone::Options::builder()
        .overflow(from.overflow().unwrap())
        .mode(clock::zone::ModeKind::Slow)
        .build();
    assert!(matches!(zone.overflow(), Some(clock::OverflowKind::Reject)));
    assert!(matches!(zone.mode(), Some(clock::zone::ModeKind::Slow)));

    let calendar = calendar::Options::new();
    calendar.set_unit(to_string.unit().unwrap());
    assert!(matches!(calendar.unit(), Some(clock::UnitKind::Week)));
}

// A literal set used under two member names is still one enum, named from
// its first use. A union with a payload belongs to its owner and member, so
// equal unions on different owners are distinct types.
#[wasm_bindgen_test]
fn literal_sets_share_and_other_unions_stay_per_member() {
    let to_string = clock::ToStringOptions::new();
    to_string.set_style(clock::StyleKind::Long);
    let from = clock::FromOptions::builder()
        .display(to_string.style().unwrap())
        .build();
    assert!(matches!(from.display(), Some(clock::StyleKind::Long)));

    to_string.set_precision(clock::PrecisionKind::Auto);
    from.set_precision_with_f64(3.0);
    assert!(matches!(
        to_string.precision(),
        Some(clock::PrecisionKind::Auto)
    ));
    assert!(matches!(
        from.precision(),
        Some(clock::PrecisionKind2::F64(p)) if p == 3.0
    ));

    let zone = clock::zone::Options::builder().limit_with_f64(10.0).build();
    assert!(matches!(
        zone.limit(),
        Some(clock::zone::LimitKind::F64(l)) if l == 10.0
    ));
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
