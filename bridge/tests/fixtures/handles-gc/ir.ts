ir 1;

enum KvError extends err {
  @js(RangeError[code = 7]) Coded(Str);
}

export class Counter {
  new(start: u32);
  inc(by: u32): u32;
  label(prefix: str): str;
  same(other: Counter?): bool;
  fail(): unit throws KvError;
  static zero(): Counter;
}

export namespace util {
  maybe(c: Counter?): Counter?;
  apply(cb: fn(u32) -> u32, x: u32): u32;
  each(xs: arr<Counter>): u32;
}

class Target {
  listen(@once cb: fn(u32));
  on(cb: fn(str) -> str);
  keep(c: Counter);
}
