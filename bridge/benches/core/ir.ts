ir 1;

class Req {}
enum Body {
  Text(str);
  Bytes(u8[]);
  Stream(Req);
}
enum Oops extends err {
  @js(RangeError) Range(Str);
}
class Map<K, V> {
  get(key: K): V?;
  set(key: K, value: V): Map<K, V>;
}

class Probe {
  noop();
  id(v: i32): i32;
  idF(v: f64): f64;
  optF(v: f64?): f64?;
  optU8(v: u8?): u8?;
  seeStr(v: str);
  makeStr(): str;
  seeBytes(v: u8[]);
  makeBytes(): u8[];
  seeF64s(v: arr<f64>);
  makeF64s(): arr<f64>;
  seeStrs(v: arr<str>);
  makeStrs(): arr<str>;
  seeRows(v: arr<(u32, str)>);
  makeRows(): arr<(u32, str)>;
  put(a: Req, b: Req);
  makeReq(): Req;
  send(b: Body);
  risky(v: i32): i32 throws Oops;
  each(cb: fn(i32) -> i32, n: u32);
  pair(): (u32, f64);
  later(): future<u32>;
  seeBox(v: Str);
  makeBox(): Str;
}

export namespace util {
  id(v: i32): i32;
  len(v: str): u32;
}
