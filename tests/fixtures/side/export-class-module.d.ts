import { DurableObject, WorkerEntrypoint as Entrypoint } from 'cloudflare:workers';

export declare class Counter extends DurableObject {
  constructor(ctx: DurableObjectState, env: Env);
  add(by: number): Promise<number>;
}

export declare class Greeter extends Entrypoint {
  constructor(ctx: DurableObjectState, env: Env);
  greet(name: string): string;
}
