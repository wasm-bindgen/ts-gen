#![cfg(target_arch = "wasm32")]

// Reads resolve union aliases to their members: `ZonedLike | DayLike` is one
// enum with `Zoned` and `string` once, and the two dictionaries share the
// `Object` arm, which takes what the class and string arms don't.
use js_sys::Object;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::union_alias_reads::*;

fn keys(object: &Object) -> String {
    Object::keys(object).join(",").into()
}

fn describe(value: RelativeToKind) -> String {
    match value {
        RelativeToKind::Zoned(zoned) => format!("zoned:{}", zoned.zone()),
        RelativeToKind::String(s) => format!("string:{s}"),
        RelativeToKind::Day(day) => format!("day:{}", day.day()),
        RelativeToKind::Object(object) => format!("object:{}", keys(&object)),
    }
}

#[wasm_bindgen_test]
fn nested_aliases_read_as_one_union() {
    let read = |kind: &str| options(kind).relative_to().map(describe);
    assert_eq!(read("zoned").as_deref(), Some("zoned:UTC"));
    assert_eq!(read("day").as_deref(), Some("day:3"));
    assert_eq!(read("string").as_deref(), Some("string:2020-01-01"));
    assert_eq!(read("zonedFields").as_deref(), Some("object:zone"));
    assert_eq!(read("dayFields").as_deref(), Some("object:day"));
    assert_eq!(read("bare").as_deref(), Some("object:day"));
    assert_eq!(read("missing"), None);
}

#[wasm_bindgen_test]
fn function_return_shares_the_enum() {
    assert_eq!(describe(relative_to("day")), "day:3");
    assert_eq!(describe(relative_to("dayFields")), "object:day");
}

#[wasm_bindgen_test]
fn alias_read_whole() {
    assert!(matches!(options("zoned").zone(), ZoneKind::Zoned(z) if z.zone() == "UTC"));
    assert!(matches!(options("string").zone(), ZoneKind::String(s) if s == "2020-01-01"));
}

#[wasm_bindgen_test]
fn lone_dictionary_keeps_its_type() {
    match options("zonedFields").zoned() {
        Some(ZonedKind::ZonedFields(fields)) => assert_eq!(fields.zone(), "Europe/London"),
        _ => panic!("expected the dictionary arm"),
    }
    assert!(matches!(
        options("zoned").zoned(),
        Some(ZonedKind::Zoned(_))
    ));
    assert!(matches!(
        options("string").zoned(),
        Some(ZonedKind::String(_))
    ));
    assert!(options("day").zoned().is_none());
}

#[wasm_bindgen_test]
fn dictionaries_alone_read_as_js_value() {
    let fields: JsValue = options("missing").fields();
    assert_eq!(keys(fields.unchecked_ref()), "day");
}
