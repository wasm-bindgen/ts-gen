//! @ts-gen --lib-name namespace-type-aliases --export namespace-type-aliases --export tests/fixtures/namespace-type-aliases.d.ts

// A type alias in a namespace is emitted in its `pub mod`. A reference lowers
// to the alias target, a generic instantiation with its arguments
// substituted, from inside the namespace or qualified from outside. Outside,
// the target's names are qualified wherever they would bind differently
// (`Events.Shadowed` from `Events.Inner`, not the global `Shadowed`), and
// the arguments keep the meaning they have where they are written (`T`).

export declare function emit(name: Events.Name, options: Events.Opts): Events.Label;
export declare function onEvent(listener: Events.Listener<string>): void;
export declare function outerPair(r: Events.Result<number>): void;
export declare function loose(x: Events.Loose<string>): void;
export declare class Holder<T> {
  on(listener: Events.Listener<T>): void;
}
export declare function tagged(t: Events.Inner.Tagged<number>): void;

declare global {
  interface Shadowed {
    globalSide: string;
  }
  namespace Events {
    interface Shadowed {
      eventsSide: number;
    }
    type Name = string;
    type Label = Name;
    interface Options {
      name: Name;
    }
    type Opts = Options;
    type Listener<E> = (event: E) => void;
    type Pair<A, B> = [A, B];
    // Erased, so emitted without its parameter.
    type Loose<T> = T | number;
    // Keeps its suffixed name so the bindings' `Result` stays the prelude's.
    type Result<T> = Pair<T, string>;
    function listen(name: Name, options: Opts): Label;
    function pair(p: Pair<string, number>): Result<number>;

    namespace Inner {
      type Id = Label;
      type Tagged<T> = [T, Shadowed];
      function find(id: Id): Options;
    }
  }
}
