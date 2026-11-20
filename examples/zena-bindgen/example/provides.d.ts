// What the worker provides: the default export's handlers, two Durable
// Objects and an RPC entrypoint.
import { DurableObject, WorkerEntrypoint } from 'cloudflare:workers';

/** @provides fetch, scheduled, queue */
declare const handler: ExportedHandler<Env>;
export default handler;

/** A Durable Object over its storage, with an RPC method. */
export declare class Counter extends DurableObject {
  constructor(ctx: DurableObjectState, env: Env);
  fetch(request: Request): Promise<Response>;
  alarm(info: AlarmInvocationInfo): Promise<void>;
  add(by: number): Promise<number>;
}

/** A Durable Object holding hibernatable WebSockets. */
export declare class Room {
  constructor(ctx: DurableObjectState, env: Env);
  fetch(request: Request): Promise<Response>;
  webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void>;
}

/** A named entrypoint, called over RPC through a service binding. */
export declare class Greeter extends WorkerEntrypoint {
  constructor(ctx: ExecutionContext, env: Env);
  greet(name: string): Promise<string>;
}
