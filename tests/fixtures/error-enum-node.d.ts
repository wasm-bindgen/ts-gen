//! @ts-gen --lib-name error-enum-node --export error-enum-node --error-enum --error-type crate::CaughtError

// Fixture: the `--error-enum` dispatch against a real JS module. A thrown
// value lands in the most derived declared class (`SubFault` before
// `Fault`), a builtin class (`TypeError`), or `Other`. Untyped bindings keep
// the `--error-type` default.

export declare class Fault extends Error {}

export declare class SubFault extends Fault {}

/**
 * Throws per `kind`: `sub`, `fault`, `type`, `string`, `number` or `opaque`.
 * @throws {Fault | SubFault | TypeError}
 */
export declare function raise(kind: string): string;

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
