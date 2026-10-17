//! @ts-gen --lib-name module-export-assignment --export module-export-assignment --export module-export-assignment/counter --export module-export-assignment/global

// A namespace a module assigns with `export =` is the module itself, so its
// members are the module's exports. They bind as module exports rather than
// through a `js_namespace` the module never exports, as in `node:assert`.
// A namespace nested in it is a module export like any other. The assigned
// binding itself is the default export, whether a function or a class. The
// namespace may be declared outside the module, as `cloudflare:workers` does,
// and is then the module's rather than a global.

declare module "module-export-assignment" {
  function check(value: unknown): void;
  namespace check {
    function strictEqual(actual: unknown, expected: unknown): boolean;
    const VERSION: string;
    namespace nested {
      function run(): string;
    }
  }
  export = check;
}

declare module "module-export-assignment/counter" {
  class Counter {
    constructor(start: number);
    readonly value: number;
    increment(): void;
  }
  export = Counter;
}

declare namespace SharedGreeter {
  function greet(name: string): string;
  class Greeter {
    constructor(prefix: string);
    greet(name: string): string;
  }
}

declare module "module-export-assignment/global" {
  export = SharedGreeter;
}
