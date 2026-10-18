//! @ts-gen --experimental-generic-mono

// Per-mono, a `null`-admitting type argument has no direction, so it keeps
// `null` exact as `JsNullable` over the native leaf; an undefined-only one
// is an `Option`. Reads fold both empties into `Option`: an awaited value,
// a callback's argument, a return or getter, `JsValue` included.

declare class Holder<T> {
  value(): T;
}
declare class Store {
  bulk(): Promise<Map<string, string | null>>;
  optional(): Map<string, number | undefined>;
  rows(): ArrayLike<number | null>;
  held(): Holder<string | null>;
  first(): Promise<string | null>;
  attachment(): any | null;
  later(): Promise<any | null>;
  watch(listener: (value: string | null, raw: any | null) => void): void;
  provide(source: () => string | null): void;
}
