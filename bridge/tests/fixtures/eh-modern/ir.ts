ir 1;

enum KvError extends err {
  @js(DOMException[name = "NotFoundError"]) NotFound(Str);
}

class Kv {
  get(key: Str): Str throws KvError;
  raw(key: Str): Str;
  name(key: str): str throws KvError;
  nameRaw(key: str): str;
}
