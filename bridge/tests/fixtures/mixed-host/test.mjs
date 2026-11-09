import { assert, lang } from "./harness.mjs";

class Probe {
  rows() {
    return this.next;
  }
  see(xs) {
    this.seen = xs;
  }
  pair() {
    return this.next;
  }
}
globalThis.Probe = Probe;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const p = new Probe();

// A host `vec` inside linear memory sits in the externref table.
const bytes = new Uint8Array([1, 2]);
p.next = [[5, bytes]];
const [ptr, x0, len] = L.call("Probe#rows()", p);
assert.equal(len, 1);
assert.equal(L.dv().getUint32(ptr, true), 5);
const v = L.ex["tw:table"].get(x0);
assert.deepEqual(v, bytes);
assert.notEqual(v, bytes, "a fresh copy");

const q = L.alloc(16, 4);
L.dv().setUint32(q, 6, true);
L.dv().setUint32(q + 4, 7, true);
const x = L.ex["tw:table/alloc"](2);
L.ex["tw:table"].set(x, new Uint8Array([3]));
L.ex["tw:table"].set(x + 1, null);
L.call("Probe#see(arr<(u32,u8[]?)>)", p, q, x, 2);
assert.deepEqual(p.seen, [[6, new Uint8Array([3])], [7, undefined]]);

p.next = ["hi", new Uint16Array([9])];
const [sp, sl, w] = L.call("Probe#pair()", p);
assert.equal(L.takeStr(sp, sl), "hi");
assert.deepEqual(w, new Uint16Array([9]));
p.next = ["hi", [9]];
L.balanced(() => assert.throws(() => L.call("Probe#pair()", p), TypeError));
console.log("done");
