// Iterables / iterators — covers the synthesis path for `Iterable<T>` /
// `AsyncIterable<T>` returns and the type-parameter propagation through
// `<T: JsGeneric>` for methods that mention bare TS generics.

interface KeyValueStore {
  // Generic — synthesized wrapper carries `<T>` and the inner iterator
  // sees the same `T`.
  list<T>(): Iterable<[string, T]>;
  // Same for async.
  listAsync<T>(): AsyncIterable<[string, T]>;
  // Direct iterator returns — no synthesis, map to `js_sys::Iterator`.
  // Generic `T` propagates to the iterator's element type.
  entries<T>(): IterableIterator<[string, T]>;
  keys(): Iterator<string>;
  // Direct async iterator return — generic.
  pages<T>(): AsyncIterator<T>;
  asyncEntries<T>(): AsyncIterableIterator<[string, T]>;
  // Bare generic in argument + return — exercises the
  // `<T: JsGeneric>` declaration on a non-iterable method.
  put<T>(key: string, value: T): void;
  get<T = unknown>(key: string): T | undefined;
}

declare class Cursor {
  // Class-method synthesis path with a generic parameter.
  walk<T>(): Iterable<T>;
}
