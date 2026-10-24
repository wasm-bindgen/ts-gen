// An alias that reaches itself is never chased, so its references name
// the alias instead of expanding without end: directly (`Next<T>`),
// through another alias (`Ping` / `Pong`), with growing arguments
// (`Grow<T>`), through an argument (`Wrapped`), from a namespace
// (`Tree.Node`), and through an alias chain (`Step`). A non-recursive
// alias nested in its own argument (`Boxed<Boxed<string>>`) still chases.

type Next<T> = (next: Next<T>) => void;

type Ping = (pong: Pong) => void;
type Pong = (ping: Ping) => void;

type Grow<T> = (next: Grow<T[]>) => void;

type Boxed<T> = Array<T>;
type Wrapped = (inner: Boxed<Wrapped>) => void;

type Step = Next<number>;

declare namespace Tree {
  type Node<T> = { value: T } | ((child: Tree.Node<T>) => void);
  type Leaf = (parent: Tree.Leaf) => void;
}

declare function step(next: Next<string>, ping: Ping): void;
declare function grow(next: Grow<string>): void;
declare function wrapped(w: Wrapped, nested: Boxed<Boxed<string>>): void;
declare function chain(s: Step): void;
declare function tree(node: Tree.Node<number>, leaf: Tree.Leaf): void;
