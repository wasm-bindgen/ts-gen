import { assert, lang } from "./harness.mjs";

class Probe {
  key() {
    return this.next;
  }
  sym() {
    return this.next;
  }
}
globalThis.Probe = Probe;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const p = new Probe();
const call = (key, next) => {
  p.next = next;
  return L.call(key, p);
};

// A symbol is a primitive: `typeof`, in the primitive tier, after the
// structural interface test, which a symbol never passes.
const s = Symbol("s");
assert.deepEqual(call("Probe#key()", s).slice(0, 1), [2]);
assert.equal(call("Probe#key()", s)[3], s);
assert.equal(call("Probe#key()", {})[0], 3);
L.throwsType(() => call("Probe#key()", 1), /Key/);

assert.equal(call("Probe#sym()", s), s);
L.throwsType(() => call("Probe#sym()", {}), /Symbol/);
L.throwsType(() => call("Probe#sym()", "s"), /Symbol/);

console.log("ok");
