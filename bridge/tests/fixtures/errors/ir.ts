ir 1;

enum KvError extends err {
  @js(DOMException[name = "NotFoundError"]) NotFound(Str);
  @js(RangeError[code = 7]) Coded(Str);
}

class Kv {
  get(key: str): str throws KvError;
  plain(key: str): str throws;
  raw(key: str): str;
  list(): arr<str> throws;
  count(): u8 throws KvError;
}
