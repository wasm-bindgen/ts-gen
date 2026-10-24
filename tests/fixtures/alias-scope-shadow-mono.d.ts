
// Per-monomorphization lowering peels an alias in its own scope as well:
// `TopMode` is the top-level `Mode` enum even inside `Shadow`, where `Mode`
// is an interface.

type Mode = "fast" | "slow";
type TopMode = Mode;
declare class Holder<T> {
  constructor(value: T);
  get(): T;
}

declare namespace Shadow {
  interface Mode { shadow: boolean; }
  function hold(h: Holder<TopMode>): Holder<TopMode>;
  function next(): Promise<TopMode>;
}
