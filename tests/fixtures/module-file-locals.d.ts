//! @ts-gen --lib-name module-file-locals --export module-file-locals

// In a module file every top-level declaration belongs to the module, with
// or without an `export` prefix: an unexported one can be exported by name
// through `export { _X as X }`, or be referred to by exported declarations.
// Treating it as global would drop it from the output while the module's
// exports still refer to it.
//
// Only what the module exports reaches JS as an import, though. An
// unexported function or variable has nothing to bind to, and an
// unexported class keeps only its instance members, which bind through a
// value rather than by importing the class.

interface _Foo {
  bar(): void;
}
declare var _Foo: { new (): _Foo };
export { _Foo as Foo };

declare class _Server {
  listen(): void;
}
export { _Server as Server };

interface Options {
  depth: number;
}
export declare function configure(o: Options): Foo;
export type Alias = Foo;

declare function helper(): void;
declare const secret: number;
declare function _ren(): void;
export { _ren as ren };

declare class Hidden {
  constructor();
  static make(): Hidden;
  m(): void;
}
export declare function reveal(): Hidden;
