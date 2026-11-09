import { assert, lang } from "./harness.mjs";

class Request {}
class Probe {
  constructor() {
    this.seen = [];
    this.next = undefined;
  }
}
for (const m of [
  "str", "optStr", "bytes", "floats", "bigs", "optBytes", "fill", "nums", "strs", "optStrs",
  "rows", "reqs", "nested", "mixed", "pair", "optPair",
]) {
  Probe.prototype[m] = function (v) {
    this.seen.push(v);
    if (typeof this.next === "function") return this.next(v);
    return this.next;
  };
}
globalThis.Request = Request;
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
const dv = () => L.dv();

// str: UTF-8 both ways, results owned by the language.
for (const s of ["", "a", "héllo", "😀 emoji", "x".repeat(10000), "\u{10ffff}"]) {
  L.balanced(() => {
    const [ptr, len] = call("Probe#str(str)", (v) => v, ...L.str(s));
    assert.equal(p.seen.pop(), s);
    assert.equal(L.takeStr(ptr, len), s);
  });
}
// A zero-length result never allocates: the dangling pointer is the alignment.
assert.deepEqual(call("Probe#str(str)", "", ...L.str("")), [1, 0]);
// Lone surrogates encode as U+FFFD.
assert.equal(L.takeStr(...call("Probe#str(str)", "a\ud800b", ...L.str(""))), "a�b");
L.balanced(() => L.throwsType(() => call("Probe#str(str)", 5, ...L.str("")), /expected str/));

// str?: len = u32::MAX is absent.
assert.deepEqual(call("Probe#optStr(str?)", null, 0, -1), [0, -1]);
assert.equal(p.seen.pop(), undefined);
L.balanced(() => assert.equal(L.takeStr(...call("Probe#optStr(str?)", "z", 0, -1)), "z"));

// T[]: typed arrays only, fresh copies.
L.balanced(() => {
  const ptr = L.bytes(new Uint8Array([1, 2, 3]));
  const [rp, rl] = call("Probe#bytes(u8[])", (v) => v, ptr, 3);
  const seen = p.seen.pop();
  assert.ok(seen instanceof Uint8Array);
  assert.deepEqual([...seen], [1, 2, 3]);
  assert.notEqual(seen.buffer, L.ex.memory.buffer);
  assert.deepEqual([...L.take(rp, rl)], [1, 2, 3]);
});
L.balanced(() => {
  const ptr = L.bytes(new Float64Array([1.5, -0, NaN]), 8);
  const [rp, rl] = call("Probe#floats(f64[])", (v) => v, ptr, 3);
  assert.deepEqual([...p.seen.pop()], [1.5, -0, NaN]);
  assert.equal(rp % 8, 0);
  const out = new Float64Array(L.take(rp, rl, 8, 8).buffer);
  assert.ok(Object.is(out[1], -0));
});
L.balanced(() => {
  const [rp, rl] = call("Probe#bigs(i64[])", new BigInt64Array([-1n, 2n ** 62n]), 8, 0);
  assert.deepEqual([...new BigInt64Array(L.take(rp, rl, 8, 8).buffer)], [-1n, 2n ** 62n]);
});
L.balanced(() => L.throwsType(() => call("Probe#bytes(u8[])", [1, 2], 1, 0)));
L.balanced(() => L.throwsType(() => call("Probe#bytes(u8[])", new Int8Array(2), 1, 0)));
assert.deepEqual(call("Probe#bytes(u8[])", new Uint8Array(0), 1, 0), [1, 0]);
assert.deepEqual(call("Probe#optBytes(u8[]?)", undefined, 0, -1), [0, -1]);

// mut u8[]: writeback into the same storage, fixed length.
{
  const ptr = L.bytes(new Uint8Array([1, 2, 3, 4]));
  call("Probe#fill(mut u8[])", (v) => v.fill(9, 1, 3), ptr, 4);
  assert.deepEqual([...L.u8().slice(ptr, ptr + 4)], [1, 9, 9, 4]);
}

// arr<T>: numeric memory parts, packed.
L.balanced(() => {
  const ptr = L.bytes(new Uint32Array([7, 4294967295]), 4);
  const [rp, rl] = call("Probe#nums(arr<u32>)", (v) => v, ptr, 2);
  assert.deepEqual(p.seen.pop(), [7, 4294967295]);
  assert.deepEqual([...new Uint32Array(L.take(rp, rl, 4, 4).buffer)], [7, 4294967295]);
});
assert.deepEqual(call("Probe#nums(arr<u32>)", [], 4, 0), [4, 0]);
L.balanced(() => L.throwsType(() => call("Probe#nums(arr<u32>)", [1, -1], 4, 0), /u32/));
L.balanced(() => L.throwsType(() => call("Probe#nums(arr<u32>)", "12", 4, 0), /u32\[\]/));

// arr<str>: nested descriptors; the language owns every nested string.
L.balanced(() => {
  const [a, b] = [L.str("one"), L.str("twö")];
  const ptr = L.alloc(16, 4);
  dv().setUint32(ptr, a[0], true);
  dv().setUint32(ptr + 4, a[1], true);
  dv().setUint32(ptr + 8, b[0], true);
  dv().setUint32(ptr + 12, b[1], true);
  const [rp, rl] = call("Probe#strs(arr<str>)", (v) => [...v, ""], ptr, 2);
  assert.deepEqual(p.seen.pop(), ["one", "twö"]);
  assert.equal(rl, 3);
  const d = dv();
  const parts = [0, 1, 2].map((i) => [d.getUint32(rp + i * 8, true), d.getUint32(rp + i * 8 + 4, true)]);
  assert.deepEqual(
    parts.map(([q, n]) => L.takeStr(q, n)),
    ["one", "twö", ""],
  );
  L.take(rp, rl, 8, 4);
});

// A failure part-way releases everything allocated so far (IR.md §15).
L.balanced(() => L.throwsType(() => call("Probe#strs(arr<str>)", ["a", "b", 5], 4, 0), /expected str/));
L.balanced(() => {
  const trap = new Proxy(["a", "b", "c"], {
    get(t, k) {
      if (k === "2") throw new RangeError("trap");
      return t[k];
    },
  });
  assert.throws(() => call("Probe#strs(arr<str>)", trap, 4, 0), RangeError);
});
L.balanced(() => {
  const getter = ["a"];
  Object.defineProperty(getter, 1, {
    get() {
      throw new SyntaxError("getter");
    },
  });
  assert.throws(() => call("Probe#strs(arr<str>)", getter, 4, 0), SyntaxError);
});
L.balanced(() => L.throwsType(() => call("Probe#nested(arr<arr<str>>)", [["a"], ["b", 1]], 4, 0)));

// Reentry mid-conversion: an element getter calls back into the language,
// whose own import allocates a result, then the outer conversion fails.
// The inner result belongs to the language; the outer partial is released.
L.balanced(() => {
  const outer = ["a", "b"];
  let inner;
  Object.defineProperty(outer, 2, {
    get() {
      const q = new Probe();
      q.next = "inner";
      inner = L.call("Probe#str(str)", q, ...L.str(""));
      throw new SyntaxError("outer");
    },
  });
  assert.throws(() => call("Probe#strs(arr<str>)", outer, 4, 0), SyntaxError);
  assert.equal(L.takeStr(...inner), "inner");
});
// The inner failure unwinds through both conversions.
L.balanced(() => {
  const outer = ["a"];
  Object.defineProperty(outer, 1, {
    get() {
      const q = new Probe();
      q.next = ["x", 1];
      return L.call("Probe#strs(arr<str>)", q, 4, 0);
    },
  });
  L.throwsType(() => call("Probe#strs(arr<str>)", outer, 4, 0), /expected str/);
});

// arr<str?>?: nullable elements and a nullable array.
assert.deepEqual(call("Probe#optStrs(arr<str?>?)", null, 0, -1), [0, -1]);
L.balanced(() => {
  const [rp, rl] = call("Probe#optStrs(arr<str?>?)", ["x", null, undefined], 0, -1);
  const d = dv();
  assert.equal(d.getInt32(rp + 12, true), -1);
  assert.equal(d.getInt32(rp + 20, true), -1);
  L.takeStr(d.getUint32(rp, true), d.getUint32(rp + 4, true));
  L.take(rp, rl, 8, 4);
});

// arr<(u32, str, Request)>: memory part (u32, ptr, len) and one table slot.
L.balanced(() => {
  const reqs = [new Request(), new Request()];
  const [rp, rx, rl] = call("Probe#rows(arr<(u32,str,Request)>)", [[1, "a", reqs[0]], [2, "bc", reqs[1]]], 0, 0, 0);
  assert.equal(rl, 2);
  const d = dv();
  assert.equal(d.getUint32(rp, true), 1);
  assert.equal(d.getUint32(rp + 12, true), 2);
  assert.equal(L.ex["tw:table"].get(rx), reqs[0]);
  assert.equal(L.ex["tw:table"].get(rx + 1), reqs[1]);
  // The language reads it back as the same values.
  assert.deepEqual(call("Probe#rows(arr<(u32,str,Request)>)", [], rp, rx, rl), [4, 0, 0]);
  assert.deepEqual(p.seen.pop(), [[1, "a", reqs[0]], [2, "bc", reqs[1]]]);
  L.takeStr(d.getUint32(rp + 4, true), d.getUint32(rp + 8, true));
  L.takeStr(d.getUint32(rp + 16, true), d.getUint32(rp + 20, true));
  L.take(rp, rl, 12, 4);
  L.ex["tw:table/free"](rx, 2);
});
L.balanced(() => L.throwsType(() => call("Probe#rows(arr<(u32,str,Request)>)", [[1, "a", {}]], 0, 0, 0), /Request/));
L.balanced(() => L.throwsType(() => call("Probe#rows(arr<(u32,str,Request)>)", [[1, "a"]], 0, 0, 0)));

// arr<Request>: all references, `(base, len)`.
L.balanced(() => {
  const r = new Request();
  const [rx, rl] = call("Probe#reqs(arr<Request>)", [r], 0, 0);
  assert.equal(rl, 1);
  assert.equal(L.ex["tw:table"].get(rx), r);
  L.ex["tw:table/free"](rx, 1);
});

// arr<(bool, i53?, f64)>: packed with present flags, unaligned access.
L.balanced(() => {
  const [rp, rl] = call("Probe#mixed(arr<(bool,i53?,f64)>)", [[true, null, 1.5], [false, -3, 2]], 0, 0);
  const d = dv();
  // bool 1 + (u8 present, i64) 9 + f64 8 = 18 bytes, aligned to 8.
  assert.equal(rp % 8, 0);
  assert.equal(d.getUint8(rp), 1);
  assert.equal(d.getUint8(rp + 1), 0);
  assert.equal(d.getFloat64(rp + 10, true), 1.5);
  assert.equal(d.getUint8(rp + 18), 0);
  assert.equal(d.getUint8(rp + 19), 1);
  assert.equal(d.getBigInt64(rp + 20, true), -3n);
  call("Probe#mixed(arr<(bool,i53?,f64)>)", [], rp, rl);
  assert.deepEqual(p.seen.pop(), [[true, undefined, 1.5], [false, -3, 2]]);
  L.take(rp, rl, 18, 8);
});

// Direct tuples flatten recursively.
assert.deepEqual(call("Probe#pair((u32,(f64,bool)))", [5, [2.5, true]], 0, 0, 0), [5, 2.5, 1]);
assert.deepEqual(p.seen.pop(), [0, [0, false]]);
L.throwsType(() => call("Probe#pair((u32,(f64,bool)))", [5, [2.5]], 0, 0, 0));
L.balanced(() => {
  const [present, n, sp, sl] = call("Probe#optPair((u16,str)?)", [3, "s"], 0, 0, 0, 0);
  assert.deepEqual([present, n], [1, 3]);
  assert.equal(L.takeStr(sp, sl), "s");
});
assert.deepEqual(call("Probe#optPair((u16,str)?)", undefined, 0, 0, 0, 0), [0, 0, 0, 0]);

console.log("ok");
