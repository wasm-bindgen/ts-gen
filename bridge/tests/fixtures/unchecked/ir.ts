ir 1;

class Request {}
enum Method {
  @js("GET") Get;
  @js("POST") Post;
}

interface Sizes {
  get width(): u32;
  get label(): str;
}

class Keys {
  static echo<K in keyof Sizes>(key: K): K;
}

class Probe {
  u8(): u8;
  i8(): i8;
  u16(): u16;
  i32(): i32;
  f64(): f64;
  bool(): bool;
  i53(): i53;
  i64(): i64;
  str(): str;
  bytes(): u8[];
  buf(): buffer;
  nums(): arr<u32>;
  strs(): arr<str>;
  pair(): (u32, str);
  method(): Method;
  request(): Request;
  maybe(): Request?;
  cb(): (fn(u32))?;
}

export class Counter {
  new(start: u32);
  get value(): u32;
}

export len(s: str): u32;
export pick<K in keyof Sizes>(key: K): u32;
