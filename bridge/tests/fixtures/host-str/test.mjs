import { assert, lang } from "./harness.mjs";

class Probe {}
for (const m of ["str", "optStr", "strs", "rows", "body", "pair"]) {
  Probe.prototype[m] = function (v) {
    this.seen = v;
    return typeof this.next === "function" ? this.next(v) : this.next;
  };
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
const table = L.ex["tw:table"];

// Strings cross as JS strings (externref), lone surrogates intact.
assert.equal(call("Probe#str(str)", (v) => v + "!", "a\ud800"), "a\ud800!");
assert.equal(p.seen, "a\ud800");
L.throwsType(() => call("Probe#str(str)", 5, "x"), /str/);
assert.equal(call("Probe#optStr(str?)", null, null), null);
assert.equal(p.seen, undefined);

// arr<str> is all references: (base, len) over the table.
L.balanced(() => {
  const [base, len] = call("Probe#strs(arr<str>)", ["x", "y"], 0, 0);
  assert.equal(len, 2);
  assert.deepEqual([table.get(base), table.get(base + 1)], ["x", "y"]);
  call("Probe#strs(arr<str>)", [], base, len);
  assert.deepEqual(p.seen, ["x", "y"]);
  L.ex["tw:table/free"](base, len);
});
L.balanced(() => L.throwsType(() => call("Probe#strs(arr<str>)", ["x", 1], 0, 0)));

// arr<(u32, str)>: a memory part and a table part.
L.balanced(() => {
  const [ptr, base, len] = call("Probe#rows(arr<(u32,str)>)", [[1, "a"], [2, "b"]], 4, 0, 0);
  assert.equal(L.dv().getUint32(ptr + 4, true), 2);
  assert.equal(table.get(base + 1), "b");
  L.take(ptr, len, 4, 4);
  L.ex["tw:table/free"](base, len);
});

// An enum's str slot is an externref.
assert.deepEqual(call("Probe#body(Body)", "t", 2, null, 5), [1, "t", 0]);
assert.equal(p.seen, 5);
assert.deepEqual(call("Probe#pair((u8,str))", [3, "q"], 0, "z"), [3, "q"]);
assert.deepEqual(p.seen, [0, "z"]);

// Coincidence: a str is already its box.
assert.equal(L.call("box:str", "s"), "s");
assert.equal(L.call("unbox:str", "s"), "s");
assert.equal(L.call("unbox:Str", "s"), "s");
L.throwsType(() => L.call("unbox:str", 1));

console.log("ok");
