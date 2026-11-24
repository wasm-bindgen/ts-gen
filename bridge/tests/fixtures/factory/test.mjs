import { assert, lang } from "./harness.mjs";

class Probe {
  str(v) {
    return v + "?";
  }
}
globalThis.Probe = Probe;

const mod = await import("./bridge.js");
assert.deepEqual(Object.keys(mod), ["createBridge"]);
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const language = compile("lang.wasm");
const bridgeModule = compile("bridge.wasm");

// Each bridge instantiates its own language instance, with its own state.
const make = (k) => {
  const b = mod.createBridge();
  const impl = {
    "util.add(u32,u32)": (a, c) => a + c + k,
    // The pass-through language hands the string over as its wire.
    "util.len(str)": (p, n) => n + k,
  };
  const L = lang(b.instantiate(language, bridgeModule, { "test:impl": impl }));
  return { b, L };
};
const one = make(1);
const two = make(2);
assert.notEqual(one.L.ex.memory, two.L.ex.memory);
assert.equal(one.b.util.add(3, 4), 8);
assert.equal(two.b.util.add(3, 4), 9);
assert.equal(one.b.util.len("ab"), 3);
assert.equal(two.b.util.len("ab"), 4);
assert.throws(() => one.b.util.add(-1, 0), TypeError);

// Imports run against the instance that called them.
const p = new Probe();
for (const { L } of [one, two]) {
  L.balanced(() => assert.equal(L.takeStr(...L.call("Probe#str(str)", p, ...L.str("x"))), "x?"));
}
assert.equal(typeof one.b.scratchUse(), "object");

// A bridge still instantiates once.
assert.throws(() => one.b.instantiate(language, bridgeModule, {}), /already instantiated/);

console.log("ok");
