#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::enum_initializers::{Cycle, Label, Loop, Merged, Mode, Restart};

// Compile-only coverage that initialisers referencing other enums and other
// declarations of a merged enum land on the values TypeScript gives them.
const _: () = {
    assert!(Mode::A as u32 == 2);
    assert!(Mode::B as u32 == 3);
    assert!(Mode::C as u32 == 10);
    assert!(Mode::D as u32 == 19);
    assert!(Mode::E as u32 == 4);
    assert!(Merged::A as u32 == 1);
    assert!(Merged::B as u32 == 2);
    assert!(Merged::C as u32 == 3);
    assert!(Restart::A as u32 == 5);
    assert!(Restart::B as u32 == 0);
    assert!(Restart::C as u32 == 1);
    assert!(Cycle::Fixed as u32 == 1);
    assert!(Loop::C as u32 == 1);
};

#[allow(dead_code)]
fn string_members(label: Label) -> bool {
    matches!(label, Label::Inner | Label::Joined)
}
