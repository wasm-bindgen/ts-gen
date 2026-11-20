// The producer names an inline object type of properties as a dictionary
// interface, `<Owner><Member>`, one per shape and place, and generic over
// the type parameters it uses; a lone index signature is a `Record`. An
// object type with methods, and `object`, read as the `Object` interface.
// A part of an alias is named from the alias: its element `<Alias>Item`, a
// lone object arm `<Alias>Object`, one of several object arms by a
// property only it has, and a mapped one by its key alias, `<Key>Map`.

interface Upload {
  readonly retry?: { count: number; delay?: number };
  headers: { [name: string]: string };
  parts: { name: string; size: number }[];
  hooks: { start(): void };
  extra: object;
}

interface Slot<T> {
  inner: { value: T; label?: string };
}

type Listing = { id: string }[];
type Shape = string | { radius: number };
type Border = number | { color: string } | { top: number; left: number };
type FilterOp = "$eq" | "$ne";
type CollectionOp = "$in" | "$nin";
type Filter = string | { [Op in FilterOp]?: string } | { [Op in CollectionOp]?: string[] };

interface Canvas {
  filter(filter: Filter): void;
  draw(listing: Listing, shape: Shape, border: Border): void;
}
// A utility over a declaration reads its inline types under the names
// the declaration gives them, so both share them.
type Search = { ranking?: { score?: number } };
type Streaming = Omit<Search, "x"> & { stream: true };
interface Headers2 {
  "x-custom-cost": { per_token?: number } | { total?: number } | string;
}
declare function search(s: Search, t: Streaming, h: Partial<Headers2>): void;
