import { assert } from "./harness.mjs";

class Probe {
  makeTrio() {
    return this.next;
  }
  seeTrio(v) {
    this.seen = v;
  }
}
globalThis.Probe = Probe;

const { instantiate, util, scratchUse, spillEmpty } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
let ex;
const p = new Probe();
const impl = {
  // A GC string only the language can make: through the import.
  "util.trio(u32)": (x) => {
    p.next = [`t${x}`, x, x / 2];
    return ex["call:Probe#makeTrio()"](p);
  },
};
ex = instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }).exports;
const base = scratchUse();
const idle = () => {
  assert.deepEqual(scratchUse(), base);
  assert.equal(spillEmpty(), true);
};

// The GC forwarder returns slot 0 and spills the numbers.
p.next = ["s", 1, 2.5];
const w = ex["call:Probe#makeTrio()"](p);
assert.equal(w.length, 3);
assert.deepEqual(w.slice(1), [1, 2.5]);
idle();
ex["call:Probe#seeTrio((str,u32,f64))"](p, ...w);
assert.deepEqual(p.seen, ["s", 1, 2.5]);
idle();

// The GC entry pops the export's spilled slots in the bridge module.
assert.deepEqual(util.trio(3), ["t3", 3, 1.5]);
idle();

console.log("ok");
