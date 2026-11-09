ir 1;

class Map<K, V> {
  get(key: K): V?;
  set(key: K, value: V): Map<K, V>;
}

class Probe {
  take(v: Str): Str;
  opt(v: Str?): Str?;
  nopt(v: @js(null) Str?): @js(null) Str?;
  nested(v: Str??): Str??;
}
