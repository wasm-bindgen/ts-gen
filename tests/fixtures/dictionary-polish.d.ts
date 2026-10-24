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
  }
}

export {};
