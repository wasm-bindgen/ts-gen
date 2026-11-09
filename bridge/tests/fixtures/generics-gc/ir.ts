ir 1;

class Map<K, V> {
  get(key: K): V?;
  set(key: K, value: V): Map<K, V>;
}

enum Mode {
  @js("off") Off;
  Count(u32);
}

class Store {
  modes(): Map<Str, Mode>;
}

export class Counter {
  new();
}
