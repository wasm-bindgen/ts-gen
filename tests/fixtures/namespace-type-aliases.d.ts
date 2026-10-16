//! @ts-gen --lib-name namespace-type-aliases --export namespace-type-aliases --export tests/fixtures/namespace-type-aliases.d.ts

// A type alias in a namespace is emitted in its `pub mod`. A plain reference
// still lowers to the alias target, while a generic instantiation names the
// emitted alias, qualified from outside the namespace.

export declare function emit(name: Events.Name, options: Events.Opts): Events.Label;
export declare function onEvent(listener: Events.Listener<string>): void;

declare global {
  namespace Events {
    type Name = string;
    type Label = Name;
    interface Options {
      name: Name;
    }
    type Opts = Options;
    type Listener<E> = (event: E) => void;
    type Pair<A, B> = [A, B];
    // Keeps its suffixed name so the bindings' `Result` stays the prelude's.
    type Result<T> = Pair<T, string>;
    function listen(name: Name, options: Opts): Label;
    function pair(p: Pair<string, number>): Result<number>;

    namespace Inner {
      type Id = Label;
      function find(id: Id): Options;
    }
  }
}
