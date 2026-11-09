import { assert } from "./harness.mjs";

class Probe {
  bytes() {
    return this.next;
  }
  see(xs) {
    this.seen = xs;
  }
  write(xs) {
    xs.fill(9);
  }
  list() {
    return this.next;
  }
  seeList(xs) {
    this.seen = xs;
  }
  rows() {
    return this.next;
  }
  pairs() {
    return this.next;
  }
  on(cb) {
    this.cb = cb;
  }
  keep(c) {
    this.kept = c;
  }
}
globalThis.Probe = Probe;

const { instantiate, Counter, util, scratchUse } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));

// Host carriers: every value crosses as the JS value itself.
let ex;
let got;
const impl = {
  "Counter.new(u32)": (self, start) => ({ n: start, self }),
  "Counter#get:value()": (self, h) => h.n,
  "Counter.zero()": () => (impl.zero ??= { n: 0 }),
  "util.sum(u32[])": (xs) => {
    got = xs;
    return xs.reduce((a, b) => a + b, 0);
  },
  "util.names(arr<str>)": (xs) => {
    got = xs;
    return xs.map((s) => s.toUpperCase());
  },
  "util.counters(arr<Counter>)": (xs) => {
    got = xs;
    return [...xs].reverse();
  },
  "util.maybe(f64[]?)": (xs) => xs,
  "util.apply(fn(u32)->u32,u32)": (f, x) => ex["call:call:fn(u32)->u32"](f, x) + 1,
  "tw:call:fn(u32)->str": (env, x) => `${env.name}:${x}`,
};
ex = instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }).exports;
const call = (k, ...a) => ex["call:" + k](...a);
const p = new Probe();

// JS → language: a validated, fresh copy.
const xs = new Uint32Array([1, 2, 3]);
assert.equal(util.sum(xs), 6);
assert.ok(got instanceof Uint32Array && got !== xs);
xs.fill(0);
assert.deepEqual(got, new Uint32Array([1, 2, 3]), "JS writes after the call miss the language's copy");
assert.throws(() => util.sum([1, 2]), TypeError);
assert.deepEqual(util.names(["a", "b"]), ["A", "B"]);
assert.throws(() => util.names(["a", 1]), TypeError);
assert.equal(util.maybe(null), undefined);
assert.deepEqual(util.maybe(new Float64Array([1.5])), new Float64Array([1.5]));
p.next = new Uint8Array([4, 5]);
const b = call("Probe#bytes()", p);
assert.deepEqual(b, p.next);
assert.notEqual(b, p.next, "an import result is the language's own copy");
p.next = [1, 2];
assert.throws(() => call("Probe#bytes()", p), TypeError);
p.next = [7, 8];
const list = call("Probe#list()", p);
assert.equal(call("arr:len", list), 2);
assert.equal(call("arr:get<u32>", list, 1), 8);
p.next = [7, -1];
assert.throws(() => call("Probe#list()", p), TypeError);
p.next = [[1], [2, 3]];
assert.deepEqual(call("Probe#rows()", p), [[1], [2, 3]]);
p.next = [[1, "a"]];
assert.deepEqual(call("Probe#pairs()", p), [[1, "a"]]);

// Language → JS: `T[]` a fresh copy too, `arr<T>` the value as is; `mut
// T[]` writes land in the language's.
const mine = new Uint8Array(3);
call("Probe#see(u8[])", p, mine);
assert.ok(p.seen instanceof Uint8Array && p.seen !== mine);
p.seen.fill(1);
assert.deepEqual(mine, new Uint8Array(3));
call("Probe#write(mut u8[])", p, mine);
assert.deepEqual(mine, new Uint8Array([9, 9, 9]));
call("Probe#seeList(arr<str>)", p, ["x"]);
assert.deepEqual(p.seen, ["x"]);

// Host handles are references, paired like GC handles.
const c = new Counter(3);
assert.equal(c.value, 3);
assert.equal(Counter.zero(), Counter.zero());
const z = Counter.zero();
assert.deepEqual(util.counters([c, z]), [z, c]);
assert.equal(got[0], c, "elements are the JS values of `T`");
call("Probe#keep(Counter)", p, call("unbox:Counter", c));
assert.equal(p.kept, c);
assert.throws(() => util.counters([c, {}]), TypeError);

// Callbacks.
const env = { name: "env" };
call("Probe#on(fn(u32)->str)", p, env);
assert.equal(p.cb(4), "env:4");
call("destroy:fn(u32)->str", env);
assert.throws(() => p.cb(4), /destroyed/);
assert.equal(util.apply((x) => x * 2, 5), 11);
assert.equal(scratchUse()[0], 72, "host values never touch scratch");
console.log("done");
