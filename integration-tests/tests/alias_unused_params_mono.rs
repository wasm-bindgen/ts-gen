#![cfg(target_arch = "wasm32")]

use js_sys::Undefined;
use ts_gen_integration_tests::alias_unused_params_mono::{
    brand, default_return, defaults, erase, misc, ns, skip, skip_return, Branded, Chained, Color,
    ColorAlias, Erased, Fetcher, Handler, Local, Pair, Partly, Skip, WithDefault,
};

// Compile-only, like `alias_unused_params`: per-monomorphization lowering
// passes an alias the same arguments, so dropped ones don't surface as
// `_js_string` variants either.
#[allow(dead_code)]
fn alias_unused_params_mono_compile() {
    let _: fn(&Branded<Local>) = brand;
    let _: fn(&Erased) -> Partly = erase;
    let _: fn(&Skip<String, bool>) = skip;
    let _: fn(
        &WithDefault<f64>,
        &Chained<f64>,
        &Pair<Undefined, Undefined>,
        &Fetcher<String>,
        &ns::WithInner<String>,
        &Pair<Undefined, Undefined>,
    ) = defaults;
    let _: fn() -> WithDefault<f64> = default_return;
    let _: fn() -> Skip<f64, bool> = skip_return;
    let _: fn(&Handler, ColorAlias) = misc;
    let _: Option<WithDefault<f64>> = None::<Pair<f64, String>>;
    let _: Option<ColorAlias> = None::<Color>;
}
