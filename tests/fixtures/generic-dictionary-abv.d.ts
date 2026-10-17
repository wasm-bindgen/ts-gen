// `ArrayBufferView` fields take a synthesised `<T: TypedArray>` generic,
// whose name skips the dictionary's own type parameters and any type
// declared in scope.

interface U {
  u: string;
}

interface GenAbv<T> {
  value?: T;
  data?: ArrayBufferView;
}

interface ReqAbv<T> {
  value: T;
  data: ArrayBufferView;
  other: ArrayBufferView;
  owner: U;
}
