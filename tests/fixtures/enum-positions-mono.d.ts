//! @ts-gen --lib-name enum-positions-mono --export enum-positions-mono

// Per-monomorphization generics carry native ABI types, so an enum stays
// named inside a locally declared generic and an awaited Promise, exactly
// as string / number stay String / f64 there. Slices and js_sys containers
// still lower it as its primitive.

export type Mode = "fast" | "slow";
export type ModeAlias = Mode;
export declare enum Level {
  Low = 1,
  High = 2,
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
  alias(m: ModeAlias): void;
  holder(h: Holder<Mode>): Holder<Level>;
  details(d: Details<ModeAlias>): Details<Level>;
  maybeHolder(): Holder<Mode | null>;
  nextMode(): Promise<Mode>;
  nextLevel(): Promise<Level | undefined>;
  nextDetails(): Promise<Details<Mode>>;
  levels(ls: Level[]): Level[];
  modes(ms: Mode[]): Mode[];
  modeArray(ms: ArrayLike<Mode>): ArrayLike<Mode>;
  onMode(cb: (m: Mode) => Level): void;
  modeOrNumber(x: Mode | number): Mode | number;
  modeOrNumberOrString(x: Mode | number | string): void;
}
