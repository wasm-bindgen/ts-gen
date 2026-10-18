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
}

export interface Options<T = Box<string>> {
  initial?: T;
}
