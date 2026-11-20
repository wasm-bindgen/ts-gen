ir 1;

class Probe {
  names(): arr<str>;
  see(xs: arr<str>);
  pairs(): arr<(u32, str?)>;
  seePairs(xs: arr<(u32, str?)>);
  nested(): arr<arr<str>>;
  seeNested(xs: arr<arr<str>>);
  counters(): arr<Counter>;
  seeCounters(xs: arr<Counter>);
  bodies(): arr<Body>;
  seeBodies(xs: arr<Body>);
}

enum Body {
  Text(str);
  Num(f64);
  Tagged(u32, str);
}

export class Counter {
  new();
}
