//! @ts-gen --lib-name nullability --export nullability

// Every combination of `null` and `undefined` a declaration can admit, at
// top-level and inner positions. Reads keep exact carriers; writes take
// `null` as an arm of its own (`f(x)` and `f_with_null()`), and `undefined`
// in an omittable parameter as omission (`f()`). The mono fixture shares
// this file, so the names match across modes.

type MaybeStr = string | null;
type OptStr = string | undefined;
type Mode = "fast" | "slow";

export interface Settings {
  plain?: string;
  explicit?: string | undefined;
  required: string | undefined;
  nullable: string | null;
  both?: string | null;
  aliased?: MaybeStr;
  mode?: Mode | null;
  empties: null | undefined;
  limit?: number | null;
  owner?: Registry | null;
  values?: number[] | null;
  key?: string | number | null;
}

export declare class Registry {
  static current?: Registry;
  static fallback: Registry | null;
  static last?: Registry | null;
  readonly pending: Promise<string | null>;
  readonly settled: Promise<number | undefined>;
}

export declare class Holder<T> {
  get(): T;
  readonly value: T | null;
}

export declare class Body {
  constructor(body: string | null, length?: number | null);
  put(value: string | null): void;
  static from(owner: Registry | null): Body;
}

export declare function optional(x: string | undefined): void;
export declare function nullable(x: string | null): void;
export declare function both(x: string | null | undefined): string | null | undefined;
export declare function optionalBoth(x?: string | null): void;
export declare function aliased(x: MaybeStr | undefined): MaybeStr | undefined;
export declare function aliasedNull(x: MaybeStr | null): MaybeStr | null;
export declare function optAliasedNull(x: OptStr | null): OptStr | null;
export declare function count(x: number | null | undefined): number | null | undefined;
export declare function countNull(x: number | null): void;
export declare function countOptional(x?: number | null): void;
export declare function flag(x: boolean | null): boolean | null;
export declare function registry(r: Registry | null | undefined): Registry | null | undefined;
export declare function registryNull(r: Registry | null): void;
export declare function registryOptional(r?: Registry | null): void;
export declare function scalar(x: string | number | null | undefined): void;
export declare function scalarNull(x: string | number | null): void;
export declare function scalarOptional(x?: string | number | null): void;
export declare function pair(x: string | number): void;
export declare function leading(a: string | undefined, b: number): void;
export declare function leadingNull(a: string | null | undefined, b: number): void;
export declare function trailing(a: number, b: string | undefined): void;
export declare function chain(a: string | undefined, b: number | null | undefined): void;
export declare function chainOptional(a?: string, b?: number | null): void;
export declare function chainThree(a?: string, b?: number | null, c?: boolean): void;
export declare function modeArg(m: Mode | null): Mode | null;
export declare function modeBoth(m: Mode | null | undefined): Mode | null | undefined;
export declare function modeOptional(m?: Mode | null): void;
export declare function emptiesOnly(x: null | undefined): null | undefined;
export declare function generic<T>(x: T | null): T | null;
export declare function genericBoth<T>(x: T | null | undefined): T | null | undefined;
export declare function inner(xs: Promise<string | null>, ys: Array<number | undefined>): void;
export declare function innerBoth(xs: Promise<string | null | undefined>): void;
export declare function callback(cb: (x: string | null) => void): void;
export declare function record(r: Record<string, string | null>): Record<string, number | undefined>;
export declare function holder(h: Holder<string | null>): Holder<number | undefined>;
export declare function fetchNull(): Promise<Registry | null>;
export declare function fetchOptional(): Promise<number | undefined>;
export declare function fetchBoth(): Promise<string | null | undefined>;
export declare function fetchGeneric<T>(): Promise<T | null>;
export declare function numbers(xs: number[] | null): number[] | null;
export declare function numbersBoth(xs: number[] | null | undefined): number[] | null | undefined;
export declare function numbersOptional(xs?: number[] | null): void;
export declare function bytes(xs: Uint8Array | number[] | null): void;
export declare function bytesBoth(xs: Uint8Array | number[] | null | undefined): void;
export declare function jsdoc(x: ?string): void;

export type Attachment =
  | { disposition: "inline"; contentId: string; filename: string }
  | { disposition: "attachment"; contentId?: undefined; filename: string };
export declare function send(attachment: Attachment): void;

export type Upload =
  | { kind: "file"; path: string }
  | { kind: "none"; path?: null }
  | { kind: "skip"; path?: undefined };
export declare function upload(upload: Upload): void;
export type Cover =
  | { kind: "image"; src: string }
  | { kind: "blank"; src: null };
export declare function cover(cover: Cover): void;
