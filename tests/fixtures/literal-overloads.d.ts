//! @ts-gen --lib-name literal-overloads --export literal-overloads
// Overloads distinguished only by a string-literal argument: the extern
// binding takes `&str` under a private `<name>_js` and one inline `pub
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
}
