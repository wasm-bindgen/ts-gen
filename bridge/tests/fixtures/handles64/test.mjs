import { assert, lang, collect } from "./harness.mjs";

class Target {
  on(cb) {
    this.cb = cb;
  }
}
globalThis.Target = Target;

const { instantiate, Counter } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));

// memory64: handles are i64, so the bridge pairs bigints.
const objs = new Map();
let next = 64n;
const drops = [];
let L;
const impl = {
  "Counter.new(u32)": (self, start) => {
    const h = (next += 8n);
    objs.set(h, { n: start });
    return h;
  },
  "Counter#inc(u32)": (self, h, by) => (objs.get(h).n += by),
  "Counter#label()": (self, h) => L.own(`#${objs.get(h).n}`),
  "Counter.maybe(Counter?)": (h) => h,
  "tw:call:fn(u32)->str": (env, x) => L.own(`${env}:${x}`),
  "tw:drop:fn(u32)->str": (env) => drops.push(env),
  "tw:drop:Counter": (h) => drops.push(h),
};
L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }));
assert.ok(L.wide);

const c = new Counter(1);
assert.equal(c.inc(2), 3);
assert.equal(L.balanced(() => c.label()), "#3");
assert.equal(Counter.maybe(c), c);
assert.equal(Counter.maybe(null), undefined);

const t = new Target();
L.call("Target#on(fn(u32)->str)", t, 4096n);
const cb = t.cb;
L.call("Target#on(fn(u32)->str)", t, 4096n);
assert.equal(t.cb, cb);
assert.equal(L.balanced(() => cb(7)), "4096:7");
L.call("Target#on(fn(u32)->str)", t, 8192n);
L.call("destroy:fn(u32)->str", 8192n);
assert.throws(() => t.cb(1), /destroyed/);

t.cb = null;
await collect();
assert.deepEqual(drops, [], "live or destroyed");
// `cb` and `c` are still live; a fresh unreferenced counter drops.
(() => new Counter(9))();
await collect();
assert.equal(drops.length, 1);
assert.equal(typeof drops[0], "bigint");

// The termination byte sits at an i64 address.
L.u8()[8] = 1;
assert.throws(() => c.inc(1), /Module terminated/);
assert.throws(() => cb(1), /Module terminated/);
console.log("done");
