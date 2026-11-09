import { assert, lang } from "./harness.mjs";

class Probe {
  fill(v) {
    this.shared = v.buffer instanceof SharedArrayBuffer;
    v.fill(5);
  }
  see(v) {
    this.shared = v.buffer instanceof SharedArrayBuffer;
    v[0] = 9;
    return v.reduce((a, b) => a + b, 0);
  }
  peek(v) {
    this.shared = v.buffer instanceof SharedArrayBuffer;
    return v.length;
  }
}
globalThis.Probe = Probe;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
assert.ok(L.ex.memory.buffer instanceof SharedArrayBuffer);
const p = new Probe();

// `*mut T[]` is a fresh copy, written back after the call.
const q = L.alloc(3, 1);
L.u8().set([1, 2, 3], q);
L.call("Probe#fill(*mut u8[])", p, q, 3);
assert.equal(p.shared, false);
assert.deepEqual([...L.u8().slice(q, q + 3)], [5, 5, 5]);

// `*T[]` is a copy that isn't written back.
L.u8().set([1, 2, 3], q);
assert.equal(L.call("Probe#see(*u8[])", p, q, 3), 14);
assert.equal(p.shared, false);
assert.deepEqual([...L.u8().slice(q, q + 3)], [1, 2, 3]);

// Without `@js(unshared)` the view is over the shared memory.
assert.equal(L.call("Probe#peek(*u16[])", p, q - (q % 2), 1), 1);
assert.equal(p.shared, true);
L.throwsType(() => L.call("Probe#peek(*u16[])", p, L.ex.memory.buffer.byteLength - 2, 2));

console.log("ok");
