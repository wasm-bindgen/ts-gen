//! @ts-gen --lib-name promise-returns --export promise-returns --export tests/fixtures/promise-returns.d.ts

// Fixture: `--promise-returns` chooses how a `Promise`-returning method or
// function is bound (compare `default.rs`, `promise.rs` and `async.rs`,
// and their `mono-` siblings).
// * `both` (the default) binds an `async fn name()`, which calls JS when
//   it is first polled, beside a sync `name_promise()` returning the
//   promise as a handle (`Promise<JsString>`). Under `--fallible` (the
//   `mono` configurations) the sync form catches a synchronous throw as
//   `Result<Promise<T>, E>`, unless the callable is a builtin or global
//   one, declared `async`, or `@throws {never}`.
// * `promise` binds one sync `name()` returning `Promise<T, E>`. The call
//   starts at once, a synchronous throw is its rejection, and awaiting it
//   yields `Result<T, E>`, with `T` lowered like a sync return.
// * `async` binds the `async fn name()` alone.
// A typed `@throws` types the rejection in every form.

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
/** Throws before returning a promise. */
export declare function throwNow(message: string): Promise<string>;
/**
 * Typed, and throws before returning a promise.
 * @throws {TypeError} on a malformed input
 */
export declare function parse(input: string): Promise<number>;
/**
 * Typed: the rejection is a `RangeError` in every form.
 * @throws {RangeError} on a negative value
 */
export declare function sqrt(value: number): Promise<number>;
/**
 * Plain under every policy.
 * @throws {never}
 */
export declare function ready(): Promise<boolean>;
/** Declared `async`: its sync form is plain under every policy. */
export declare async function size(): Promise<number>;
/** A `null` argument arm gets each form too. */
export declare function store(key: string, value: string | null): Promise<string>;
/** A callback's `Promise` is an inner position, the same in every mode. */
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
  /** Overloads: each expansion gets each form. */
  describe(): Promise<string>;
  describe(prefix: string): Promise<string>;
  /** A `Promise`-typed property: a getter, sync in every mode. */
  readonly loaded: Promise<boolean>;
}
export declare function items(): Promise<Item[]>;
export declare function maybeItem(name: string): Promise<Item | null>;

declare global {
  /** A global function: its sync form is plain under `--fallible`. */
  function promiseReturnsTick(): Promise<string>;
}
