//! @ts-gen --lib-name modules

// Modules: each ambient module is a block whose export list names
// its value declarations; types reach across blocks by their spec; a
// block's local declaration beside another's export keeps its own name.
// An inline union of types every block reads alike is the root's, one enum
// for all of them, and spelled by its arms when more than one use shares
// it.

declare module "store" {
  export interface Entry {
    readonly key: string;
  }
  export class Store {
    constructor();
    get(key: string): Entry | null;
  }
  export function open(name: string): Store;
  export { Store as default };
}

declare module "client" {
  import { Store } from "store";
  export function connect(store: Store): void;
  interface Local {
    x: number;
  }
  export function local(): Local;
}

// A global namespace a module assigns with `export =` is the module, so
// a dictionary synthesized inside it is declared there too. A class
// outside it extends one of its classes by the qualified name.
declare namespace StepModule {
  export type StepConfig = {
    retries?: { limit: number };
  };
  export function step(config: StepConfig): void;
  export class Runner {
    run(): void;
  }
}
declare module "steps" {
  export = StepModule;
}
declare class Retrier extends StepModule.Runner {
  retry(): void;
}

declare function read(): string | ArrayBuffer | number;
declare module "sink" {
  export function peek(): string | ArrayBuffer | number;
}
