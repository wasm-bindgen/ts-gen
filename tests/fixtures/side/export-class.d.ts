/** A counter kept in durable storage. */
export declare class Counter extends DurableObject {
  constructor(ctx: DurableObjectState, env: Env);
  increment(by: number): Promise<number>;
  readonly count: number;
  label: string | null;
  fetch(request: Request): Promise<Response>;
  static restore(id: string): Counter;
}

export declare class Registry {
  static create(): Registry;
  readonly size: number;
}

export declare function version(): string;

export declare function rename(from: string | null, to?: string | null): void;
