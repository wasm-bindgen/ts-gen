#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::literal_discriminants::{
    count, direction, flag, level, ratio, repeat, size, wide, DirectionReturn, Feedback,
    FeedbackValue, FlagKind, LevelReturn, Options, RepeatKind, Scaled, SizeKind,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that literal discriminants lower to Rust wasm-bindgen accepts.
#[allow(dead_code)]
fn literal_discriminants_compile(feedback: &Feedback) {
    let _: Options = Options::new_0();
    let _: Options = Options::builder_2().label("label").build();
    let _: Scaled = Scaled::new_neg_1();
    let _: Scaled = Scaled::new_0_5();

    match level() {
        LevelReturn::N0 | LevelReturn::N1 | LevelReturn::N2 => {}
    }
    match direction() {
        DirectionReturn::Neg1 | DirectionReturn::N0 | DirectionReturn::N1 => {}
    }
    match feedback.value() {
        Some(FeedbackValue::Neg1 | FeedbackValue::N1) | None => {}
    }

    let _: f64 = ratio();
    let _: f64 = count();
    let _: f64 = wide();
    match size() {
        SizeKind::Auto | SizeKind::F64(_) => {}
    }
    match repeat() {
        RepeatKind::Bool(_) | RepeatKind::X | RepeatKind::Y => {}
    }
    match flag() {
        FlagKind::Bool(_) | FlagKind::ArrayBuffer(_) => {}
    }
}
