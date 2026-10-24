//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::dictionary_polish_mono::{
    polish::{
        Approximate, Gauge, Holder, HourStepping, Maker, MinuteStepping, Options,
        PartialBoxLikeObject, PartialOmitLikeBoxLikeObject, PluralizeTimeUnit, PluralizeUnit,
        PluralizeUnitHour, PluralizeUnitMinute, Rounder, Rounding, RoundingHour, RoundingTimeUnit,
        Stepped, TimeLikeObject, Transition,
    },
    DirectionKind, LevelReturnKind, MinuteSteppingLargestUnitKind, StepKind,
    SteppedLargestUnitKind,
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
fn literal_factories_keep_a_builder() {
    assert!(matches!(
        Transition::new_next().direction(),
        DirectionKind::Next
    ));
    let transition = Transition::builder_next()
        .direction(DirectionKind::Previous)
        .build();
    assert!(matches!(transition.direction(), DirectionKind::Previous));
}

#[wasm_bindgen_test]
fn literal_only_parameters_specialise_per_instantiation() {
    // The erased base reads `U` at its bound, every unit.
    let all = Rounding::builder()
        .smallest_unit(PluralizeUnit::Days)
        .build();
    assert!(matches!(all.smallest_unit(), Some(PluralizeUnit::Days)));

    // `Rounding<TimeUnit>` narrows its units and derefs to the base.
    let time = RoundingTimeUnit::builder()
        .smallest_unit(PluralizeTimeUnit::Minutes)
        .largest_unit(SteppedLargestUnitKind::Hour)
        .increment(5.0)
        .build();
    assert!(matches!(
        time.smallest_unit(),
        Some(PluralizeTimeUnit::Minutes)
    ));
    let base: &Rounding = &time;
    assert!(matches!(base.smallest_unit(), Some(PluralizeUnit::Minutes)));
    assert_eq!(base.increment(), Some(5.0));

    // A narrowed unit converts into every wider one.
    let hour: PluralizeTimeUnit = PluralizeUnitHour::Hour.into();
    time.set_smallest_unit(hour);
    let wide: PluralizeUnit = PluralizeTimeUnit::Hours.into();
    base.set_smallest_unit(wide);
    assert!(matches!(
        time.smallest_unit(),
        Some(PluralizeTimeUnit::Hours)
    ));

    // A parent instantiation narrows the interface's own members, and the
    // parameter alone reads as its argument.
    let minute = MinuteStepping::builder()
        .step("minute")
        .smallest_unit(PluralizeUnitMinute::Minute)
        .largest_unit(MinuteSteppingLargestUnitKind::Auto)
        .build();
    assert_eq!(minute.step().as_deref(), Some("minute"));
    let stepped: &Stepped = &minute;
    assert!(matches!(stepped.step(), Some(StepKind::Minute)));
    assert!(matches!(
        stepped.smallest_unit(),
        Some(PluralizeTimeUnit::Minute)
    ));

    // An alias of an instantiation is its specialisation.
    let hourly = HourStepping::builder()
        .smallest_unit(PluralizeUnitHour::Hours)
        .build();
    assert!(matches!(
        hourly.smallest_unit(),
        Some(PluralizeUnitHour::Hours)
    ));
}

// `Rounder` has no backing module: compile-only coverage that each use site
// takes its specialisation, the base at the bound.
#[allow(dead_code)]
fn use_sites_take_their_specialisation(rounder: &Rounder) {
    rounder.round(&Rounding::new());
    rounder.round_time(&RoundingTimeUnit::new());
    rounder.round_again(&RoundingTimeUnit::new());
    rounder.step(&HourStepping::new());
    let _: PluralizeTimeUnit = rounder.clock().now();
    // A variant is named for the base its specialisation extends.
    rounder.snap(PluralizeTimeUnit::Hours);
    rounder.snap_with_rounding(&RoundingTimeUnit::new());
    rounder.nudge(&RoundingTimeUnit::new());
    rounder.nudge_with_rounding_hour(&RoundingHour::new());
}

// Compile-only: a merged class reads its literal-only parameter at its
// bound.
#[allow(dead_code)]
fn merged_classes_erase_literal_only_parameters(gauge: &Gauge) {
    let _: LevelReturnKind = gauge.level();
}
