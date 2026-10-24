//! @ts-gen --lib-name union-alias-reads --export union-alias-reads
// Union aliases. A named alias whose arms a read can tell apart (classes
// and primitives, with at most one dictionary) is one enum, which its reads
// return and its arguments take. One with several dictionaries is an
// argument enum only, and its reads, like those of an anonymous union of
// aliases, resolve the aliases to their members: nested aliases are one flat
// union with repeats dropped, and plain-object dictionaries share one arm.
// Exercised against a real JS module in the integration tests.

export declare class Zoned {
  constructor(zone: string);
  readonly zone: string;
}

export declare class Day {
  constructor(day: number);
  readonly day: number;
  static from(item: DayLike, options?: FromOptions): Day;
  static compare(one: DayLike, two: DayLike): number;
  equals(other: DayLike): boolean;
  until(other: DayLike, options?: FromOptions): number;
  shift(offset: Offset): Day;
  // A string-literal cohort beside an alias argument.
  step(direction: "next" | "previous", other: DayLike): string;
  pick(other?: MaybeDay): string;
  // `undefined` on a parameter that cannot be omitted takes the enum.
  between(start: DayLike | undefined, end: DayLike): string;
  later(other: DayLike): Promise<string>;
}

export interface ZonedFields {
  zone: string;
}

export interface DayFields {
  day: number;
}

export interface FromOptions {
  overflow?: "constrain" | "reject";
}

export type ZonedLike = Zoned | ZonedFields | string;
export type DayLike = Day | Zoned | DayFields | string;
export type Zone = Zoned | string;
// Two dictionaries a read can't tell apart.
export type Fields = ZonedFields | DayFields;
// Primitive arms convert from their Rust values.
export type Offset = number | string | boolean;
// The empties of an alias keep the nullability model: `null` is an arm of
// its own, `undefined` omits.
export type MaybeDay = DayLike | null | undefined;

export interface RelativeOptions {
  /** Two aliases sharing `Zoned` and `string`, with a dictionary each. */
  relativeTo?: ZonedLike | DayLike | undefined;
  /** A union alias read whole. */
  zone: Zone;
  /** Its dictionary arm is the read's fallback. */
  zoned?: ZonedLike;
  /** Only dictionaries. */
  fields: Fields;
}

export interface Span {
  start: DayLike;
  relativeTo?: DayLike;
}

/** Options whose `relativeTo` holds the kind of value named. */
export declare function options(kind: string): RelativeOptions;

/** The kind of value named, as a function return. */
export declare function relativeTo(kind: string): ZonedLike | DayLike;

/** The arm the item reached JS as, and its day. */
export declare function describe(item: DayLike): string;
export declare function describeSpan(span: Span): string;
export declare function describeFields(fields: Fields): string;
// An anonymous union keeps one binding per arm.
export declare function either(item: Day | string): string;
