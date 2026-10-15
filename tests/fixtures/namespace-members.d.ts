//! @ts-gen --lib-name namespace-members --export namespace-members --export tests/fixtures/namespace-members.d.ts

// A global namespace is emitted as a `pub mod`. Its members resolve from
// where they are referenced, so a sibling wins over a same-named global, and
// a reference from another module is qualified with the namespace path.

export declare function make(): Events.Emitter;

declare global {
  interface Emitter {
    emit(event: string): boolean;
  }
  function globalEmitter(): Emitter;

  namespace Events {
    type Name = string;
    interface Emitter {
      on(event: Name): void;
    }
    interface Options {
      emitter: Emitter;
    }
    function listen(name: Name, options: Options): Emitter;

    namespace Inner {
      interface Handle {
        emitter: Emitter;
        options: Events.Options;
      }
    }
  }

  interface Stream extends Events.Emitter {
    pipe(): void;
  }

  namespace Wasm {
    class Global {
      constructor();
      readonly value: number;
    }
    function current(): Global;
  }
}
