// What the worker provides: the default export's fetch, and a Durable
// Object class.

/** @provides fetch */
declare const handler: ExportedHandler<Env>;
export default handler;

/** A Durable Object counting the requests it serves. */
export declare class Counter {
  constructor(ctx: DurableObjectState, env: Env);
  fetch(request: Request): Promise<Response>;
}
