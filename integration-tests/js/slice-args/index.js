const describe = (values) =>
  `${Array.isArray(values) ? "Array" : values.constructor.name}(${Array.from(values, String).join(",")})`;

export class Item {
  constructor(name) {
    this.name = name;
  }
  toString() {
    return this.name;
  }
}

export const numbers = describe;
export const counts = describe;
export const literals = describe;
export const levels = describe;
export const strings = describe;
export const flags = describe;
export const bigints = describe;
export const items = describe;
export const generic = describe;
export const bytes = describe;
export const words = describe;
export const unsigned = describe;
export const buffered = describe;
export const clamped = describe;
export const pixels = describe;
const describeEmpty = (values) =>
  values === undefined ? "none" : values === null ? "null" : describe(values);
export const floats = describeEmpty;
export const maybeBytes = describeEmpty;
export const maybeClamped = describeEmpty;
export const mixed = (nums, bytes) => `${describe(nums)}|${describe(bytes)}`;
export const spelled = describe;
export const spelled_readonly = describe;
export const array_like = describe;
export class Bag {
  take(values) {
    return describe(values);
  }
}
export const mixedGeneric = (values, weights, data) =>
  [describe(values), describe(weights), describe(data)].join(" ");
const describeBuffer = (v) =>
  v instanceof ArrayBuffer
    ? `ArrayBuffer(${new Uint8Array(v).join(",")})`
    : `${v.constructor.name}(${new Uint8Array(v.buffer, v.byteOffset, v.byteLength).join(",")})`;
export const hash = describeBuffer;
export const store = (key, value) =>
  `${key}=${typeof value === "string" ? value : describeBuffer(value)}`;
export const echo = (data) => data;
export const peek = describeBuffer;
export const concat = (parts) => parts.map(describeBuffer).join("+");
export const owned = (data) => data.buffer.byteLength === data.byteLength;
export const ownedView = owned;
export const peekAs = (label, data) =>
  `${typeof label === "string" ? label : label.name}:${describeBuffer(data)}`;
const slabs = {
  buffer: () => new Uint8Array([1, 2, 3]).buffer,
  bytes: () => new Uint8Array([1, 2, 3, 4]).subarray(1, 3),
  floats: () => new Float32Array([1, 2]),
  view: () => new DataView(new Uint8Array([9, 8, 7, 6]).buffer, 1, 2),
};
export const slab = (kind) => slabs[kind]();
export const maybeSlab = (kind) => (kind === undefined ? undefined : slabs[kind]());
export const slabLater = async (kind) => slabs[kind]();
export class Slab {
  constructor(kind) {
    this.kind = kind;
  }
  get data() {
    return slabs[this.kind]();
  }
  bytes() {
    return slabs[this.kind]();
  }
  static of(kind) {
    return slabs[kind]();
  }
}
export const decode = (input, label) =>
  `${label ?? "utf-8"}:${input === undefined ? "none" : describeBuffer(input)}`;
