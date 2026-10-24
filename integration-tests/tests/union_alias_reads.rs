#![cfg(target_arch = "wasm32")]

// Union aliases, read and written. `ZonedLike`, `DayLike` and `Zone` have
// arms a read can tell apart, so each is one enum, which their reads return
// and their arguments take as `impl Into<Alias>`. An anonymous union of
// aliases reads as one flat enum: `ZonedLike | DayLike` has `Zoned` and
// `string` once, and its two dictionaries share the `Object` arm, which
// takes what the class and string arms don't. `Fields`, two dictionaries,
// is an argument enum, and reads as `JsValue`.
use js_sys::{JsString, Object};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::union_alias_reads::*;

fn keys(object: &Object) -> String {
    Object::keys(object).join(",").into()
}

fn show(value: RelativeToKind) -> String {
    match value {
        RelativeToKind::Zoned(zoned) => format!("zoned:{}", zoned.zone()),
        RelativeToKind::String(s) => format!("string:{s}"),
        RelativeToKind::Day(day) => format!("day:{}", day.day()),
        RelativeToKind::Object(object) => format!("object:{}", keys(&object)),
    }
}

#[wasm_bindgen_test]
fn nested_aliases_read_as_one_union() {
    let read = |kind: &str| options(kind).relative_to().map(show);
    assert_eq!(read("zoned").as_deref(), Some("zoned:UTC"));
    assert_eq!(read("day").as_deref(), Some("day:3"));
    assert_eq!(read("string").as_deref(), Some("string:2020-01-01"));
    assert_eq!(read("zonedFields").as_deref(), Some("object:zone"));
    assert_eq!(read("dayFields").as_deref(), Some("object:day"));
    assert_eq!(read("bare").as_deref(), Some("object:day"));
    assert_eq!(read("missing"), None);
}

// A union with payloads is one enum per member, so the function return has
// its own, with the same arms.
#[wasm_bindgen_test]
fn function_return_reads_the_same_arms() {
    assert!(matches!(relative_to("day"), RelativeToReturnKind::Day(d) if d.day() == 3.0));
    assert!(matches!(
        relative_to("dayFields"),
        RelativeToReturnKind::Object(o) if keys(&o) == "day"
    ));
}

#[wasm_bindgen_test]
fn alias_reads_its_own_enum() {
    assert!(matches!(options("zoned").zone(), Zone::Zoned(z) if z.zone() == "UTC"));
    assert!(matches!(options("string").zone(), Zone::String(s) if s == "2020-01-01"));
}

#[wasm_bindgen_test]
fn dictionary_arm_is_the_fallback() {
    match options("zonedFields").zoned() {
        Some(ZonedLike::ZonedFields(fields)) => assert_eq!(fields.zone(), "Europe/London"),
        _ => panic!("expected the dictionary arm"),
    }
    assert!(matches!(
        options("zoned").zoned(),
        Some(ZonedLike::Zoned(_))
    ));
    assert!(matches!(
        options("string").zoned(),
        Some(ZonedLike::String(_))
    ));
    assert!(options("day").zoned().is_none());
}

#[wasm_bindgen_test]
fn read_passes_straight_back() {
    let target = options("string");
    target.set_zone(options("zoned").zone());
    assert!(matches!(target.zone(), Zone::Zoned(z) if z.zone() == "UTC"));
    target.set_zoned(options("zonedFields").zoned().unwrap());
    assert!(matches!(target.zoned(), Some(ZonedLike::ZonedFields(_))));

    let span = Span::new(&Day::new(10.0).unwrap());
    assert_eq!(describe(span.start()), "day:10");
    span.set_relative_to("1");
    assert!(matches!(span.relative_to(), Some(DayLike::String(s)) if s == "1"));
}

#[wasm_bindgen_test]
fn dictionaries_alone_read_as_js_value_and_write_as_their_enum() {
    let fields: JsValue = options("missing").fields();
    assert_eq!(keys(fields.unchecked_ref()), "day");
    assert_eq!(describe_fields(ZonedFields::new("UTC")), "zone:UTC");
    assert_eq!(describe_fields(&DayFields::new(4.0)), "day:4");
}

// A named union alias is one binding taking `impl Into<Alias>`: a class
// instance (owned or borrowed), a string (`&str`, `String`, `JsString`) and
// a dictionary each convert to its enum and reach JS as themselves.
#[wasm_bindgen_test]
fn compare_takes_every_arm() {
    let day = Day::new(10.0).unwrap();
    let zoned = Zoned::new("UTC").unwrap();
    let fields = DayFields::new(5.0);
    assert_eq!(Day::compare(&day, "10"), 0.0);
    assert_eq!(Day::compare(&day, &zoned), 1.0);
    assert_eq!(Day::compare(zoned, fields.clone()), -1.0);
    assert_eq!(Day::compare(String::from("5"), &fields), 0.0);
    assert_eq!(Day::compare(JsString::from("9"), &day), -1.0);
    assert_eq!(Day::try_compare(&fields, &day).unwrap(), -1.0);
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

    // The optional trailing options still omit or pass.
    let options = FromOptions::builder()
        .overflow(OverflowKind::Reject)
        .build();
    let _ = Day::from_with_options("1", &options);
    assert_eq!(day.until(&fields), 0.0);
    assert_eq!(day.until_with_options("15", &options), -5.0);
}

#[wasm_bindgen_test]
fn primitive_arms_convert_from_their_rust_values() {
    let day = Day::new(10.0).unwrap();
    assert_eq!(day.shift(2.0).day(), 12.0);
    assert_eq!(day.shift("3").day(), 13.0);
    assert_eq!(day.shift(true).day(), 11.0);
}

#[wasm_bindgen_test]
fn wrappers_compose_with_literals_nulls_options_and_async() {
    let day = Day::new(10.0).unwrap();
    assert_eq!(day.step_next("1"), "next:string");
    assert_eq!(day.step_previous(&day), "previous:day");
    assert_eq!(day.pick(), "none");
    assert_eq!(day.pick_with_other(&day), "day");
    assert_eq!(day.pick_with_null(), "null");
    let _: MaybeDay = MaybeDay::from("1");
    assert_eq!(day.between(None, "1"), "undefined..string");
    assert_eq!(
        day.between(Some(DayLike::from(&day)), DayFields::new(1.0)),
        "day..fields"
    );
}

#[wasm_bindgen_test]
async fn async_wrapper_converts_before_awaiting() {
    let day = Day::new(10.0).unwrap();
    assert_eq!(day.later("1").await.unwrap(), "later:string");
    assert_eq!(day.later(&day).await.unwrap(), "later:day");
}

#[wasm_bindgen_test]
fn free_functions_and_dictionary_fields_take_the_alias() {
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

    // An anonymous union still binds per arm.
    assert_eq!(either_with_str("x"), "string");
}
