#![cfg(target_arch = "wasm32")]

use js_sys::ArrayBuffer;
use ts_gen_integration_tests::discriminated_union_enum::{
    Attachment, CodeKind, DispositionKind, Payload, Reply,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that branch factories call the merged setter: a literal
// discriminant is passed as its enum variant, and a narrowed field calls its
// member's overload.
#[allow(dead_code)]
fn discriminated_union_enum_compile() {
    let inline: Attachment = Attachment::new_inline("cid");
    let _: Attachment = Attachment::new_attachment("a.txt");
    inline.set_disposition(DispositionKind::Attachment);

    let ok: Reply = Reply::new_200("body");
    let _: Reply = Reply::new_404("missing");
    ok.set_code(CodeKind::N404);

    let _: Payload = Payload::new_text("hi");
    let _: Payload = Payload::new_binary(&ArrayBuffer::new(1));
}
