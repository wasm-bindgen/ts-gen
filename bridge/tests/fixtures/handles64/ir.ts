ir 1;

export class Counter {
  new(start: u32);
  inc(by: u32): u32;
  label(): str;
  static maybe(c: Counter?): Counter?;
}

class Target {
  on(cb: fn(u32) -> str);
}
