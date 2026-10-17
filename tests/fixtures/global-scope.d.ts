//! @ts-gen --input tests/fixtures/global-scope/a.d.ts --input tests/fixtures/global-scope/b.d.ts

// The top-level declarations of every script file bind into one global
// namespace and merge there, so the `interface Win` both files declare is
// one type with the members of both. An interface and a constructor-shaped
// `var` in different files are one class in either file order (`XV`, `VX`,
// `XC`, `CX`). Members another file adds to the constructor interface are
// statics of the class in either order (`XS`, `SX`), and its construct
// signatures are constructor overloads. An interface keeps its
// name as a type beside a function of the same name (`XF`, `FX`).
