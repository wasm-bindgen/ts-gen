//! @ts-gen --experimental-generic-mono

// A reference passes no argument for an erased parameter, so a return
// whose only string leaf is that argument (`Key` in `ListResult<M, Key>`)
// has no `_js_string` variant: it would lower to `list`'s own type. The
// erased method gets a `_typed` twin keeping its parameter (`list_typed::<M>`).

interface Entry<M, Key extends string = string> {
  name: Key;
  meta?: M;
}
type ListResult<M, Key extends string = string> =
  | { done: false; keys: Entry<M, Key>[]; cursor: string }
  | { done: true; keys: Entry<M, Key>[] };
interface Store<Key extends string = string> {
  list<M = unknown>(): Promise<ListResult<M, Key>>;
}
declare function store(): Store;
