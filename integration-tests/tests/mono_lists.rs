//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, JsString, Map, Nullable, Number, Set};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::mono_lists::*;

#[wasm_bindgen_test]
fn string_lists_accept_any_string_element() {
    assert_eq!(tags(&["a".to_string()]), "Array(string:a)");
    assert_eq!(tags(&["a", "b"]), "Array(string:a,string:b)");
    assert_eq!(tags(&[JsString::from("j")]), "Array(string:j)");
    assert_eq!(tagged(&["t"]), "Array(string:t)");
    let handle: Array<JsString> = Array::new_typed();
    handle.push(&JsString::from("h"));
    assert_eq!(tags_like(&handle), "Array(string:h)");
    let empty: &[&str] = &[];
    assert_eq!(tags(empty), "Array()");
}

#[wasm_bindgen_test]
fn variadics_spread_any_list() {
    assert_eq!(join("-", &["a", "b"]), "a-b");
    assert_eq!(join("-", &[JsString::from("c")]), "c");
    assert_eq!(sum(&[1.0, 2.5]), 3.5);
    let bag: Bag<f64> = bag_of(&[1.5, 2.0]);
    assert_eq!((bag.size(), bag.first()), (2.0, 1.5));
    let bag: Bag<JsString> = bag_of(&[JsString::from("a")]);
    assert_eq!(bag.first(), "a");
    assert_eq!(join_tags(&["x", "y"]), "x,y");
    let labels = Labels::new().unwrap();
    assert_eq!(labels.add(&["a"]), "Array(object:a)");
    assert_eq!(labels.add_2(&["b", "c"]), "Array(string:b,string:c)");
}

#[wasm_bindgen_test]
fn container_string_leaves_accept_any_string_shape() {
    let row: Array<JsString> = Array::new_typed();
    row.push(&JsString::from("r"));
    assert_eq!(rows(&[row]), "Array(string:r)");
    let set: Set<JsString> = Set::new_typed();
    set.add(&JsString::from("s"));
    assert_eq!(unique(&set), "Array(string:s)");
    let map: Map<JsString, Number> = Map::new_typed();
    map.set(&JsString::from("k"), &Number::from(2.0));
    assert_eq!(lookup(&map, "k"), 2.0);
}

#[cfg(js_sys_unstable_apis)]
#[wasm_bindgen_test]
fn string_list_handles_hold_rust_strings() {
    let handle: Array<String> = ["h", "i"].into_iter().map(String::from).collect();
    assert_eq!(tags_like(&handle), "Array(string:h,string:i)");
    assert_eq!(rows(&[handle]), "Array(string:h,string:i)");
    let set: Set<String> = Set::new_typed();
    set.add(&"s".to_string());
    assert_eq!(unique(&set), "Array(string:s)");
}

#[wasm_bindgen_test]
fn string_list_returns_have_a_handle_variant() {
    assert_eq!(get_tags(), ["a", "b"]);
    let handle: Array<JsString> = get_tags_js_string();
    assert_eq!(handle.length(), 2);
    assert_eq!(get_maybe_tags(), None);
    assert!(get_maybe_tags_js_string().is_none());
}

#[wasm_bindgen_test]
async fn awaited_string_lists_have_a_handle_variant() {
    assert_eq!(fetch_tags().await.unwrap(), ["x"]);
    let handle: Array<JsString> = fetch_tags_js_string().await.unwrap();
    assert_eq!(handle.length(), 1);
    let handle: Array<JsString> = fetch_tags_promise().await.unwrap();
    assert_eq!(handle.length(), 1);
    let maybe: Option<Vec<String>> = fetch_maybe_tags().await.unwrap();
    assert_eq!(maybe, Some(vec!["y".to_string(), "z".to_string()]));
    let maybe: Option<Array<JsString>> = fetch_maybe_tags_js_string().await.unwrap();
    assert_eq!(maybe.map(|a| a.length()), Some(2));
}

#[wasm_bindgen_test]
fn boolean_lists_cross_as_bool() {
    assert_eq!(flags(&[true, false]), "Array(boolean:true,boolean:false)");
    assert_eq!(get_flags(), [true, false]);
    assert!(matches!(get_value(), GetValueKind::VecOfBool(v) if v == [true, false]));
}

#[wasm_bindgen_test]
fn properties_and_builders_take_string_lists() {
    let labels = Labels::new().unwrap();
    labels.set_values(&["a", "b"]);
    assert_eq!(labels.values(), ["a", "b"]);
    assert_eq!(labels.values_js_string().length(), 2);
    labels.set_flags(&[true]);
    assert_eq!(labels.flags(), [true]);

    let options = LabelOptions::builder()
        .values(&[JsString::from("x")])
        .flags(&[false])
        .build();
    assert_eq!(
        describe_options(&options),
        "Array(string:x) Array(boolean:false)"
    );
}

#[wasm_bindgen_test]
fn variadic_constructors_spread_the_list() {
    assert_eq!(Total::new(&[1.0, 2.5]).unwrap().value(), 3.5);
}

#[wasm_bindgen_test]
fn union_lists_keep_the_enum_as_their_element() {
    let read = marks();
    assert!(matches!(&read[0], MarkOrStr::Mark(m) if m.text() == "m"));
    assert!(matches!(&read[1], MarkOrStr::String(s) if s == "s"));
    assert_eq!(describe_marks(&read), "Array(object:Mark(m),string:s)");

    let read = maybe_marks();
    assert!(matches!(&read[0], Some(MarkOrStr::Mark(m)) if m.text() == "m"));
    assert!(read[1].is_none());
    assert!(matches!(&read[2], Some(MarkOrStr::String(s)) if s == "s"));

    let mark = Mark::new("w").unwrap();
    assert_eq!(
        describe_maybe_marks(&[
            Nullable::Value(MarkOrStr::Mark(mark.clone())),
            Nullable::Null,
            Nullable::Value(MarkOrStr::String("t".into())),
        ]),
        "Array(object:Mark(w),object:null,string:t)"
    );

    let options = MarkOptions::builder()
        .marks(&[MarkOrStr::String("o".into()), MarkOrStr::Mark(mark)])
        .build();
    assert_eq!(
        describe_mark_options(&options),
        "Array(string:o,object:Mark(w))"
    );
    assert_eq!(options.marks().map(|marks| marks.len()), Some(2));

    // A list of a union is an arm of a union.
    let Some(MarkRouteToKind::VecOfJsValue(to)) = route().to() else {
        panic!("expected the list arm");
    };
    assert!(matches!(&to[0], MarkOrStr::Mark(m) if m.text() == "m"));
    let route = MarkRoute::builder().to_with_slice(&to).build();
    assert_eq!(describe_route(&route), "Array(object:Mark(m),string:s)");
}

#[wasm_bindgen_test]
fn literal_lists_keep_the_enum_as_their_element() {
    assert_eq!(modes(&[Mode::Slow, Mode::Fast]), [Mode::Slow, Mode::Fast]);
    assert_eq!(
        levels(&[LevelsReturn::N2, LevelsReturn::N1]),
        [LevelsReturn::N2, LevelsReturn::N1]
    );
    assert_eq!(
        maybe_kinds(&[
            Nullable::Value(MaybeKindsReturn::Y),
            Nullable::Null,
            Nullable::Value(MaybeKindsReturn::X),
        ]),
        [Some(MaybeKindsReturn::Y), None, Some(MaybeKindsReturn::X)]
    );

    // A getter reads what its setter wrote.
    let options = ModeOptions::builder()
        .modes(&[Mode::Fast])
        .kinds(&[ModeOptionsKinds::Y, ModeOptionsKinds::X])
        .levels(&[ModeOptionsLevels::N2])
        .build();
    assert_eq!(
        describe_mode_options(&options),
        "Array(string:fast) Array(string:y,string:x) Array(number:2)"
    );
    assert_eq!(options.modes(), Some(vec![Mode::Fast]));
    assert_eq!(
        options.kinds(),
        Some(vec![ModeOptionsKinds::Y, ModeOptionsKinds::X])
    );
    options.set_levels(&options.levels().unwrap());
    assert_eq!(options.levels(), Some(vec![ModeOptionsLevels::N2]));
}
