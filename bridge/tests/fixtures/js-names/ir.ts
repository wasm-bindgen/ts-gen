ir 1;

// Declarations written under an escaped name (§4): the bridge reaches each
// by its JS path, and exports each under its JS name.
@js(name = "Str")
class Str_ {
  new(n: u32);
  static make(): Str_;
  get n(): u32;
}

@js(name = "Any")
namespace Any_ {
  f(): u32;

  @js(name = "Vec")
  namespace Vec_ {
    g(): u32;

    @js(name = "Bool")
    class Bool_ {
      static of(): Bool_;
    }
  }
}

module "./host.mjs" {
  @js(name = "Unit")
  namespace Unit_ {
    h(): u32;

    @js(name = "in")
    class in_ {
      static make(): in_;
    }
  }
  export { Unit_ as Unit };
}

@js(name = "Arr")
export class Arr_ {
  new(n: u32);
  get n(): u32;
}

@js(name = "Tuple")
export namespace Tuple_ {
  twice(n: u32): u32;

  @js(name = "new")
  namespace new_ {
    thrice(n: u32): u32;
  }
}
