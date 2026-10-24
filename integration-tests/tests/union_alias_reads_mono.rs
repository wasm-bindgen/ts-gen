//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

// `union_alias_reads.rs` against the per-monomorphization bindings, which
// read union aliases the same way.
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
fn lone_dictionary_keeps_its_type() {
    match options("zonedFields").zoned() {
        Some(ZonedKind::ZonedFields(fields)) => assert_eq!(fields.zone(), "Europe/London"),
        _ => panic!("expected the dictionary arm"),
    }
    assert!(matches!(options("zoned").zone(), ZoneKind::Zoned(_)));
}
