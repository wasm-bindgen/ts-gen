import { assert, lang } from "./harness.mjs";

class Request {}
class Probe {}
for (const m of ["u8", "i8", "u16", "i32", "f64", "bool", "i53", "i64", "str", "bytes", "nums", "method", "request"]) {
  Probe.prototype[m] = function () {
    return this.next;
  };
}
Object.assign(globalThis, { Request, Probe });

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const p = new Probe();
const call = (key, next) => {
  p.next = next;
  return L.call(key, p);
};

// No TypeErrors; narrow integers keep their carrier's range.
assert.equal(call("Probe#u8()", 300), 44);
assert.equal(call("Probe#u8()", "7"), 7);
assert.equal(call("Probe#u8()", undefined), 0);
assert.equal(call("Probe#i8()", 200), -56);
assert.equal(call("Probe#u16()", -1), 65535);
assert.equal(call("Probe#i32()", 2 ** 32 + 5), 5);
assert.equal(call("Probe#i32()", 4.9), 4);
assert.ok(Number.isNaN(call("Probe#f64()", NaN)));
assert.equal(call("Probe#f64()", "2.5"), 2.5);
assert.equal(call("Probe#bool()", "x"), 1);
assert.equal(call("Probe#bool()", 0), 0);
assert.equal(call("Probe#i53()", 3.7), 3n);
assert.equal(call("Probe#i64()", 5), 5n);
assert.equal(call("Probe#i64()", NaN), 0n);
L.balanced(() => assert.equal(L.takeStr(...call("Probe#str()", 42)), "42"));
L.balanced(() => assert.deepEqual(call("Probe#bytes()", [1]), [1, 0]));
L.balanced(() => {
  const [ptr, len] = call("Probe#nums()", [1, "2", -1]);
  assert.deepEqual([...new Uint32Array(L.take(ptr, len, 4, 4).buffer)], [1, 2, 4294967295]);
});
{
  const o = {};
  assert.equal(call("Probe#request()", o), o);
}

// Dispatch still runs: an unmatched enum value is a TypeError.
assert.equal(call("Probe#method()", "POST"), 2);
L.throwsType(() => call("Probe#method()", "PUT"), /Method/);

console.log("ok");
