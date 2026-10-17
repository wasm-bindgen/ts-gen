//! @ts-gen --input tests/fixtures/global-scope/a.d.ts --input tests/fixtures/global-scope/b.d.ts
//! @ts-gen --export tests/fixtures/global-scope/b.d.ts

// `--export b.d.ts` emits the globals b.d.ts declares: `f`, `Proc`, and
// `Win`, which carries a.d.ts's members too as it is one global. `f` takes
// an `OnlyA`, which only a.d.ts declares, so it is emitted as a type;
// `proc` is a.d.ts's alone and is not emitted.
