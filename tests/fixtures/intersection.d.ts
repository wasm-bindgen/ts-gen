//! @ts-gen --external Foreign=::js_sys::Date --lib-name intersection --export intersection

// `A & { ... }` is the interface the user would have written as
// `interface X extends A { ... }`, so it hoists through the same path as
// an inline literal: references become `extends`, literals are met into
// the own members (required if any branch requires). `get`'s refined
// overload therefore gets its own `StoreOptions extends GetOptions` with
// a required `onlyIf`. Hoisting needs every parent to be a class or
// interface; a type parameter, a primitive alias or an unresolved name
// leaves the intersection to codegen, where it is bounded by its most
// specific member (`Derived & Base` -> `Derived`). A parameter typed by
// named types alone is bounded too rather than given a fresh subtype.

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
export type Str = string;
export interface Store {
  get(key: string, options: GetOptions & { onlyIf: Conditional }): Promise<string | null>;
  get(key: string, options?: GetOptions): Promise<string>;
  refine(value: Derived & Base): void;
  same(value: Derived & Derived): void;
  unrelated(value: Derived & Unrelated): void;
  shape(value: { a: string } & { b: number }): void;
  generic<T>(value: T & { id: string }): void;
  primitive(value: Str & { c: string }): void;
  anything(value: Derived & any): void;
}
export type Branded<T> = T & { __brand: string };
export type Both = Derived & Unrelated;
export type Refined = GetOptions & { onlyIf: Conditional };
export interface Fetcher {
  fetch(input: string): Promise<string>;
}
export type Stub = Fetcher & { readonly id: string };
export interface Provider<T> {
  call(): T;
}
export type GenericStub<T> = Provider<T> & Fetcher & { readonly id: string };
export type Unknown = Missing & Fetcher & { readonly id: string };
export type Ext = Foreign & { extra: string };
export interface Message {
  role: "user" | "assistant" | (string & {});
}
// Parents are checked through alias chains and intersection aliases, and a
// generic alias whose target mentions its own type parameters is refused.
export type Str2 = Str;
export type ViaStr2 = Str2 & { extra: string };
export type ViaStr = Str & { extra: string };
export type ViaViaStr = ViaStr & { z: string };
export type ViaBranded = Branded<Base> & { y: string };
export type BaseAlias = Base;
export type ViaBaseAlias = BaseAlias & { w: string };
export type Same<T> = T;
export type ViaSame = Same<Base> & { y: string };
export type ProviderOf<T> = Provider<T[]>;
export type ViaProviderOf = ProviderOf<string> & { y: string };
