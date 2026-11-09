import { assert, lang } from "./harness.mjs";

class Probe {
  str(v) {
    this.seen = v;
    return this.next;
  }
  optStr(v) {
    return this.str(v);
  }
  strs(v) {
    return this.str(v);
  }
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

// WTF-16 strings are code units in language memory, lone surrogates and all.
const str = (s) => {
  const q = L.alloc(s.length * 2, 2);
  for (let i = 0; i < s.length; i++) L.dv().setUint16(q + i * 2, s.charCodeAt(i), true);
  return [L.P(q), L.P(s.length)];
};
const take = (q, len) => {
  const b = L.take(q, len, 2, 2);
  return String.fromCharCode(...new Uint16Array(b.buffer, b.byteOffset, b.length / 2));
};
const odd = "a\uD800é😀";

L.balanced(() => {
  const [q, len] = call("Probe#str(str)", odd, ...str("in\uDC00"));
  assert.equal(p.seen, "in\uDC00");
  assert.equal(take(q, len), odd);
});
L.balanced(() => {
  const [q, len] = call("Probe#optStr(str?)", null, ...str("x"));
  assert.equal(len, -1);
  call("Probe#optStr(str?)", null, 0, -1);
  assert.equal(p.seen, undefined);
  const [q2, len2] = call("Probe#optStr(str?)", "", 0, -1);
  assert.equal(take(q2, len2), "");
});
L.balanced(() => {
  const items = ["b", "\uDBFF"].map(str);
  const arr = L.alloc(16, 4);
  items.forEach(([q, len], i) => {
    L.dv().setInt32(arr + i * 8, q, true);
    L.dv().setInt32(arr + i * 8 + 4, len, true);
  });
  const [q, n] = call("Probe#strs(arr<str>)", ["c", odd], arr, 2);
  assert.deepEqual(p.seen, ["b", "\uDBFF"]);
  const dv = L.dv();
  const out = [0, 1].map((i) => take(dv.getInt32(q + i * 8, true), dv.getInt32(q + i * 8 + 4, true)));
  L.take(q, n, 8, 4);
  assert.deepEqual(out, ["c", odd]);
});
L.throwsType(() => call("Probe#str(str)", 5, ...str("")), /str/);
console.log("ok");
