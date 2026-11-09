import { assert, lang } from "./harness.mjs";

class Probe {
  constructor() {
    this.next = undefined;
  }
}
for (const m of ["missing", "loose", "count", "boxed"]) {
  Object.defineProperty(Probe.prototype, m, {
    get() {
      return this.next;
    },
  });
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

// Under `strict-null`, a `@js(null) T?` takes only `null` as absence.
assert.deepEqual(call("Probe#get:missing()", null), [0, -1]);
assert.throws(() => call("Probe#get:missing()", undefined), TypeError);
assert.deepEqual(call("Probe#get:count()", null), [0, 0]);
assert.throws(() => call("Probe#get:count()", undefined), TypeError);
assert.equal(call("Probe#get:boxed()", null), null);
assert.throws(() => call("Probe#get:boxed()", undefined), TypeError);

// A plain `T?` still takes both.
assert.deepEqual(call("Probe#get:loose()", null), [0, -1]);
assert.deepEqual(call("Probe#get:loose()", undefined), [0, -1]);
