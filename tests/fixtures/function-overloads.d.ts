//! @ts-gen --input tests/fixtures/function-overloads/b.d.ts

// The declarations of one function are its overloads, within a file, a
// namespace or across files (`f`'s `number` form is in b.d.ts), and they
// expand together through signature flattening as a method's do.

declare function parse(input: string): object;
declare function parse(input: ArrayBuffer): object;
declare function parse(input: string, reviver: Function): object;

declare function f(x: string): void;

declare namespace N {
  function g(x: string): void;
  function g(x: number): void;
}
