ir 1;

enum KvError extends err {
  @js(DOMException[name = "NotFoundError"]) NotFound(Str);
  @js(RangeError[code = 7]) Coded(Str);
}

export class Counter {
  new(start: u32);
  get value(): u32;
  set value(v: u32);
  inc(by: u32): u32;
  label(prefix: str): str;
  same(other: Counter?): bool;
  pick(x: u32): str;
  pick(x: str): str;
  fail(): unit throws KvError;
  static zero(): Counter;
}

class Base {
  new();
  baseName(): str;
}

export class Sub extends Base {
  @js(super = ()) new(x: u32);
  twice(): u32;
  static make(): Sub;
  static stray(): Sub;
}

class Needy {
  new(x: u32);
}

export class Orphan extends Needy {
  new(x: u32);
  static make(): Orphan;
}

class Plain {}

export class Dual extends Plain {
  new(x: Any);
  new(x: str);
}

export namespace util {
  twice(x: u32): u32;
  make(n: u32): Counter;
  stray(): Counter;
  apply(cb: fn(u32) -> u32, x: u32): u32;
  maybe(c: Counter?): Counter?;
  names(): arr<str>;
  @js(args = (b, undefined, a)) swap(a: u32, b: u32): u32;
  @js(name = "opt", args = (x, opts? { mode = "fast", tag = t })) fast(x: u32, opts: Any?, t: Any?): u32;
  @js(name = "opt", args = (x, opts? { mode = "slow" })) slow(x: u32, opts: Any?): u32;
  @js(args = (Mode.FAST)) fastOnly(): u32;
}

class Mode {
  static get FAST(): str;
}

export version(): str;

class Target {
  listen(@once cb: fn(u32));
  on(cb: fn(u32) -> str);
  keep(c: Counter);
  give(): Counter;
}
