import { assert, lang } from "./harness.mjs";

class Stream {}
class Sink {
  constructor() {
    this.seen = [];
  }
  write(b) {
    this.seen.push(b);
    return 1;
  }
  pair(a, b) {
    this.seen.push([a, b]);
    return 2;
  }
  opt(b) {
    this.seen.push(b);
    return 3;
  }
  nul(b) {
    this.seen.push(b);
    return 4;
  }
  both(b) {
    this.seen.push(b);
    return 5;
  }
  set body(b) {
    this.seen.push(b);
  }
}
Object.assign(globalThis, { Stream, Sink });

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const s = new Sink();
const seen = () => s.seen.pop();

// The whole enum and each arm make the same JS call.
assert.equal(L.call("Sink#write(Body)", s, 1, ...L.str("hi"), 0, null), 1);
assert.equal(seen(), "hi");
assert.equal(L.call("Sink#write(Body)", s, 2, 0, 0, 2.5, null), 1);
assert.equal(seen(), 2.5);
assert.equal(L.call("Sink#write(str)", s, ...L.str("hi")), 1);
assert.equal(seen(), "hi");
assert.equal(L.call("Sink#write(f64)", s, 2.5), 1);
assert.equal(seen(), 2.5);
const st = new Stream();
assert.equal(L.call("Sink#write(Stream)", s, st), 1);
assert.equal(seen(), st);

// Each `union` parameter varies independently.
assert.equal(L.call("Sink#pair(str,u32)", s, ...L.str("a"), 7), 2);
assert.deepEqual(seen(), ["a", 7]);
assert.equal(L.call("Sink#pair(Id,str)", s, 2, 7, 0, ...L.str("b")), 2);
assert.deepEqual(seen(), [7, "b"]);
assert.equal(L.call("Sink#pair(u32,Id)", s, 3, 1, ...L.str("c"), 0), 2);
assert.deepEqual(seen(), [3, "c"]);

// `?`s carry over to each arm.
assert.equal(L.call("Sink#opt(Body?)", s, 0, 0, 0, 0, null), 3);
assert.equal(seen(), undefined);
assert.equal(L.call("Sink#opt(f64?)", s, 0, 0), 3);
assert.equal(seen(), undefined);
assert.equal(L.call("Sink#opt(f64?)", s, 1, 4), 3);
assert.equal(seen(), 4);
assert.equal(L.call("Sink#nul(str?)", s, 0, -1), 4);
assert.equal(seen(), null);
assert.equal(L.call("Sink#nul(u32?)", s, 1, 9), 4);
assert.equal(seen(), 9);
assert.equal(L.call("Sink#both(u32??)", s, 2, 1, 0), 5);
assert.equal(seen(), 0);
assert.equal(L.call("Sink#both(u32??)", s, 2, 0, 0), 5);
assert.equal(seen(), null);
assert.equal(L.call("Sink#both(u32??)", s, 1, 0, 0), 5);
assert.equal(seen(), undefined);

L.call("Sink#set:body(str)", s, ...L.str("set"));
assert.equal(seen(), "set");
L.call("Sink#set:body(Body)", s, 2, 0, 0, 1.5, null);
assert.equal(seen(), 1.5);
