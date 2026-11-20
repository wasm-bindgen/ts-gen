//! @ts-gen --with std/std.ir

// `--with std/std.ir` resolves lib types against the `std` lib IR (§5)
// instead of stubbing them: `with std;`, and no declarations of its own
// for `Date`, `Map` or `Uint8Array`. A lib type `std` doesn't declare is
// still stubbed, and the input's own declarations come first.

interface Entry {
  readonly created: Date;
  readonly tags: Map<string, number>;
  readonly body: Uint8Array;
  readonly pattern: RegExp | null;
  readonly other: WeakRef<Entry>;
}

interface Error {
  readonly code: number;
}

declare function fail(reason: string): Error;

// A lib union alias read beside `undefined` or `null` is its enum, as it is
// alone.
interface FormatSettings {
  locales?: Intl.LocalesArgument;
  required: Intl.LocalesArgument;
  nullable: Intl.LocalesArgument | null;
}

// A literal set a lib enum holds exactly is that enum.
declare function display(style: "long" | "short"): void;

// Type parameter defaults carry over; an `undefined` argument is `unit`.
interface Store<Env = undefined, Key extends string = string> {
  env(): Env;
}
declare function store(s: Store): Promise<never>;

// `symbol` is std's `Symbol`, and `PropertyKey` a union over it.
declare function describe(s: symbol): string;
declare function keyOf(k: PropertyKey): void;
declare const tag: unique symbol;
