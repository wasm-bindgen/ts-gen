//! @ts-gen --export x:shapes

// A dynamic-union enum is emitted in the module of its first use, and its
// members are lowered from the union's use site. `Id` and the `A | B`
// LUB `Base` are private to the module, so they only resolve from the
// module's own scope: `Array<JsString>` and `Promise<Base>`.

declare module "x:shapes" {
  type Id = string;
  class Base {
    base(): void;
  }
  class A extends Base {
    a(): void;
  }
  class B extends Base {
    b(): void;
  }
  export function list(): Array<Id> | Promise<A | B>;
}
