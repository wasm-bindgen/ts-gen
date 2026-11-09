import { assert } from "./harness.mjs";

class Target {
  listen(cb) {
    this.listened = cb;
  }
  on(cb) {
    this.cb = cb;
  }
  keep(c) {
    this.kept = c;
  }
}
globalThis.Target = Target;

const { instantiate, Counter, util, scratchUse } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));

// GC handles are references: the language's objects, played by JS objects.
let ex;
const calls = [];
const zero = { n: 0 };
let seen;
const impl = {
  "Counter.new(u32)": (self, start) => ({ n: start, self }),
  "Counter#inc(u32)": (self, h, by) => (h.n += by),
  "Counter#label(str)": (self, h, prefix) => `${prefix}${h.n}`,
  "Counter#same(Counter?)": (self, h, o) => (h === o ? 1 : 0),
  "Counter#fail()": () => {
    throw new WebAssembly.Exception(ex["tag:KvError"], [2, "coded"]);
  },
  // A base-less class is constructed natively, and wrapped on crossing.
  "Counter.zero()": () => zero,
  "util.maybe(Counter?)": (h) => h,
  "util.apply(fn(u32)->u32,u32)": (f, x) => ex["call:call:fn(u32)->u32"](f, x) + 1,
  "util.each(arr<Counter>)": (xs) => {
    seen = xs;
    return 1;
  },
  "tw:call:fn(u32)": (env, x) => {
    calls.push([env, x]);
  },
  "tw:call:fn(str)->str": (env, s) => `${env.name}:${s}`,
};
ex = instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }).exports;
const base = scratchUse();
const handleOf = (w) => ex["call:unbox:Counter"](w);

const c = new Counter(5);
assert.equal(c.inc(2), 7);
assert.equal(handleOf(c).self, c);
assert.equal(c.label("n="), "n=7");
assert.equal(c.same(c), true);
assert.equal(c.same(null), false);
assert.throws(() => c.fail(), (e) => e instanceof RangeError && e.code === 7);
assert.equal(Counter.zero(), Counter.zero());
assert.equal(handleOf(Counter.zero()), zero);
assert.equal(util.maybe(c), c, "identity round-trip");
assert.equal(util.maybe(null), undefined);
assert.equal(ex["call:box:Counter"](zero), Counter.zero());
assert.equal(util.apply((x) => x * 10, 4), 41);
assert.equal(util.each([c, Counter.zero()]), 1);
assert.deepEqual(scratchUse(), base, "entries restore scratch");

const t = new Target();
ex["call:Target#keep(Counter)"](t, handleOf(c));
assert.equal(t.kept, c);

const env = { name: "env" };
ex["call:Target#on(fn(str)->str)"](t, env);
const cb = t.cb;
ex["call:Target#on(fn(str)->str)"](t, env);
assert.equal(t.cb, cb);
assert.equal(cb("x"), "env:x");
assert.equal(ex["call:unbox:fn(str)->str"](cb), env);
assert.equal(ex["call:tryUnbox:fn(str)->str"](cb), env);
assert.equal(ex["call:tryUnbox:fn(str)->str"](() => {}), null);
assert.equal(ex["call:tryUnbox:Counter"](c), handleOf(c));
assert.equal(ex["call:tryUnbox:Counter"]({}), null);

const once = {};
ex["call:Target#listen(fn(u32))"](t, once);
t.listened(1);
assert.throws(() => t.listened(2), TypeError);
const doomed = {};
ex["call:Target#listen(fn(u32))"](t, doomed);
const f = t.listened;
ex["call:destroy:fn(u32)"](doomed);
assert.throws(() => f(1), /destroyed/);
assert.deepEqual(calls, [[once, 1]]);
const z = Counter.zero();
ex["call:destroy:Counter"](zero);
assert.throws(() => z.inc(1), /destroyed/);
assert.notEqual(Counter.zero(), z, "destroy unpairs");

// Termination: every entry checks the flag first.
ex["tw:terminated"].value = 1;
assert.throws(() => c.inc(1), /Module terminated/);
assert.throws(() => cb("x"), /Module terminated/);
console.log("done");
