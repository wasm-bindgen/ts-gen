import { assert } from "./harness.mjs";

const { instantiate, scratchUse, Counter } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const impl = {
  "Counter.new()": (self) => ({ self }),
  "tw:call:fn(u32)->u32": (env, x) => env.base + x,
};
const ex = instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }).exports;
const base = scratchUse();

// Erased `T` is `anyref`: JS values internalize, and small numbers come
// back from `i31ref`.
const m = new Map();
assert.equal(ex["call:Map#set(K,V)"](m, "a", 7), m);
assert.equal(m.get("a"), 7);
assert.equal(ex["call:Map#get(K)"](m, "a"), 7);
const o = { x: 1 };
ex["call:Map#set(K,V)"](m, "o", o);
assert.equal(ex["call:Map#get(K)"](m, "o"), o);
assert.equal(ex["call:Map#get(K)"](m, "none"), null);

// A handle enters an erased position only boxed, as its wrapper, and
// leaves through `tw:unbox`.
const c = new Counter();
const w = ex["call:Map#get(K)"](new Map([["k", c]]), "k");
assert.equal(w, c, "the wrapper reaches the language as is");
assert.equal(ex["call:unbox:Counter"](w).self, c);
const native = { native: true };
const boxed = ex["call:box:Counter"](native);
assert.ok(boxed instanceof Counter, "a natively made handle wraps on boxing");
ex["call:Map#set(K,V)"](m, "c", boxed);
assert.equal(m.get("c"), boxed);
assert.equal(ex["call:unbox:Counter"](m.get("c")), native);
assert.throws(
  () => ex["call:Map#set(K,V)"](m, "raw", ex["test:struct"]()),
  /a language object in an erased position/,
);
// A callback that never crossed is boxed into a callable wrapper.
const env = { base: 10 };
ex["call:Map#set(K,V)"](m, "f", ex["call:box:fn(u32)->u32"](env));
assert.equal(typeof m.get("f"), "function");
assert.equal(m.get("f")(2), 12);

// Number boxes are `i31ref` when integral and in range, by value, and the
// host number otherwise; `Bool` is the host boolean.
const isI31 = (v) => ex["test:isI31"](v) === 1;
assert.ok(isI31(ex["call:box:u8"](7)));
assert.ok(isI31(ex["call:box:f64"](3)));
assert.ok(!isI31(ex["call:box:f64"](3.5)));
assert.equal(ex["call:box:f64"](3.5), 3.5);
const yes = ex["call:box:bool"](1);
assert.equal(yes, true);
assert.ok(!isI31(yes));
ex["call:Map#set(K,V)"](m, "b", yes);
assert.equal(m.get("b"), true, "a Bool reaches JS as a boolean");
assert.equal(ex["call:unbox:f64"](ex["test:i31"](3)), 3, "an F64 unboxes from i31");
assert.equal(ex["call:unbox:f64"](3.5), 3.5);

// An enum is a valid erased type argument: its box is the arm's host value.
const modes = new Map();
const off = ex["call:box:Mode"](1, 0);
const count = ex["call:box:Mode"](2, 5);
assert.equal(off, "off");
assert.equal(count, 5);
ex["call:Map#set(K,V)"](modes, "off", off);
ex["call:Map#set(K,V)"](modes, "count", count);
assert.deepEqual(ex["call:unbox:Mode"](ex["call:Map#get(K)"](modes, "off")), [1, 0]);
assert.deepEqual(ex["call:unbox:Mode"](ex["call:Map#get(K)"](modes, "count")), [2, 5]);
assert.deepEqual(ex["call:unbox:Mode?"](ex["call:Map#get(K)"](modes, "none")), [0, 0]);
assert.throws(() => ex["call:unbox:Mode"]("on"), TypeError);

// `bigint` is an `(array (mut i64))` of its limbs.
for (const v of [0n, -1n, 2n ** 64n, -(2n ** 200n)]) {
  assert.equal(ex["call:box:bigint"](ex["call:unbox:bigint"](v)), v);
}
assert.throws(() => ex["call:unbox:bigint"](1), TypeError);

// A mono variant with a GC `str` key.
const key = ex["call:unbox:str"]("a");
assert.deepEqual(ex["call:Map#get<str,u32>(K)"](m, key), [1, 7]);
assert.deepEqual(scratchUse(), base);
console.log("done");
