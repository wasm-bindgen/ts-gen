// Test helpers over a language module built from tests/runtime.wat.
import assert from "node:assert/strict";

export { assert };

// `names` maps renamed protocol exports (`{ "tw:free": "__free" }`).
export function lang(instance, names = {}) {
  const ex = instance.exports;
  const memory = () => ex[names.memory ?? "memory"];
  const free = (...a) => ex[names["tw:free"] ?? "tw:free"](...a);
  const wide = typeof ex["test:bytes"]() === "bigint";
  const P = (n) => (wide ? BigInt(n) : n);
  const N = (n) => Number(n);
  const te = new TextEncoder();
  const td = new TextDecoder();
  // The bridge fills the table constants at instantiation (IR.md §14).
  const table = ex[names["tw:table"] ?? "tw:table"];
  if (table) {
    const c = ex[names["tw:table/const_base"] ?? "tw:table/const_base"].value;
    assert.deepEqual([0, 1, 2, 3].map((i) => table.get(c + i)), [undefined, null, true, false]);
  }
  const L = {
    ex,
    wide,
    P,
    call(key, ...args) {
      const f = ex["call:" + key];
      if (!f) throw new Error(`no forwarder for ${key}`);
      return f(...args);
    },
    u8: () => new Uint8Array(memory().buffer),
    dv: () => new DataView(memory().buffer),
    alloc(size, align = 1) {
      return N(ex["test:alloc"](P(align), P(Math.max(size, 1))));
    },
    // A UTF-8 string in memory the language owns: `[ptr, len]` wire values.
    str(s) {
      const b = te.encode(s);
      const p = L.alloc(b.length);
      L.u8().set(b, p);
      return [P(p), P(b.length)];
    },
    bytes(b, align = 1) {
      const u = new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
      const p = L.alloc(u.length, align);
      L.u8().set(u, p);
      return P(p);
    },
    // A UTF-8 string the language allocates and hands over, as a result.
    own(s) {
      const b = te.encode(s);
      if (b.length === 0) return [P(1), P(0)];
      const p = N(ex[names["tw:realloc"] ?? "tw:realloc"](P(0), P(0), P(1), P(b.length)));
      L.u8().set(b, p);
      return [P(p), P(b.length)];
    },
    readStr(p, len) {
      return td.decode(L.u8().slice(N(p), N(p) + N(len)));
    },
    // Takes ownership of a result allocation: reads it and frees it.
    take(p, len, size = 1, align = 1) {
      const n = N(len) * size;
      const out = L.u8().slice(N(p), N(p) + n);
      if (n > 0) free(P(p), P(n), P(align));
      return out;
    },
    takeStr(p, len) {
      return td.decode(L.take(p, len));
    },
    live() {
      return { allocations: ex["test:live"](), bytes: N(ex["test:bytes"]()), slots: ex["test:tlive"]() };
    },
    // Asserts that `f` leaves no allocation behind.
    balanced(f) {
      const before = L.live();
      const r = f();
      assert.deepEqual(L.live(), before);
      return r;
    },
    throwsType(f, re) {
      assert.throws(f, (e) => e instanceof TypeError && (!re || re.test(e.message)));
    },
  };
  return L;
}

// Ticks until finalizers have run after a full GC.
export async function collect() {
  for (let i = 0; i < 4; i++) {
    globalThis.gc();
    await new Promise((r) => setTimeout(r, 0));
  }
}
