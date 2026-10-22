//! @ts-gen --input tests/fixtures/import-require-module/dep.d.ts --lib-name import-require-module --export import-require-module --export dep

// `import x = require("m")` makes a file a module, as `import` and
// `export` do, so with `--lib-name` its top-level declarations belong to
// the library rather than the global scope, and `dep` binds the module.

import dep = require("dep");

declare class Walker {
  constructor(o: dep.Options);
  walk(): void;
}
declare function walk(o: dep.Options): Walker;
