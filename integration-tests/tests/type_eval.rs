#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use ts_gen_integration_tests::type_eval::scoped::PartialSettings;
use ts_gen_integration_tests::type_eval::{
    mapped, patch, ExcludeUnitHour, FlagsAOrB, Holder, KeyOfExtended, LooseOptions, Mixed, Named,
    NotMinute, OptionKey, PartialBox, PartialOptions, Patch, PickOptionsNameOrSize, Plural,
    PluralHourOrMinute, ReadonlyOptions, RequiredOptions, Rounding, Sized_,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that evaluated types take the shapes wasm-bindgen accepts:
// string enums for literal results and all-optional dictionaries with
// builders for object results, flattened members included.
#[allow(dead_code)]
fn evaluated_types_compile(
    rounding: &Rounding<JsString>,
    frozen: &ReadonlyOptions,
    required: &RequiredOptions,
    holder: &Holder<JsString>,
    loose: &LooseOptions,
    flags: &FlagsAOrB,
    open: &Patch,
) {
    // Each use of a mapped alias takes its instantiation; a use that
    // can't evaluate still names the alias.
    mapped(loose, required, flags, &PartialOptions::new());
    patch(open);
    let _: f64 = required.size();
    let partial_box: PartialBox<JsString> = holder.partial();
    let _: Option<JsString> = partial_box.value();
    let _: Holder<JsString> = Holder::new(&PartialBox::builder().label("l").build());
    let partial = PartialOptions::new();
    partial.set_name("n");
    partial.set_size(1.0);
    let _: Option<String> = partial.name();
    let _: Option<bool> = partial.fixed();
    let picked = PickOptionsNameOrSize::builder("n").size(2.0).build();
    let _: String = picked.name();
    let _: Option<f64> = Sized_::builder().size(3.0).build().size();
    let named = Named::new();
    named.set_name("n");
    named.set_label("l");
    let _: bool = named.fixed();
    let _: String = Mixed::builder("n").size(4.0).build().name();
    let _: Option<f64> = PartialSettings::builder().depth(1.0).build().depth();
    let _: bool = frozen.fixed();
    let _: Option<Plural> = rounding.smallest_unit();
    rounding.set_smallest_unit(Plural::Hours);
    let _ = (
        ExcludeUnitHour::Minute,
        NotMinute::Hour,
        KeyOfExtended::Extra,
        OptionKey::Fixed,
        PluralHourOrMinute::Minutes,
    );
}
