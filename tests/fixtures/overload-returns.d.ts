// Overloads keep their own return types: `get(key)` resolves to the
// value, `get(keys)` to a Map, and `get_with_keys_promise` to the Map's
// promise; a sync overload next to an async one stays sync (and keeps
// its `try_` companion).

interface Storage {
  get<T = unknown>(key: string): Promise<T | undefined>;
  get<T = unknown>(keys: string[]): Promise<Map<string, T>>;
  // Each overload's own type parameters are declared on its expansions
  // (`U` is inferred from `fallback`; `get(keys)`'s nested `T` erases).
  fetch<T = unknown>(key: string): Promise<T>;
  fetch<U = unknown>(keys: string[], fallback: U): Promise<Map<string, U>>;
  first(): string;
  first(column: string): Promise<string | null>;
}
