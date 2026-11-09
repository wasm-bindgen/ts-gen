ir 1;

class Request {}
enum Method {
  @js("GET") Get;
  @js("POST") Post;
}

class Map<K, V> {
  set(key: K, value: V): Map<K, V>;
}

class Probe {
  take(v: Str): Str;
  opt(v: Str?): Str?;
  arr(v: Arr<str>): Arr<str>;
  tup(v: Tuple<u32, str>): Tuple<u32, str>;
  num(v: F64): F64;
  nopt(v: @js(null) Str?);
  nested(v: Str??): Str??;
}
