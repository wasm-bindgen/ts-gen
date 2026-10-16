//! @ts-gen --lib-name module-namespace-merge --export module-namespace-merge --export module-namespace-merge/os --export tests/fixtures/module-namespace-merge.d.ts

// Same-named namespaces of different modules, or of a module and the global
// scope, share one `pub mod`, as `constants` does across `node:fs` and
// `node:os`. Each member keeps its own module and namespace path, and
// colliding statics are numbered within the shared module.

declare module "module-namespace-merge" {
  export namespace constants {
    const O_RDONLY: number;
    namespace flags {
      const EXCL: number;
    }
  }
}

declare module "module-namespace-merge/os" {
  export namespace constants {
    const O_RDONLY: number;
    namespace flags {
      const NOFOLLOW: number;
    }
    namespace signals {
      const SIGINT: number;
    }
  }
}

declare namespace constants {
  const VERSION: string;
}
