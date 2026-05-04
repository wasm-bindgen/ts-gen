// Fixture: `@throws {never}` JSDoc → opt out of fallible variants.
//
// Sync callables annotated with `@throws {never}` get *no* `try_*`
// companion. Async (Promise-returning) callables drop the `Result`
// wrapper and `catch` attribute, becoming `async fn -> T`.
// Constructors ignore the annotation — JS `new` always catches.

declare class NeverThrows {
  /**
   * Always succeeds — no `try_safe_op` companion.
   * @throws {never}
   */
  safeOp(x: number): string;

  /**
   * Async, declared infallible — emits `async fn -> Loaded` (no Result).
   * @throws {never}
   */
  loaded(): Promise<Loaded>;

  /**
   * Sync method without `@throws {never}` — keeps the `try_normal_op`
   * companion as usual.
   */
  normalOp(x: number): string;

  /**
   * Sync with a typed throws — keeps `try_typed_op` returning the
   * typed error. `Throws::Type` is the existing behavior.
   * @throws {RangeError}
   */
  typedOp(x: number): string;

  /**
   * Constructor with `@throws {never}` is a no-op — JS `new` can
   * always throw, so the constructor still catches.
   * @throws {never}
   */
  constructor(name: string);
}

interface Loaded {
  readonly ready: boolean;
}

/**
 * Free function with `@throws {never}` — no `try_pure_compute`.
 * @throws {never}
 */
declare function pureCompute(x: number): number;

/**
 * Async free function with `@throws {never}` — no Result wrapper.
 * @throws {never}
 */
declare function infallibleFetch(url: string): Promise<string>;
