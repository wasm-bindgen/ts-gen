#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::enum_aliases::{Api, Bits, Flags, Kind, Unresolved, Version};

// Compile-only coverage that same-valued members become associated consts
// of the first variant with that value, and that evaluated initialisers
// land on the value TypeScript gives them.
const _: () = {
    assert!(matches!(Version::Latest, Version::V2));
    assert!(matches!(Version::Current, Version::V2));
    assert!(matches!(Version::Qualified, Version::V1));
    assert!(matches!(Version::Next, Version::V2));
    assert!(Flags::ReadWrite as u32 == 3);
    assert!(Flags::All as i32 == 7);
    assert!(Flags::Mask as i32 == -1);
    assert!(Flags::Negated as i32 == -1);
    assert!(matches!(Kind::Default, Kind::Text));
    assert!(matches!(Kind::Fallback, Kind::Text));
    assert!(Bits::Both as u32 == 3);
    assert!(Unresolved::C as u32 == 2);
    assert!(Unresolved::D as u32 == 7);
};

#[allow(dead_code)]
fn enum_alias_signatures_compile(api: &Api) {
    api.set_version(Version::Latest);
    api.set_flags(Flags::All);
    api.set_kind(Kind::Default);
    let _: Version = api.version();
}
