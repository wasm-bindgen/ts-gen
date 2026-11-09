ir 1;

enum Body {
  Text(str);
  Num(u32);
}

class Probe {
  str(v: str): str;
  optStr(v: str?): str?;
  strs(v: arr<str>): arr<str>;
  rows(v: arr<(u32, str)>): arr<(u32, str)>;
  body(v: Body): Body;
  pair(v: (u8, str)): (u8, str);
}
