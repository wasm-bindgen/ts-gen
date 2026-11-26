import { assert } from "./harness.mjs";

globalThis.P = class {
  two() {
    return [1, 2];
  }
};
const { instantiate, app, spillEmpty } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const ex = instantiate(compile("lang.wasm"), compile("bridge.wasm")).exports;
const live = ex["test:live"];

assert.deepEqual(app.pair("abc"), [3, 3]);
assert.equal(live(), 0);

// A spill left unread faults the next entry before its arguments are the
// language's, so the bridge frees them.
ex.dirty(new P());
// Only a call with spilled results checks it (§12).
assert.equal(app.len("abc"), 3);
assert.equal(live(), 0);
assert.throws(() => app.pair("abc"), (e) => e instanceof TypeError && /left unread/.test(e.message));
assert.equal(live(), 0);
assert.equal(spillEmpty(), true);
assert.deepEqual(app.pair("abc"), [3, 3]);
console.log("done");
