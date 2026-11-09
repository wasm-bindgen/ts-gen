ir 1;

enum Body {
  Text(str);
  Num(f64);
}

class Probe {
  makeStr(): str;
  seeStr(v: str);
  makeOptStr(): str?;
  seeOptStr(v: str?);
  makeStrs(): arr<str>;
  seeStrs(v: arr<str>);
  makePair(): (u32, str);
  seePair(v: (u32, str));
  makeBody(): Body;
  seeBody(v: Body);
  echo(v: str): str;
}
