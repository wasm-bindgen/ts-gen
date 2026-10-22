//! @ts-gen --input tests/fixtures/duplicate-identifiers/other.d.ts

// TypeScript rejects a declaration whose kind can't merge with one its name
// already has ("Duplicate identifier"): a type alias merges with no other
// type declaration, a class with no other class, and an enum with no class,
// interface or type alias. The first declaration keeps the name and the
// later one is skipped with a warning, also when it is in a later file
// (`other.d.ts`).

type Dup = { a: number };
interface Dup {
  b: string;
}

// The skipped alias's inline `{ x }` hoists nothing.
interface IfaceFirst {
  b: string;
}
type IfaceFirst = { a: { x: number } };

type Prim = string;
interface Prim {
  b: string;
}

type Mode = "on" | "off";
type Mode = "up" | "down";

declare class TwoClasses {
  a(): void;
}
declare class TwoClasses {
  b(): void;
}

declare class Widget {
  w(): void;
}
type Widget = { a: number };

declare enum Color {
  Red,
}
interface Color {
  x: number;
}

interface Later {
  x: number;
}
declare enum Later {
  A,
}

declare namespace N {
  type T = { a: number };
  interface T {
    b: string;
  }
}

// Every merge TypeScript allows still happens.
interface Merged {
  a: number;
}
interface Merged {
  b: string;
}

interface Both {
  i(): void;
}
declare class Both {
  c(): void;
}

declare class Late {
  c(): void;
}
interface Late {
  i(): void;
}

declare var VarClass: { prototype: VarClass; new (): VarClass };
interface VarClass {
  v(): void;
}

type Opts = { a: number };
declare namespace Opts {
  function make(): Opts;
}

declare enum Level {
  Low,
}
declare enum Level {
  High = 1,
}
declare namespace Level {
  function parse(s: string): Level;
}

declare function use(
  dup: Dup,
  first: IfaceFirst,
  prim: Prim,
  mode: Mode,
  two: TwoClasses,
  widget: Widget,
  color: Color,
  later: Later,
  t: N.T,
): void;
