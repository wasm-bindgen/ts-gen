ir 1;

// Interfaces a separate value constructs (§6): `new Wrap(...)`, with
// overloads, a second value's named constructor beside a dictionary's own,
// a module's type-only interface through its exported value, and a global
// interface through a module's.
interface Stub {
  @js(new = Wrap) new(value: str);
  @js(new = Wrap) new(n: f64, unit: str);
  get value(): str;
}
interface Point {
  @js(object) new();
  @js(new = Shapes.Pt) new pt(x: f64);
  get x(): f64?;
}
module "./host.mjs" {
  interface Counter {
    @js(new = Count) new(start: f64);
    count(): f64;
  }
  export { Count, Make };
}
interface Remote {
  @js(new = "./host.mjs".Make) new();
  get id(): f64;
}
