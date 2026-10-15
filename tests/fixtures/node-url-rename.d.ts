//! @ts-gen --lib-name node:url --export node:url

// `node:url` has no `_URL` or `_fileURLToPath` exports, so instantiation
// fails unless every binding imports by the public name.
declare module "node:url" {
  class _URL {
    constructor(input: string);
    readonly href: string;
    static canParse(input: string): boolean;
  }
  function _fileURLToPath(url: string): string;
  export { _URL as URL, _fileURLToPath as fileURLToPath };
}
