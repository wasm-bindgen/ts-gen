
// Per-monomorphization type parameters are unbounded, except where the
// output needs a bound: a parameter inside a callback's `Function<…>` stays
// a JS handle, and a dictionary helper restates the ABI bound of each setter
// argument shape it forwards (`&T`, `Option<T>`, `&[T]`).

interface Handler<Env = unknown> {
  fetch?: (request: string, env: Env) => void;
  name?: string;
}
declare function serve(h: Handler): void;

interface Rows<T> {
  rows: T[];
}
interface Slot<T> {
  value: T | undefined;
  fallback?: T;
}
declare function fill(r: Rows<string>, s: Slot<number>): void;
