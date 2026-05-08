//! @ts-gen --errors-as-error

// Fixture: with `--errors-as-error`, the default error type for fallible
// bindings (those without a `@throws` annotation) is `Error` instead of
// `JsValue`. `@throws` annotations naming a specific type are unaffected.

interface Foo {
  /** Plain method — fallible variant uses the new default error type. */
  bar(x: number): string;

  /**
   * Async method — `Result<T, Error>` instead of `Result<T, JsValue>`.
   */
  baz(): Promise<number>;

  /**
   * Method with explicit `@throws` — the typed error wins regardless
   * of the flag.
   *
   * @throws {TypeError} when input is invalid
   */
  parse(input: string): number;
}

declare class Resource {
  /** Constructor — wasm-bindgen always catches; default error follows the flag. */
  constructor(path: string);
}

/** Free function — same default-error rule. */
declare function fetchData(url: string): Promise<string>;
