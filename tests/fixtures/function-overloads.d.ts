//! @ts-gen --input tests/fixtures/function-overloads/b.d.ts

// The declarations of one function or method are its overloads, within a
// file, a namespace or across files (`f`'s `number` form is in b.d.ts),
// and they expand together through signature flattening. Each is its own
// declaration, with its own return type, type parameters, JSDoc and
// `@throws`.

declare function parse(input: string): object;
declare function parse(input: ArrayBuffer): object;
declare function parse(input: string, reviver: Function): object;

declare function f(x: string): void;

declare namespace N {
  function g(x: string): void;
  function g(x: number): void;
}

declare function sz(x: string): string;
declare function sz(x: number): number;
declare function gen<T>(x: T): T;
declare function gen(x: number, y: number): number;

/** Reads text. */
declare function read(x: string): string;
/**
 * Reads a byte.
 * @throws {RangeError} When out of range.
 */
declare function read(x: number): number;

declare class Twin {
  sz(x: string): string;
  sz(x: number): number;
  gen<T>(x: T): T;
  gen(x: number, y: number): number;
  /** Reads text. */
  read(x: string): string;
  /**
   * Reads a byte.
   * @throws {RangeError} When out of range.
   */
  read(x: number): number;
  static of(x: string): Twin;
  static of(x: number): number;
}
