ir 1;

class Stream {}

enum Body {
  Text(str);
  Num(f64);
  Stream(Stream);
}
enum Id {
  Name(str);
  Num(u32);
}

class Sink {
  write(b: union Body): u32;
  pair(a: union Id, b: union Id): u32;
  opt(b: union Body?): u32;
  nul(b: @js(null) union Id?): u32;
  both(b: union Id??): u32;
  set body(b: union Body);
}
