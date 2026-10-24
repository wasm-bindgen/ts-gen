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

/** `lookup`, resolved asynchronously. */
export declare function lookupAsync(key: string): Promise<string | null | undefined>;

/** Resolves to `null` for an unknown key. */
export declare function find(key: string): Promise<Options | null>;

/** Tells `undefined`, `null` and a value apart. */
export declare function classify(value: string | null | undefined): string;

/** An `Options` whose `label` is missing, `undefined`, `null` or `"x"`. */
export declare function options(label: string): Options;

export declare function configure(options: Options): string;

/** How many arguments arrived, and what the last one was. */
export declare function arity(a: string | undefined, b?: number | null): string;

/** A non-trailing `undefined` is passed, not omitted. */
export declare function pick(a: string | undefined, b: number): string;

// A parameter that only admits `null` is `only_null_with_null()`, never a
// bare `only_null()`, which would read as omitting it.
/** Reports how many arguments arrived and the first. */
export declare function onlyNull(value: null): string;
