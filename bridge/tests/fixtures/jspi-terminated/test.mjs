import { assert } from "./harness.mjs";

class Api {
  constructor() {
    this.resolvers = [];
  }
  later() {
    return new Promise((r) => this.resolvers.push(r));
  }
}
globalThis.Api = Api;

const { instantiate, app } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const instance = instantiate(compile("lang.wasm"), compile("bridge.wasm"));
const resumed = instance.exports["test:resumed"];
const caught = instance.exports["test:caught"];
const state = new Uint8Array(instance.exports.memory.buffer);

// Live, a settlement resumes the parked call.
let api = new Api();
let p = app.run(api);
api.resolvers[0](4);
assert.equal(await p, 4);
assert.equal(resumed(), 1);

// Terminated while they're parked, the calls reject from JS instead: the
// stacks stay parked, so the language's `catch_all` never runs.
api = new Api();
p = app.run(api);
const other = app.run(new Api());
state[8] = 1;
api.resolvers[0](5);
await assert.rejects(p, /Module terminated/);
await assert.rejects(other, /Module terminated/);
assert.equal(resumed(), 1, "the language didn't run again");
assert.equal(caught(), 0);

console.log("ok");
