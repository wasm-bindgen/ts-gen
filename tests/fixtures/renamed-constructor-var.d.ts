//! @ts-gen --lib-name x --export x

// An interface and a constructor variable sharing a local name describe one
// class. Exporting that name under a public name keeps the local name as the
// Rust type, since the interface members refer to it, and binds the class to
// the public name: the inline form, the `XConstructor` form, and a var with
// no instance interface.

declare module "x" {
  interface _Foo {
    bar(): void;
  }
  var _Foo: { new (): _Foo; from(n: number): _Foo };
  export { _Foo as Foo };

  interface _Bar {
    baz(): string;
  }
  interface _BarConstructor {
    new (n: number): _Bar;
    readonly VERSION: string;
  }
  var _Bar: _BarConstructor;
  export { _Bar as Bar };

  var _Plain: { new (): object };
  export { _Plain as Plain, _Plain as AlsoPlain };
}
