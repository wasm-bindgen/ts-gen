import { assert, lang } from "./harness.mjs";

class Probe {}
for (const m of ["names", "pairs", "nested", "counters", "bodies"]) {
  Probe.prototype[m] = function () {
    return this.next;
  };
}
for (const m of ["see", "seePairs", "seeNested", "seeCounters", "seeBodies"]) {
  Probe.prototype[m] = function (v) {
    this.seen = v;
  };
}
globalThis.Probe = Probe;

const { instantiate, scratchUse, Counter } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const impl = { "Counter.new()": (self) => ({ self }) };
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }));
const p = new Probe();
const base = scratchUse();

// GC strings in a linear `arr<T>` sit externalized in the language's table:
// JS → language and back through the bridge's conversions.
p.next = ["a", "héllo", ""];
const [x0, n] = L.call("Probe#names()", p);
assert.equal(n, 3);
assert.equal(typeof L.ex["tw:table"].get(x0), "object", "an externalized GC array");
L.call("Probe#see(arr<str>)", p, x0, n);
assert.deepEqual(p.seen, ["a", "héllo", ""]);
assert.deepEqual(scratchUse(), base);

p.next = [[1, "x"], [2, null]];
const w = L.call("Probe#pairs()", p);
L.call("Probe#seePairs(arr<(u32,str?)>)", p, ...w);
assert.deepEqual(p.seen, [[1, "x"], [2, undefined]]);

p.next = [["a"], [], ["b", "c"]];
const nw = L.call("Probe#nested()", p);
L.call("Probe#seeNested(arr<arr<str>>)", p, ...nw);
assert.deepEqual(p.seen, [["a"], [], ["b", "c"]]);
assert.deepEqual(scratchUse(), base);

// GC handles sit externalized as they are, and keep wrapper identity.
const c = new Counter();
p.next = [c, c];
const cw = L.call("Probe#counters()", p);
assert.equal(L.ex["tw:table"].get(cw[0]).self, c, "the handle, externalized");
L.call("Probe#seeCounters(arr<Counter>)", p, ...cw);
assert.deepEqual(p.seen, [c, c]);
assert.equal(p.seen[0], c);

// A payload enum with GC payloads: the arm and its joined slots, the GC
// string externalized in the table and converted by the bridge.
p.next = ["hi", 2.5, [7, "seven"], ""];
const bw = L.call("Probe#bodies()", p);
// The layout's form (IR.md §11): a 4-byte arm, then the slots joined
// first-fit (a table reference, an `f64`, an `i32`), so 16 bytes and one
// table slot per element.
const [bp, bb, bn] = bw;
assert.equal(bn, 4);
const dv = L.dv();
assert.deepEqual([dv.getInt32(bp + 32, true), dv.getFloat64(bp + 36, true), dv.getInt32(bp + 44, true)], [3, 0, 7]);
assert.deepEqual([dv.getInt32(bp + 16, true), dv.getFloat64(bp + 20, true)], [2, 2.5]);
assert.equal(L.ex["tw:table"].get(bb + 1), null, "an unused reference slot is null");
assert.equal(typeof L.ex["tw:table"].get(bb + 2), "object", "an externalized GC string");
L.call("Probe#seeBodies(arr<Body>)", p, ...bw);
assert.deepEqual(p.seen, ["hi", 2.5, [7, "seven"], ""]);
assert.deepEqual(scratchUse(), base);
p.next = ["ok", true];
L.balanced(() => L.throwsType(() => L.call("Probe#bodies()", p)));
assert.deepEqual(scratchUse(), base);

// A failing element releases what was allocated and restores scratch.
p.next = ["a", 5];
L.balanced(() => L.throwsType(() => L.call("Probe#names()", p), /str/));
assert.deepEqual(scratchUse(), base);
console.log("done");
