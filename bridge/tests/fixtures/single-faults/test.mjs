import { assert } from "./harness.mjs";

globalThis.P = class {
  two() {
    return [1, 2];
  }
};
const { instantiate, spillEmpty } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const ex = instantiate(compile("lang.wasm"), compile("bridge.wasm")).exports;
const p = new P();

const fault = (f, re) => {
  assert.throws(f, (e) => e instanceof TypeError && re.test(e.message));
  assert.equal(spillEmpty(), true, "a fault resets the spill");
};
fault(() => ex.under(), /wasn't pushed/);
fault(() => ex.unread(p), /left unread/);
fault(() => ex.over(), /overflowed/);
assert.deepEqual(ex.read(p), [1, 2]);
assert.equal(spillEmpty(), true);
console.log("ok");
