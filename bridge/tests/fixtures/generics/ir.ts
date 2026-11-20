ir 1;

class Map<K, V> {
  new();
  get(key: K): V?;
  set(key: K, value: V): Map<K, V>;
}

class Util {
  static first<T>(xs: arr<T>): T?;
  static pair<A, B>(a: A, b: B): (A, B);
}

class Bag<T in (u32, str)> {
  put(v: T);
}
