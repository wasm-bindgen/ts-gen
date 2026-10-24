//! @ts-gen --lib-name shape-checks --export shape-checks --export tests/fixtures/shape-checks.d.ts

// An interface or object type only describes a shape: no JS constructor
// exists for `instanceof` to test against, so `dyn_into` accepts any object
// and the type carries no `js_name` or `js_namespace`. A class keeps its
// `instanceof` check. A string enum and a constructor overload carry no
// `js_namespace` / `js_name` either, since wasm-bindgen ignores them there.
//
// In a returned union a shape arm would take every object listed after it,
// so it goes after the class arms and, when last, is the `fallback`; two
// shape arms can't be told apart and share one `Object` arm.

declare global {
  namespace Shapes {
    interface RoundingOptions {
      smallestUnit?: Unit;
      roundingIncrement?: number;
    }
    interface Handle {
      close(): void;
    }
    type Unit = "hour" | "minute";
    class Span {
      constructor(hours: number);
      constructor(hours: number, unit: Unit);
      readonly hours: number;
      readonly unit: Unit;
      round(options: RoundingOptions): Span;
      handle(): Handle;
    }
    class Ticket {
      constructor(id: number);
      readonly id: number;
    }
    interface Init {
      body?: string;
    }
    function pick(kind: number): RoundingOptions | Ticket;
    function pickAny(kind: number): RoundingOptions | Init | Ticket;
    function pickOrText(kind: number): RoundingOptions | string;
  }

  interface GlobalInit {
    status?: number;
  }
}

export interface ModuleOptions {
  verbose?: boolean;
}

export type Choice = { kind: "a"; a: number } | { kind: "b"; b: string };
