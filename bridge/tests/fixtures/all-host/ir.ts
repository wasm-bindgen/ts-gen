ir 1;

export class Counter {
  new(start: u32);
  get value(): u32;
  static zero(): Counter;
}

export namespace util {
  sum(xs: u32[]): u32;
  names(xs: arr<str>): arr<str>;
  counters(xs: arr<Counter>): arr<Counter>;
  maybe(xs: f64[]?): f64[]?;
  apply(cb: fn(u32) -> u32, x: u32): u32;
}

class Probe {
  bytes(): u8[];
  see(xs: u8[]);
  write(xs: mut u8[]);
  list(): arr<u32>;
  seeList(xs: arr<str>);
  rows(): arr<arr<u32>>;
  pairs(): arr<(u32, str)>;
  on(cb: fn(u32) -> str);
  keep(c: Counter);
}
