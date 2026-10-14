// The unsuffixed name belongs to the first overload's shortest expansion,
// unless a shorter expansion shares its leading parameter names: `(key)`
// owns `get` even though the required-options overload is declared
// first, and `first()` owns `first`. `put(entries)` has different
// leading names, so `put(key, value)` keeps the base name.

interface GetOptions {
  range?: string;
}
interface Conditional {
  etag?: string;
}
interface Store {
  get(key: string, options: Conditional): Promise<string | null>;
  get(key: string, options?: GetOptions): Promise<string>;
  first(column: string): Promise<string | null>;
  first(): Promise<string | null>;
  put(key: string, value: string, options?: GetOptions): Promise<void>;
  put(entries: Record<string, string>, options?: GetOptions): Promise<void>;
}
