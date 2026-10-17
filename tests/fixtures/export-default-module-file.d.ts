//! @ts-gen --lib-name export-default-module-file --export export-default-module-file

// A `--lib-name` module file's top-level `export default` binds the module's
// `default` export, as in a `declare module` block.

export default class Widget {
  constructor(name: string);
  static create(): Widget;
}
