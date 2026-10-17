//! @ts-gen --lib-name node-nullable --export node-nullable
// Node-style API where `null` and `undefined` are distinct inputs: a
// default-parameter initializer fires on `undefined` and `=== null` is
// checked explicitly. Exercised against a real JS module in the
// integration tests.

export interface Options {
  /** `undefined` takes the default; `null` means "no label". */
  label?: string | null;
}

/** `null` disables the fallback; `undefined` takes the default `"anon"`. */
export declare function describe(name: string | null): string;

/** `undefined` takes the default `1`. */
export declare function repeat(count?: number): string;

/** Omitted, explicit `null`, or a value: three distinct inputs. */
export declare function greet(name?: string | null): string;

/** Returns `null` for an unknown key and `undefined` for a known empty one. */
export declare function lookup(key: string): string | null | undefined;

export declare function configure(options: Options): string;
