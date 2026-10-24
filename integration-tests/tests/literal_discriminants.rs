#![cfg(target_arch = "wasm32")]

use js_sys::Nullable;

use ts_gen_integration_tests::literal_discriminants::{
    count, direction, flag, level, ratio, repeat, size, wide, DirectionReturnKind, Feedback,
    FlagReturnKind, KeepAliveKind, Options, RepeatReturnKind, Scaled, SizeReturnKind, ValueKind,
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
        KeepAliveKind::N0 | KeepAliveKind::N1 | KeepAliveKind::N2 => {}
    }
    match direction() {
        DirectionReturnKind::Neg1 | DirectionReturnKind::N0 | DirectionReturnKind::N1 => {}
    }
    match feedback.value() {
        Nullable::Value(ValueKind::Neg1 | ValueKind::N1) | Nullable::Null => {}
    }

    let _: f64 = ratio();
    let _: f64 = count();
    let _: f64 = wide();
    match size() {
        SizeReturnKind::Auto | SizeReturnKind::F64(_) => {}
    }
    match repeat() {
        RepeatReturnKind::Bool(_) | RepeatReturnKind::X | RepeatReturnKind::Y => {}
    }
    match flag() {
        FlagReturnKind::Bool(_) | FlagReturnKind::ArrayBuffer(_) => {}
    }
}
