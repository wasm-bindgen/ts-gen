// A method-level type parameter no argument mentions after lowering is
// kept only when the (awaited) return is exactly `T`, `T | undefined` or
// `T | null`: there the turbofish is the cast. Nested in a container it
// erases to the default of the type it instantiates, else the method's
// own default, else `any`; absent from the return altogether it is
// dropped. `_promise` companions follow the awaited form.

interface Row<T = Record<string, unknown>> {
  results: T[];
}
interface Cursor<T = string> {
  next(): T;
}
interface Store {
  // 1. mentioned by an argument: inferred at the call.
  put<T>(key: string, value: T): Promise<void>;
  // 2. the whole return: kept, the caller names the cast.
  get<T = unknown>(key: string): Promise<T | undefined>;
  first<T = unknown>(): Promise<T | null>;
  json<T>(): Promise<T>;
  // A callback that lost `T` (its `Promise<T>` erases) does not count
  // as mentioning it; the whole-return rule still keeps it.
  transaction<T>(closure: (tx: Store) => Promise<T>): Promise<T>;
  // 3. nested: erased. `Row<T = Record<…>>` wins over the method's
  // `T = unknown`; a builtin container has no default, so the method's
  // applies; `Cursor<T = string>` has one even when the method does not.
  run<T = unknown>(): Promise<Row<T>>;
  list<T = unknown>(): Promise<Map<string, T>>;
  scan<T>(): Cursor<T>;
  // 4. absent from the return once the variadic callback lowers to the
  // untyped `Function`: dropped.
  schedule<Args extends unknown[]>(callback: (...args: Args) => void, ...args: Args): number;
}
declare function enterSpan<A extends unknown[], T>(name: string, fn: (...args: A) => T, ...args: A): T;
