//! @ts-gen --lib-name namespace-merge --export namespace-merge

// A namespace merged with a same-named class, function or interface leaves
// that declaration as the type, while `Name.Member` resolves through the
// namespace, whichever is declared first.

export declare class Foo {
  m(options: Foo.Options): void;
}
export declare namespace Foo {
  interface Options {
    a: string;
  }
}
export declare function makeFoo(): Foo;

export declare namespace Bar {
  type Power = "on" | "off";
}
export declare class Bar {
  power(power: Bar.Power): Bar;
}

export declare function make(config: make.Config): void;
export declare namespace make {
  interface Config {
    c: string;
  }
}

export interface Shape {
  axis(axis: Shape.Axis): void;
}
export declare namespace Shape {
  type Axis = "x" | "y";
}
