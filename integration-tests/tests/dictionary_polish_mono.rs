//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::dictionary_polish_mono::{
    polish::{
        spans::{self, HourSpan, Span},
        steps::{self, AnySpan, DaySpan, DaySpan2},
        Approximate, Bounded, Gauge, Holder, HourSpanning, HourStepping, MinuteStepping, Options,
        PartialBoxLikeObject, PartialOmitLikeBoxLikeObject, PickedRounding, PluralizeTimeUnit,
        PluralizeUnit, Rounder, Rounding, RoundingU, Stepped, TimeLikeObject, TimePicking,
        TimeUnit, Transition,
    },
    DayOrHour, DayOrTimeUnit, Hour, LargestUnitHour, LargestUnitMinute, LargestUnitStepsUnit,
    LargestUnitTimeUnit, MaximumUnitDayOrHour, Minute, PluralizeUnitDayOrHour, PluralizeUnitMinute,
    SpanningLargestUnitHour, TransitionDirectionKind,
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
        TransitionDirectionKind::Next
    ));
    let transition = Transition::builder_next()
        .direction(TransitionDirectionKind::Previous)
        .build();
    assert!(matches!(
        transition.direction(),
        TransitionDirectionKind::Previous
    ));
}

// Generic over any instantiation, through the trait bounding `U`.
fn smallest<U: RoundingU>(rounding: &Rounding<U>) -> Option<U::PluralizeU> {
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
        .largest_unit(LargestUnitTimeUnit::Hour)
        .increment(5.0)
        .build();
    assert!(matches!(
        time.smallest_unit(),
        Some(PluralizeTimeUnit::Minutes)
    ));
    time.set_largest_unit(LargestUnitTimeUnit::Auto);
    assert!(matches!(
        time.largest_unit(),
        Some(LargestUnitTimeUnit::Auto)
    ));
    assert_eq!(time.increment(), Some(5.0));
    assert!(matches!(smallest(&time), Some(PluralizeTimeUnit::Minutes)));

    // A child instantiating its parent derefs to it, so it passes where
    // the generic base is expected, and the parameter alone reads as the
    // argument.
    let minute = MinuteStepping::builder()
        .step(Minute::Minute)
        .smallest_unit(PluralizeUnitMinute::Minute)
        .largest_unit(LargestUnitMinute::Auto)
        .build();
    assert!(matches!(minute.step(), Some(Minute::Minute)));
    let stepped: &Stepped<Minute> = &minute;
    assert!(matches!(stepped.step(), Some(Minute::Minute)));
    assert!(matches!(
        smallest(&minute),
        Some(PluralizeUnitMinute::Minute)
    ));

    // An alias of an instantiation, at a literal, is that instantiation.
    let hourly: HourStepping = Stepped::<Hour>::builder()
        .smallest_unit(spans::Unit::Hours)
        .step(Hour::Hour)
        .build();
    assert!(matches!(smallest(&hourly), Some(spans::Unit::Hours)));

    // A function its parent doesn't read gets a trait extending the
    // parent's, so the child reads both.
    let bounded = Bounded::<DayOrHour>::builder()
        .smallest_unit(PluralizeUnitDayOrHour::Days)
        .maximum_unit(MaximumUnitDayOrHour::None)
        .build();
    assert!(matches!(
        bounded.maximum_unit(),
        Some(MaximumUnitDayOrHour::None)
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
        .largest_unit(LargestUnitHour::Hours)
        .build();
    assert!(matches!(
        rounding.largest_unit(),
        Some(LargestUnitHour::Hours)
    ));
    let spanning = HourSpanning::builder()
        .largest_unit(SpanningLargestUnitHour::Hour)
        .build();
    assert!(matches!(
        spanning.largest_unit(),
        Some(SpanningLargestUnitHour::Hour)
    ));

    // `Spans.Unit` and `Steps.Unit` are two arguments of `Span`.
    let hour = HourSpan::builder()
        .largest_unit(LargestUnitHour::Hours)
        .build();
    assert!(matches!(hour.largest_unit(), Some(LargestUnitHour::Hours)));
    let day = DaySpan::builder()
        .largest_unit(LargestUnitStepsUnit::Day)
        .build();
    let span: &Span<steps::Unit> = &day;
    assert!(matches!(
        span.largest_unit(),
        Some(LargestUnitStepsUnit::Day)
    ));
    // Passed on through `Mid`, `Unit` is still `Steps.Unit`.
    let day = DaySpan2::builder()
        .largest_unit(LargestUnitStepsUnit::Hours)
        .build();
    assert!(matches!(
        day.largest_unit(),
        Some(LargestUnitStepsUnit::Hours)
    ));
    // Left to its default, `Unit` is `Spans.Unit`, where `Spanned` declares it.
    let any = AnySpan::builder()
        .largest_unit(LargestUnitHour::Hour)
        .build();
    assert!(matches!(any.largest_unit(), Some(LargestUnitHour::Hour)));
}

// `Rounder` has no backing module: compile-only coverage that each use site
// takes its instantiation.
#[allow(dead_code)]
fn use_sites_take_their_instantiation(rounder: &Rounder) {
    rounder.round(&Rounding::new());
    rounder.round_time(&Rounding::<TimeUnit>::new());
    rounder.round_again(&Rounding::<TimeUnit>::new());
    rounder.step(&HourStepping::new());
    rounder.bound(&Bounded::<DayOrHour>::new());
    let _: spans::Unit = rounder.clock().now();
    rounder.snap(PluralizeTimeUnit::Hours);
    rounder.snap_with_rounding(&Rounding::<TimeUnit>::new());
    rounder.nudge(&Rounding::<TimeUnit>::new());
    rounder.nudge_with_rounding(&Rounding::<Hour>::new());
}

// Compile-only: a merged class reads its literal-only parameter as its
// primitive.
#[allow(dead_code)]
fn merged_classes_erase_literal_only_parameters(gauge: &Gauge) {
    let _: String = gauge.level();
}

// Compile-only: a literal argument the parse can't evaluate past still
// instantiates the family at its enum.
#[allow(dead_code)]
fn literal_arguments_instantiate_where_evaluation_stops(approx: &Approximate) {
    approx.fuzzy(&Rounding::<Hour>::new());
}
