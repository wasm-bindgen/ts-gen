//! @ts-gen --export Context

// A root's references into a namespace (`Cloudflare.Stub`) emit what they
// reach in its `pub mod`; `Env` and `Unrelated` are not reached.

declare namespace Cloudflare {
  interface Env {}
  type Exports = { [K in keyof Env]: Env[K] };
  interface Stub {
    readonly id: string;
  }
  type Kind = "http" | "tcp";
  interface Socket {
    readonly port: number;
  }
}

declare namespace Storage {
  interface Stub {
    readonly region: string;
  }
}

declare abstract class Context {
  readonly exports: Cloudflare.Exports;
  readonly stub: Cloudflare.Stub;
  // A namespaced enum crosses by value like a single-segment one.
  open(kind: Cloudflare.Kind): void;
  // Overload suffixes name a qualified type by its full path, so
  // `Cloudflare.Stub`, `Cloudflare.Socket` and `Storage.Stub` stay distinct.
  connect(target: string | Cloudflare.Stub | Cloudflare.Socket | Storage.Stub): void;
}

declare abstract class Unrelated {
  readonly n: number;
}
