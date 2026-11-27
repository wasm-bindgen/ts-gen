ir 1;

class Request {}
enum Method {
  @js("GET") Get;
  @js("POST") Post;
}
interface Sizes {
  get width(): u32;
}

class Keys {
  static echo<K in keyof Sizes>(key: K): K;
}

class Probe {
  u8(): u8;
  f64(): f64;
  str(): str;
  bytes(): u8[];
  nums(): arr<u32>;
  pair(): (u32, str);
  method(): Method;
  request(): Request;
  maybe(): Request?;
}

export namespace app {
  u8(x: u8): u8;
  f64(x: f64): f64;
  len(s: str): u32;
  bytes(b: u8[]): u32;
  nums(a: arr<u32>): u32;
  pair(p: (u32, str)): u32;
  method(m: Method): u32;
  request(r: Request): bool;
  maybe(r: Request?): bool;
  pick<K in keyof Sizes>(key: K): u32;
}
