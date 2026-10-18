//! @ts-gen --experimental-generic-mono --lib-name literal-overloads-mono --export literal-overloads-mono
// Per-monomorphization literal-keyed wrappers: a `_js_string` twin's arms
// are named off the twin (`get_text_js_string`), and a generic arm restates
// the bounds the macro puts on its extern. Overloads keyed by a literal type
// argument are bindings of their own, named by the literal and the
// parameter (`get_json_with_options`); each sets the field the literal types
// on a copy of the caller's options, which take the declared
// `Partial<GetOptions<…>>` (`GetOptionsPartial`). A `Partial` of a class
// is the class.

export interface Store {
  get(key: string, type: "text"): Promise<string | null>;
  get<T = unknown>(key: string, type: "json"): Promise<T | null>;
  read(key: string, type: "text"): string;
  read<T = unknown>(key: string, type: "json"): T;
}
export interface GetOptions<Type> {
  type: Type;
  cacheTtl?: number;
}
export interface Namespace {
  get(key: string, options?: Partial<GetOptions<undefined>>): Promise<string | null>;
  get(key: string, options?: GetOptions<"text">): Promise<string | null>;
  get<T = unknown>(key: string, options?: GetOptions<"json">): Promise<T | null>;
  get(key: string, options?: GetOptions<"arrayBuffer">): Promise<ArrayBuffer | null>;
  get(key: string[], options?: GetOptions<"text">): Promise<Map<string, string | null>>;
  get<T = unknown>(key: string[], options?: GetOptions<"json">): Promise<Map<string, T | null>>;
}
export interface Cell<V> {
  take(mode: "now"): V;
  take(mode: "later"): Promise<V>;
}
export declare class Config {
  name: string;
}
export interface Admin {
  update(config: Partial<Config>): void;
}
export declare function store(): Store;
