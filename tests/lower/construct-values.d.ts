//! @ts-gen --lib-name construct-values

// A constructor-shaped value a type alias names constructs the type its
// signatures return (IR.md §6): `@js(new = ...)` constructors on it, the
// value's own parameters mapped to the type's. A second source names its
// constructors after its value, a module's value reaches a global type
// through its spec, and a value returning another type keeps its getter.

interface Stub<T> {
  get(): T;
}
type Wrap<T> = Stub<T>;
declare const Wrap: { new <T>(value: T): Stub<T> };
type Wrap2<T> = Stub<T>;
declare const Wrap2: { new <T>(value: T): Stub<T> };

declare namespace Shapes {
  type Label = Labelled;
  const Label: {
    new (text: string): Labelled;
    new (n: number, unit: string): Labelled;
  };
  interface Labelled {
    text(): string;
  }
}

interface Options {
  depth?: number;
}
type Opts = Options;
declare const Opts: { new (depth: number): Options };

declare module "remote" {
  type Remote<T> = Stub<T>;
  export const Remote: { new <T>(value: T, at: string): Stub<T> };
  type Counter = Count;
  export const Counter: { new (start: number): Count };
  interface Count {
    count(): number;
  }
}

type Num = number;
declare const Num: { new (): number };
