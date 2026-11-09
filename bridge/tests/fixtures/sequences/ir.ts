ir 1;

class Request {}

class Probe {
  str(v: str): str;
  optStr(v: str?): str?;
  bytes(v: u8[]): u8[];
  floats(v: f64[]): f64[];
  bigs(v: i64[]): i64[];
  optBytes(v: u8[]?): u8[]?;
  fill(v: mut u8[]);
  nums(v: arr<u32>): arr<u32>;
  strs(v: arr<str>): arr<str>;
  optStrs(v: arr<str?>?): arr<str?>?;
  rows(v: arr<(u32, str, Request)>): arr<(u32, str, Request)>;
  reqs(v: arr<Request>): arr<Request>;
  nested(v: arr<arr<str>>): arr<arr<str>>;
  mixed(v: arr<(bool, i53?, f64)>): arr<(bool, i53?, f64)>;
  pair(v: (u32, (f64, bool))): (u32, (f64, bool));
  optPair(v: (u16, str)?): (u16, str)?;
}
