//! @ts-gen --lib-name export-assignment-file --export export-assignment-file

// The CommonJS library shape: a module file whose `export =` assigns a
// function merged with a namespace. As in a `declare module` body, the
// function is the default export and the namespace members are named
// exports, without `js_namespace`.

declare function check(value: unknown): void;
declare namespace check {
  function strictEqual(actual: unknown, expected: unknown): boolean;
}
export = check;
