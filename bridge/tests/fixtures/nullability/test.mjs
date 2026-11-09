import { assert, lang } from "./harness.mjs";

class Probe {
  constructor() {
    this.seen = [];
    this.next = undefined;
  }
  get missing() {
    return this.next;
  }
}
for (const m of ["flags", "pair", "maybe", "boxed"]) {
  Probe.prototype[m] = function (...args) {
    this.seen.push(args);
    return this.next;
  };
}
globalThis.Probe = Probe;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const p = new Probe();
const call = (key, next, ...wire) => {
  p.next = next;
  return L.call(key, p, ...wire);
};
const seen = () => p.seen.pop();

// `@js(null)` is per `T?` node: its absence crosses as null, others as
// undefined.
call("Probe#flags(bool?,bool?)", undefined, 2, 2);
assert.deepEqual(seen(), [null, undefined]);
call("Probe#flags(bool?,bool?)", undefined, 1, 0);
assert.deepEqual(seen(), [true, false]);
call("Probe#pair((f64?,f64?))", undefined, 0, 0, 0, 0);
assert.deepEqual(seen(), [[null, undefined]]);
call("Probe#pair((f64?,f64?))", undefined, 1, 1.5, 1, 2.5);
assert.deepEqual(seen(), [[1.5, 2.5]]);

// JS → Wasm, null and undefined are both absence.
assert.deepEqual(call("Probe#get:missing()", null), [0, -1]);
assert.deepEqual(call("Probe#get:missing()", undefined), [0, -1]);

// `str??`: the outer absence is undefined and the inner null.
assert.deepEqual(call("Probe#maybe(str??)", undefined, 1, 0, 0).slice(0, 1), [1]);
assert.deepEqual(call("Probe#maybe(str??)", null, 1, 0, 0), [2, 0, -1]);
call("Probe#maybe(str??)", undefined, 1, 0, 0);
assert.deepEqual(seen(), [undefined]);
call("Probe#maybe(str??)", undefined, 2, 0, -1);
assert.deepEqual(seen(), [null]);

// Under `box-none = null`, the box `Str??` is `Str?`, with a warning: both
// empties enter as ref.null, which leaves as undefined.
assert.equal(call("Probe#boxed(Str??)", undefined, null), null);
assert.deepEqual(seen(), [undefined]);
assert.equal(call("Probe#boxed(Str??)", null, undefined), null);
assert.deepEqual(seen(), [undefined]);
assert.equal(call("Probe#boxed(Str??)", "s", "t"), "s");
assert.deepEqual(seen(), ["t"]);
// Unboxed as `str?`, undefined is absent.
assert.deepEqual(L.call("unbox:str?", undefined), [0, -1]);
L.throwsType(() => L.call("unbox:str?", 5), /str/);

console.log("ok");
