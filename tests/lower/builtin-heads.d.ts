// A namespace named like a built-in type is escaped, as a class is, so no
// path through one starts with the built-in.
declare namespace str {
  interface X {
    a: number;
  }
}
declare namespace mut {
  interface Y {
    b: number;
  }
}
declare function take(x: str.X, y: mut.Y): void;
declare namespace Outer {
  namespace f64 {
    interface Z {
      c: number;
    }
  }
  function take(z: f64.Z): void;
}
