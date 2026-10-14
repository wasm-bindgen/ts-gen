//! @ts-gen --fallible --error-type crate::Error

// Fixture: `--fallible` — sync methods, static methods, free functions and
// constructors emit one `Result`-returning binding under the primary name
// (no panicking variant, no `try_` prefix). Getters/setters stay
// infallible unless annotated `@throws {X}`; `@throws {never}` makes a
// callable plain.

declare class Headers {
  constructor(init?: Record<string, string>);

  /** Sync method — `Result<Option<String>, crate::Error>`, no `try_get`. */
  get(name: string): string | null;

  /**
   * Sync void method with a typed throws — `Result<(), TypeError>`.
   * @throws {TypeError} on an invalid header name
   */
  set(name: string, value: string): void;

  /**
   * Declared infallible — plain `bool`.
   * @throws {never}
   */
  has(name: string): boolean;

  /** Async — unchanged: already a single `Result` form. */
  text(): Promise<string>;

  /** Getter — infallible by default. */
  readonly size: number;

  /**
   * Getter with a typed throws — `Result<String, RangeError>` with `catch`.
   * @throws {RangeError} when the body was already consumed
   */
  readonly consumedText: string;

  /** Writable property — the setter never catches. */
  label: string;

  /** Static method — same rule as instance methods. */
  static parse(raw: string): Headers;

  /**
   * Static getter with a typed throws.
   * @throws {TypeError}
   */
  static readonly defaults: Headers;
}

/** Free function — `Result<f64, crate::Error>`. */
declare function compute(x: number): number;

/**
 * Free function declared infallible — plain `f64`.
 * @throws {never}
 */
declare function pureCompute(x: number): number;
