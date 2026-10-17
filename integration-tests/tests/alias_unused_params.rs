#![cfg(target_arch = "wasm32")]

use js_sys::{Boolean, JsString, Number, Undefined};
use ts_gen_integration_tests::alias_unused_params::{
    brand, default_return, defaults, erase, misc, ns, qualified_return, skip, skip_return, Branded,
    Chained, Chosen, Color, Erased, Fetcher, Handler, Holder, Local, Pair, Partly, Skip,
    WithDefault,
};
use wasm_bindgen::JsValue;

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that aliases drop the type parameters their targets don't use,
// at the declaration and at every instantiation.
#[allow(dead_code)]
fn alias_unused_params_compile(holder: &Holder<JsString>, local: Local) {
    let _: fn(&Branded<Local>) = brand;
    let _: fn(&Erased) -> Partly = erase;
    let _: fn(&Skip<JsString, Boolean>) = skip;
    let _: fn(
        &WithDefault<Number>,
        &Chained<Number>,
        &Pair<Undefined, Undefined>,
        &Fetcher<JsString>,
        &ns::WithInner<JsString>,
        &Pair<Undefined, Undefined>,
    ) = defaults;
    let _: fn() -> WithDefault<Number> = default_return;
    let _: fn() -> Skip<Number, Boolean> = skip_return;
    let _: fn(&Handler, Color, Color, Color, &Pair<Chosen<Number>, JsString>) = misc;
    let _: fn() -> ns::WithInner<JsString> = qualified_return;
    let _: Option<WithDefault<Number>> = None::<Pair<Number, JsString>>;
    let _: Option<ns::WithInner<JsString>> = None::<Pair<JsString, ns::Inner>>;
    let _: Option<Fetcher> = None::<Pair<Undefined, Undefined>>;
    let _: Branded<Local> = local;
    let _: Erased = holder.get();
    let _: Erased = holder.entry::<JsValue>();
    let pair: Pair<JsString, JsString> = Pair::new(&JsString::from("a"), &JsString::from("b"));
    holder.take(&pair);
}
