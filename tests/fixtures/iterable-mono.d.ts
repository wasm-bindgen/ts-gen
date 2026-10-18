// A declared `[Symbol.iterator]()` binds as `iterator()` and makes `&Type`
// iterable: a `[K, V]` item iterates as `(K, V)`, any other as itself.

declare class Headers {
  constructor();
  get(name: string): string | null;
  [Symbol.iterator](): IterableIterator<[key: string, value: string]>;
}
declare class Names {
  [Symbol.iterator](): IterableIterator<string>;
}
declare abstract class Cursor<T> {
  next(): T | undefined;
  [Symbol.iterator](): IterableIterator<T>;
}
