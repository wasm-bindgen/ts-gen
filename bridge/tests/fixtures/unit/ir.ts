ir 1;

class Pair<A, B = Str> {
  get first(): A;
  get second(): B;
}

class Probe {
  take(v: unit, s: str): str;
  give(): unit;
  pair(v: (unit, u32)): (unit, u32);
  boxed(v: Unit): Unit;
  pairs(): Pair<unit, Str>;
}
