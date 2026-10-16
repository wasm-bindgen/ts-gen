//! @ts-gen --lib-name module-export-assignment --export module-export-assignment

// A namespace a module assigns with `export =` is the module itself, so its
// members are the module's exports. They bind as module exports rather than
// through a `js_namespace` the module never exports, as in `node:assert`.
// A namespace nested in it is a module export like any other.

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
