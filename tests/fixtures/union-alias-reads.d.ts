//! @ts-gen --lib-name union-alias-reads --export union-alias-reads
// Reads resolve union aliases to their members, so nested aliases are one
// flat union with repeats dropped. Plain-object dictionaries carry nothing
// to tell them apart at runtime and share one arm; a union of nothing else
// reads as `JsValue`. Exercised against a real JS module in the
// integration tests.

export declare class Zoned {
  constructor(zone: string);
  readonly zone: string;
}

export declare class Day {
  constructor(day: number);
  readonly day: number;
}

export interface ZonedFields {
  zone: string;
}

export interface DayFields {
  day: number;
}

export type ZonedLike = Zoned | ZonedFields | string;
export type DayLike = Day | Zoned | DayFields | string;
export type Zone = Zoned | string;
export type Fields = ZonedFields | DayFields;

export interface RelativeOptions {
  /** Two aliases sharing `Zoned` and `string`, with a dictionary each. */
  relativeTo?: ZonedLike | DayLike | undefined;
  /** A union alias read whole. */
  zone: Zone;
  /** A lone dictionary keeps its own type. */
  zoned?: ZonedLike;
  /** Only dictionaries. */
  fields: Fields;
}

/** Options whose `relativeTo` holds the kind of value named. */
export declare function options(kind: string): RelativeOptions;

/** The kind of value named, as a function return. */
export declare function relativeTo(kind: string): ZonedLike | DayLike;
