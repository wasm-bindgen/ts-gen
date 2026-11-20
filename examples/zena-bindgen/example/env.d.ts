// The worker's own declarations, beside workers-types.

// Its bindings, from config.capnp.
interface Env {
  readonly GREETING: string;
  readonly HELPER: Fetcher;
  readonly COUNTER: DurableObjectNamespace;
  readonly ROOM: DurableObjectNamespace;
  readonly GREETER: GreeterStub;
}

// TypeScript derives a stub's methods from its class with mapped types,
// which the IR doesn't carry: the RPC stubs are declared.
interface CounterStub extends DurableObjectStub {
  add(by: number): Promise<number>;
}
interface GreeterStub extends Fetcher {
  greet(name: string): Promise<string>;
}

// The JSON `/json` reads.
interface Order {
  item: string;
  quantity?: number;
  tags?: string[];
}

// A row of the Durable Object's SQL table.
interface Visit {
  id: number;
  path: string;
}

// JS globals the tour uses that neither workers-types nor std.ir declares.
interface Json {
  parse(text: string): any;
  stringify(value: any): string;
}
declare const JSON: Json;

// A typed array is an `ArrayBufferView`, so `digest` takes one: beside the
// class, the IR gives it the copy `u8[]`, a Zena array.
interface SubtleCrypto {
  digest(algorithm: string, data: Uint8Array): Promise<ArrayBuffer>;
}

// workers-types declares the rewriter's handlers as optional methods,
// which lower as methods to call. As a property the handler is a
// function to set.
interface ElementHandlers {
  element?: (element: Element) => void;
}
interface HTMLRewriter {
  on(selector: string, handlers: ElementHandlers): HTMLRewriter;
}

// The name the app uses for a literal union workers-types writes inline:
// an alias names every use of its literals.
type NoStoreOrNoCache = "no-store" | "no-cache";
