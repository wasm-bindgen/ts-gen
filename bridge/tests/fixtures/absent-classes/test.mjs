import { assert, lang } from "./harness.mjs";
import { setNext } from "./host.mjs";

class Present {}
let next;
const ret = () => next;
Object.assign(globalThis, { Present, absent: ret, maybe: ret, deep: ret, pick: ret, only: ret, present: ret });

const { instantiate, take } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(
  instantiate(compile("lang.wasm"), compile("bridge.wasm"), {
    "test:impl": { "take(Absent)": () => 7 },
  }),
);
const call = (key, v) => {
  next = v;
  return L.call(key);
};

// A class with no value checks as an interface: any non-null object.
const o = {};
assert.equal(call("absent()", o), o);
{
  const pr = new Present();
  assert.equal(call("absent()", pr), pr);
}
L.throwsType(() => call("absent()", 5), /Absent/);
L.throwsType(() => call("absent()", null), /Absent/);
assert.equal(call("maybe()", null), null);
assert.equal(call("maybe()", o), o);
L.throwsType(() => call("maybe()", "s"), /Absent/);
assert.equal(call("deep()", o), o);
L.throwsType(() => call("deep()", 1), /Deep/);
setNext(o);
assert.equal(L.call("gone()"), o);
setNext(true);
L.throwsType(() => L.call("gone()"), /Gone/);
// A present class still checks nominally.
L.throwsType(() => call("present()", o), /Present/);
// Arguments too.
assert.equal(take(o), 7);
assert.throws(() => take(1), TypeError);

// Its union arm never matches, so discrimination falls through.
const p = new Present();
assert.deepEqual(call("pick()", p), [2, p, 0]);
assert.deepEqual(call("pick()", 1.5), [3, null, 1.5]);
assert.deepEqual(call("pick()", o), [-1, o, 0]);
L.throwsType(() => call("only()", o), /Only/);

// tw:instanceof is 0 for anything.
assert.equal(L.call("instanceof:Absent", o), 0);
assert.equal(L.call("instanceof:ns.Deep", o), 0);
assert.equal(L.call("instanceof:Present", p), 1);
assert.equal(L.call("instanceof:Present", o), 0);

console.log("ok");
