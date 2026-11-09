ir 1;

enum KvError extends err {
  @js(DOMException[name = "NotFoundError"]) NotFound(Str);
}

export namespace util {
  pair(x: u32): (u32, str);
  maybe(x: f64?): f64?;
  lookup(key: str): str throws KvError;
  count(key: str): u32 throws KvError;
}

class Probe {
  triple(): (u32, str, f64);
  maybe(): f64?;
  get(key: str): (u32, str) throws KvError;
  on(cb: fn(u32) -> (u32, str));
}
