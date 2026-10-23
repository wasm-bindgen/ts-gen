//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, Function, JsString, Nullable, Number};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::enum_positions_mono::{
    Details, Holder, Level, Mode, ModeAlias, ModeOrNumberReturnKind, Probe,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that per-monomorphization generics carry enums by name, while
// slices and js_sys containers still lower them to their JS primitive.
#[allow(dead_code)]
async fn enum_positions_mono_compile(
    probe: &Probe,
    modes: &Array<JsString>,
    callback: &Function<fn(JsString) -> Number>,
) -> Result<(), JsValue> {
    probe.set_level(Level::High);
    let _: Level = probe.level();
    let alias: ModeAlias = Mode::Fast;
    probe.alias(alias);

    let holder: Holder<Mode> = Holder::new(Mode::Slow)?;
    holder.set(Mode::Fast);
    let _: Mode = holder.get();
    let levels: Holder<Level> = probe.holder(&holder);
    let _: Level = levels.get();
    let maybe: Holder<Nullable<Mode>> = probe.maybe_holder();
    let _: Nullable<Mode> = maybe.get();

    let details: Details<Mode> = Details::builder(Mode::Fast).fallback(Mode::Slow).build();
    let _: Mode = details.value();
    let _: Option<Mode> = details.fallback();
    let _: Level = probe.details(&details).value();

    let _: Mode = probe.next_mode().await?;
    let _: Option<Level> = probe.next_level().await?;
    let _: Mode = probe.next_details().await?.value();

    let _: Vec<f64> = probe.levels(&[Level::Low as u32 as f64]);
    let _: Vec<String> = probe.modes(&["fast".to_string()]);
    let _: Array<JsString> = probe.mode_array(modes);
    probe.on_mode(callback);

    match probe.mode_or_number(Mode::Fast) {
        ModeOrNumberReturnKind::Mode(mode) => probe.alias(mode),
        ModeOrNumberReturnKind::F64(_) => {}
    }
    probe.mode_or_number_with_f64(1.0);
    probe.mode_or_number_or_string(Mode::Slow);
    probe.mode_or_number_or_string_with_number_or_string("fast");
    probe.mode_or_number_or_string_with_number_or_string(2.0);
    Ok(())
}
