//! @ts-gen --experimental-generic-mono --annotate tests/fixtures/side/string-modes-mono.d.ts

// Under per-monomorphization a string return gets a `String` binding and a
// `_js_string` twin; `{@string copy}` keeps only the `String` form and
// `{@string ref}` only the handle, under the base name.

declare abstract class Headers {
  /** @returns The header value {@string copy} */
  get(name: string): string | null;
  /** {@string ref} */
  getAll(name: string): string[];
  keys(): IterableIterator<string>;
  /** {@string copy} */
  readonly type: string;
  readonly use: string;
  static readonly origin: string;
}

/** {@string copy} */
interface Meta {
  readonly id: string;
  readonly tags: string[];
  /** {@string both} */
  readonly body: string;
  /** {@string ref} */
  text(): Promise<string>;
}

interface Body {
  readonly contentType: string;
  readonly length: number;
  text(): Promise<string>;
  json(): Promise<unknown>;
}

interface Cookies {
  readonly first: string;
  readonly second: string;
}

/** @returns {string} the id {@string copy} */
declare function makeId(): string;
declare function makeName(): string;
