import { assert } from "./harness.mjs";

class Request {}
class Probe {}
for (const m of ["str", "bytes", "nums", "request", "maybe"]) {
  Probe.prototype[m] = function () {
    return this.next;
  };
}
Object.assign(globalThis, { Request, Probe });

const { instantiate, app } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const impl = {
  "app.len(str)": (s) => s.length,
  "app.bytes(u8[])": (b) => b.length,
  "app.nums(arr<u32>)": (a) => a.length,
  "app.request(Request)": () => 1,
  "app.maybe(Request?)": (r) => (r === null ? 0 : 1),
};
const ex = instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }).exports;
const p = new Probe();
const call = (key, next) => {
  p.next = next;
  return ex[`call:Probe#${key}()`](p);
};
const type = (f, re) => assert.throws(f, (e) => e instanceof TypeError && re.test(e.message));

// The same wrong values: trusted as an import's results, where the
// language holds the JS value itself, and a `TypeError` as an export's
// arguments.
assert.equal(call("str", 42), 42);
type(() => app.len(42), /str/);
// A `T[]` is still the fresh copy, of the carrier's own class.
assert.deepEqual(call("bytes", [1, 2]), new Uint8Array([1, 2]));
type(() => app.bytes([1, 2]), /u8\[\]/);
{
  const xs = [7, "8"];
  const got = call("nums", xs);
  assert.deepEqual(got, xs);
  assert.notEqual(got, xs);
  type(() => app.nums(xs), /u32/);
}
{
  const o = {};
  assert.equal(call("request", o), o);
  type(() => app.request(o), /Request/);
}
assert.equal(app.len("abc"), 3);
assert.equal(app.bytes(new Uint8Array(2)), 2);

// A nullable host value reads either empty as none.
assert.equal(call("maybe", undefined), null);
assert.equal(app.maybe(undefined), false);
assert.equal(app.maybe(new Request()), true);

console.log("ok");
