//! @ts-gen --lib-name containers

// Containers and names: namespaces nest, a class and namespace of
// one name fold the namespace into the class's statics, reserved names
// escape with `@js(name)`, and lib types the input only references are
// stubbed at the top level.

declare namespace Outer {
  namespace Inner {
    interface Leaf {
      readonly id: string;
    }
    function make(): Leaf;
  }
  function root(): Inner.Leaf;
}

declare class Widget {
  constructor(name: string);
  readonly name: string;
}
declare namespace Widget {
  function create(name: string): Widget;
  const version: string;
}

declare class String {
  length(): number;
}
declare function bytes(): Uint8Array;
declare function later(): Promise<string>;
declare class Any {
  readonly kind: string;
}
declare function any(): Any;
