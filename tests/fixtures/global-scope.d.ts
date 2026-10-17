//! @ts-gen --input tests/fixtures/global-scope/a.d.ts --input tests/fixtures/global-scope/b.d.ts

// The top-level declarations of every script file bind into one global
// namespace and merge there, so the `interface Win` both files declare is
// one type with the members of both.
