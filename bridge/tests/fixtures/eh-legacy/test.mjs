import { assert } from "./harness.mjs";

class Kv {
  constructor() {
    this.next = undefined;
    this.seen = [];
  }
}
for (const m of ["get", "raw", "name", "nameRaw"]) {
  Kv.prototype[m] = function (key) {
    this.seen.push(key);
    if (typeof this.next === "function") return this.next();
    return this.next;
  };
}
globalThis.Kv = Kv;

const { instantiate, scratchUse } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const ex = instantiate(compile("lang.wasm"), compile("bridge.wasm")).exports;
const kv = new Kv();
const throwing = (e) => () => {
  throw e;
};
const missing = () => new DOMException("missing", "NotFoundError");

// The language catches a mapped tag.
kv.next = throwing(missing());
assert.deepEqual(ex.get(kv, "k"), [2, "missing"]);
kv.next = throwing(new TypeError("t"));
assert.deepEqual(ex.get(kv, "k"), [1, "t"]);
kv.next = "v";
assert.deepEqual(ex.get(kv, "k"), [0, "v"]);

// A caught and rethrown unknown exception keeps its identity.
{
  const err = new SyntaxError("raw");
  kv.next = throwing(err);
  assert.throws(() => ex.raw(kv, "k"), (e) => e === err);
}

// An uncaught tag leaves the language as a WebAssembly.Exception.
kv.next = throwing(missing());
assert.throws(
  () => ex.relay(kv, "k"),
  (e) => e instanceof WebAssembly.Exception && e.is(ex.tag) && e.getArg(ex.tag, 0) === 2,
);

// GC strings go through bridge.wasm forwarders, which restore the scratch
// bump pointers on success and on every unwind.
const before = scratchUse();
kv.next = "abc";
assert.deepEqual(ex.name(kv, 5), [0, 3, null]);
assert.equal(kv.seen.pop(), "aaaaa");
assert.deepEqual(scratchUse(), before);
for (let i = 0; i < 20; i++) {
  kv.next = throwing(missing());
  assert.deepEqual(ex.name(kv, 100000), [2, 0, "missing"]);
}
assert.deepEqual(scratchUse(), before);
kv.next = 5;
{
  const [arm, len, msg] = ex.name(kv, 3);
  assert.deepEqual([arm, len], [1, 0]);
  assert.match(msg, /expected str/);
}
assert.deepEqual(scratchUse(), before);
{
  const err = new SyntaxError("through");
  kv.next = throwing(err);
  assert.throws(() => ex.nameRaw(kv, 100000), (e) => e === err);
}
assert.deepEqual(scratchUse(), before);
kv.next = "😀".repeat(1000);
assert.equal(ex.nameRaw(kv, 2), 4000);
assert.deepEqual(scratchUse(), before);

console.log("ok");
