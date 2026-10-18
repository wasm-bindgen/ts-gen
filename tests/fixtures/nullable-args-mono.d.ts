//! @ts-gen --experimental-generic-mono

// Under per-monomorphization `null` is an ordinary arm of a written
// argument: each other alternative is taken bare, and `null` has a binding
// of its own that passes `null` itself (`f(v)` and `f_with_null()`), for
// arguments, setters and builders alike. An optional parameter's trailing
// `null` arm is left out, as omitting the argument covers it, and so is the
// `null` of `any | null`, which `&JsValue` takes.

declare class Signal {}
declare class Body<T = unknown> {
  constructor(body: string | null, length?: number | null);
  static from(signal: Signal | null, tags: string[] | null): Body;
  put<V>(value: V | null): void;
}
interface ListOptions {
  prefix?: string | null;
  limit?: number | null;
  signal?: Signal | null;
  body?: string | Signal | null;
}
declare function clearTimeout(id: number | null): void;
interface PutOptions {
  metadata?: any | null;
}
interface Lease {
  holder: string | null;
}
declare function lease(options: PutOptions, lease: Lease): void;

type BodyInit = string | ArrayBuffer | Signal;
interface ResponseInit {
  status?: number;
  // An optional `any` reads as `Option<JsValue>`.
  cf?: any;
  // An optional property that can only be absent has no accessors.
  error?: never;
}
declare class Reply {
  constructor(body?: BodyInit | null, init?: ResponseInit);
}
declare class Bucket {
  put(key: string, value: ArrayBuffer | string | null): Promise<void>;
}
