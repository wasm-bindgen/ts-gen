//! @ts-gen --provides tests/fixtures/side/export-class-module.d.ts

// A `--provides` file imports its base class from a module, as a worker
// does: the name is the module's class, not the global interface.

interface Env {}
interface DurableObjectState {
  readonly id: string;
}
interface DurableObject {
  alarm?(): void | Promise<void>;
}

declare module "cloudflare:workers" {
  export abstract class DurableObject {
    constructor(ctx: DurableObjectState, env: Env);
  }
  export abstract class WorkerEntrypoint {
    constructor(ctx: DurableObjectState, env: Env);
  }
}
