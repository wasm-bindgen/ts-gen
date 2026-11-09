//! @ts-gen --lib-name never

// A `never` return always throws, even against `@throws {never}`. A
// property that holds no value (`never`, `undefined`) isn't lowered, and
// either inside another type reads as `Any?`.

declare function fail(message: string): never;
/** @throws {never} */
declare function failAnyway(): never;
declare function rejectLater(): Promise<never>;
declare const impossible: never;

interface A { a: string }
interface B { b: string }
interface Result<T = undefined> {
  value: T;
  error?: never;
  absent?: undefined;
  // `A[] & B[]` is an array of `A & B`.
  both: A[] & B[];
}
declare function result(r: Result): void;
// An `undefined` default reaches the alias's arguments as `unit`.
type Fetcher<T = undefined> = Result<T>;
declare function fetcher(f: Fetcher): void;
