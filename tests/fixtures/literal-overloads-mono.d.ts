//! @ts-gen --experimental-generic-mono --lib-name literal-overloads-mono --export literal-overloads-mono
// Per-monomorphization literal-keyed wrappers: a `_js_string` twin's arms
// are named off the twin (`get_text_js_string`), and a generic arm restates
// the bounds the macro puts on its extern.

export interface Store {
  get(key: string, type: "text"): Promise<string | null>;
  get<T = unknown>(key: string, type: "json"): Promise<T | null>;
  read(key: string, type: "text"): string;
  read<T = unknown>(key: string, type: "json"): T;
}
export interface Cell<V> {
  take(mode: "now"): V;
  take(mode: "later"): Promise<V>;
}
export declare function store(): Store;
