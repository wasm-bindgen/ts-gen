//! @ts-gen --lib-name export-default --export export-default --export export-default/create --export export-default/version

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
