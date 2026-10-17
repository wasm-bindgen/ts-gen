//! @ts-gen --export tests/fixtures/constructor-vars.d.ts --export m

// A constructor `var` and an interface of the same name in one container
// are one class. The var's type, resolved from the var, gives the class its
// constructor and statics, whichever comes first and wherever the
// constructor interface is declared: `N.Foo`'s is outside `N`. A module's
// constructor var alone is a class of its own. Construct signatures are
// overloads (`Multi`), and a qualified annotation resolves from the var
// like a bare one (`Q.Qual`).

interface FooCtor {
  new (): N.Foo;
  readonly prototype: N.Foo;
  create(): N.Foo;
}

declare namespace N {
  interface Foo {
    go(): void;
  }
  var Foo: FooCtor;

  var Before: BeforeCtor;
  interface Before {
    b(): void;
  }
  interface BeforeCtor {
    new (x: number): Before;
    readonly prototype: Before;
  }
}

declare namespace R {
  interface Q {
    q(): void;
  }
  var Q: { prototype: Q; new (): Q };

  var P: { prototype: P; new (): P };
  interface P {
    p(): void;
  }
}

interface WCtor {
  new (): W;
  readonly prototype: W;
  make(): W;
}
interface W {
  w(): void;
}
declare var W: WCtor;

declare module "m" {
  const W: WCtor;
}

interface Multi {
  m(): void;
}
interface MultiCtor {
  new (): Multi;
  new (n: number): Multi;
  new (s: string, n: number): Multi;
  readonly prototype: Multi;
}
declare var Multi: MultiCtor;

declare namespace Ctors {
  interface QualCtor {
    new (): Q.Qual;
    readonly prototype: Q.Qual;
    make(): Q.Qual;
  }
}
declare namespace Q {
  interface Qual {
    q(): void;
  }
  var Qual: Ctors.QualCtor;
}
