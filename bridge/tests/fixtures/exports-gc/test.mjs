import { assert } from "./harness.mjs";

class Request {}
class Slot {
  set(v) {
    this.value = v;
  }
}
class Probe {
  see(...a) {
    this.seen = a;
  }
}
for (const m of ["u8", "str", "bytes", "nums", "request", "maybe"]) {
  Probe.prototype[m] = function () {
    return this.next;
  };
}
Object.assign(globalThis, { Request, Slot, Probe });

const { instantiate, app } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const impl = {
  "app.u8(u8)": (x) => x,
  "app.len(str)": () => 1,
  "app.bytes(u8[])": () => 1,
  "app.nums(arr<u32>)": () => 1,
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

// The same wrong values: trusted as an import's results, read back here
// from the GC values the language was given, and a `TypeError` as an
// export's arguments.
assert.equal(call("u8", 300), 44);
type(() => app.u8(300), /u8/);
ex["call:Probe#see(str,u8[],arr<u32>)"](p, call("str", 42), call("bytes", [1, 2]), call("nums", { length: 2, 0: 7, 1: "8" }));
assert.deepEqual(p.seen, ["42", new Uint8Array([1, 2]), [7, 8]]);
type(() => app.len(42), /str/);
type(() => app.bytes([1, 2]), /u8\[\]/);
type(() => app.nums({ length: 2, 0: 7, 1: 8 }), /arr<u32>|u32\[\]/);
{
  const o = {};
  assert.equal(call("request", o), o);
  type(() => app.request(o), /Request/);
}

// A nullable host value reads either empty as none.
assert.equal(call("maybe", undefined), null);
assert.equal(app.maybe(undefined), false);
assert.equal(app.maybe(new Request()), true);

// A language object still can't reach JS from an erased position.
type(() => ex["call:Slot#set(T)"](new Slot(), ex["test:struct"]()), /a language object in an erased position/);

console.log("ok");
