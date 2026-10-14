//! @ts-gen --export tests/fixtures/zena-names.d.ts --export mail
// Declarations named like what `--target zena` defines or references
// itself, and names the target synthesises beside declared ones.
declare class Error {
  constructor(message: string);
  readonly message: string;
}
declare class String {
  readonly length: number;
}
declare class FixedArray {}
declare function isEmpty(x: string): boolean;
declare function isUndefined(x: string): boolean;
declare function numberValue(): number;
declare const Undefined: number;
declare function await_(p: Promise<void>): void;
declare function describe(e: Error, s: String): string;
// A declared static `create` keeps its name; the constructor yields.
declare class Message {
  constructor();
  static create(n: number): Message;
}
// A module function's overloads take the specifier prefix together.
declare function send(m: string): void;
declare module "mail" {
  function send(m: string): void;
  function send(m: string, opt: number): void;
}
