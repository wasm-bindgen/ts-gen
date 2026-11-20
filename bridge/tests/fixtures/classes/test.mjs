import { assert, lang } from "./harness.mjs";
import { Point } from "./host.mjs";

class Bag {
  static count = 0;
  constructor(name) {
    this.name = name;
    this.items = ["a", "b"];
    this.calls = [];
    Bag.count++;
  }
  static make() {
    return new Bag("made");
  }
  describe(...args) {
    return `${this.name}:${JSON.stringify(args)}`;
  }
  swap(...args) {
    return JSON.stringify(args.map((a) => (a === undefined ? "u" : a))) + args.length;
  }
  gather(...args) {
    return JSON.stringify(args);
  }
  withOpts(x, opts) {
    return `${x}:${JSON.stringify(opts)}`;
  }
  keys(opts) {
    return Object.keys(opts).join(",");
  }
  signal(s) {
    return s;
  }
  get 0() {
    return this.items[0];
  }
  isBag(v) {
    return v instanceof Bag;
  }
  fill(view) {
    this.view = view;
    view.fill(7);
  }
  fillUnshared(view) {
    this.fill(view);
  }
  sum(view) {
    return view.reduce((a, b) => a + b, 0);
  }
  echo(v) {
    return String(v);
  }
  echoMissing(v) {
    return String(v);
  }
  *[Symbol.iterator]() {
    yield* this.items;
  }
}
class Sub extends Bag {}
Object.defineProperty(Bag.prototype, 0, {
  get() {
    return this.items[0];
  },
  set(v) {
    this.items[0] = v;
  },
});
globalThis.Bag = Bag;
globalThis.Sub = Sub;
globalThis.version = "1.0";
globalThis.add = (a, b) => a + b;
globalThis.math = Math;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const str = (w) => L.takeStr(...w);

// Constructors, statics and accessors.
const bag = L.balanced(() => L.call("Bag.new(str)", ...L.str("b1")));
assert.ok(bag instanceof Bag);
assert.equal(bag.name, "b1");
assert.equal(L.call("Bag.get:count()"), 1);
L.call("Bag.set:count(u32)", 10);
assert.equal(Bag.count, 10);
assert.ok(L.call("Bag.make()") instanceof Bag);
assert.equal(str(L.call("Bag#get:name()", bag)), "b1");
L.call("Bag#set:name(str)", bag, ...L.str("b2"));
assert.equal(bag.name, "b2");

// Receivers come from the language, which owns its casts (§4).
assert.equal(str(L.call("Bag#get:name()", new Sub("sub"))), "sub");

// @js(name) and constant args.
assert.equal(str(L.call("Bag#label()", bag)), "b2:[]");
assert.equal(str(L.call("Bag#shout()", bag)), 'b2:["loud",2]');
// Reordering and undefined padding.
assert.equal(str(L.call("Bag#swap(i32,i32)", bag, 1, 2)), '[2,"u",1]3');
// Spread.
L.balanced(() => {
  const p = L.bytes(new Float64Array([1.5, 2]), 8);
  assert.equal(str(L.call("Bag#gather(str,arr<f64>)", bag, ...L.str("f"), p, 2)), '["f",1.5,2]');
});
// A nullable object argument with fills, created when absent.
assert.equal(str(L.call("Bag#withOpts(u8,Options?)", bag, 3, null)), '3:{"mode":"fill"}');
{
  const o = { keep: 1 };
  assert.equal(str(L.call("Bag#withOpts(u8,Options?)", bag, 3, o)), '3:{"keep":1,"mode":"fill"}');
}
// A field filled from a parameter, and left off when that parameter is absent.
assert.equal(str(L.call("Bag#fieldKeys(Options?,str,str?)", bag, null, ...L.str("m"), ...L.str("t"))), "mode,tag");
assert.equal(str(L.call("Bag#fieldKeys(Options?,str,str?)", bag, null, ...L.str("m"), 0, -1)), "mode");
// let: a value the bridge creates for the call.
{
  const s = L.call("Bag#signal()", bag);
  assert.ok(s instanceof AbortSignal);
  assert.equal(L.call("AbortSignal#get:aborted()", s), 0);
}
// Indexing.
assert.deepEqual(str(L.call("Bag#at(u32)", bag, 0)), "a");
assert.deepEqual(L.call("Bag#at(u32)", bag, 5), [0, -1]);
L.call("Bag#setAt(u32,str)", bag, 0, ...L.str("z"));
assert.equal(bag.items[0], "z");
// Well-known symbols.
assert.deepEqual([...L.call("Bag#iterator()", bag)], ["z", "b"]);
assert.equal(L.call("Bag#isBag(Any)", bag, bag), 1);

// *T[] views over language memory, bounds-checked.
{
  const p = L.alloc(4, 1);
  L.call("Bag#fill(*mut u8[])", bag, p, 4);
  assert.deepEqual([...L.u8().slice(p, p + 4)], [7, 7, 7, 7]);
  assert.equal(bag.view.buffer, L.ex.memory.buffer);
  // An unshared memory needs no copy.
  L.call("Bag#fillUnshared(*mut u8[])", bag, p, 4);
  assert.equal(bag.view.buffer, L.ex.memory.buffer);
  L.ex["tw:free"](p, 4, 1);
  const q = L.bytes(new Float64Array([1, 2, 3.5]), 8);
  assert.equal(L.call("Bag#sum(*f64[])", bag, q, 3), 6.5);
  L.throwsType(() => L.call("Bag#sum(*f64[])", bag, L.ex.memory.buffer.byteLength - 8, 2));
}
// @js(null) on a parameter: absence flows to JS as null.
assert.equal(str(L.call("Bag#echo(str?)", bag, 0, -1)), "null");
assert.equal(str(L.call("Bag#echoMissing(str?)", bag, 0, -1)), "undefined");

// @js(object) constructors and null-writing setters.
{
  const o = L.call("Options.new()");
  assert.deepEqual(Object.getPrototypeOf(o), Object.prototype);
  L.call("Options#set:mode(str)", o, ...L.str("cors"));
  assert.equal(o.mode, "cors");
  assert.equal(str(L.call("Options#get:mode()", o)), "cors");
  L.call("Options#set:modeWithNull()", o);
  assert.equal(o.mode, null);
  assert.deepEqual(L.call("Options#get:mode()", o), [0, -1]);
}

// Globals are looked up live.
assert.equal(str(L.call("get:version()")), "1.0");
globalThis.version = "2.0";
assert.equal(str(L.call("get:version()")), "2.0");
L.call("set:version(str)", ...L.str("3.0"));
assert.equal(globalThis.version, "3.0");
assert.equal(L.call("add(i32,i32)", 2, 3), 5);
globalThis.add = (a, b) => a * b;
assert.equal(L.call("add(i32,i32)", 2, 3), 6);
assert.equal(L.call("math.biggest(f64,f64)", 2, 3), 3);

// Module declarations, through their exports.
{
  const pt = L.call("Point.new(f64,f64)", 3, 4);
  assert.ok(pt instanceof Point);
  assert.equal(L.call("Point#norm()", pt), 5);
  assert.equal(L.call("Point#get:x()", pt), 3);
  assert.equal(L.call("Point.origin()").x, 0);
  const h = L.call("hidden()");
  assert.equal(str(L.call("Hidden#get:secret()", h)), "s3");
}

console.log("ok");
