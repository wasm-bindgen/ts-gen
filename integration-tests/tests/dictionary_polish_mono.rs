//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::dictionary_polish_mono::{
    polish::{
        spans::{self, HourSpan, Span},
        steps::{self, DaySpan, DaySpan2},
        Approximate, Bounded, DayOrHour, DayOrHourMaximumUnitKind, DayOrTimeUnit, Gauge, Holder,
        Hour, HourLargestUnitKind, HourLargestUnitKind2, HourSpanning, HourStepping, Maker, Minute,
        MinuteLargestUnitKind, MinuteStepping, Options, PartialBoxLikeObject,
        PartialOmitLikeBoxLikeObject, PickedRounding, PluralizeTimeUnit, PluralizeUnit,
        PluralizeUnitDayOrHour, PluralizeUnitHour, PluralizeUnitMinute, Rounder, Rounding,
        RoundingU, Stepped, TimeLikeObject, TimePicking, TimeUnit, TimeUnitLargestUnitKind,
        Transition,
    },
    DirectionKind, LevelReturnKind,
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

// Generic over any instantiation, through the trait bounding `U`.
fn smallest<U: RoundingU>(rounding: &Rounding<U>) -> Option<U::PluralizeUnit> {
    rounding.smallest_unit()
}

#[wasm_bindgen_test]
fn literal_only_parameters_stay_generic() {
    // The default instantiation reads `U` at its bound, every unit.
    let all: Rounding = Rounding::builder()
        .smallest_unit(PluralizeUnit::Days)
        .build();
    assert!(matches!(all.smallest_unit(), Some(PluralizeUnit::Days)));

    // `Rounding<TimeUnit>` narrows its members through the trait's
    // associated types, both reading and writing.
    let time = Rounding::<TimeUnit>::builder()
        .smallest_unit(PluralizeTimeUnit::Minutes)
        .largest_unit(TimeUnitLargestUnitKind::Hour)
        .increment(5.0)
        .build();
    assert!(matches!(
        time.smallest_unit(),
        Some(PluralizeTimeUnit::Minutes)
    ));
    time.set_largest_unit(TimeUnitLargestUnitKind::Auto);
    assert!(matches!(
        time.largest_unit(),
        Some(TimeUnitLargestUnitKind::Auto)
    ));
    assert_eq!(time.increment(), Some(5.0));

    // A narrowed unit converts into every wider one.
    let hour: PluralizeTimeUnit = PluralizeUnitHour::Hour.into();
    time.set_smallest_unit(hour);
    assert!(matches!(smallest(&time), Some(PluralizeTimeUnit::Hour)));

    // A child instantiating its parent derefs to it, so it passes where
    // the generic base is expected.
    let minute = MinuteStepping::builder()
        .step(Minute::Minute)
        .smallest_unit(PluralizeUnitMinute::Minute)
        .largest_unit(MinuteLargestUnitKind::Auto)
        .build();
    assert!(matches!(minute.step(), Some(Minute::Minute)));
    let stepped: &Stepped<Minute> = &minute;
    assert!(matches!(stepped.step(), Some(Minute::Minute)));
    assert!(matches!(
        smallest(&minute),
        Some(PluralizeUnitMinute::Minute)
    ));

    // An alias of an instantiation is that instantiation.
    let hourly: HourStepping = Stepped::<Hour>::builder()
        .smallest_unit(PluralizeUnitHour::Hours)
        .step(Hour::Hour)
        .build();
    assert!(matches!(smallest(&hourly), Some(PluralizeUnitHour::Hours)));

    // A function its parent doesn't read gets a trait extending the
    // parent's, so the child reads both.
    let bounded = Bounded::<DayOrHour>::builder()
        .smallest_unit(PluralizeUnitDayOrHour::Days)
        .maximum_unit(DayOrHourMaximumUnitKind::None)
        .build();
    assert!(matches!(
        bounded.maximum_unit(),
        Some(DayOrHourMaximumUnitKind::None)
    ));
    assert!(matches!(
        smallest(&bounded),
        Some(PluralizeUnitDayOrHour::Days)
    ));

    // A flattened `Pick` shares the parent's trait.
    let picked = TimePicking::builder()
        .smallest_unit(PluralizeTimeUnit::Hours)
        .build();
    let base: &PickedRounding<TimeUnit> = &picked;
    assert!(matches!(
        base.smallest_unit(),
        Some(PluralizeTimeUnit::Hours)
    ));
    let _: PickedRounding<DayOrTimeUnit> = PickedRounding::new();
}

#[wasm_bindgen_test]
fn associated_types_resolve_within_their_family() {
    // `Rounding` and `Spanning` both read a `LargestUnit` of `Hour`, each
    // its own.
    let rounding = Stepped::<Hour>::builder()
        .largest_unit(HourLargestUnitKind::Hours)
        .build();
    assert!(matches!(
        rounding.largest_unit(),
        Some(HourLargestUnitKind::Hours)
    ));
    let spanning = HourSpanning::builder()
        .largest_unit(HourLargestUnitKind2::Hour)
        .build();
    assert!(matches!(
        spanning.largest_unit(),
        Some(HourLargestUnitKind2::Hour)
    ));

    // `Spans.Unit` and `Steps.Unit` are two arguments of `Span`.
    let hour = HourSpan::builder()
        .largest_unit(spans::UnitLargestUnitKind::Hours)
        .build();
    assert!(matches!(
        hour.largest_unit(),
        Some(spans::UnitLargestUnitKind::Hours)
    ));
    let day = DaySpan::builder()
        .largest_unit(spans::UnitLargestUnitKind2::Day)
        .build();
    let span: &Span<steps::Unit> = &day;
    assert!(matches!(
        span.largest_unit(),
        Some(spans::UnitLargestUnitKind2::Day)
    ));
    // Passed on through `Mid`, `Unit` is still `Steps.Unit`.
    let day = DaySpan2::builder()
        .largest_unit(spans::UnitLargestUnitKind2::Hours)
        .build();
    assert!(matches!(
        day.largest_unit(),
        Some(spans::UnitLargestUnitKind2::Hours)
    ));
}

// `Rounder` has no backing module: compile-only coverage that each use site
// takes its instantiation, the default at the bound.
#[allow(dead_code)]
fn use_sites_take_their_instantiation(rounder: &Rounder) {
    rounder.round(&Rounding::new());
    rounder.round_time(&Rounding::<TimeUnit>::new());
    rounder.round_again(&Rounding::<TimeUnit>::new());
    rounder.step(&HourStepping::new());
    rounder.bound(&Bounded::new());
    let _: PluralizeUnitHour = rounder.clock().now();
    rounder.snap(PluralizeTimeUnit::Hours);
    rounder.snap_with_rounding(&Rounding::<TimeUnit>::new());
    rounder.nudge(&Rounding::<TimeUnit>::new());
    rounder.nudge_with_rounding(&Rounding::<Hour>::new());
}

// Compile-only: a merged class reads its literal-only parameter at its
// bound.
#[allow(dead_code)]
fn merged_classes_erase_literal_only_parameters(gauge: &Gauge) {
    let _: LevelReturnKind = gauge.level();
}

// Compile-only: a reference evaluation leaves as written takes the base.
#[allow(dead_code)]
fn literal_only_arguments_drop_where_evaluation_leaves_them(approx: &Approximate) {
    let _ = approx.fuzzy(&Rounding::new());
    let _: Result<Approximate, wasm_bindgen::JsValue> = Maker::new(&Rounding::new());
}
