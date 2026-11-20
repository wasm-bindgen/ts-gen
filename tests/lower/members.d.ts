//! @ts-gen --lib-name members

// Members: getters and setters, readonly, optional and nullable
// properties, statics, constructors, index signatures and dictionaries.

declare class Counter {
  constructor(start?: number);
  value: number;
  readonly label: string;
  maybe: string | null;
  missing?: string;
  either: string | null | undefined;
  handler: any | null;
  static readonly zero: Counter;
  static from(n: number): Counter;
  increment(by?: number): number;
  reset(): void;
  done(): Promise<void>;
}

interface Options {
  name: string;
  retries?: number;
  signal?: AbortSignal | null;
}

interface Bag {
  [key: string]: number;
}

declare function configure(options: Options): void;
declare function bag(): Bag;

// A member keyed by a well-known symbol (a method, property, accessor or
// static) is `@js(symbol = ...)`, named for the symbol, from any block of
// a merged declaration. One whose key collides with a string member's is
// renamed, still with no `name`.
// Other symbol keys have no IR form.
interface Lines {
  readonly count: number;
}
interface Lines {
  [Symbol.iterator](): Iterator<string>;
  readonly [Symbol.toStringTag]: string;
}
declare class Lock {
  release(): void;
  [Symbol.dispose](): void;
  dispose(force: boolean): void;
  static [Symbol.hasInstance](value: unknown): boolean;
  get [Symbol.species](): Lock;
  [Symbol.toPrimitive](hint: string): string;
  toPrimitive(hint: string): number;
}
