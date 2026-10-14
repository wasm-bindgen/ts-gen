#![cfg(target_arch = "wasm32")]

use js_sys::Uint8Array;

use ts_gen_integration_tests::enum_args::{Kind, SendOptions, Sender};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that string enums take the `Variant = "value"` shape wasm-bindgen
// accepts, pass by value in argument position, and that optional
// `ArrayBufferView` builder methods carry the setter's `TypedArray` bound.
#[allow(dead_code)]
fn enum_signatures_compile(sender: &Sender, bytes: &Uint8Array) {
    let options = SendOptions::builder()
        .kind(Kind::Bytes)
        .payload(bytes)
        .build();
    let _: Option<Kind> = options.kind();
    sender.send(Kind::Text);
    sender.send_with_options(Kind::Text, &options);
    let _: Kind = sender.kind();
}
