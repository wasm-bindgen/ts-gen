// The module's own exports, in the shape JS importers see.
/** A top-level function. */
export function version(): number;

/** Same leaf as a default member: both are kept apart in Rust. */
export function add(a: number, b: number): number;

/** Types from js_sys in a provider signature. */
export function sum(values: Record<string, number>): number;
export function describeSymbol(s: symbol): string;
export function double(x: bigint): bigint;
export function keys(o: { a: number }): string[];
export function notify(cb: (e: Error | null, n: number) => void): void;
export function retOpt(x: string | null): string;
/** A nullable result is a write: `Nullable<String>`, so JS receives `null`. */
export function find(id: number): string | null;

export const api: {
  math: {
    /** Nested two deep. */
    add(a: number, b: number): number;
  };
  str: {
    /** Same leaf name on another path. */
    add(a: string, b: string): string;
  };
};

declare const handler: {
  /** Receives a value, returns one. */
  issue(id: number): Ticket;
  describe(ticket: Ticket | null): string;
  /** Collides with the top-level `add` by leaf. */
  add(a: number): number;
};
export default handler;
