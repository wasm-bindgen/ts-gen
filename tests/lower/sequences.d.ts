//! @ts-gen --lib-name sequences

// Sequences: a written typed array is the class and its copy
// `T[]` (a `number[]` beside it is the same copy); `arr<T>` and tuples are
// JS arrays; type arguments are boxes (`Arr`, `Tuple`, `Str`). A
// `{@slice view}` adds a zero-copy `*T[]` beside the copy, `@js(unshared)`
// over an `ArrayBuffer`.

declare function digest(data: Uint8Array): Uint8Array;
declare function fill(values: Float32Array | number[]): void;
declare function big(values: BigInt64Array): void;
declare function clamp(px: Uint8ClampedArray): void;
declare function pairs(): [string, number][];
declare function list(xs: Array<string>): ReadonlyArray<number>;
declare class Stream<T> {
  read(): T;
}
declare function bytes(): Stream<Uint8Array>;
declare function arrays(): Stream<string[]>;
declare function tuples(): Stream<[string, number]>;
/** @param buf {@slice view} filled with random bytes */
declare function random(buf: Uint8Array<ArrayBuffer>): void;
/** @param data {@slice view} copied by the callee */
declare function write(offset: number, data: Float32Array | number[]): void;
