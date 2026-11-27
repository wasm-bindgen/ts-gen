import { assert, lang } from "./harness.mjs";

class Request {}
class Probe {}
const members = [
  "u8", "i8", "u16", "i32", "f64", "bool", "i53", "i64", "str", "bytes", "buf",
  "nums", "strs", "pair", "method", "request", "maybe", "cb",
];
for (const m of members) {
  Probe.prototype[m] = function () {
    return this.next;
  };
}
const Keys = { echo: () => Keys.next };
Object.assign(globalThis, { Request, Probe, Keys });

const { instantiate, Counter, len, pick } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
let L;
const impl = {
  "Counter.new(u32)": () => 64,
  "Counter#get:value()": (self, h) => h,
  "len(str)": (p, n) => (L.take(p, n), n),
  'pick<"width">(K)': () => 7,
  "tw:drop:Counter": () => {},
};
L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }));
const p = new Probe();
const call = (key, next) => {
  p.next = next;
  return L.call(key, p);
};

// Trusted: no value is tested against its type. A narrow integer still
// narrows, so the language reads its carrier in range.
assert.equal(call("Probe#u8()", 300), 44);
assert.equal(call("Probe#u8()", "7"), 7);
assert.equal(call("Probe#u8()", undefined), 0);
assert.equal(call("Probe#i8()", 200), -56);
assert.equal(call("Probe#u16()", -1), 65535);
assert.equal(call("Probe#bool()", "x"), 1);
assert.equal(call("Probe#bool()", 0), 0);
// The 32-bit integers and the floats are Wasm's to convert.
assert.equal(call("Probe#i32()", 2 ** 32 + 5), 5);
assert.equal(call("Probe#i32()", 4.9), 4);
assert.ok(Number.isNaN(call("Probe#f64()", NaN)));
assert.equal(call("Probe#f64()", "2.5"), 2.5);
// The 64-bit carriers take a number or a bigint through `BigInt`, which
// throws on what isn't an integer.
assert.equal(call("Probe#i53()", 5), 5n);
assert.equal(call("Probe#i64()", 5), 5n);
assert.equal(call("Probe#i64()", 7n), 7n);
assert.throws(() => call("Probe#i53()", 3.7), RangeError);
assert.throws(() => call("Probe#i64()", NaN), RangeError);

// An encoded string keeps its one guard, so a non-string can't be
// measured as one.
L.balanced(() => assert.equal(L.takeStr(...call("Probe#str()", 42)), "42"));
L.balanced(() => assert.equal(len(42), 2));

// A `T[]` copies whatever has a length. One with none allocates nothing.
L.balanced(() => assert.deepEqual([...L.take(...call("Probe#bytes()", [1, 2]))], [1, 2]));
L.balanced(() => assert.deepEqual(call("Probe#bytes()", {}), [1, 0]));
L.balanced(() => assert.deepEqual(call("Probe#bytes()", 5), [1, 0]));
L.balanced(() => assert.deepEqual([...L.take(...call("Probe#buf()", new Uint8Array([3, 4]).buffer))], [3, 4]));
L.balanced(() => assert.deepEqual(call("Probe#buf()", {}), [1, 0]));
const nums = (v) => {
  const [ptr, n] = call("Probe#nums()", v);
  return [...new Uint32Array(L.take(ptr, n, 4, 4).buffer)];
};
L.balanced(() => assert.deepEqual(nums([1, "2", -1]), [1, 2, 4294967295]));
L.balanced(() => assert.deepEqual(nums({ length: 2, 0: 7, 1: 8 }), [7, 8]));
L.balanced(() => assert.deepEqual(nums({}), []));
L.balanced(() => {
  const [ptr, n] = call("Probe#strs()", ["a", 5]);
  const strs = [];
  for (let i = 0; i < n; i++) {
    strs.push(L.takeStr(L.dv().getInt32(ptr + i * 8, true), L.dv().getInt32(ptr + i * 8 + 4, true)));
  }
  L.take(ptr, n, 8, 4);
  assert.deepEqual(strs, ["a", "5"]);
});
L.balanced(() => {
  const [n, sp, sl] = call("Probe#pair()", { 0: 300, 1: "b", 2: "more" });
  assert.equal(n, 300);
  assert.equal(L.takeStr(sp, sl), "b");
});
{
  const o = {};
  assert.equal(call("Probe#request()", o), o);
}
// A literal result isn't compared.
Keys.next = "height";
L.call('Keys.echo<"width">(K)');

// A nullable host value or function still reads either empty as none.
assert.equal(call("Probe#maybe()", undefined), null);
assert.equal(call("Probe#cb()", undefined), null);
assert.equal(L.call("unbox:Request?", undefined), null);
assert.equal(L.call("unboxUnchecked:Request?", undefined), null);
assert.equal(L.call("arr:get<Request?>", [undefined], 0), null);

// `tw:unbox` and `tw:arr get` trust as the rest does. The array and the
// index are still tested.
assert.equal(L.call("unbox:u8", 300), 44);
assert.equal(L.call("arr:get<u8>", [300], 0), 44);
L.throwsType(() => L.call("arr:get<u8>", [1], 1), /out of range/);
L.throwsType(() => L.call("arr:get<u8>", { length: 1, 0: 1 }, 0), /Arr/);

// Dispatch still runs: an unmatched enum value is a TypeError.
assert.equal(call("Probe#method()", "POST"), 2);
L.throwsType(() => call("Probe#method()", "PUT"), /Method/);

// So do handle lookups, and a literal an export takes is still compared.
const c = new Counter(1);
assert.equal(c.value, 64);
const value = Object.getOwnPropertyDescriptor(Counter.prototype, "value").get;
L.throwsType(() => value.call({}), /Counter/);
L.throwsType(() => L.call("unbox:Counter", {}), /Counter/);
L.call("destroy:Counter", 64);
L.throwsType(() => c.value, /destroyed/);
assert.equal(pick("width"), 7);
L.throwsType(() => pick("height"), /width/);

console.log("ok");
