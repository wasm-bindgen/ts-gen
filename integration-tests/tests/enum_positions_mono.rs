//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, Function, JsString, Nullable};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::enum_positions_mono::{
    Details, Holder, Level, Mode, ModeAlias, ModeOrNumberArgReturnKind, ModeOrNumberReturnKind,
    Probe,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that per-monomorphization generics, js_sys containers and
// callbacks carry enums by name, while slices still lower them to their JS
// primitive.
#[allow(dead_code)]
async fn enum_positions_mono_compile(
    probe: &Probe,
    modes: &Array<JsString>,
    callback: &Function<fn(Mode) -> Level>,
    holder: &Holder<Mode>,
) -> Result<(), JsValue> {
    probe.set_level(Level::High);
    let _: Level = probe.level();
    let alias: ModeAlias = Mode::Fast;
    probe.alias(alias);

    // `Holder::new(&Mode)`, `holder.set(&Mode)` and `Details::builder(&Mode)`
    // need `&Mode: IntoWasmAbi`, which the pinned wasm-bindgen implements for
    // primitives but not for its enums, so the `Holder<Mode>` comes in from
    // outside and the `Details<Mode>` is read back from `nextDetails()`.
    let _: Mode = holder.get();
    let levels: Holder<Level> = probe.holder(holder);
    let _: Level = levels.get();
    let maybe: Holder<Nullable<Mode>> = probe.maybe_holder();
    let _: Nullable<Mode> = maybe.get();

    let details: Details<Mode> = probe.next_details().await?;
    let _: Mode = details.value();
    let _: Option<Mode> = details.fallback();
    let _: Level = probe.details(&details).value();

    let _: Mode = probe.next_mode().await?;
    let _: Option<Level> = probe.next_level().await?;
    let _: Option<Mode> = probe.maybe_mode().await?;

    let _: Vec<f64> = probe.levels(&[Level::Low as u32 as f64]);
    let _: Vec<String> = probe.modes(&["fast".to_string()]);
    let _: Array<Mode> = probe.mode_array(modes);
    probe.on_mode(callback);

    match probe.mode_or_number() {
        ModeOrNumberReturnKind::Mode(mode) => probe.alias(mode),
        ModeOrNumberReturnKind::F64(_) => {}
    }
    // A union enum is shared only by sites of one JS name.
    match probe.mode_or_number_arg(Mode::Fast) {
        ModeOrNumberArgReturnKind::Mode(mode) => probe.alias(mode),
        ModeOrNumberArgReturnKind::F64(_) => {}
    }
    probe.mode_or_number_arg_with_f64(1.0);
    probe.mode_or_number_or_string(Mode::Slow);
    probe.mode_or_number_or_string_with_str("fast");
    probe.mode_or_number_or_string_with_f64(2.0);
    Ok(())
}
