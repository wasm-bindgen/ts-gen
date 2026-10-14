//! @ts-gen --error-type crate::Error

// Fixture: `--error-type <path>` sets the default error type for fallible
// bindings (those without a `@throws` annotation) to an arbitrary Rust
// path. The path must implement `From<JsValue>`. `@throws` annotations
// naming a specific type are unaffected.

interface Foo {
  /** Plain method — the `try_` companion uses the configured error type. */
  bar(x: number): string;

  /** Async method — `Result<T, crate::Error>`. */
  baz(): Promise<number>;

  /**
   * Explicit `@throws` still wins over the configured default.
   *
   * @throws {TypeError} when input is invalid
   */
  parse(input: string): number;
}

declare class Resource {
  /** Constructors always catch; the error type follows the flag. */
  constructor(path: string);
}

/** Free function — same default-error rule. */
declare function fetchData(url: string): Promise<string>;
