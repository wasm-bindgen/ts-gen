//! @ts-gen --lib-name dictionary-polish --export tests/fixtures/dictionary-polish.d.ts

// Generic alias instantiations take short names when the argument fills the
// alias's role, reuse a declaration they are structurally identical to,
// all-optional dictionaries implement `Default`, and literal factories keep
// a builder.

declare global {
  namespace Polish {
    type PartialShapeLike<T extends object> = {
      [P in Exclude<keyof T, "id">]?: T[P] | undefined;
    };

    // Mapping over `keyof` a utility is no declared interface to reuse,
    // and adds only its own dictionary.
    type PartialOmitLike<T extends object> = {
      [P in keyof Omit<T, "id">]?: T[P];
    };

    interface BoxLikeObject {
      id: string;
      width?: number | undefined;
      height: number;
    }

    // Every member already optional, with no `id` to drop:
    // `PartialShapeLike<TimeLikeObject>` is this type.
    interface TimeLikeObject {
      hour?: number | undefined;
      minute?: number | undefined;
    }

    type TimeUnit = "hour" | "minute";
    type PluralizeUnit<T extends "day" | TimeUnit> =
      | T
      | { day: "days"; hour: "hours"; minute: "minutes" }[T];

    interface Options {
      box?: PartialShapeLike<BoxLikeObject>;
      time?: PartialShapeLike<TimeLikeObject>;
      omitted?: PartialOmitLike<BoxLikeObject>;
      unit?: PluralizeUnit<TimeUnit>;
    }

    interface Holder<T> {
      value?: T;
    }

    interface Transition {
      direction: "next" | "previous";
    }

    // A literal-only type parameter is erased, `Rounding` reading it at its
    // bound, and each instantiation is specialised instead. A use site
    // narrowing a member names a dictionary extending the base with the
    // narrowed members, shared by every instantiation narrowing alike, and
    // one at the bound is the base. A parent instantiation narrows the
    // extending interface's own members, and an alias of one is that
    // specialisation. A constraint written as literals is literal-only too,
    // and the parameter alone reads as its argument.
    interface Rounding<U extends "day" | TimeUnit> {
      smallestUnit?: PluralizeUnit<U>;
      largestUnit?: "auto" | PluralizeUnit<U>;
      increment?: number;
    }

    interface Stepped<S extends "hour" | "minute"> extends Rounding<S> {
      step?: S;
    }

    interface MinuteStepping extends Stepped<"minute"> {}

    type HourStepping = Stepped<"hour">;

    // An interface with methods can't be specialised, so its instantiations
    // are the base.
    interface Clock<U extends TimeUnit> {
      now(): PluralizeUnit<U>;
    }

    interface Rounder {
      round(options: Rounding<"day" | TimeUnit>): void;
      roundTime(options: Rounding<TimeUnit>): void;
      roundAgain(options: Rounding<Exclude<"day" | TimeUnit, "day">>): void;
      step(options: Stepped<"hour">): void;
      clock(): Clock<"hour">;
    }
  }
}

export {};
