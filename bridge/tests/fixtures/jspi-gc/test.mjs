import { assert } from "./harness.mjs";

class Api {
  constructor() {
    this.resolvers = [];
  }
  later() {
    // Calls back into a synchronous export from under a promising call.
    if (this.inner) {
      const inner = this.inner;
      this.inner = null;
      this.before = scratchUse();
      this.result = app.stuck(inner);
    }
    return new Promise((r) => this.resolvers.push(r));
  }
  mark() {
    this.scratch = scratchUse();
  }
}
globalThis.Api = Api;

const { instantiate, app, scratchUse, spillEmpty } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
instantiate(compile("lang.wasm"), compile("bridge.wasm"));
const empty = scratchUse();

// A synchronous export has no stack to suspend: the `SuspendError` is the
// language's to catch, with scratch as it was.
const stuck = new Api();
assert.equal(app.stuck(stuck), 7);
assert.deepEqual(stuck.scratch, empty);

// Two calls parked at once, resumed out of order.
const a = new Api();
const b = new Api();
const pa = app.run(a);
const pb = app.run(b);
b.resolvers[0](1);
b.resolvers[1](2);
assert.equal(await pb, 3);
a.resolvers[0](10);
a.resolvers[1](20);
assert.equal(await pa, 30);

// A GC argument, translated by the bridge module's entry under the root,
// while another call runs on scratch.
const e = new Api();
const echoed = app.echo(e, "A");
const other = new Api();
const ran = app.run(other);
e.resolvers[0](1);
assert.equal(await echoed, 66);
other.resolvers[0](1);
other.resolvers[1](1);
assert.equal(await ran, 2);

// A suspension under a synchronous callback finds a JS frame in its way,
// and throws the same. The promising call it ran under goes on to park
// and resume.
const outer = new Api();
const inner = new Api();
outer.inner = inner;
const nested = app.echo(outer, "A");
assert.equal(outer.result, 7);
assert.deepEqual(inner.scratch, outer.before);
outer.resolvers[0](1);
assert.equal(await nested, 66);

// A GC list for `suspend_list`.
const r = new Api();
const raced = app.race(r);
r.resolvers[1](9);
assert.equal(await raced, 109);

// A pending `throws = js` error after a suspension rejects the call.
const f = new Api();
const failed = app.fail(f, "no");
f.resolvers[0](0);
await assert.rejects(failed, (x) => x instanceof Error && x.message === "no");

assert.deepEqual(scratchUse(), empty);
assert.equal(spillEmpty(), true);
console.log("ok");
