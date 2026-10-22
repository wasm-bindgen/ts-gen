//! @ts-gen --input tests/fixtures/no-cross-file-names/a.d.ts --input tests/fixtures/no-cross-file-names/b.d.ts --input tests/fixtures/no-cross-file-names/dep.d.ts --lib-name no-cross-file-names --export no-cross-file-names --export no-cross-file-names-dep

// A name resolves only from where it is written, as in TypeScript: a module
// file's unexported declarations are visible to no other file, and a
// `declare module` block's to no other block, so a reference to one that
// isn't imported is "Cannot find name". Each is unresolved here, an error
// and `use JsValue as …`, rather than bound to the declaration of that name
// another file happens to have: `Secret` in a.d.ts, `Shared` in both a.d.ts
// and b.d.ts, and `Hidden` in dep.d.ts's other block of the module.

export declare function useSecret(s: Secret): void;
export declare function useShared(s: Shared): void;
