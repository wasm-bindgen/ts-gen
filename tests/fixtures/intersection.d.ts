//! @ts-gen --lib-name intersection --export intersection

// `A & { ... }` is the interface the user would have written as
// `interface X extends A { ... }`, so it hoists through the same path as
// an inline literal: references become `extends`, literals are met into
// the own members (required if any branch requires). `get`'s refined
// overload therefore gets its own `StoreOptions extends GetOptions` with
// a required `onlyIf`. Where nothing hoists (a non-object member like
// `string & {}`, or a position that does not synthesise) the
// intersection is bounded by its most specific member at codegen.

export interface Conditional {
  etag?: string;
}
export interface GetOptions {
  onlyIf?: Conditional;
  range?: string;
}
export interface Base {
  id: string;
}
export interface Derived extends Base {
  extra: string;
}
export interface Unrelated {
  other: string;
}
export interface Store {
  get(key: string, options: GetOptions & { onlyIf: Conditional }): Promise<string | null>;
  get(key: string, options?: GetOptions): Promise<string>;
  refine(value: Derived & Base): void;
  unrelated(value: Derived & Unrelated): void;
  shape(value: { a: string } & { b: number }): void;
}
export type Refined = GetOptions & { onlyIf: Conditional };
export interface Fetcher {
  fetch(input: string): Promise<string>;
}
export type Stub = Fetcher & { readonly id: string };
export interface Message {
  role: "user" | "assistant" | (string & {});
}
