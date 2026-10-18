//! @ts-gen --experimental-generic-mono

// Per-monomorphization type parameters are unbounded. A callback's
// `Function<fn(..) -> R>` takes native leaves and any parameter (`void` is
// `()`), and a dictionary helper restates the ABI bound of a setter argument
// only where it names a parameter bare (`Option<&T>`, `&[T]`).

interface Handler<Env = unknown> {
  fetch?: (request: string, env: Env) => void;
  scheduled?: (env: Env) => void | Promise<void>;
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
interface Step<T> {
  done?: boolean;
  value?: T | null;
}
declare function fill(r: Rows<string>, s: Slot<number>, t: Step<string>): void;

interface Store {
  blockConcurrencyWhile<T>(callback: () => Promise<T>): Promise<T>;
  forEach(callback: (value: string, key: string, parent: Store) => void): void;
  sync(): Promise<void>;
}
