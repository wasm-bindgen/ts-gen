#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::enum_args::{Kind, SendOptions, Sender};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that string enums take the `Variant = "value"` shape wasm-bindgen
// accepts and pass by value in argument position.
#[allow(dead_code)]
fn enum_signatures_compile(sender: &Sender) {
    let options = SendOptions::builder().kind(Kind::Bytes).build();
    let _: Option<Kind> = options.kind();
    sender.send(Kind::Text);
    sender.send_with_options(Kind::Text, &options);
    let _: Kind = sender.kind();
}
