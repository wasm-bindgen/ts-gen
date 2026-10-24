//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::dictionary_polish_mono::{
    polish::{
        Holder, Options, PartialBoxLikeObject, PartialOmitLikeBoxLikeObject, PluralizeTimeUnit,
        TimeLikeObject, Transition,
    },
    DirectionKind,
};

// Dictionaries need no backing module, so these run.
#[wasm_bindgen_test]
fn short_and_deduplicated_instantiations() {
    let options = Options::default();
    options.set_box(&PartialBoxLikeObject::builder().width(2.0).build());
    assert_eq!(options.box_().unwrap().width(), Some(2.0));
    assert_eq!(options.box_().unwrap().height(), None);

    // `PartialShapeLike<TimeLikeObject>` is `TimeLikeObject` itself.
    let time: TimeLikeObject = TimeLikeObject::builder().hour(3.0).build();
    options.set_time(&time);
    assert_eq!(options.time().unwrap().hour(), Some(3.0));

    // `keyof Omit<…>` maps to its own dictionary, without `id`.
    options.set_omitted(&PartialOmitLikeBoxLikeObject::builder().height(4.0).build());
    assert_eq!(options.omitted().unwrap().height(), Some(4.0));

    options.set_unit(PluralizeTimeUnit::Minutes);
    assert!(matches!(options.unit(), Some(PluralizeTimeUnit::Minutes)));

    let holder: Holder<JsString> = Default::default();
    assert_eq!(holder.value(), None);
}

#[wasm_bindgen_test]
fn literal_choice_is_a_factory_argument() {
    // Per-mono a same-shape literal choice is the factory's argument, with
    // no `new_<literal>` / `builder_<literal>` pair.
    let transition = Transition::new(DirectionKind::Next);
    assert!(matches!(transition.direction(), DirectionKind::Next));
    transition.set_direction(DirectionKind::Previous);
    assert!(matches!(transition.direction(), DirectionKind::Previous));
}
