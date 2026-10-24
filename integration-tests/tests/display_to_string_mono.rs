//! @ts-gen --experimental-generic-mono --fallible

#![cfg(target_arch = "wasm32")]

use std::fmt::Write;

use js_sys::{JsString, RangeError};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::display_to_string_mono::{
    labelled, Boxed, Moment, Pure, Stamp, StampOptions, Strict, SubStamp,
};

fn install() {
    js_sys::eval(
        "class Label {
            constructor(label) { this.label = label; }
            toString(options) { return options?.upper ? this.label.toUpperCase() : this.label; }
        }
        globalThis.Stamp = class extends Label {};
        globalThis.SubStamp = class extends globalThis.Stamp {};
        globalThis.Moment = class extends Label {};
        globalThis.Pure = class extends Label {};
        globalThis.Strict = class extends Label {
            toString() { if (!this.label) throw new RangeError('empty'); return this.label; }
        };
        globalThis.Boxed = class {
            constructor(value) { this.value = value; }
            toString() { return `Boxed(${this.value})`; }
        };
        globalThis.labelled = (label) => ({ toString: () => label });",
    )
    .unwrap();
}

// Under `--fallible` the catching form keeps its `try_` name, so
// `.to_string()` is still `ToString`; the `_js_string` variant stays.
#[wasm_bindgen_test]
fn display_calls_to_string() {
    install();
    let upper = StampOptions::builder().upper(true).build();

    let stamp = Stamp::new("stamp").unwrap();
    assert_eq!(format!("{stamp}"), "stamp");
    let owned: String = stamp.to_string();
    assert_eq!(owned, "stamp");
    assert_eq!(stamp.try_to_string().unwrap(), "stamp");
    let js: JsString = stamp.to_string_js_string().unwrap();
    assert_eq!(String::from(js), "stamp");
    assert_eq!(stamp.to_string_with_options(&upper).unwrap(), "STAMP");

    // `ToString` is reached on the parent through `Deref`.
    let sub: SubStamp = js_sys::eval("new SubStamp('sub')")
        .unwrap()
        .unchecked_into();
    assert_eq!(sub.to_string(), "sub");

    let moment = Moment::new("moment").unwrap();
    assert_eq!(format!("[{moment}]"), "[moment]");

    assert_eq!(Pure::new("pure").unwrap().to_string(), "pure");
    let boxed = Boxed::new(&JsString::from("v")).unwrap();
    assert_eq!(format!("{boxed}"), "Boxed(v)");
    assert_eq!(format!("{}", labelled("l").unwrap()), "l");
}

#[wasm_bindgen_test]
fn throwing_to_string_is_a_fmt_error() {
    install();
    let strict = Strict::new("").unwrap();
    let mut out = String::new();
    assert!(write!(out, "{strict}").is_err());
    let error: RangeError = strict.try_to_string().unwrap_err();
    assert!(error.is_instance_of::<RangeError>());
    assert_eq!(Strict::new("ok").unwrap().to_string(), "ok");
}
