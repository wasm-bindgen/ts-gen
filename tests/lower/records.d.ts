//! @ts-gen --lib-name records

// `Record<K, V>` and index signatures are `@js(index)` access: producer-named
// `at` and `setAt`. A `Record` is a lib interface, keyed by `str` unless `K`
// is `number`.

interface Env {
  vars: Record<string, string>;
  readonly flags: Record<number, boolean>;
  lookup(headers: Record<string, string | string[]>): Record<string, Env>;
  configure(options: { [name: string]: number }): void;
}

interface Bag {
  readonly [key: string]: unknown;
}

interface List {
  [index: number]: string;
  readonly length: number;
}
