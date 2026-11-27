ir 1;

class Request {}

class Probe {
  str(): str;
  bytes(): u8[];
  nums(): arr<u32>;
  request(): Request;
  maybe(): Request?;
}

export namespace app {
  len(s: str): u32;
  bytes(b: u8[]): u32;
  nums(a: arr<u32>): u32;
  request(r: Request): bool;
  maybe(r: Request?): bool;
}
