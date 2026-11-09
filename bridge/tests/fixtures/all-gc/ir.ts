ir 1;

class Request {}
class ReadableStream {}
enum Method {
  @js("GET") Get;
  @js("POST") Post;
}
enum Body {
  Text(str);
  Bytes(u8[]);
  Stream(ReadableStream);
}
enum Value {
  Num(f64);
  Pair(u32, str);
  Many(arr<str>);
  @fallback Other(Any);
}

class Probe {
  makeStr(): str;
  seeStr(v: str);
  makeOptStr(): str?;
  seeOptStr(v: str?);
  makeBytes(): u8[];
  seeBytes(v: u8[]);
  makeFloats(): f64[];
  seeFloats(v: f64[]);
  makeBigs(): i64[];
  seeBigs(v: i64[]);
  makeOptBytes(): u8[]?;
  seeOptBytes(v: u8[]?);
  makeNums(): arr<u32>;
  seeNums(v: arr<u32>);
  makeU8s(): arr<u8>;
  seeU8s(v: arr<u8>);
  makeI8s(): arr<i8>;
  seeI8s(v: arr<i8>);
  makeBools(): arr<bool>;
  seeBools(v: arr<bool>);
  makeI16s(): arr<i16>;
  seeI16s(v: arr<i16>);
  makeU16s(): arr<u16>;
  seeU16s(v: arr<u16>);
  makeF16s(): arr<f16>;
  seeF16s(v: arr<f16>);
  makeI53s(): arr<i53>;
  seeI53s(v: arr<i53>);
  makeStrs(): arr<str>;
  seeStrs(v: arr<str>);
  makeOptStrs(): arr<str?>?;
  seeOptStrs(v: arr<str?>?);
  makeRows(): arr<(u32, str, Request)>;
  seeRows(v: arr<(u32, str, Request)>);
  makeNested(): arr<arr<str>>;
  seeNested(v: arr<arr<str>>);
  makeMixed(): arr<(bool, i53?, f64)>;
  seeMixed(v: arr<(bool, i53?, f64)>);
  makePair(): (u32, str);
  seePair(v: (u32, str));
  makeOptPair(): (u16, str)?;
  seeOptPair(v: (u16, str)?);
  makeBody(): Body;
  seeBody(v: Body);
  makeOptBody(): Body?;
  seeOptBody(v: Body?);
  makeValue(): Value;
  seeValue(v: Value);
  makeMethod(): Method;
  seeMethod(v: Method);
  makeBodies(): arr<Body>;
  seeBodies(v: arr<Body>);
  fill(v: mut u8[]);
}
