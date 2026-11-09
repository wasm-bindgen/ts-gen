import { assert, lang } from "./harness.mjs";

globalThis.Wrap = class {
  constructor(...args) {
    this.value = args.join(" ");
  }
};
globalThis.Shapes = {
  Pt: class {
    constructor(x) {
      this.x = x;
    }
  },
};

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const str = (w) => L.takeStr(...w);

// Each overload runs `new` on the value, not on the interface.
const a = L.balanced(() => L.call("Stub.new(str)", ...L.str("hi")));
assert.ok(a instanceof globalThis.Wrap);
assert.equal(str(L.call("Stub#get:value()", a)), "hi");
const b = L.balanced(() => L.call("Stub.new(f64,str)", 2, ...L.str("kg")));
assert.equal(str(L.call("Stub#get:value()", b)), "2 kg");

// A second constructor beside the dictionary's own `{}`.
const d = L.call("Point.new()");
assert.deepEqual(Object.keys(d), []);
const p = L.call("Point.new:pt(f64)", 3);
assert.ok(p instanceof globalThis.Shapes.Pt);
assert.deepEqual(L.call("Point#get:x()", p), [1, 3]);

// A module's interface has no value; its constructor reaches the export.
const c = L.call("Counter.new(f64)", 5);
assert.equal(L.call("Counter#count()", c), 6);

// A global interface constructed through a module's export.
const r = L.call("Remote.new()");
assert.equal(L.call("Remote#get:id()", r), 7);
