import { assert } from "./harness.mjs";

class Probe {
  triple() {
    return this.next;
  }
  maybe() {
    return this.next;
  }
  get() {
    if (this.thrown) throw this.thrown;
    return this.next;
  }
  on(cb) {
    this.cb = cb;
  }
}
globalThis.Probe = Probe;

const { instantiate, util, scratchUse, spillEmpty } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));

let ex;
const call = (k, ...a) => ex["call:" + k](...a);
const impl = {
  "util.pair(u32)": (x) => [x + 1, `n${x}`],
  "util.maybe(f64?)": (present, x) => (present ? [1, x * 2] : [0, 0]),
  // `throws = js`: the language sets the error and returns zero values.
  "util.lookup(str)": (key) => {
    if (key === "k") return "v";
    call("err:set:KvError", 2, `no ${key}`);
    return null;
  },
  "tw:call:fn(u32)->(u32,str)": (env, x) => [x + env.base, "t"],
  "util.count(str)": (key) => {
    if (key === "k") return 7;
    call("err:set:KvError", -1, `bad ${key}`);
    return 0;
  },
};
ex = instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }).exports;
const p = new Probe();
const idle = () => {
  assert.deepEqual(scratchUse(), [584, 0]);
  assert.equal(spillEmpty(), true);
};
idle();

// Imports: slot 0 returns, the rest are popped from the spill in slot order.
p.next = [3, "three", 1.5];
assert.deepEqual(call("Probe#triple()", p), [3, "three", 1.5]);
idle();
p.next = 2.5;
assert.deepEqual(call("Probe#maybe()", p), [1, 2.5]);
p.next = undefined;
assert.deepEqual(call("Probe#maybe()", p), [0, 0]);
idle();
// A JS function's multi-value result spills too.
assert.deepEqual(call("call:fn(u32)->(u32,str)", (x) => [x * 10, "cb"], 4), [40, "cb"]);
idle();
// And a trampoline's results are drained after its slot 0.
call("Probe#on(fn(u32)->(u32,str))", p, { base: 100 });
assert.deepEqual(p.cb(5), [105, "t"]);
idle();
// `tw:future new` returns the future and its resolver.
const [f, r] = call("future:new");
assert.ok(f != null && typeof r.resolve === "function");
idle();

// Exports: the language pushes slots 1.. and returns slot 0.
assert.deepEqual(util.pair(4), [5, "n4"]);
assert.equal(util.maybe(2), 4);
assert.equal(util.maybe(undefined), undefined);
idle();

// `throws = js` imports: the original JS exception reaches the language,
// which reads it with `("tw:err", E)`.
const missing = new DOMException("missing", "NotFoundError");
p.thrown = missing;
let caught;
try {
  call("Probe#get(str)", p, "k");
} catch (e) {
  caught = e;
}
assert.equal(caught, missing);
assert.deepEqual(call("err:KvError", caught), [2, "missing"]);
assert.deepEqual(call("err:KvError", new TypeError("t")), [1, "t"]);
assert.deepEqual(call("err:KvError", 42), [-1, "42"]);
p.thrown = null;
p.next = [1, "one"];
assert.deepEqual(call("Probe#get(str)", p, "k"), [1, "one"]);
idle();
// Validation errors propagate the same way.
p.next = ["x", "one"];
assert.throws(() => call("Probe#get(str)", p, "k"), TypeError);
idle();

// `throws = js` exports: a pending error replaces the results.
assert.equal(util.lookup("k"), "v");
assert.throws(
  () => util.lookup("q"),
  (e) => e instanceof DOMException && e.name === "NotFoundError" && e.message === "no q",
);
assert.equal(util.count("k"), 7);
assert.throws(() => util.count("q"), (e) => e.constructor === Error && e.message === "bad q");
// The pending error is consumed: the next call returns normally.
assert.equal(util.count("k"), 7);
idle();

console.log("ok");
