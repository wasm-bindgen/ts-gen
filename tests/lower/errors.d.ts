//! @ts-gen --lib-name errors

// Errors and async: `@throws {X}` names an enum, shared by every
// use of the same classes, so a typed member is never bare `throws`; on a
// promise it's the rejection, `future<T, E>`. It extends `err` with an arm
// per class, a subclass first, unless it names `TypeError` or `RangeError`,
// which `err` folds together: then it has an arm per class and its own
// `@fallback Error`.
// A future-returning call taking a dictionary with an `AbortSignal`
// `signal` aborts through a fresh controller when the future is dropped.

declare class DOMException extends Error {
  constructor(message?: string, name?: string);
  readonly name: string;
}
declare class QuotaExceededError extends Error {}

interface Kv {
  /** @throws {DOMException} */
  get(key: string): string;
  /** @throws {DOMException | QuotaExceededError} */
  put(key: string, value: string): Promise<void>;
  /** @throws {TypeError} */
  list(): string[];
  /** @throws {DOMException} */
  readonly size: number;
  /** @throws {TypeError | RangeError} */
  check(key: string): void;
  /** @throws {Error | QuotaExceededError | DOMException} */
  grow(): void;
  /** @throws {Error} */
  close(): void;
  count(): number;
}
declare class StorageError extends DOMException {}
/** @throws {DOMException | StorageError} */
declare function open(): void;

interface AbortSignal {
  readonly aborted: boolean;
}
declare class AbortController {
  constructor();
  readonly signal: AbortSignal;
  abort(reason?: any): void;
}
interface RequestInit {
  method?: string;
  signal?: AbortSignal | null;
}
declare function fetch(input: string, init?: RequestInit): Promise<string>;
