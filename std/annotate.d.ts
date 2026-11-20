// ABI choices for std.ir, layered onto TypeScript's lib declarations by
// tests/std.rs. Lengths, sizes and offsets are `u32`, as in js-sys;
// positions that may count from the end (`at`, `slice`, `fill`) are `i32`;
// typed array elements are their element type. `Date` stays `f64`, since an
// invalid date reads as `NaN`.

interface ArrayBuffer {
  /** @tw-abi u32 */
  readonly byteLength: number;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  slice(begin?: number, end?: number): ArrayBuffer;
}

interface ArrayBufferView {
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
}

interface DataView {
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /**
   * @tw-abi i8
   * @tw-abi byteOffset: u32
   */
  getInt8(byteOffset: number): number;
  /**
   * @tw-abi byteOffset: u32
   * @tw-abi value: i8
   */
  setInt8(byteOffset: number, value: number): void;
  /**
   * @tw-abi u8
   * @tw-abi byteOffset: u32
   */
  getUint8(byteOffset: number): number;
  /**
   * @tw-abi byteOffset: u32
   * @tw-abi value: u8
   */
  setUint8(byteOffset: number, value: number): void;
  /**
   * @tw-abi i16
   * @tw-abi byteOffset: u32
   */
  getInt16(byteOffset: number, littleEndian?: boolean): number;
  /**
   * @tw-abi byteOffset: u32
   * @tw-abi value: i16
   */
  setInt16(byteOffset: number, value: number, littleEndian?: boolean): void;
  /**
   * @tw-abi u16
   * @tw-abi byteOffset: u32
   */
  getUint16(byteOffset: number, littleEndian?: boolean): number;
  /**
   * @tw-abi byteOffset: u32
   * @tw-abi value: u16
   */
  setUint16(byteOffset: number, value: number, littleEndian?: boolean): void;
  /**
   * @tw-abi i32
   * @tw-abi byteOffset: u32
   */
  getInt32(byteOffset: number, littleEndian?: boolean): number;
  /**
   * @tw-abi byteOffset: u32
   * @tw-abi value: i32
   */
  setInt32(byteOffset: number, value: number, littleEndian?: boolean): void;
  /**
   * @tw-abi u32
   * @tw-abi byteOffset: u32
   */
  getUint32(byteOffset: number, littleEndian?: boolean): number;
  /**
   * @tw-abi byteOffset: u32
   * @tw-abi value: u32
   */
  setUint32(byteOffset: number, value: number, littleEndian?: boolean): void;
  /**
   * @tw-abi f32
   * @tw-abi byteOffset: u32
   */
  getFloat32(byteOffset: number, littleEndian?: boolean): number;
  /**
   * @tw-abi byteOffset: u32
   * @tw-abi value: f32
   */
  setFloat32(byteOffset: number, value: number, littleEndian?: boolean): void;
  /**
   * @tw-abi f64
   * @tw-abi byteOffset: u32
   */
  getFloat64(byteOffset: number, littleEndian?: boolean): number;
  /**
   * @tw-abi byteOffset: u32
   * @tw-abi value: f64
   */
  setFloat64(byteOffset: number, value: number, littleEndian?: boolean): void;
  /**
   * @tw-abi i64
   * @tw-abi byteOffset: u32
   */
  getBigInt64(byteOffset: number, littleEndian?: boolean): bigint;
  /**
   * @tw-abi byteOffset: u32
   * @tw-abi value: i64
   */
  setBigInt64(byteOffset: number, value: bigint, littleEndian?: boolean): void;
  /**
   * @tw-abi u64
   * @tw-abi byteOffset: u32
   */
  getBigUint64(byteOffset: number, littleEndian?: boolean): bigint;
  /**
   * @tw-abi byteOffset: u32
   * @tw-abi value: u64
   */
  setBigUint64(byteOffset: number, value: bigint, littleEndian?: boolean): void;
}

interface RegExp {
  /** @tw-abi u32 */
  lastIndex: number;
}

interface Map<K, V> {
  /** @tw-abi u32 */
  readonly size: number;
}

interface Set<T> {
  /** @tw-abi u32 */
  readonly size: number;
}

interface Int8Array {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi i8
   * @tw-abi index: i32
   */
  at(index: number): number | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: i8
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: number, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: i8
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: number, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: i8
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: number, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: i8
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: number, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<number>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): Int8Array;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): Int8Array;
  /**
   * @tw-abi index: i32
   * @tw-abi value: i8
   */
  with(index: number, value: number): Int8Array;
}

interface Uint8Array {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi u8
   * @tw-abi index: i32
   */
  at(index: number): number | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: u8
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: number, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: u8
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: number, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: u8
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: number, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: u8
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: number, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<number>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): Uint8Array;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): Uint8Array;
  /**
   * @tw-abi index: i32
   * @tw-abi value: u8
   */
  with(index: number, value: number): Uint8Array;
}

interface Uint8ClampedArray {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi u8
   * @tw-abi index: i32
   */
  at(index: number): number | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: u8
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: number, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: u8
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: number, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: u8
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: number, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: u8
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: number, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<number>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): Uint8ClampedArray;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): Uint8ClampedArray;
  /**
   * @tw-abi index: i32
   * @tw-abi value: u8
   */
  with(index: number, value: number): Uint8ClampedArray;
}

interface Int16Array {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi i16
   * @tw-abi index: i32
   */
  at(index: number): number | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: i16
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: number, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: i16
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: number, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: i16
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: number, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: i16
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: number, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<number>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): Int16Array;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): Int16Array;
  /**
   * @tw-abi index: i32
   * @tw-abi value: i16
   */
  with(index: number, value: number): Int16Array;
}

interface Uint16Array {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi u16
   * @tw-abi index: i32
   */
  at(index: number): number | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: u16
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: number, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: u16
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: number, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: u16
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: number, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: u16
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: number, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<number>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): Uint16Array;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): Uint16Array;
  /**
   * @tw-abi index: i32
   * @tw-abi value: u16
   */
  with(index: number, value: number): Uint16Array;
}

interface Int32Array {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi i32
   * @tw-abi index: i32
   */
  at(index: number): number | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: number, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: i32
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: number, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: i32
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: number, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: i32
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: number, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<number>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): Int32Array;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): Int32Array;
  /**
   * @tw-abi index: i32
   * @tw-abi value: i32
   */
  with(index: number, value: number): Int32Array;
}

interface Uint32Array {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi u32
   * @tw-abi index: i32
   */
  at(index: number): number | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: u32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: number, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: u32
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: number, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: u32
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: number, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: u32
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: number, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<number>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): Uint32Array;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): Uint32Array;
  /**
   * @tw-abi index: i32
   * @tw-abi value: u32
   */
  with(index: number, value: number): Uint32Array;
}

interface Float32Array {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi f32
   * @tw-abi index: i32
   */
  at(index: number): number | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: f32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: number, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: f32
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: number, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: f32
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: number, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: f32
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: number, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<number>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): Float32Array;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): Float32Array;
  /**
   * @tw-abi index: i32
   * @tw-abi value: f32
   */
  with(index: number, value: number): Float32Array;
}

interface Float64Array {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi f64
   * @tw-abi index: i32
   */
  at(index: number): number | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: f64
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: number, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: f64
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: number, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: f64
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: number, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: f64
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: number, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<number>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): Float64Array;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): Float64Array;
  /**
   * @tw-abi index: i32
   * @tw-abi value: f64
   */
  with(index: number, value: number): Float64Array;
}

interface BigInt64Array {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi i64
   * @tw-abi index: i32
   */
  at(index: number): bigint | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: i64
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: bigint, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: i64
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: bigint, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: i64
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: bigint, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: i64
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: bigint, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<bigint>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): BigInt64Array;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): BigInt64Array;
  /**
   * @tw-abi index: i32
   * @tw-abi value: i64
   */
  with(index: number, value: bigint): BigInt64Array;
}

interface BigUint64Array {
  /** @tw-abi u32 */
  readonly BYTES_PER_ELEMENT: number;
  /** @tw-abi u32 */
  readonly byteLength: number;
  /** @tw-abi u32 */
  readonly byteOffset: number;
  /** @tw-abi u32 */
  readonly length: number;
  /**
   * @tw-abi u64
   * @tw-abi index: i32
   */
  at(index: number): bigint | undefined;
  /**
   * @tw-abi target: i32
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  copyWithin(target: number, start: number, end?: number): this;
  /**
   * @tw-abi value: u64
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  fill(value: bigint, start?: number, end?: number): this;
  /**
   * @tw-abi searchElement: u64
   * @tw-abi fromIndex: i32
   */
  includes(searchElement: bigint, fromIndex?: number): boolean;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: u64
   * @tw-abi fromIndex: i32
   */
  indexOf(searchElement: bigint, fromIndex?: number): number;
  /**
   * @tw-abi i32
   * @tw-abi searchElement: u64
   * @tw-abi fromIndex: i32
   */
  lastIndexOf(searchElement: bigint, fromIndex?: number): number;
  /** @tw-abi offset: u32 */
  set(array: ArrayLike<bigint>, offset?: number): void;
  /**
   * @tw-abi start: i32
   * @tw-abi end: i32
   */
  slice(start?: number, end?: number): BigUint64Array;
  /**
   * @tw-abi begin: i32
   * @tw-abi end: i32
   */
  subarray(begin?: number, end?: number): BigUint64Array;
  /**
   * @tw-abi index: i32
   * @tw-abi value: u64
   */
  with(index: number, value: bigint): BigUint64Array;
}
