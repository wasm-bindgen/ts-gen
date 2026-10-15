//! @ts-gen --export tests/fixtures/provides-worker.d.ts --provides ExportedHandler.fetch,tail
// A worker-shaped surface in the shape `@cloudflare/workers-types` uses:
// optional properties typed by function aliases returning `T | Promise<T>`.
// Exercised through the `--provides` Rust wrapper in the integration tests.
declare class Request {
  constructor(url: string);
  readonly url: string;
}

declare class Response {
  constructor(body: string);
  readonly body: string;
}

declare class ExecutionContext {
  constructor();
  waitUntil(promise: Promise<any>): void;
}

interface Env {
  [key: string]: any;
}

type ExportedHandlerFetchHandler = (
  request: Request,
  env: Env,
  ctx: ExecutionContext,
) => Response | Promise<Response>;

type ExportedHandlerTailHandler = (
  events: string[],
  env: Env,
  ctx: ExecutionContext,
) => void | Promise<void>;

interface ExportedHandler {
  /** Handles a request. */
  fetch?: ExportedHandlerFetchHandler;
  /** Receives trace events. */
  tail?: ExportedHandlerTailHandler;
  scheduled?(cron: string, env: Env): Promise<void>;
  /** Always provided: a plain method with an optional parameter. */
  health(verbose?: boolean): string;
}
