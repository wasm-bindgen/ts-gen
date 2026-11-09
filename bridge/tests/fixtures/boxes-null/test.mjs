import { assert, lang } from "./harness.mjs";

class Probe {
  constructor() {
    this.seen = [];
    this.next = undefined;
  }
}
for (const m of ["take", "opt", "nopt", "nested"]) {
  Probe.prototype[m] = function (v) {
    this.seen.push(v);
    return this.next;
  };
}
globalThis.Probe = Probe;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const p = new Probe();
const call = (key, next, v) => {
  p.next = next;
  return L.call(key, p, v);
};
const seen = () => p.seen.pop();
const unit = L.call("unit:");

// Under `box-none = null`, both empties enter an optional box as ref.null,
// and ref.null leaves as the node's default: undefined, or null under
// `@js(null)`. Unit is a value constant, read as none too.
for (const next of [undefined, null]) {
  assert.equal(call("Probe#opt(Str?)", next, null), null);
  assert.equal(seen(), undefined);
  assert.equal(call("Probe#nopt(Str?)", next, null), null);
  assert.equal(seen(), null);
}
call("Probe#opt(Str?)", "s", unit);
assert.equal(seen(), undefined);
call("Probe#nopt(Str?)", "s", unit);
assert.equal(seen(), null);
assert.equal(call("Probe#opt(Str?)", "s", "t"), "s");
assert.equal(seen(), "t");

// A non-optional box node stays faithful.
assert.equal(call("Probe#take(Str)", undefined, "t"), undefined);
assert.equal(call("Probe#take(Str)", null, "t"), null);

// A box `Str??` is `Str?`, with a warning.
assert.equal(call("Probe#nested(Str??)", undefined, null), null);
assert.equal(seen(), undefined);

// tw:box writes none as ref.null, and tw:unbox reads both empties as none.
assert.equal(L.call("box:bool?", 2), null);
assert.deepEqual(L.call("unbox:str?", undefined), L.call("unbox:str?", null));

// Erased positions enter both empties as ref.null, and write ref.null as
// undefined: an `Any` null through one comes back out as undefined.
{
  const m = new Map([["n", null]]);
  const v = L.call("Map#get(K)", m, "n");
  assert.equal(v, null);
  assert.equal(L.call("Map#get(K)", m, "zz"), null);
  L.call("Map#set(K,V)", m, "back", v);
  assert.ok(m.has("back"));
  assert.equal(m.get("back"), undefined);
}

console.log("ok");
