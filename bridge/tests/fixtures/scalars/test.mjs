import { assert, lang } from "./harness.mjs";

class Probe {
  constructor() {
    this.seen = [];
    this.next = undefined;
  }
}
for (const m of [
  "bool", "i8", "u8", "i16", "u16", "i32", "u32", "i53", "u53", "i64", "u64", "f32", "f64",
  "optBool", "optU8", "optI16", "optU32", "optI53", "optF64", "optI64",
]) {
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

// Wasm → JS: the language's wire value becomes the IR type's JS value.
function sent(key, ...wire) {
  p.next = undefined;
  try {
    L.call(key, p, ...wire);
  } catch {}
  return p.seen.pop();
}
// JS → Wasm: the host's return value becomes the wire value, or TypeError.
function got(key, value, ...wire) {
  p.next = value;
  return L.call(key, p, ...wire);
}

assert.equal(sent("Probe#bool(bool)", 1), true);
assert.equal(sent("Probe#bool(bool)", 0), false);
assert.equal(sent("Probe#i8(i8)", -128), -128);
assert.equal(sent("Probe#u8(u8)", 255), 255);
assert.equal(sent("Probe#u32(u32)", -1), 4294967295);
assert.equal(sent("Probe#i53(i53)", -(2n ** 53n - 1n)), -(2 ** 53 - 1));
assert.equal(sent("Probe#u53(u53)", 2n ** 53n - 1n), 2 ** 53 - 1);
assert.equal(sent("Probe#i64(i64)", -(2n ** 63n)), -(2n ** 63n));
assert.equal(sent("Probe#u64(u64)", -1n), 2n ** 64n - 1n);
assert.equal(sent("Probe#f64(f64)", -0), -0);
assert.ok(Object.is(sent("Probe#f64(f64)", -0), -0));

assert.equal(got("Probe#bool(bool)", true, 0), 1);
assert.equal(got("Probe#bool(bool)", false, 0), 0);
L.throwsType(() => got("Probe#bool(bool)", 1, 0), /expected bool/);
L.throwsType(() => got("Probe#bool(bool)", undefined, 0));

const ranges = [
  ["Probe#u8(u8)", 0, 255],
  ["Probe#i8(i8)", -128, 127],
  ["Probe#u16(u16)", 0, 65535],
  ["Probe#i16(i16)", -32768, 32767],
  ["Probe#i32(i32)", -(2 ** 31), 2 ** 31 - 1],
];
for (const [key, lo, hi] of ranges) {
  assert.equal(got(key, lo, 0), lo);
  assert.equal(got(key, hi, 0), hi);
  L.throwsType(() => got(key, lo - 1, 0));
  L.throwsType(() => got(key, hi + 1, 0));
  L.throwsType(() => got(key, 4.5, 0));
  L.throwsType(() => got(key, NaN, 0));
  L.throwsType(() => got(key, "1", 0));
  L.throwsType(() => got(key, 1n, 0));
}
assert.equal(got("Probe#u16(u16)", -0, 0), 0);
assert.equal(got("Probe#u32(u32)", 4294967295, 0), -1);
L.throwsType(() => got("Probe#u32(u32)", 4294967296, 0));

assert.equal(got("Probe#i53(i53)", 2 ** 53 - 1, 0n), 2n ** 53n - 1n);
L.throwsType(() => got("Probe#i53(i53)", 2 ** 53, 0n));
L.throwsType(() => got("Probe#u53(u53)", -1, 0n));
L.throwsType(() => got("Probe#u53(u53)", 2 ** 53, 0n));

// i64/u64 are bigints, and accept an integral number in range.
assert.equal(got("Probe#i64(i64)", 5, 0n), 5n);
assert.equal(got("Probe#i64(i64)", -(2n ** 63n), 0n), -(2n ** 63n));
L.throwsType(() => got("Probe#i64(i64)", 2n ** 63n, 0n));
L.throwsType(() => got("Probe#i64(i64)", 2 ** 63, 0n));
L.throwsType(() => got("Probe#i64(i64)", 0.5, 0n));
assert.equal(got("Probe#u64(u64)", 2n ** 64n - 1n, 0n), -1n);
L.throwsType(() => got("Probe#u64(u64)", -1n, 0n));
L.throwsType(() => got("Probe#u64(u64)", 2n ** 64n, 0n));

assert.ok(Number.isNaN(got("Probe#f64(f64)", NaN, 0)));
assert.equal(got("Probe#f32(f32)", 1.5, 0), 1.5);
L.throwsType(() => got("Probe#f64(f64)", "1.5", 0));
L.throwsType(() => got("Probe#f64(f64)", 1n, 0));

// Niches and present pairs (IR.md §10).
assert.equal(sent("Probe#optBool(bool?)", 2), undefined);
assert.equal(sent("Probe#optBool(bool?)", 1), true);
assert.equal(got("Probe#optBool(bool?)", null, 0), 2);
assert.equal(got("Probe#optBool(bool?)", undefined, 0), 2);
assert.equal(got("Probe#optBool(bool?)", false, 0), 0);
assert.equal(sent("Probe#optU8(u8?)", -(2 ** 31)), undefined);
assert.equal(sent("Probe#optU8(u8?)", 0), 0);
assert.equal(got("Probe#optU8(u8?)", null, 0), -(2 ** 31));
assert.equal(got("Probe#optU8(u8?)", 7, 0), 7);
L.throwsType(() => got("Probe#optU8(u8?)", 256, 0));
assert.equal(got("Probe#optI16(i16?)", -1, 0), -1);
assert.deepEqual(got("Probe#optU32(u32?)", undefined, 0, 0), [0, 0]);
assert.deepEqual(got("Probe#optU32(u32?)", 4294967295, 0, 0), [1, -1]);
assert.equal(sent("Probe#optU32(u32?)", 0, 5), undefined);
assert.equal(sent("Probe#optU32(u32?)", 1, -1), 4294967295);
assert.equal(got("Probe#optI53(i53?)", null, 0n), -(2n ** 63n));
assert.equal(sent("Probe#optI53(i53?)", -(2n ** 63n)), undefined);
assert.equal(sent("Probe#optI53(i53?)", 0n), 0);
// Out of range from the language is its bug: a TypeError, niche aside.
L.throwsType(() => L.call("Probe#i53(i53)", p, 2n ** 53n, 0n), /outside i53/);
L.throwsType(() => L.call("Probe#u53(u53)", p, -1n, 0n), /outside u53/);
L.throwsType(() => L.call("Probe#optI53(i53?)", p, -(2n ** 63n) + 1n, 0n), /outside i53/);
assert.deepEqual(got("Probe#optF64(f64?)", null, 0, 0), [0, 0]);
assert.ok(Number.isNaN(got("Probe#optF64(f64?)", NaN, 0, 0)[1]));
assert.equal(sent("Probe#optF64(f64?)", 1, NaN) !== undefined, true);
assert.deepEqual(got("Probe#optI64(i64?)", 3, 0, 0n), [1, 3n]);

console.log("ok");
