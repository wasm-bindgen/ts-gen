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
  type Mode = "on" | "off";
}
export declare class Bar {
  mode(mode: Bar.Mode): Bar;
}

export declare function make(config: make.Config): void;
export declare namespace make {
  interface Config {
    c: string;
  }
}

export interface Shape {
  kind(kind: Shape.Kind): void;
}
export declare namespace Shape {
  type Kind = "x" | "y";
}
