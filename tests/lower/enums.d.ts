//! @ts-gen --lib-name enums

// Enums: a literal union becomes a `<Anchor>Kind` enum where it's first
// used, a named literal union keeps its name where it's declared, a union
// with value types becomes a payload enum, repeated values are aliases,
// and a type parameter bounded by an enum is bounded `in` it.

declare enum Level {
  Low = 1,
  High = 2,
  Max = 2,
}

declare const enum Format {
  Text = "text",
  Json = "json",
  Default = "text",
}

type Unit = "day" | "hour";

declare namespace Time {
  type Body = string | ArrayBuffer | ReadableStream;
  function add(amount: number, unit: Unit): void;
  function send(body: Body): void;
  function round<U extends Unit>(unit: U): U;
}

declare function level(): Level;
declare function format(f: Format): void;
declare function align(where: "start" | "end"): void;

type Base = "a" | "b";
type More = Base | "c";
type Code = 200 | 404 | -1;
declare function more(m: More): void;
declare function base(): Base;
declare function code(): Code;
declare function retry(c: 1 | 2 | 3): void;
declare function ratio(): 0.5 | 1;
declare function big(n: bigint): bigint;

// An enum merged with a namespace takes its members as statics.
declare enum Color {
  Red = 1,
  Green = 2,
}
declare namespace Color {
  function parse(s: string): Color;
  const all: Color[];
}

// An open literal set keeps its literals, and any other string in its
// fallback arm.
type Mode = "fast" | "slow" | string;
declare function mode(m: Mode): Mode;
declare function speed(m: "fast" | "slow" | string, hint?: "x" | (string & {})): void;
declare function tier(): "lo" | "hi" | string | number | null;
