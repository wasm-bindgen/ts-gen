// The worker's bindings from config.capnp, merged into workers-types' `Env`.
interface Env {
  readonly GREETING: string;
  readonly BACKEND: Fetcher;
  readonly COUNTER: DurableObjectNamespace;
  readonly GREETER: GreeterStub;
  readonly SELF: SelfStub;
}

// The RPC surface of the `Greeter` entrypoint (provides.d.ts), as its
// stub sees it: every call is a promise.
interface GreeterStub {
  greet(name: string, style?: "plain" | "loud"): Promise<string>;
  /** @tw-abi u32 */
  checksum(bytes: Uint8Array): Promise<number>;
  /** @tw-abi n: u32 */
  factorial(n: number): Promise<bigint>;
  /**
   * @tw-abi a: u32
   * @tw-abi b: u32
   */
  divide(a: number, b: number): Promise<[number, number]>;
}

// This worker as a service, to run its own `scheduled` and `queue`
// handlers.
interface SelfStub {
  scheduled(options: ScheduledOptions): Promise<ScheduledResult>;
  queue(queueName: string, messages: QueueMessage[]): Promise<QueueResult>;
}
interface ScheduledOptions {
  cron?: string;
}
interface ScheduledResult {
  readonly outcome: string;
}
interface QueueMessage {
  id: string;
  timestamp: Date;
  body: unknown;
  attempts: number;
}
interface QueueResult {
  readonly outcome: string;
}

// The JSON global, which neither workers-types nor std.tw declares.
declare namespace JSON {
  /** @throws {SyntaxError} if the text is not JSON */
  function parse(text: string): any;
  function stringify(value: any): string;
}

// The shape `/json` reads its body as.
interface Order {
  item: string;
  /** @tw-abi u32 */
  quantity: number;
  tags: string[];
  note?: string;
}

// A row of the Durable Object's `visits` table.
interface Visit {
  /** @tw-abi u32 */
  readonly id: number;
  readonly agent: string;
}

// HTMLRewriter's element handlers as callback properties: workers-types
// declares them as optional methods, which lower as methods to call.
interface ElementHandlers {
  element?: (element: Element) => void;
  text?: (text: Text) => void;
}
interface HTMLRewriter {
  on(selector: string, handlers: ElementHandlers): HTMLRewriter;
}

// The names the app uses for two literal unions written inline: an alias
// names every use of its literals.
type NoStoreOrNoCache = "no-store" | "no-cache";
type PlainOrLoud = "plain" | "loud";
