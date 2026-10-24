//! @ts-gen --lib-name promise-only --export promise-only --export tests/fixtures/promise-only.d.ts
//! @ts-gen-variants promise-only

// Fixture: `--promise-only` binds every `Promise`-returning method and
// function once, as a sync `name()` returning the `Promise<T>`, with no
// `async fn` and no `name_promise()` form (compare `default.rs` and
// `default-promise-only.rs`). The JS call happens when `name()` is
// called, and callers write `name().await`. Under `--fallible` (the
// `mono` configurations) it returns `Result<Promise<T>, E>` unless the
// callable is a builtin or global one, declared `async`, or
// `@throws {never}`. Under `--experimental-generic-mono` the `T` is the
// awaited Rust value (`Promise<String>`, `Promise<Array<String>>`,
// `Promise<Nullable<String>>`); without it, the `js_sys` handle
// (`Promise<JsString>`).

export type Tag = string;
export type Mode = "fast" | "slow";

export declare function text(): Promise<string>;
export declare function count(): Promise<number>;
export declare function flag(): Promise<boolean>;
export declare function done(): Promise<void>;
export declare function tags(): Promise<string[]>;
export declare function maybeTags(): Promise<Tag[] | null>;
export declare function rows(): Promise<string[][]>;
export declare function lookup(key: string): Promise<string | null>;
export declare function find(key: string): Promise<string | undefined>;
export declare function peek(key: string): Promise<string | null | undefined>;
export declare function mode(): Promise<Mode>;
export declare function anything(): Promise<any>;
export declare function identity<T>(value: T): Promise<T>;
/** Rejects: the error comes out of the `await`. */
export declare function reject(message: string): Promise<string>;
/** Throws before returning a promise: `Err` from the call under `--fallible`. */
export declare function throwNow(message: string): Promise<string>;
/**
 * Typed sync throw.
 * @throws {TypeError} on a malformed input
 */
export declare function parse(input: string): Promise<number>;
/**
 * Plain under every policy.
 * @throws {never}
 */
export declare function ready(): Promise<boolean>;
/** Declared `async`: plain under every policy. */
export declare async function size(): Promise<number>;
/** A `null` argument arm gets a promise-returning wrapper too. */
export declare function store(key: string, value: string | null): Promise<string>;
/** A callback's `Promise` is an inner position, unchanged by the flag. */
export declare function run(callback: () => Promise<string>): Promise<string>;

export interface Wrapper<T> {
  value: T;
}
export declare function boxed(): Promise<Wrapper<string>>;

export declare class Item {
  constructor(name: string);
  readonly name: string;
  rename(name: string): Promise<Item>;
  static load(name: string): Promise<Item>;
  /** Overloads: one promise-returning binding per expansion. */
  describe(): Promise<string>;
  describe(prefix: string): Promise<string>;
  /** A `Promise`-typed property: a getter, sync with or without the flag. */
  readonly loaded: Promise<boolean>;
}
export declare function items(): Promise<Item[]>;
export declare function maybeItem(name: string): Promise<Item | null>;

declare global {
  /** A global function: plain under `--fallible`. */
  function promiseOnlyTick(): Promise<string>;
}
