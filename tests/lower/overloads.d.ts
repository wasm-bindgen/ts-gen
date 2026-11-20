//! @ts-gen --lib-name overloads

// Overload expansion: union parameters split into overloads,
// `null` and literal arms move into `@js(args)`, trailing optional
// parameters give arity overloads, and colliding keys are renamed after
// what tells them apart, keeping the JS name.

interface KV {
  get(key: string, type: "text"): Promise<string | null>;
  get(key: string, type: "json"): Promise<unknown>;
  put(key: string, value: string | ArrayBuffer, options?: PutOptions): Promise<void>;
}

interface PutOptions {
  ttl?: number;
}

declare class Node {
  append(child: Node | string): void;
  remove(): void;
  signal: AbortSignal | null;
}

declare function draw(x: number, y: number): void;
declare function draw(p: Point): void;
declare function pick(mode: "fast" | "slow", n?: number): string;
declare function rest(first: string, ...more: number[]): void;
// Omitting `label` drops the rest too, or `data[0]` would become the label.
declare function optionalRest(label?: string, ...data: any[]): void;
declare function tupleRest<Args extends any[]>(cb: (...args: Args) => void, ms?: number, ...args: Args): number;
declare function middle(a: string, b: number | undefined, c: string): void;

interface Point {
  x: number;
  y: number;
}

// Overloads telling apart only by a literal inside an options object are
// named after it; a later overload making the same call as an earlier
// one is unreachable in TypeScript and dropped.
interface GetOptions<Type> {
  type: Type;
  ttl?: number;
}
interface Store {
  read(key: string, options?: Partial<GetOptions<undefined>>): Promise<string | null>;
  read(key: string, options?: GetOptions<"text">): Promise<string | null>;
  read<T = unknown>(key: string, options?: GetOptions<"json">): Promise<T | null>;
  read(key: string, options?: GetOptions<"arrayBuffer">): Promise<ArrayBuffer | null>;
  write(key: string, value: string, options?: { onlyIf: string }): Promise<boolean | null>;
  write(key: string, value: string, options?: { ttl: number }): Promise<boolean>;
}
