// An alias's target resolves where the alias is declared, not where it is
// used. Inside `Shadow`, `Inner` names `Shadow.Inner`, but `TopX` and
// `TopAll` mean the top-level `Inner`, so their targets are lowered from the
// top level and qualified from `mod shadow` (`super::Inner`).

interface Pair<A, B> { a: A; b: B; }
interface Inner { top: string; }
type TopX = Pair<Inner, Inner>;
type TopAll<T = Inner> = Pair<T, T>;
type InnerOrName = Inner | string;
type Chained = InnerOrName;
type Names = string | number;

declare namespace Shadow {
  interface Inner { shadow: boolean; }
  function use(a: TopX, b: TopAll): void;
  function wrapped(a: TopX[], b: Promise<TopAll>): TopX | null;
  // A union whose members the namespace would read differently can't fan
  // out into overloads naming them, so the alias stays whole.
  function either(a: InnerOrName, b: Chained): InnerOrName;
  // Members that read the same still fan out.
  function name(n: Names): void;
  // The namespace's own `Inner` is unaffected.
  function own(i: Inner): void;
}
