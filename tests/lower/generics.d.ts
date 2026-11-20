//! @ts-gen --lib-name generics --bound Box.T=Str,f64 --bound Crate.T=Str

// Generics: type parameters and subtype bounds stay, boxed where
// they're arguments. A literal-only parameter stays generic, bounded `in`
// the enums its instantiations pass.

declare class Box<T> {
  constructor(value: T);
  get(): T;
  map<U>(f: (value: T) => U): Box<U>;
}

declare class Shape {
  area(): number;
}
declare function biggest<S extends Shape>(shapes: S[]): S;

declare function wrap(n: number): Box<number>;
declare function strings(): Box<string[]>;

type DateUnit = "year" | "month" | "day";
type TimeUnit = "hour" | "minute" | "second";
type Plural<T extends DateUnit | TimeUnit> = `${T}s`;

type Pluralize<T extends DateUnit | TimeUnit> =
  | T
  | { year: "years"; month: "months"; day: "days"; hour: "hours"; minute: "minutes"; second: "seconds" }[T];

type Amount<T extends DateUnit | TimeUnit> = `${number} ${T}`;

// A projection of a literal-only parameter is an associated type, with a
// row per argument (a template over literals too); one that isn't literals
// per literal reads at the bound.
interface RoundingOptions<Units extends DateUnit | TimeUnit> {
  amount?: Amount<Units>;
  smallestUnit?: Units;
  plural?: Plural<Units>;
  unit?: Pluralize<Units> | null;
  largestUnit?: "auto" | Pluralize<Units>;
}
interface TimeOptions extends RoundingOptions<Exclude<TimeUnit, "hour">> {}

declare function untilDate(options: RoundingOptions<DateUnit>): void;
declare function untilTime(options: RoundingOptions<TimeUnit>): void;
declare function untilAny(options: RoundingOptions<DateUnit | TimeUnit>): void;
// A few literals as an argument are named for themselves, one alone too,
// and a union's name follows its spelling.
declare function untilMonth(options: RoundingOptions<"year" | "month">): void;
declare function untilHour(options: RoundingOptions<"hour">): void;
declare function untilDay(options: RoundingOptions<"day" | TimeUnit>): void;
// A builtin `Intl` alias keeps its namespace.
declare function format(locales?: Intl.LocalesArgument): string;

// A parameter passed on instantiates its parent at each of its own
// arguments, so the parent's rows cover what the child reads through it.
interface RoundToOptions<Units extends DateUnit | TimeUnit> extends RoundingOptions<Units> {
  smallestUnit: Units;
}
declare function round(options: RoundToOptions<Exclude<DateUnit, "year">>): void;

// An enum has no type parameters, so a union over them reads as `Any`.
interface Step<T> {
  readonly value: T;
}
interface Done<R> {
  readonly result: R;
}
interface Stepper<T, R> {
  next(): Step<T> | Done<R>;
}

// A constructor has no type parameters in the IR: one reads as its
// default, else a class it's bounded by, else `Any?`.
declare class Store {}
interface View {}
declare var View: {
  new <S extends Store = Store>(store: S): View;
  new <S extends Store>(store: S, offset: number): View;
  new <T>(value: T, offset: number, length: number): View;
};

// A class's parent keeps its type arguments.
declare class Pipe<I = any, O = any> {
  write(chunk: I): O;
}
declare class Encoder extends Pipe<string, Uint8Array> {}

// A constraint written as literals is literal-only too. A class's
// instantiations are the class, so one an interface merges into reads its
// parameter as the primitive.
interface Stepped<S extends "hour" | "minute"> extends RoundingOptions<S> {
  step?: S;
}
// Literals beside the parameter alone are an associated type too, named
// within its family where another family's row differs.
interface Spanning<U extends "hour" | "week"> {
  smallestUnit?: U;
  largestUnit?: "auto" | U;
}
declare function step(options: Stepped<"hour" | "minute">, span: Spanning<"hour" | "week">): void;
declare function spanHour(span: Spanning<"hour">): void;
interface Gauge<S extends "low" | "high"> {
  level(): S;
}
declare var Gauge: {
  new (): Gauge<"low">;
};

// A type parameter bounded by `Record<K, V>` reads as `V` at any key.
declare class Signal {}
declare class Bus<M extends Record<string, Signal> = Record<string, Signal>> {
  emit(signal: M[keyof M]): void;
}

// A key parameter, `K extends keyof M`, erases to `str`, and `M[K]` reads
// at its bound's keys, so as any of `M`'s values.
declare class Halt {}
interface SignalMap {
  start: Signal;
  halt: Halt;
}
declare function listen<K extends keyof SignalMap>(type: K, handler: (signal: SignalMap[K]) => void): void;

// A bound the IR can't hold is diagnosed and leaves the parameter
// unbounded, as is a `--bound` naming no parameter (`Crate.T`).
declare function first<T extends Signal[]>(items: T): T;

// A construct signature building the class at its own parameters, in
// order, takes the class's parameters for them, whatever their names.
interface Cell<T> {
  readonly value: T;
}
declare var Cell: {
  new <U>(value: U): Cell<U>;
};
