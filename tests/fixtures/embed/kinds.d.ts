// Kinds for the discriminator run, declared beside cloudflare-worker.d.ts
// (which has `HeadersKind`), each a function's result so that the IR has it.
type Method = "GET" | "POST";
declare enum Level {
  Low = 1,
  High = 2,
}
declare namespace Level {
  function name(level: Level): string;
}
interface Point {
  x: number;
}
type Mixed = "auto" | number | Request | Point;
type Lists = string[] | number[] | [string, number] | ((x: number) => void) | boolean | bigint;
type Nested = Method | Request;
declare function method(): Method;
declare function level(): Level;
declare function mixed(): Mixed;
declare function lists(): Lists;
declare function nested(): Nested;
declare module "embed:kinds" {
  class Thing {}
  function things(): Thing | string[];
  enum Shade {
    Light = "light",
    Dark = "dark",
  }
  function shade(): Shade;
}
