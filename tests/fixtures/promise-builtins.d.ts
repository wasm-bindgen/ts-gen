//! @ts-gen --lib-name promise-builtins --export promise-builtins --export tests/fixtures/promise-builtins.d.ts
//! @ts-gen --export node:fs --export fs --export cloudflare:sockets --export bun:sqlite --export user-lib --export npm:lodash --export jsr:@std/path --export https://esm.sh/foo

// Fixture: under `--fallible` (the `mono` snapshot) an async callable's
// sync `_promise` form catches a synchronous throw, except where calling
// it never throws synchronously, because the host's async functions
// reject rather than throw:
// * a builtin, whose binding resolves to a specifier with a `scheme:`
//   (`node:fs`, `cloudflare:sockets`, `bun:sqlite`) or a bare Node
//   builtin (`fs`), but not a package registry's (`npm:`, `jsr:`), a
//   `data:` or `blob:` URL, or a URL with a `//` (`https://…`);
// * a global: a global function, a global class's methods and statics,
//   a global namespace's functions;
// * a callable declared `async`. TypeScript rejects `async` in an ambient
//   context, but ts-gen parses every input, a `.ts` one included, as
//   declarations, and the modifier comes through.
// `@throws` on an async callable describes the rejection, which the
// `async fn` form keeps carrying either way.

/** The `--lib-name` module is the user's: `Result<Promise<JsString>, JsValue>`. */
export declare function load(url: string): Promise<string>;

/**
 * Typed: `Result<Promise<Number>, TypeError>`.
 * @throws {TypeError} on a malformed key
 */
export declare function count(key: string): Promise<number>;

/** Declared `async`: plain `Promise<Number>`. */
export declare async function size(key: string): Promise<number>;

export declare class Store {
  /** User module: `Result<Promise<JsString>, JsValue>`. */
  get(key: string): Promise<string>;
  /** Declared `async`: plain. */
  async put(key: string, value: string): Promise<void>;
  /** Declared `async`: plain. */
  static async open(name: string): Promise<Store>;
}

declare global {
  /** A global function: plain `Promise<Undefined>`. */
  function sleep(ms: number): Promise<void>;

  class Cache {
    /** A global class's method: plain. */
    match(url: string): Promise<string | undefined>;
    /**
     * `@throws` types the rejection: the `async fn` form is
     * `Result<(), TypeError>`, the `_promise` form stays plain.
     * @throws {TypeError} on a non-GET request
     */
    put(url: string, body: string): Promise<void>;
    /** A global class's static: plain. */
    static open(name: string): Promise<Cache>;
  }

  namespace scheduler {
    /** A function of a global namespace: plain. */
    function wait(ms: number): Promise<void>;
  }
}

declare module "node:fs" {
  /** `node:` builtin: plain. */
  export function readFile(path: string): Promise<string>;
}

declare module "fs" {
  /** Bare Node builtin: plain. */
  export function readFile(path: string): Promise<string>;
}

declare module "cloudflare:sockets" {
  export interface Socket {
    /** `cloudflare:` builtin method: plain. */
    close(): Promise<void>;
  }
  /** `cloudflare:` builtin: plain. */
  export function connect(address: string): Promise<Socket>;
}

declare module "bun:sqlite" {
  /** Any other specifier with a `scheme:` is a builtin: plain. */
  export function open(path: string): Promise<number>;
}

declare module "user-lib" {
  /** A user module: `Result<Promise<JsString>, JsValue>`. */
  export function fetchText(url: string): Promise<string>;
}

declare module "npm:lodash" {
  /** A package registry's scheme is no builtin: `Result<Promise<JsString>, JsValue>`. */
  export function fetchText(url: string): Promise<string>;
}

declare module "jsr:@std/path" {
  /** A package registry's scheme is no builtin: `Result<Promise<JsString>, JsValue>`. */
  export function fetchText(url: string): Promise<string>;
}

declare module "https://esm.sh/foo" {
  /** A URL is no builtin: `Result<Promise<JsString>, JsValue>`. */
  export function fetchText(url: string): Promise<string>;
}
