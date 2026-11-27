ir 1;

class Request {}

class Slot<T> {
  set(value: T);
}

class Probe {
  u8(): u8;
  str(): str;
  bytes(): u8[];
  nums(): arr<u32>;
  request(): Request;
  maybe(): Request?;
  see(s: str, b: u8[], n: arr<u32>);
}

export namespace app {
  u8(x: u8): u8;
  len(s: str): u32;
  bytes(b: u8[]): u32;
  nums(a: arr<u32>): u32;
  request(r: Request): bool;
  maybe(r: Request?): bool;
}
