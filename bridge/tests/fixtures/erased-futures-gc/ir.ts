ir 1;

// Under GC handles a future of a type parameter is still read on its
// erasure: `get<Any?>` returns `externref`, where the erased value is
// `anyref`, and the language converts (§16).
class Box<T> {}

class Kv {
  get<T>(key: str): future<T?>;
  boxed<T>(): future<Box<T>>;
}
