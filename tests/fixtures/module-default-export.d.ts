//! @ts-gen --lib-name module-default-export --export module-default-export

// A module file's default export binds only `default`, under the local's
// Rust name, also when the file has no `export { … }`.

export default function f(): void;
