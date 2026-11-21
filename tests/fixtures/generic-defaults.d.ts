//! @ts-gen --lib-name generic-defaults --export generic-defaults

// Type-parameter defaults land on the `pub type` declaration only; methods
// and helper impls keep the plain `<T: JsGeneric>` form.
export interface Box<T = unknown> {
  readonly value: T;
  set(value: T): void;
}

export type Loose = number | string;

export declare class Holder<T = Loose, U = Box<T>> {
  constructor();
  get(): T;
  boxed(): U;
  // `U`'s default `Box<T>` names Holder's own `T`, so the uninferrable `V`
  // erases to the method's fallback (`any`) instead.
  static make<V>(): Holder<string, V>;
}

export interface Options<T = Box<string>> {
  initial?: T;
}

// Trailing arguments equal to the declared Rust defaults are left to them.
export declare function looseBox(): Box<unknown>;
export declare function looseHolder(): Holder<Loose, Box<Loose>>;
export declare function stringHolder(): Holder<string, Box<string>>;
export declare function mixedHolder(): Holder<string, Box<unknown>>;
export declare function makeHolder<V>(): Holder<string, V>;

// A method-level type parameter no argument mentions after lowering reads
// as what TypeScript infers with nothing to go on: the default of the
// type it instantiates in the return, else the method's own default, else
// its class or interface bound, else `any`. A `_typed` twin keeps the
// parameter for the caller to name (`get_typed::<Row>(key)`), unless the
// return never mentions it.

export interface Row<T = Record<string, unknown>> {
  results: T[];
}
export interface Cursor<T = string> {
  next(): T;
}
export interface Store {
  // 1. mentioned by an argument: inferred at the call.
  put<T>(key: string, value: T): Promise<void>;
  // A rest array infers its element only per-monomorphization, where it
  // lowers to `&[T]`; elsewhere it is `&[JsValue]` and `T` erases.
  append<T>(...items: T[]): Row<T>;
  // 2. the whole return: the default, and the twin names the cast.
  get<T = unknown>(key: string): Promise<T | undefined>;
  first<T = unknown>(): Promise<T | null>;
  json<T>(): Promise<T>;
  // A callback's parameters and return count as mentions, unless it has
  // a rest parameter: then it is the untyped `Function` and `T` is open.
  transaction<T>(closure: (tx: Store) => Promise<T>): Promise<T>;
  collect<T>(make: () => Promise<T>): Promise<Map<string, T>>;
  each<T>(fn: (item: T) => void): Promise<Map<string, T>>;
  eachRest<T>(fn: (item: T, ...rest: unknown[]) => void): Promise<Map<string, T>>;
  // 3. nested: `Row<T = Record<…>>` wins over the method's
  // `T = unknown`; a builtin container has no default, so the method's
  // applies; `Cursor<T = string>` has one even when the method does not.
  run<T = unknown>(): Promise<Row<T>>;
  list<T = unknown>(): Promise<Map<string, T>>;
  scan<T>(): Cursor<T>;
  // A class or interface bound stands in for a missing default.
  pick<T extends Cursor>(): T;
  // 4. absent from the return once the variadic callback lowers to the
  // untyped `Function`: dropped.
  schedule<Args extends unknown[]>(callback: (...args: Args) => void, ...args: Args): number;
}
export declare function of<T>(...items: T[]): Row<T>;
export declare function enterSpan<A extends unknown[], T>(name: string, fn: (...args: A) => T, ...args: A): T;
// Every declaration of a free function is an overload with its own type
// parameters, so its own default applies.
export declare function lookup(key: string): void;
export declare function lookup<T = number>(): Map<string, T>;
