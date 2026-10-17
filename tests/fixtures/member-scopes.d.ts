//! @ts-gen --input tests/fixtures/member-scopes/a.d.ts --input tests/fixtures/member-scopes/b.d.ts --input tests/fixtures/member-scopes/mod.d.ts --lib-name member-scopes-mod --export ia --export ic --export ix --export ib2 --export tests/fixtures/member-scopes.d.ts --export tests/fixtures/member-scopes/a.d.ts --export member-scopes-mod

// A name in a merged declaration resolves from the declaration it is
// written in, as in TypeScript: a constructor from its construct
// signature's interface (`QFoo`, `NN.NFoo`), a property, a parent and a
// namespace member from their own block. a.d.ts and b.d.ts declare one
// module "ix" whose blocks import different `T`s, and mod.d.ts merges
// into a.d.ts's globals with an imported `Ev`. member-scopes-rev takes
// the files in the other order.

declare namespace QN {
  interface Loc {
    l: number;
  }
  interface QFooCtor {
    new (l: Loc): QFoo;
    readonly prototype: QFoo;
    mk(l: Loc): QFoo;
  }
}
interface QFoo {
  go(): void;
}
interface Loc {
  g: string;
}
declare var QFoo: QN.QFooCtor;

interface NFooCtor {
  new (l: Loc): NN.NFoo;
  readonly prototype: NN.NFoo;
  mk(l: Loc): NN.NFoo;
}
declare namespace NN {
  interface Loc {
    l: number;
  }
  interface NFoo {
    go(): void;
  }
  var NFoo: NFooCtor;
}
