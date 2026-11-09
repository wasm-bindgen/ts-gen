ir 1;

class Request {}
enum Body {
  Text(str);
  Stream(Request);
}

class Probe {
  str(v: str): str;
  optStr(v: str?): str?;
  floats(v: f64[]): f64[];
  strs(v: arr<str>): arr<str>;
  rows(v: arr<(u32, str, Request)>): arr<(u32, str, Request)>;
  body(v: Body): Body;
  fill(p: *mut u8[]);
}
