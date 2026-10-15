#![cfg(target_arch = "wasm32")]

use js_sys::{Array, ArrayTuple, Function, JsOption, JsString, Map, Number, Object};

use ts_gen_integration_tests::enum_positions::{
    Level, LevelOrStringReturnKind, Mode, ModeAlias, ModeOrNumberReturnKind,
    ModeOrRegionReturnKind, ModeOrStringReturnKind, Probe, Region,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that enums are named by value at the top level and as
// dynamic-union payloads, and lower to their JS primitive inside slices,
// generics and callbacks.
#[allow(dead_code)]
async fn enum_positions_compile(
    probe: &Probe,
    modes: &Array<JsString>,
    callback: &Function<fn(JsString) -> Number>,
) -> Result<(), wasm_bindgen::JsValue> {
    probe.set_level(Level::High);
    let _: Level = probe.level();
    probe.region(Region::Eu);
    let _: Region = probe.region_of();
    let alias: ModeAlias = Mode::Fast;
    probe.alias(alias);

    let _: Vec<f64> = probe.levels(&[Level::Low as u32 as f64]);
    let _: Vec<String> = probe.modes(&["fast".to_string()]);
    let _: Array<JsString> = probe.mode_array(modes);
    probe.on_mode(callback);

    let mode: JsString = probe.next_mode().await?;
    let _: Option<Mode> = Mode::from_js_value(&mode);
    let _: JsOption<JsString> = probe.maybe_mode().await?;
    let _: Map<JsString, JsString> = probe.mode_map();
    let _: Object<JsString> = probe.mode_record();
    let _: ArrayTuple<(JsString, Number)> = probe.pair();

    match probe.mode_or_number() {
        ModeOrNumberReturnKind::Mode(mode) => probe.alias(mode),
        ModeOrNumberReturnKind::F64(_) => {}
    }
    match probe.mode_or_string() {
        ModeOrStringReturnKind::Mode(mode) => probe.alias(mode),
        ModeOrStringReturnKind::String(_) => {}
    }
    match probe.mode_or_region() {
        ModeOrRegionReturnKind::Mode(mode) => probe.alias(mode),
        ModeOrRegionReturnKind::Region(region) => probe.region(region),
    }
    match probe.level_or_string() {
        LevelOrStringReturnKind::Level(level) => probe.set_level(level),
        LevelOrStringReturnKind::String(_) => {}
    }
    Ok(())
}
