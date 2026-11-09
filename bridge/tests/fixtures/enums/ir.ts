ir 1;

class ReadableStream {}
class Request {}

enum Method {
  @js("GET") Get;
  @js("POST") Post;
  Latest = .Post;
}
enum Mode {
  @js("cors") Cors;
  @js("same-origin") SameOrigin;
  @fallback Unknown;
}
enum Flags: u32 {
  Read = 1;
  Write = 2;
  ReadWrite = 3;
}
enum Small: i8 {
  Neg = -1;
  Pos = 1;
}
enum Body {
  Text(str);
  Bytes(u8[]);
  Stream(ReadableStream);
}
enum Value {
  Num(f64);
  Flag(bool);
  Big(i64);
  Pair(u32, str);
  Inner(Method);
  Many(arr<Request>);
  @fallback Other(Any);
}
interface Init {
  get url(): str;
}
enum Arg {
  Later(future<u32>);
  Url(str);
  Init(Init);
}
enum Strict {
  Num(u8);
  Name(str);
}
enum DateUnit {
  @js("year") Year;
  @js("day") Day;
}
enum TimeUnit {
  @js("hour") Hour = 3;
  @js("minute") Minute;
}
enum AnyUnit extends DateUnit, TimeUnit {
  @js("week") Week;
}
enum Gapped {
  Num(u8) = 5;
  Name(str);
}

class Probe {
  method(v: Method): Method;
  optMethod(v: Method?): Method?;
  mode(v: Mode): Mode;
  flags(v: Flags): Flags;
  optFlags(v: Flags?): Flags?;
  small(v: Small): Small;
  body(v: Body): Body;
  optBody(v: Body?): Body?;
  value(v: Value): Value;
  strict(v: Strict): Strict;
  arg(): Arg;
  unit(v: AnyUnit): AnyUnit;
  time(v: TimeUnit): TimeUnit;
  gapped(v: Gapped): Gapped;
  maybe(v: str??): str??;
}
