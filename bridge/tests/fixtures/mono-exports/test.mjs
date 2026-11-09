import { assert } from "./harness.mjs";

const { instantiate, util } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const called = [];
const impl = {
  "util.id<u32>(T)": (x) => (called.push("u32"), x + 1),
  "util.id<str>(T)": (x) => (called.push("str"), x + "!"),
  "util.pick<f64>(T,T)": (a, b) => (called.push("f64"), Math.max(a, b)),
  "util.pick<bool>(T,T)": (a, b) => (called.push("bool"), a & b),
  // Exported erased: one entry for every `T`.
  "util.either(T)": (x) => x,
};
instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl });

// Each variant dispatches on its arguments, in the bound's order.
assert.equal(util.id(4), 5);
assert.equal(util.id("a"), "a!");
assert.throws(() => util.id(true), TypeError);
assert.equal(util.pick(1.5, 2.5), 2.5);
assert.equal(util.pick(true, false), false);
assert.throws(() => util.pick(1, true), TypeError);
assert.deepEqual(called, ["u32", "str", "f64", "bool"]);
const o = {};
assert.equal(util.either(o), o);

console.log("ok");
