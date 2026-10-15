//! @ts-gen --lib-name enum-aliases --export enum-aliases

// A member whose value repeats an earlier member's is an alias of it, not a
// second variant: wasm-bindgen rejects duplicate discriminants and could not
// tell two same-valued string variants apart when decoding. Initialisers
// that reference earlier members or combine them in constant expressions
// evaluate to their value rather than taking the next auto-increment.

export declare enum Version {
  V1 = 1,
  V2 = 2,
  Latest = 2,
  Current = V2,
  Qualified = Version.V1,
  Next,
}

export declare enum Flags {
  None = 0,
  Read = 1 << 0,
  Write = 1 << 1,
  ReadWrite = Read | Write,
  All = ReadWrite | 4,
  Mask = ~0,
  Negated = -Read,
}

export declare enum Kind {
  Text = "text",
  Bytes = "bytes",
  Default = "text",
  Fallback = Text,
}

// Classification follows the evaluated values, so an enum of constant
// expressions alone is numeric.
export declare enum Bits {
  Low = 1 << 0,
  High = 1 << 1,
  Both = Low | High,
}

// A member referencing something outside the enum has a value we cannot
// evaluate, so it is skipped rather than given a guessed discriminant that
// could collide with a real member.
export declare enum Other {
  X = 7,
}

export declare enum Unresolved {
  A = 1,
  B = Other.X,
  C = 2,
}

export interface Api {
  version: Version;
  flags: Flags;
  kind: Kind;
}
