#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::sanitised_keys::{
    compare, dollar, dollar_dollar, each, underscore_underscore, version, DollarUnderscore, Op,
    Pair, Sigil, Store,
};

// Compile-only coverage that wasm-bindgen accepts the `js_name` / `js_class`
// combinations emitted for sanitised names.
#[allow(dead_code)]
fn sanitised_bindings_compile(pair: &Pair) {
    let socket = pair._0();
    pair.set_0(&socket);
    let _ = Pair::_2();
    Pair::set_2(&socket);
    let _: String = pair.public();
    pair.set_public("x");
    let _: String = Pair::cfmodel();
    Pair::set_cfmodel("x");
    pair.emit("event");
    pair.cfrun();
    let _: Pair = Pair::create();

    let _ = Store::new();
    let _: Store = Store::create();
    let _: f64 = Store::count();
    Store::set_count(1.0);

    let _: Pair = dollar("#id");
    let _: String = version.with(|v| v.clone());

    let _: Vec<Pair> = dollar_dollar("#id");
    let _: f64 = underscore_underscore.with(|v| *v);
    each(1.0, "x");
    compare(Op::LtEq);
    let symbols = DollarUnderscore::new().unwrap();
    symbols.set_underscore(&symbols.underscore());
    let _ = [Sigil::Dollar, Sigil::Underscore];
}
