
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

// An omitted argument is left to the Rust alias's default, which resolves
// where the alias is declared (`Ns.Inner`). `Chained`'s default names the
// dropped `T`, so the use site fills it in. A bare alias means its defaults.
type WithDefault<T, U = string> = Pair<T, U>;
type Chained<T, U = T> = Pair<U, U>;
type Fetcher<T = undefined, Reserved = never> = Pair<T, T>;
interface Inner {
  top: string;
}
declare namespace Ns {
  interface Inner {
    ns: number;
  }
  type WithInner<X, T = Inner> = Pair<X, T>;
  type Fetcher<T = undefined, Reserved = never> = Pair<T, T>;
}
declare function defaults(
  a: WithDefault<number>,
  b: Chained<number>,
  c: Fetcher,
  d: Fetcher<string>,
  e: Ns.WithInner<string>,
  f: Ns.Fetcher,
): void;
declare function defaultReturn(): WithDefault<number>;
declare function skipReturn(): Skip<number, string, boolean>;

// A parameter only spelled as a path segment isn't used, and an alias to an
// enum drops every parameter.
declare namespace Cf {
  interface Env {
    e: string;
  }
}
type Handler<Env = Cf.Env> = (env: Cf.Env) => void;
declare enum Color {
  Red = "red",
  Green = "green",
}
type ColorAlias<T> = Color;
declare namespace Ns {
  type ColorOf<T> = Color;
}
// A parameter target is an enum only through its argument or default.
type Chosen<T = Color> = T;
declare function misc(
  handler: Handler<string>,
  color: ColorAlias<number>,
  qualified: Ns.ColorOf<string>,
  chosen: Chosen,
  given: Pair<Chosen<number>, string>,
): void;
declare function qualifiedReturn(): Ns.WithInner<string>;

interface Holder<T> {
  get(): Erased<T>;
  take(value: Skip<T, number, string>): void;
  entry<U>(): Erased<U>;
}
