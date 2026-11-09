ir 1;

class AbortSignal {
  get aborted(): bool;
}
class AbortController {
  new();
  get signal(): AbortSignal;
}
class Options {
  @js(object) new();
  set mode(v: str);
  @js(name = "mode", args = (null)) set modeWithNull();
  get mode(): str?;
  @js(null) get missing(): str?;
}
class Bag {
  new(name: str);
  static get count(): u32;
  static set count(v: u32);
  static make(): Bag;
  get name(): str;
  set name(v: str);
  @js(name = "describe") label(): str;
  @js(name = "describe", args = ("loud", 2)) shout(): str;
  @js(args = (b, undefined, a)) swap(a: i32, b: i32): str;
  @js(args = (first, ...rest)) gather(first: str, rest: arr<f64>): str;
  @js(args = (x, opts? { mode = "fill" })) withOpts(x: u8, opts: Options?): str;
  @js(name = "keys", args = (opts? { mode = m, tag = t })) fieldKeys(opts: Options?, m: str, t: str?): str;
  @js(let c = AbortController.new(), args = (c.signal)) signal(): AbortSignal;
  @js(index) at(i: u32): str?;
  @js(index) setAt(i: u32, v: str);
  @js(symbol = iterator) iterator(): Any;
  isBag(v: Any): bool;
  fill(buf: *mut u8[]);
  @js(args = (p)) sum(p: *f64[]): f64;
  fillUnshared(@js(unshared) buf: *mut u8[]);
  echo(@js(null) v: str?): str;
  echoMissing(v: str?): str;
}
class Sub extends Bag {}

get version(): str;
set version(v: str);
add(a: i32, b: i32): i32;
namespace math {
  @js(name = "max") biggest(a: f64, b: f64): f64;
}

module "./host.mjs" {
  class Point {
    new(x: f64, y: f64);
    get x(): f64;
    static origin(): Point;
    norm(): f64;
  }
  interface Hidden {
    get secret(): str;
  }
  hidden(): Hidden;
  export { Point, Point as default, hidden as makeHidden };
}
