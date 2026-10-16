//! @ts-gen --lib-name declared-modules --export declared-modules --export declared-modules/fs --export declared-modules/os --export tests/fixtures/declared-modules.d.ts

// Each host module is emitted in its own `pub mod`, named by its
// specifier, so the `constants` of `…/fs` and `…/os` stay separate APIs,
// as in `node:fs` and `node:os`. The `--lib-name` module is the library
// being bound and stays at the top level. A global namespace on the same
// path shares the `pub mod` with the modules under it.

export interface Options {
  depth: number;
}
export declare function options(): Options;

declare module "declared-modules/fs" {
  export namespace constants {
    const O_RDONLY: number;
  }
  export function open(path: string): Handle;
}

declare module "declared-modules/os" {
  export namespace constants {
    const O_RDONLY: number;
    namespace signals {
      const SIGINT: number;
    }
  }
  export function hostname(): string;
}

declare global {
  interface Handle {
    fd: number;
  }
  namespace DeclaredModules {
    const VERSION: string;
  }
}
