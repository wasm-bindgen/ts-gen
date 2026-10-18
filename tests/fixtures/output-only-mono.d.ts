//! @ts-gen --experimental-generic-mono

// Per-mono, a dictionary or discriminated union the bindings only ever read
// (a return, a getter, a callback's argument) has no write side: no
// factories, setters or builder. Reaching it as input anywhere (an
// argument, a written dictionary's field, a callback's return) keeps them,
// as does not reaching it at all.

interface ListKey<M> {
  name: string;
  metadata?: M;
}
type ListResult<M> =
  | { complete: false; keys: ListKey<M>[]; cursor: string }
  | { complete: true; keys: ListKey<M>[] };
interface Event {
  kind: string;
}
interface Reply {
  status: number;
}
interface Shared {
  id: string;
}
interface Unused {
  label?: string;
}
type Listener = (event: Event) => Reply;
declare class Store {
  list<M = unknown>(): Promise<ListResult<M>>;
  on(listener: Listener): void;
  get(): Shared;
  put(value: Shared): void;
}
