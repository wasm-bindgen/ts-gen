//! @ts-gen --input tests/fixtures/ambient-module-import-base-dir/script.d.ts --input tests/fixtures/ambient-module-import-base-dir/aug.d.ts --lib-name ambient-module-import-base-dir-aug --export lib

// As in TypeScript, a module name in a `declare module` block resolves from
// the file the block is in, not from where ts-gen runs: script.d.ts's
// "dep-pkg" is found in the node_modules next to it, while the harness runs
// from the crate root. A relative name in a script's `declare module` block
// is an error (TS2439) TypeScript never resolves, so `Rel` in
// `fromRelative` is unresolved though rel.d.ts exists. A module
// augmentation may not import (TS2667): aug.d.ts's top-level import of
// `Rel` reaches its block, the block's own import of `Pkg` does not.
