import { assert, lang } from "./harness.mjs";

class Request {}
class Probe {}
for (const m of ["str", "optStr", "floats", "strs", "rows", "body"]) {
  Probe.prototype[m] = function (v) {
    this.seen = v;
    return typeof this.next === "function" ? this.next(v) : this.next;
  };
}
Probe.prototype.fill = function (view) {
  view.fill(3);
};
Object.assign(globalThis, { Request, Probe });

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
assert.ok(L.wide);
const p = new Probe();
const call = (key, next, ...wire) => {
  p.next = next;
  return L.call(key, p, ...wire);
};

// Pointers and lengths are i64 (bigint) on the wire.
L.balanced(() => {
  const [ptr, len] = call("Probe#str(str)", (v) => v + "!", ...L.str("héllo"));
  assert.equal(typeof ptr, "bigint");
  assert.equal(p.seen, "héllo");
  assert.equal(L.takeStr(ptr, len), "héllo!");
});
assert.deepEqual(call("Probe#str(str)", "", ...L.str("")), [1n, 0n]);
// str?: len = u64::MAX is absent.
assert.deepEqual(call("Probe#optStr(str?)", null, 0n, -1n), [0n, -1n]);
assert.equal(p.seen, undefined);
L.balanced(() => L.throwsType(() => call("Probe#str(str)", 1, ...L.str(""))));

L.balanced(() => {
  const [rp, rl] = call("Probe#floats(f64[])", (v) => v, L.bytes(new Float64Array([0.5, 2]), 8), 2n);
  assert.deepEqual([...p.seen], [0.5, 2]);
  assert.deepEqual([...new Float64Array(L.take(rp, rl, 8, 8).buffer)], [0.5, 2]);
});

// arr<str>: 16-byte descriptors.
L.balanced(() => {
  const [a, b] = [L.str("one"), L.str("two")];
  const ptr = L.alloc(32, 8);
  const d = L.dv();
  d.setBigUint64(ptr, a[0], true);
  d.setBigUint64(ptr + 8, a[1], true);
  d.setBigUint64(ptr + 16, b[0], true);
  d.setBigUint64(ptr + 24, b[1], true);
  const [rp, rl] = call("Probe#strs(arr<str>)", (v) => [...v, "three"], BigInt(ptr), 2n);
  assert.deepEqual(p.seen, ["one", "two"]);
  assert.equal(rl, 3n);
  const e = L.dv();
  const got = [0, 1, 2].map((i) => {
    const o = Number(rp) + i * 16;
    return L.takeStr(e.getBigUint64(o, true), e.getBigUint64(o + 8, true));
  });
  assert.deepEqual(got, ["one", "two", "three"]);
  L.take(rp, rl, 16, 8);
});
L.balanced(() => L.throwsType(() => call("Probe#strs(arr<str>)", ["a", 1], 8n, 0n)));

// arr<(u32, str, Request)>: 4 + 16 bytes of memory per element, an i32 table base.
L.balanced(() => {
  const r = new Request();
  const [rp, rx, rl] = call("Probe#rows(arr<(u32,str,Request)>)", [[9, "x", r]], 0n, 0, 0n);
  assert.equal(typeof rx, "number");
  const d = L.dv();
  const o = Number(rp);
  assert.equal(d.getUint32(o, true), 9);
  assert.equal(L.ex["tw:table"].get(rx), r);
  L.takeStr(d.getBigUint64(o + 4, true), d.getBigUint64(o + 12, true));
  L.take(rp, rl, 20, 8);
  L.ex["tw:table/free"](rx, 1);
});

// Payload enum slots are pointer-sized.
L.balanced(() => {
  const [arm, ptr, len, x] = call("Probe#body(Body)", "t", 2, 0n, 0n, new Request());
  assert.equal(arm, 1);
  assert.equal(x, null);
  assert.equal(L.takeStr(ptr, len), "t");
});

// *T[] views take an i64 address and length.
{
  const q = L.alloc(2);
  call("Probe#fill(*mut u8[])", undefined, BigInt(q), 2n);
  assert.deepEqual([...L.u8().slice(q, q + 2)], [3, 3]);
}

console.log("ok");
