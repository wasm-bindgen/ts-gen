//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

// `union_alias_reads.rs` against the per-monomorphization bindings. A
// union alias names a trait each arm's argument type implements, so an
// argument taking `impl DayLike` crosses as the arm itself, and its enum is
// `DayLikeKind`, which reads return and which implements the trait too.
use js_sys::JsString;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::union_alias_reads_mono::*;

#[wasm_bindgen_test]
fn nested_aliases_read_as_one_union() {
    assert!(matches!(
        options("zoned").relative_to(),
        Some(RelativeToKind::Zoned(z)) if z.zone() == "UTC"
    ));
    assert!(matches!(
        options("day").relative_to(),
        Some(RelativeToKind::Day(d)) if d.day() == 3.0
    ));
    assert!(matches!(
        options("string").relative_to(),
        Some(RelativeToKind::String(s)) if s == "2020-01-01"
    ));
    assert!(matches!(
        options("dayFields").relative_to(),
        Some(RelativeToKind::Object(_))
    ));
    assert!(options("missing").relative_to().is_none());
    assert!(matches!(
        relative_to("zonedFields"),
        RelativeToKind::Object(_)
    ));
}

#[wasm_bindgen_test]
fn alias_reads_its_kind_enum() {
    match options("zonedFields").zoned() {
        Some(ZonedLikeKind::ZonedFields(fields)) => assert_eq!(fields.zone(), "Europe/London"),
        _ => panic!("expected the dictionary arm"),
    }
    assert!(matches!(options("zoned").zone(), ZoneKind::Zoned(_)));
    assert!(matches!(options("string").zone(), ZoneKind::String(s) if s == "2020-01-01"));
}

#[wasm_bindgen_test]
fn read_passes_straight_back() {
    // `RelativeOptions` is only ever read, so per-mono it has no setters;
    // `Span` is written too, and a read of it passes back to its writes.
    let span = Span::new(&Day::new(10.0).unwrap());
    assert_eq!(describe(span.start()), "day:10");
    span.set_relative_to(span.start());
    assert!(matches!(span.relative_to(), Some(DayLikeKind::Day(d)) if d.day() == 10.0));
    span.set_relative_to("1");
    assert!(matches!(span.relative_to(), Some(DayLikeKind::String(s)) if s == "1"));
    assert!(matches!(options("zoned").zone(), ZoneKind::Zoned(z) if z.zone() == "UTC"));
}

#[wasm_bindgen_test]
fn dictionaries_alone_write_through_the_trait_or_the_enum() {
    assert_eq!(describe_fields(&ZonedFields::new("UTC")), "zone:UTC");
    assert_eq!(describe_fields(DayFields::new(4.0)), "day:4");
    assert_eq!(
        describe_fields(FieldsKind::DayFields(DayFields::new(5.0))),
        "day:5"
    );
}

// Each arm's own argument type: a class instance (owned or borrowed), a
// string (`&str`, `String`, `&String`, `JsString`) and a dictionary.
#[wasm_bindgen_test]
fn compare_takes_every_arm() {
    let day = Day::new(10.0).unwrap();
    let zoned = Zoned::new("UTC").unwrap();
    let fields = DayFields::new(5.0);
    assert_eq!(Day::compare(&day, "10"), 0.0);
    assert_eq!(Day::compare(&day, &zoned), 1.0);
    assert_eq!(Day::compare(zoned, fields.clone()), -1.0);
    assert_eq!(Day::compare(String::from("5"), &fields), 0.0);
    assert_eq!(Day::compare(&String::from("5"), &fields), 0.0);
    assert_eq!(Day::compare(JsString::from("9"), &day), -1.0);
    assert_eq!(Day::try_compare(&fields, &day).unwrap(), -1.0);
    assert_eq!(Day::compare(DayLikeKind::from("10"), &day), 0.0);
}

#[wasm_bindgen_test]
fn from_and_equals_take_every_arm() {
    let day = Day::new(10.0).unwrap();
    let fields = DayFields::new(10.0);
    let from_day = Day::from(&day);
    let from_str = Day::from("10");
    let from_fields = Day::from(&fields);
    for d in [&from_day, &from_str, &from_fields] {
        assert_eq!(d.day(), 10.0);
    }
    assert!(day.equals(&from_day));
    assert!(day.equals("10"));
    assert!(day.equals(&fields));
    assert!(!day.equals(&Zoned::new("UTC").unwrap()));
    assert!(day.try_equals(String::from("10")).unwrap());

    let options = FromOptions::builder()
        .overflow(OverflowKind::Reject)
        .build();
    let _ = Day::from_with_options("1", &options);
    assert_eq!(day.until(&fields), 0.0);
    assert_eq!(day.until_with_options("15", &options), -5.0);
}

#[wasm_bindgen_test]
fn primitive_arms_take_their_rust_values() {
    let day = Day::new(10.0).unwrap();
    assert_eq!(day.shift(2.0).day(), 12.0);
    assert_eq!(day.shift("3").day(), 13.0);
    assert_eq!(day.shift(true).day(), 11.0);
    assert_eq!(day.shift(OffsetKind::F64(4.0)).day(), 14.0);
}

#[wasm_bindgen_test]
fn compose_with_literals_nulls_options_and_async() {
    let day = Day::new(10.0).unwrap();
    assert_eq!(day.step_next("1"), "next:string");
    assert_eq!(day.step_previous(&day), "previous:day");
    assert_eq!(day.pick(), "none");
    assert_eq!(day.pick_with_other(&day), "day");
    assert_eq!(day.pick_with_null(), "null");
    // `undefined` that can't be omitted takes the enum in its `Option`.
    assert_eq!(day.between(None, "1"), "undefined..string");
    assert_eq!(
        day.between(Some(DayLikeKind::from(&day)), DayFields::new(1.0)),
        "day..fields"
    );
}

#[wasm_bindgen_test]
async fn async_binding_takes_the_trait() {
    let day = Day::new(10.0).unwrap();
    assert_eq!(day.later("1").await.unwrap(), "later:string");
    assert_eq!(day.later(&day).await.unwrap(), "later:day");
}

#[wasm_bindgen_test]
fn free_functions_and_dictionary_fields_take_the_trait() {
    let day = Day::new(10.0).unwrap();
    assert_eq!(describe(&day), "day:10");
    assert_eq!(describe("2"), "string:2");
    assert_eq!(describe(DayFields::new(3.0)), "fields:3");
    assert_eq!(describe(&Zoned::new("UTC").unwrap()), "zoned:0");

    let span = Span::new("1");
    assert_eq!(describe_span(&span), "string->none");
    let span = Span::builder(&day).relative_to(DayFields::new(1.0)).build();
    assert_eq!(describe_span(&span), "day->fields");
    span.set_relative_to("1");
    assert_eq!(describe_span(&span), "day->string");

    assert_eq!(either_with_str("x"), "string");
}
