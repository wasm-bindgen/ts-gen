// Type parameters constrained to a primitive have no Rust generic form:
// `Key extends string` can only be instantiated with string subtypes,
// which all cross the ABI as `&str`. The parameter is dropped from the
// declaration and reads as the primitive; references that still pass an
// argument for it (`ListResult<Metadata, Key>`) drop that argument.
// `keyof X` constraints read as `string`.

interface ListKey<Metadata = unknown, Key extends string = string> {
  name: Key;
  metadata?: Metadata;
}
interface ListResult<Metadata = unknown, Key extends string = string> {
  keys: ListKey<Metadata, Key>[];
  cursor: string;
}
interface Store<Key extends string = string> {
  get(key: Key): Promise<string | null>;
  list<Metadata = unknown>(): Promise<ListResult<Metadata, Key>>;
  tag<Level extends "info" | "warn">(level: Level, count: number): void;
  bump<N extends number>(by: N): N;
}
declare class Target<EventMap = {}> {
  addEventListener<Type extends keyof EventMap>(type: Type, handler: (ev: EventMap[Type]) => void): void;
}
interface Env {
  KV: Store;
  TYPED: Store<"a" | "b">;
}

// An argument for an erased parameter is dropped wherever the reference
// sits, including inside `keyof`, an indexed access, a mapped type and an
// inline object literal, so `Slot<"a", V>` binds `V`, not `"a"`.
interface Point { x: number; y: number; }
interface Slot<K extends string, V> {
  value: V;
}
interface Nested {
  indexed: Slot<"a", number>["value"];
  keys: keyof Pick<Point, Slot<"a", "x">["value"]>;
  mapped: { [P in Slot<"a", "x" | "y">["value"]]: Slot<"b", boolean>["value"] };
  literal: { inner: Slot<"a", string[]>["value"] }["inner"];
}
