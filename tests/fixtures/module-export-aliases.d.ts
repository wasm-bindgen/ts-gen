//! @ts-gen --lib-name module-export-aliases --export module-export-aliases

// One local exported under several names: a function binds once per public
// name, and a type is emitted under its local name with an alias per
// public name.

interface _Foo {
  bar(): void;
}
declare function _helper(): void;
export { _Foo as Foo, _Foo as Bar, _helper as a, _helper as b };
