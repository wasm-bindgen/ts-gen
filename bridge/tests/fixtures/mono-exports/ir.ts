ir 1;

export namespace util {
  id<T in (u32, str)>(x: T): T;
  pick<T in (f64, bool)>(a: T, b: T): T;
  either<T>(x: T): T;
}
