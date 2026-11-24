//! @ts-gen --lib-name error-enum-node --export error-enum-node --error-enum --error-type crate::CaughtError

// Fixture: the `--error-enum` dispatch against a real JS module. A thrown
// value lands in the most derived declared class (`SubFault` before
// `Fault`), a builtin class (`TypeError`, `RangeError`), or `Other`. Untyped
// bindings keep the `--error-type` default. Variant idents that repeat (a class named
// `Other`, `Codes.Fault` beside `CodesFault`) are numbered.

export declare class Fault extends Error {}

export declare class SubFault extends Fault {}

/**
 * Throws per `kind`: `sub`, `fault`, `type`, `range`, `string`, `number` or
 * `opaque`.
 * @throws {Fault | SubFault | TypeError | RangeError}
 */
export declare function raise(kind: string): string;

export declare class Other extends Error {}

export declare class CodesFault extends Error {}

export declare namespace Codes {
  class Fault extends Error {}
}

/**
 * Throws per `kind`: `other`, `codes-fault` or `codes.fault`.
 * @throws {Other | CodesFault | Codes.Fault}
 */
export declare function raiseNamed(kind: string): string;

/** `raise` without a `@throws`: the `crate::CaughtError` default. */
export declare function raiseUntyped(kind: string): string;

export declare class Gate {
  constructor(open: boolean);
  /**
   * Throws when the gate is closed.
   * @throws {SubFault}
   */
  readonly value: number;
}

/**
 * Iterable where every binding catches: `iterator()` is a `Result` of the
 * `--error-type` default under `--fallible`, which only converts from a
 * thrown value.
 */
export declare class Kinds {
  constructor();
  [Symbol.iterator](): IterableIterator<string>;
}
