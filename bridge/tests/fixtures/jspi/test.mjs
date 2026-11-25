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

const { instantiate, app, scratchUse } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
instantiate(compile("lang.wasm"), compile("bridge.wasm"));
const empty = scratchUse();

// The export runs up to its first suspension, then returns a promise.
const api = new Api();
const p = app.run(api);
assert.ok(p instanceof Promise);
assert.equal(api.resolvers.length, 2);
api.resolvers[0](3);
api.resolvers[1](5);
assert.equal(await p, 8);

// Two calls parked at once resume in the order their futures settle.
const a = new Api();
const b = new Api();
const pa = app.run(a);
const pb = app.run(b);
const order = [];
pa.then(() => order.push("a"));
pb.then(() => order.push("b"));
b.resolvers[0](1);
b.resolvers[1](2);
assert.equal(await pb, 3);
a.resolvers[0](10);
a.resolvers[1](20);
assert.equal(await pa, 30);
assert.deepEqual(order, ["b", "a"]);

// Several calls can be parked on one future, and each resumes with its
// state once it settles.
const s = new Api();
app.share(s);
const peeks = [app.peek(), app.peek()];
s.resolvers[0](7);
assert.deepEqual(await Promise.all(peeks), [1, 1]);
// Dropping a future a call is parked on resumes it with 0.
app.share(new Api());
const dropped = app.peek();
app.unshare();
assert.equal(await dropped, 0);

// `suspend_race` resumes with the first future to settle, leaving the
// others to the language, and with the lowest of those already settled.
const r = new Api();
const raced = app.race(r);
r.resolvers[2](7);
assert.equal(await raced, 207);
const l = new Api();
const lowest = app.lowest(l);
l.resolvers[1](0);
l.resolvers[2](0);
assert.equal(await lowest, 1);
await assert.rejects(app.empty(), (e) => e instanceof TypeError && /empty list/.test(e.message));

// A callback returning a future suspends the same way.
const cb = app.plus(40);
const c = new Api();
const called = cb(c);
c.resolvers[0](2);
assert.equal(await called, 42);

assert.deepEqual(scratchUse(), empty);
console.log("ok");
