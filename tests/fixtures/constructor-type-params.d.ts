// `declare var X: { new <T>(…): X<T> }` merged with `interface X<T>`:
// the construct signature's own `<T>` binds the class parameter of the
// same name, and the interface's body scope (where `T` is declared)
// carries over to the merged class so member types resolve. A named
// constructor interface (`Slot`) behaves the same.

interface Holder<Value = unknown> {
  readonly value?: Value;
  reset(): Holder<Value>;
}

declare var Holder: {
  prototype: Holder;
  new <Value = unknown>(initial?: Value): Holder<Value>;
};

interface Pair<A, B> {
  readonly first: A;
  readonly second: B;
}

declare var Pair: {
  prototype: Pair<unknown, unknown>;
  new <A, B>(first: A, second: B): Pair<A, B>;
  swap<A, B>(pair: Pair<A, B>): Pair<B, A>;
};

interface Slot<T = string> {
  value: T;
}

interface SlotConstructor {
  new <T>(value: T): Slot<T>;
  readonly prototype: Slot;
}

declare var Slot: SlotConstructor;
