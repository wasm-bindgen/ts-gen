// TS 5.7+ typed arrays are generic over their buffer. js-sys's aren't, so the
// buffer argument is dropped without a warning.
declare function digest(data: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer>;
declare function samples(): Float64Array<ArrayBufferLike>;
declare function clamped(data: Uint8ClampedArray<ArrayBuffer>): void;
declare function view(data: DataView<ArrayBuffer>): DataView<ArrayBuffer>;
declare function pair(data: Uint8Array<ArrayBuffer> | number[]): void;
declare function chunks(): Uint8Array<ArrayBuffer>[];

interface Frame {
  payload: ArrayBufferView<ArrayBuffer>;
  bytes?: BigUint64Array<SharedArrayBuffer>;
}
