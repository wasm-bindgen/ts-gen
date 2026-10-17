// A Rust alias can't declare a type parameter its target doesn't use
// (E0091). Parameters the lowered target doesn't mention are dropped from
// the alias and from every instantiation.

interface Local {
  l: string;
}

interface Pair<A, B> {
  a: A;
  b: B;
}

type Branded<T, B> = T & { __brand: B };
type Erased<T> = T extends string ? Local : never;
type Partly<T, U> = Local;
type Skip<A, B, C> = Pair<A, C>;

declare function brand(value: Branded<Local, "id">): void;
declare function erase(value: Erased<string>): Partly<string, number>;
declare function skip(value: Skip<string, number, boolean>): void;

// Kept parameters keep their defaults, so an instantiation can leave them
// out. `U`'s default keeps `T`, and a bare alias means its defaults.
type WithDefault<T, U = string> = Pair<T, U>;
type Chained<T, U = T> = Pair<U, U>;
type Fetcher<T = undefined, Reserved = never> = Pair<T, T>;
declare function defaults(
  a: WithDefault<number>,
  b: Chained<number>,
  c: Fetcher,
  d: Fetcher<string>,
): void;

interface Holder<T> {
  get(): Erased<T>;
  take(value: Skip<T, number, string>): void;
  entry<U>(): Erased<U>;
}
