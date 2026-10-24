//! @ts-gen --lib-name type-eval --export type-eval

// Type-level constructs evaluate over concrete operands: utilities,
// `keyof`, indexed access and mapped types, through generic aliases.

export type Unit = "hour" | "minute" | "second";

// A utility named at a use site becomes a string enum of its spelling.
export declare function roundTo(unit: Exclude<Unit, "hour">): void;
export declare function pickUnit(unit: Extract<Unit | "day", Unit>): void;
export declare function present(value: NonNullable<string | null | undefined>): void;

export interface Options {
  /** The name. */
  name: string;
  size?: number | undefined;
  readonly fixed: boolean;
}

// Object utilities synthesise a dictionary, or keep the type when they
// change nothing.
export declare function configure(
  partial: Partial<Options>,
  required: Required<Options>,
  frozen: Readonly<Options>,
  picked: Pick<Options, "name" | "size">,
  omitted: Omit<Options, "fixed">,
  unchanged: Partial<Pick<Options, "size">>,
): void;

// An alias to an evaluated type is promoted under its own name.
export type Sized = Pick<Options, "size">;
export type Minute = Exclude<Unit, "hour" | "second">;
export type OptionKey = keyof Options;

// A utility in `extends` has no parent to defer to, so it flattens.
export interface Named extends Omit<Options, "size"> {
  label?: string | undefined;
}
export interface Mixed extends Pick<Options, "name">, Partial<Pick<Options, "size">> {}

// Mapped types over a literal key set, homomorphic or not.
export type Loose<T> = { [P in keyof T]?: T[P] | undefined };
export type Strict<T> = { [P in keyof T]-?: T[P] };
export type Flags<K extends string> = { [P in K]: boolean };
export declare function mapped(
  loose: Loose<Options>,
  strict: Strict<Options>,
  flags: Flags<"a" | "b">,
): void;

// Indexed access distributes over a literal key union.
export type Plural<T extends Unit> = T | { hour: "hours"; minute: "minutes"; second: "seconds" }[T];
export declare function plural(all: Plural<Unit>, some: Plural<"hour" | "minute">): void;

// A type parameter out of scope evaluates at its bound: one enum for every
// instantiation, named after the alias.
export interface Rounding<U extends Unit> {
  smallestUnit?: Plural<U> | undefined;
  largestUnit?: "auto" | Plural<U> | undefined;
}
export declare function round(opts: Rounding<"hour">): void;

// Inherited properties take part in `keyof` and the object utilities.
export interface Extended extends Options {
  extra: number;
}
export declare function extended(keys: keyof Extended, partial: Partial<Extended>): void;

// `never` has no value.
export declare function fail(): never;
export declare function fallback(value: Exclude<Unit, Unit>): void;

export declare namespace Scoped {
  type Mode = "fast" | "slow";
  type NotFast = Exclude<Mode, "fast">;
  interface Settings { mode?: Mode | undefined; depth: number }
  function apply(settings: Partial<Settings>, mode: NotFast): void;
}

// Methods have no property type: a utility over a class keeps only its
// properties, and an unconstrained parameter can't be evaluated.
export declare class Widget {
  size: number;
  render(): void;
}
export declare function widget(partial: Partial<Widget>): void;
export declare function unbound<T>(keys: keyof T, partial: Partial<T>): void;
