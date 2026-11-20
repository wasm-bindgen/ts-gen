// What the worker provides: the default export's handlers, a Durable
// Object, and an RPC entrypoint.

import { WorkerEntrypoint } from "cloudflare:workers";

/** @provides fetch, scheduled, queue */
declare const handler: ExportedHandler<Env>;
export default handler;

/** A Durable Object: a counter in KV and SQL storage, with an alarm and hibernating WebSockets. */
export declare class Counter {
  constructor(ctx: DurableObjectState, env: Env);
  fetch(request: Request): Promise<Response>;
  /** The runtime passes an `alarmInfo`, which an export that doesn't declare it ignores. */
  alarm(): Promise<void>;
  webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void>;
}

/** An RPC entrypoint, reached through the `GREETER` service binding. */
export declare class Greeter extends WorkerEntrypoint<Env> {
  constructor(ctx: ExecutionContext, env: Env);
  greet(name: string, style?: "plain" | "loud"): string;
  /** @tw-abi u32 */
  checksum(bytes: ArrayBufferView): number;
  /** @tw-abi n: u32 */
  factorial(n: number): bigint;
  /**
   * @tw-abi a: u32
   * @tw-abi b: u32
   */
  divide(a: number, b: number): [number, number];
}
