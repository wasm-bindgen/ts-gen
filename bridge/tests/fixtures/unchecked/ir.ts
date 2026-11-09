ir 1;

class Request {}
enum Method {
  @js("GET") Get;
  @js("POST") Post;
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
  nums(): arr<u32>;
  method(): Method;
  request(): Request;
}
