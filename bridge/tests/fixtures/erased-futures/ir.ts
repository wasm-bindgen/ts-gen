ir 1;

// Futures of type parameters: the language imports their operations on
// the erasure (`take<Any?>`), as the lowered JSON names them.
class Box<T> {}

class Kv {
  get<T>(key: str): future<T?>;
  boxed<T>(): future<Box<T>>;
}
