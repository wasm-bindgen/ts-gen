//! @ts-gen --experimental-generic-mono --fallible

#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::temporal_mono::{
    temporal::{
        Instant, PlainDateTime, PluralizeDayOrTimeUnit, PluralizeTimeUnit, PluralizeUnit,
        RoundToOptions, RoundingOptions, RoundingOptionsUnits, RoundingOptionsWithLargestUnit,
        TimeUnit,
    },
    DateUnitOrTimeUnit, DayOrTimeUnit,
};

// `Temporal` is a global this fixture doesn't provide, so these are
// compile-only: each family member reads its associated types through the
// trait bounding its parameter, and each use site takes its instantiation.

#[allow(dead_code)]
fn smallest<U: RoundingOptionsUnits>(options: &RoundingOptions<U>) -> Option<U::PluralizeUnits> {
    options.smallest_unit()
}

#[allow(dead_code)]
fn families_read_through_their_traits(instant: &Instant, date_time: &PlainDateTime) {
    let all: RoundingOptions = RoundingOptions::builder()
        .smallest_unit(PluralizeUnit::Days)
        .build();
    let _: Option<PluralizeUnit> = smallest(&all);

    let to = RoundToOptions::<TimeUnit>::new(PluralizeTimeUnit::Minutes);
    // A child instantiating its parent derefs to it.
    let _: Option<PluralizeTimeUnit> = smallest(&to);
    let _ = instant.round_with_round_to_options(&to);

    let day = RoundToOptions::<DayOrTimeUnit>::builder(PluralizeDayOrTimeUnit::Day).build();
    let _ = date_time.round_with_round_to_options(&day);

    let largest = RoundingOptionsWithLargestUnit::<DateUnitOrTimeUnit>::builder()
        .smallest_unit(PluralizeUnit::Hours)
        .build();
    let _: Option<PluralizeUnit> = smallest(&largest);
}
