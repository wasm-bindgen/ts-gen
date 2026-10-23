//! @ts-gen --lib-name enum-positions --export enum-positions

// Enums are named only in top-level argument / return positions, where
// they pass by value, and as dynamic-union payloads. Inside slices,
// generics and callbacks they lower as the string / number they carry.
// Per-monomorphization generics carry native ABI types instead, so an enum
// stays named inside a locally declared generic and an awaited Promise,
// exactly as string / number stay String / f64 there.

export type Mode = "fast" | "slow";
export type ModeAlias = Mode;
export declare enum Level {
  Low = 1,
  High = 2,
}

export declare namespace Codes {
  type Region = "eu" | "us";
}

export declare class Holder<T> {
  constructor(value: T);
  get(): T;
  set(value: T): void;
}

export interface Details<T> {
  value: T;
  fallback?: T;
}

export declare class Probe {
  level: Level;
  region(r: Codes.Region): void;
  regionOf(): Codes.Region;
  alias(m: ModeAlias): void;
  levels(ls: Level[]): Level[];
  modes(ms: Mode[]): Mode[];
  modeArray(ms: ArrayLike<Mode>): ArrayLike<Mode>;
  rest(...ms: Mode[]): void;
  onMode(cb: (m: Mode) => Level): void;
  nextMode(): Promise<Mode>;
  maybeMode(): Promise<Mode | null>;
  modeMap(): Map<string, Mode>;
  modeRecord(): Record<string, Mode>;
  pair(): [Mode, Level];
  modeOrNumber(): Mode | number;
  modeOrString(): Mode | string;
  modeOrRegion(): Mode | Codes.Region;
  levelOrString(): Level | string;
  holder(h: Holder<Mode>): Holder<Level>;
  details(d: Details<ModeAlias>): Details<Level>;
  maybeHolder(): Holder<Mode | null>;
  nextLevel(): Promise<Level | undefined>;
  nextDetails(): Promise<Details<Mode>>;
  modeOrNumberArg(x: Mode | number): Mode | number;
  modeOrNumberOrString(x: Mode | number | string): void;
}
