import { assert, lang, collect } from "./harness.mjs";

class Base {
  constructor() {
    this.based = true;
  }
  baseName() {
    return "base";
  }
}
class Needy {
  constructor(x) {
    this.x = x;
  }
}
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
  give() {
    return this.giving;
  }
}
class Mode {
  static FAST = "fast";
}
class Plain {}
Object.assign(globalThis, { Base, Needy, Target, Mode, Plain });

const { instantiate, Counter, Sub, Orphan, Dual, util, version } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));

// The language, played by JS: handles are numbers naming its objects.
const objs = new Map();
let next = 64;
const alloc = (o) => {
  const h = next;
  next += 8;
  objs.set(h, o);
  return h;
};
const drops = [];
const calls = [];
let L;
let thrown = null;
let seen;
// A handle the language keeps, which `util.stray()` returns.
let kept;
const impl = {
  // Linear languages must not keep `this`, so the test holds it weakly.
  "Counter.new(u32)": (self, start) => alloc({ n: start, self: new WeakRef(self) }),
  "Counter#get:value()": (self, h) => objs.get(h).n,
  "Counter#set:value(u32)": (self, h, v) => {
    objs.get(h).n = v;
  },
  "Counter#inc(u32)": (self, h, by) => (objs.get(h).n += by),
  "Counter#label(str)": (self, h, p, len) => L.own(`${L.readStr(p, len)}${objs.get(h).n}`),
  "Counter#same(Counter?)": (self, h, o) => (h === o ? 1 : 0),
  "Counter#pick(u32)": () => L.own("num"),
  "Counter#pick(str)": () => L.own("str"),
  "Counter#fail()": () => {
    throw new WebAssembly.Exception(L.ex["tag:KvError"], [2, "missing"]);
  },
  // A base-less class is constructed natively.
  "Counter.zero()": () => (objs.zero ??= alloc({ n: 0 })),
  "Sub.new(u32)": (self, x) => alloc({ n: x }),
  "Sub#twice()": (self, h) => objs.get(h).n * 2,
  // An imported base's instance is constructed through JS (`tw:new`).
  "Sub.make()": () => L.call("unbox:Sub", L.call("new:Sub.new(u32)", 21)),
  "Sub.stray()": () => alloc({ n: 0 }),
  "Orphan.new(u32)": (self, x) => alloc({ n: x }),
  "Orphan.make()": () => L.call("unbox:Orphan", L.call("new:Orphan.new(u32)", 1)),
  "Dual.new(Any)": (self) => alloc({ kind: "Any" }),
  "Dual.new(str)": (self) => alloc({ kind: "str" }),
  "util.twice(u32)": (x) => x * 2,
  "util.make(u32)": (n) => alloc({ n }),
  "util.stray()": () => kept,
  "util.apply(fn(u32)->u32,u32)": (f, x) => L.call("call:fn(u32)->u32", f, x) + 1,
  "util.maybe(Counter?)": (h) => h,
  "util.names()": () => {
    const items = ["a", "bc"].map((s) => L.own(s));
    const p = Number(L.ex["tw:realloc"](0, 0, 4, 16));
    items.forEach(([sp, sl], i) => {
      L.dv().setInt32(p + i * 8, sp, true);
      L.dv().setInt32(p + i * 8 + 4, sl, true);
    });
    return [p, 2];
  },
  "util.swap(u32,u32)": (a, b) => a * 10 + b,
  "util.fast(u32,Any?,Any?)": (...args) => (seen = args, 100),
  "util.slow(u32,Any?)": (...args) => (seen = args, 200),
  "util.fastOnly()": () => 300,
  "version()": () => L.own("1.0"),
  "tw:call:fn(u32)": (env, x) => {
    calls.push([env, x]);
    if (thrown) throw thrown;
  },
  "tw:call:fn(u32)->str": (env, x) => L.own(`${env}:${x}`),
  "tw:drop:fn(u32)": (env) => drops.push(["fn(u32)", env]),
  "tw:drop:fn(u32)->str": (env) => drops.push(["fn(u32)->str", env]),
  "tw:drop:Counter": (h) => drops.push(["Counter", h]),
  "tw:drop:Sub": (h) => drops.push(["Sub", h]),
  "tw:drop:Orphan": (h) => drops.push(["Orphan", h]),
  "tw:drop:Dual": (h) => drops.push(["Dual", h]),
};
L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }));
const handleOf = (w) => L.call("unbox:Counter", w);

// Exported classes: the constructor pairs `this` with the new handle.
const c = new Counter(5);
assert.ok(c instanceof Counter);
assert.equal(c.value, 5);
c.value = 7;
assert.equal(c.inc(2), 9);
assert.equal(objs.get(handleOf(c)).self.deref(), c, "`this` is the JS instance");
assert.equal(L.balanced(() => c.label("n=")), "n=9");
assert.equal(c.same(c), true);
assert.equal(c.same(null), false);
assert.equal(c.same(new Counter(1)), false);
L.throwsType(() => c.same({}), /Counter/);
L.throwsType(() => new Counter("x"), /u32/);
L.throwsType(() => new Counter(), /new Counter/);
L.throwsType(() => c.inc(), /Counter.inc/);
// Trailing `undefined`s past the parameters are dropped, as WebIDL does.
assert.equal(L.balanced(() => c.label("n=", undefined, undefined)), "n=9");
L.throwsType(() => c.label("n=", 2), /Counter.label/);
L.throwsType(() => Counter.prototype.inc.call({}, 1), /Counter/);

// Dispatch: arity, then type tests in declaration order.
assert.equal(L.balanced(() => c.pick(1)), "num");
assert.equal(L.balanced(() => c.pick("s")), "str");
L.throwsType(() => c.pick(true), /no overload of `Counter.pick`/);

// A language error leaves as its JS class.
assert.throws(
  () => c.fail(),
  (e) => e instanceof DOMException && e.name === "NotFoundError" && e.message === "missing",
);

// A natively constructed instance is wrapped on first crossing, without
// its JS constructor, and pairs: one wrapper per handle.
const z = Counter.zero();
assert.ok(z instanceof Counter);
assert.equal(z.value, 0);
assert.equal(objs.get(handleOf(z)).self, undefined, "the constructor didn't run");
assert.equal(Counter.zero(), z);
const m = util.make(3);
assert.equal(m.value, 3);
assert.equal(util.maybe(m), m, "identity round-trip");
assert.equal(util.maybe(c), c);
assert.equal(util.maybe(null), undefined);
assert.equal(L.call("box:Counter", handleOf(m)), m);

// An imported base: `super` runs with its real arguments either way.
const s = new Sub(4);
assert.ok(s instanceof Sub && s instanceof Base && s.based);
assert.equal(s.twice(), 8);
const made = Sub.make();
assert.ok(made instanceof Sub && made instanceof Base && made.based);
assert.equal(made.twice(), 42);
assert.equal(made.baseName(), "base");
assert.equal(new Orphan(2).x, 2);
const o = Orphan.make();
assert.ok(o instanceof Orphan && o instanceof Needy);
assert.equal(o.x, 1);
// Its wrapper can't be recreated, so a handle without one is a TypeError.
L.throwsType(() => Sub.stray(), /`Sub` handle with no live wrapper/);

// `tw:new` takes the constructor it names, though JS dispatch alone would
// take the first matching arm.
const dual = (w) => objs.get(L.call("unbox:Dual", w)).kind;
assert.equal(dual(new Dual("s")), "Any");
assert.equal(dual(L.call("new:Dual.new(str)", ...L.own("s"))), "str");
assert.equal(dual(new Dual("s")), "Any", "the pick is spent");

// Namespaces and functions.
assert.equal(util.twice(21), 42);
assert.deepEqual(L.balanced(() => util.names()), ["a", "bc"]);
assert.equal(L.balanced(() => version()), "1.0");
assert.equal(util.apply((x) => x * 10, 4), 41);

// Incoming templates bind by name, match fills and constant paths.
assert.equal(util.swap(1, undefined, 2), 21);
L.throwsType(() => util.swap(1, 0, 2), /swap/);
const tag = {};
const fast = { mode: "fast", tag };
assert.equal(util.opt(1, fast), 100);
assert.deepEqual(seen, [1, fast, tag]);
const slow = { mode: "slow" };
assert.equal(util.opt(1, slow), 200);
assert.deepEqual(seen, [1, slow]);
assert.equal(util.opt(1), 100);
assert.deepEqual(seen, [1, null, null]);
L.throwsType(() => util.opt(1, { mode: "other" }), /no overload of `opt`/);
assert.equal(util.fastOnly("fast"), 300);
L.throwsType(() => util.fastOnly("slow"), /fastOnly/);
L.throwsType(() => util.apply(5, 4), /fn\(u32\)->u32/);

// Handles reaching JS through imports.
const t = new Target();
L.call("Target#keep(Counter)", t, handleOf(c));
assert.equal(t.kept, c);
t.giving = m;
assert.equal(L.call("Target#give()", t), handleOf(m));
t.giving = {};
L.throwsType(() => L.call("Target#give()", t), /Counter/);

// Callbacks: the wrapper pairs, and calls reach the trampoline.
L.call("Target#on(fn(u32)->str)", t, 200);
const cb = t.cb;
L.call("Target#on(fn(u32)->str)", t, 200);
assert.equal(t.cb, cb, "the same handle yields the same function");
assert.equal(L.balanced(() => cb(7)), "200:7");
assert.equal(L.call("box:fn(u32)->str", 200), cb);
assert.equal(L.call("unbox:fn(u32)->str", cb), 200);
L.throwsType(() => L.call("unbox:fn(u32)->str", () => {}), /fn\(u32\)->str/);
// tw:tryUnbox: the handle, or none (0) when not a live wrapper.
assert.equal(L.call("tryUnbox:fn(u32)->str", cb), 200);
assert.equal(L.call("tryUnbox:fn(u32)->str", () => {}), 0);
assert.equal(L.call("tryUnbox:Counter", c), handleOf(c));
assert.equal(L.call("tryUnbox:Counter", {}), 0);
assert.equal(L.call("tryUnbox:Counter", s), 0, "a wrapper of another class");

// `@once` revokes after the first call; `destroy` revokes at once.
L.call("Target#listen(fn(u32))", t, 300);
t.listened(1);
L.throwsType(() => t.listened(2), /destroyed/);
assert.deepEqual(calls, [[300, 1]]);
L.call("Target#listen(fn(u32))", t, 308);
const doomed = t.listened;
L.call("destroy:fn(u32)", 308);
L.throwsType(() => doomed(1), /destroyed/);
assert.deepEqual(calls, [[300, 1]], "a dead wrapper never enters the language");
L.call("Target#listen(fn(u32))", t, 308);
assert.notEqual(t.listened, doomed, "destroy unpairs");
t.listened(3);

// Callbacks share the entry error membrane.
thrown = new WebAssembly.Exception(L.ex["tag:KvError"], [3, "coded"]);
L.call("Target#listen(fn(u32))", t, 316);
assert.throws(() => t.listened(1), (e) => e instanceof RangeError && e.code === 7 && e.message === "coded");
thrown = null;

// Finalizers: each collected wrapper drops its handle once; destroyed
// handles are never dropped.
t.listened = t.cb = t.kept = t.giving = null;
for (let i = 0; i < 10; i++) L.call("box:fn(u32)->str", 1000 + i * 8);
const counters = [];
for (let i = 0; i < 5; i++) counters.push(handleOf(util.make(i)));
await collect();
const dropped = (kind) =>
  drops
    .filter(([k]) => k === kind)
    .map(([, h]) => h)
    .sort((a, b) => a - b);
for (let i = 0; i < 10; i++) assert.ok(dropped("fn(u32)->str").includes(1000 + i * 8), `fn ${i} dropped`);
assert.ok(!dropped("fn(u32)->str").includes(200), "a live wrapper isn't dropped");
for (const h of counters) assert.ok(dropped("Counter").includes(h), `counter ${h} dropped`);
// `@once` keeps the finalizer; the destroyed 308 wrapper's drop is gone,
// and its successor drops 308 once.
assert.deepEqual(dropped("fn(u32)"), [300, 308, 316]);
for (const kind of ["fn(u32)", "fn(u32)->str", "Counter"]) {
  const hs = dropped(kind);
  assert.equal(new Set(hs).size, hs.length, `${kind}: each handle dropped once`);
}
// Live wrappers are kept.
assert.ok(!dropped("Counter").includes(handleOf(c)));
assert.equal(c.value, 9);

// A base-less wrapper holds no state, so one collected while the language
// keeps its object is recreated when the handle crosses again.
kept = alloc({ n: 11 });
let w = util.stray();
assert.equal(w.value, 11);
w = null;
await collect();
assert.ok(dropped("Counter").includes(kept), "the collected wrapper dropped JS's hold");
w = util.stray();
assert.ok(w instanceof Counter);
assert.equal(w.inc(1), 12);
assert.equal(util.stray(), w, "and pairs again");

// A JS subclass instance comes back as the language class: its state lives
// on the collected wrapper.
class MyCounter extends Counter {
  constructor(n) {
    super(n);
    this.extra = true;
  }
}
let mine = new MyCounter(4);
assert.ok(mine instanceof MyCounter && mine.extra);
kept = handleOf(mine);
assert.equal(util.stray(), mine);
mine = null;
await collect();
const back = util.stray();
assert.ok(back instanceof Counter && !(back instanceof MyCounter));
assert.equal(back.extra, undefined);
assert.equal(back.value, 4);
console.log("done");
