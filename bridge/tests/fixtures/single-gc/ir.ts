ir 1;

export namespace util {
  trio(x: u32): (str, u32, f64);
}

class Probe {
  makeTrio(): (str, u32, f64);
  seeTrio(v: (str, u32, f64));
}
