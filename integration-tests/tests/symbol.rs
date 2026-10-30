#![cfg(target_arch = "wasm32")]

use js_sys::{Function, Symbol, Undefined};

use ts_gen_integration_tests::symbol::{
    all, create, describe, find, for_each, lookup, set_key, set_key_with_symbol, LookupReturnKind,
    Tagged,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that `symbol` lowers to `js_sys::Symbol` in every position.
#[allow(dead_code)]
fn symbol_signatures_compile(symbol: &Symbol, callback: &Function<fn(Symbol) -> Undefined>) {
    let _: String = describe(symbol);
    let _: Symbol = create();
    let _: Option<Symbol> = find("name");
    let _: Vec<Symbol> = all();
    for_each(callback);

    set_key("key");
    set_key_with_symbol(symbol);
    match lookup() {
        LookupReturnKind::String(_) => {}
        LookupReturnKind::Symbol(_) => {}
    }

    let tagged = Tagged::builder(symbol).alias(symbol).build();
    let _: Symbol = tagged.tag();
    let _: Option<Symbol> = tagged.alias();
}
