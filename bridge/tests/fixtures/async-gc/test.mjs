import { assert } from "./harness.mjs";

class Api {
  fetch() {
    return this.next;
  }
}
globalThis.Api = Api;

const { instantiate, app, scratchUse } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const tick = () => new Promise((r) => setTimeout(r, 0));

let ex;
let resolver;
const wakes = [];
const impl = {
  "tw:future/wake": (w) => wakes.push(w),
  "app.run()": () => {
    const [f, r] = ex["call:future:new"]();
    resolver = r;
    return f;
  },
};
ex = instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }).exports;
const base = scratchUse();

// A GC string taken from one future resolves another, through scratch.
const api = new Api();
api.next = Promise.resolve("héllo");
const f = ex["call:Api#fetch()"](api);
assert.equal(ex["call:future:poll"](f, 4), 0);
await tick();
assert.deepEqual(wakes, [4]);
assert.equal(ex["call:future:poll"](f, 4), 1);
const s = ex["call:future:take<str>"](f);
const p = app.run();
ex["call:future:resolve<str>"](resolver, s);
assert.equal(await p, "héllo");
assert.deepEqual(scratchUse(), base);

// Once terminated, wakes stop.
let res;
api.next = new Promise((r) => (res = r));
const g = ex["call:Api#fetch()"](api);
ex["call:future:poll"](g, 5);
ex["tw:terminated"].value = 1;
res("x");
await tick();
assert.deepEqual(wakes, [4]);
console.log("done");
