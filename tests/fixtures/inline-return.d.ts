// Fixture: inline `{ … }` method return types are hoisted like inline
// parameter types, named `<Parent><Member>Return`. A union of literals
// merges into one dictionary, so a `done`/`value` iterator-result shape
// becomes one type instead of an `Object | Object` dynamic union;
// enclosing type parameters the literal mentions become the hoisted
// type's own. `never` reads back as `undefined`, so `value?: never`
// merges into `Option<T>`.
//
// Where literals are not hoisted (properties, mixed unions), members
// that lower to the same type are one union member.

declare class Cursor<T> {
  next(): { done?: false; value: T } | { done: true; value?: never };
  address(): { port: number; family: string };
  cost(): { total: number } | { perItem: number } | string;
}

interface ImageOptions {
  border?: { color: string; width: number } | { color: string; top: number };
}
