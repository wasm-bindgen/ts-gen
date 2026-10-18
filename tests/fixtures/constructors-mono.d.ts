
// A constructor binds the class parameters its arguments mention and
// defaults the rest, so `new()` needs no annotation; a `_typed` twin binds
// every class parameter. A construct signature returning concrete arguments
// returns them as written.

interface UnderlyingSource<R = any> {
  start?: (controller: any) => void;
}
interface UnderlyingByteSource {
  type: "bytes";
}
interface Stream<R = any> {
  readonly locked: boolean;
}
declare const Stream: {
  prototype: Stream;
  new (source: UnderlyingByteSource): Stream<Uint8Array>;
  new <R = any>(source?: UnderlyingSource<R>): Stream<R>;
};

declare class Transform<I = any, O = any> {
  constructor(transformer?: UnderlyingSource<I>);
  readonly readable: Stream<O>;
}

declare class Reader<R = any> {
  constructor(stream: Stream);
  read(): Promise<R>;
}
