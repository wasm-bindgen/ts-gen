//! @ts-gen --lib-name export-default --export export-default --export export-default/create --export export-default/version --export export-default/merged --export export-default/renamed

// A default export has no name of its own: the declaration keeps its local
// Rust name and binds `default`, as for `export =`. That covers a default
// class (whose constructor and statics take `js_class = "default"`), a
// default function, and `export default <identifier>` naming a value.

declare module "export-default" {
  export default class Processor {
    constructor(name: string);
    readonly name: string;
    static named(name: string): Processor;
  }
}

declare module "export-default/create" {
  export default function create(name: string): string;
}

declare module "export-default/version" {
  const version: string;
  export default version;
}

// A namespace merged with a default export is reached through `default`, and
// one merged with a renamed export through its public name.
declare module "export-default/merged" {
  function make(): string;
  namespace make {
    function helper(): string;
  }
  export default make;
}

declare module "export-default/renamed" {
  function make(): string;
  namespace make {
    function helper(): string;
  }
  export { make as create };
}
