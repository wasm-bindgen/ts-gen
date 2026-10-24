//! @ts-gen --lib-name literal-overloads --export literal-overloads
// Overloads distinguished only by a string-literal argument: the extern
// binding takes `&str` under a private `__<name>` and one inline `pub
// fn` per literal bakes its value in, named `<base>_<literal>` with the
// cohort's usual `_with_` suffix after it. Arms with the same return
// type share one extern; a different return type gets its own. A lone
// literal (`getAll(name: "Set-Cookie")`) is not a cohort and keeps the
// plain `&str` binding.

export interface KVNamespace {
  get(key: string): Promise<string | null>;
  get(key: string, type: "text"): Promise<string | null>;
  get(key: string, type: "json"): Promise<any>;
  get(key: string, type: "arrayBuffer"): Promise<ArrayBuffer | null>;
  get(key: string, type: "stream"): Promise<Uint8Array | null>;
  get(key: string[], type: "text"): Promise<Map<string, string | null>>;
  get(key: string[], type: "json"): Promise<Map<string, any>>;
  /** A literal union is one overload with several arms. */
  mode(kind: "fast" | "safe"): boolean;
}

export interface Headers {
  getAll(name: "Set-Cookie"): string[];
}

export declare class SubtleCrypto {
  exportKey(format: "jwk", key: string): Promise<object>;
  exportKey(format: "raw" | "pkcs8" | "spki", key: string): Promise<ArrayBuffer>;
  static describe(format: "jwk"): string;
  static describe(format: "raw"): string;
}

// A generic receiver and a generic arm, modelled on workers-types'
// `KVNamespace` (whose `Key extends string` erases to `&str`): each wrapper
// carries its extern's ABI bounds (`ExpectedValue: NullableFromWasmAbi`,
// and under mono `Key: IntoWasmAbi`).
export interface KVStore<Key = unknown> {
  get(key: Key, type: "text"): Promise<string | null>;
  get<ExpectedValue = unknown>(key: Key, type: "json"): Promise<ExpectedValue | null>;
  /** A `null` arm on a generic receiver keeps the receiver's `Key`. */
  put(key: Key, kind: "a" | "b", value: string | null): void;
}

// A literal cohort with a `null` arm: the `(kind, null)` expansions form a
// cohort of their own, named `_with_null`, whose wrappers drop the `null`
// parameter as well and pass `null` itself, so the extern binds it as
// `&JsValue`.
export declare class Picker {
  pick(kind: "a" | "b", x: ArrayBuffer | null): void;
  static make(kind: "a" | "b", x: string | null): Picker;
  /** The `null` arm's wrappers and extern drop `T`, which only it named. */
  put<T>(kind: "a" | "b", x: T | null): void;
  /** Under mono, each `null` arm has a `_js_string` sibling. */
  name(kind: "a" | "b", x: ArrayBuffer | null): string;
  /** `_promise` forms follow per `null` arm. */
  load(kind: "a" | "b", x: ArrayBuffer | null): Promise<string | null>;
}

// A plain `null` arm beside a cohort: `mode`'s, left alone by the literal
// arms, is still `mode_with_null()`, never a bare `mode()`. Its private
// binding is claimed before the cohort's, so the two never share a name:
// `f_with_null()` binds `__f_with_null` and the `null` cohort
// `__f_with_null_2`.
export interface Clash {
  mode(kind: "x" | "y" | null): void;
  f(x: ArrayBuffer | null): void;
  f(kind: "a" | "b", x: ArrayBuffer | null): void;
}

// A literal repeated across overloads is one arm: `"b"` gets one wrapper.
export interface Repeats {
  tag(m: "a" | "b"): string;
  tag(m: "b" | "c"): string;
}

// A generic class: a static returning the class hoists its type arguments
// onto `impl<T> Boxed<T>`, while a bare static binds the type's defaults,
// `impl Boxed`; the constructor's wrappers land where its return puts them.
export declare class Boxed<T = unknown> {
  constructor(kind: "a" | "b");
  static label(kind: "a" | "b"): string;
  static of<T>(kind: "a" | "b", value: T): Boxed<T>;
  /** A static's `null` arm lands where its return puts it. */
  static from<T>(kind: "a" | "b", value: T | null): Boxed<T>;
}

// A free function's and a constructor's `null` cohorts; the free
// function's `T`, named only by the `null` position, goes with it.
export declare function pickFree<T>(kind: "a" | "b", x: T | null): void;
export declare class Opener {
  constructor(kind: "a" | "b", x: ArrayBuffer | null);
}
