import { assert, collect } from "./harness.mjs";

class Api {
  constructor() {
    this.resolvers = [];
  }
  later() {
    return new Promise((r) => this.resolvers.push(r));
  }
}
globalThis.Api = Api;

const { instantiate, app, Task, scratchUse } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const instance = instantiate(compile("lang.wasm"), compile("bridge.wasm"));
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

// `suspend_list` resumes with the first future to settle, leaving the
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
// And keeps its environment while it's parked, though JS has let the
// function go.
const envs = instance.exports["test:envs"];
const dropped = envs();
const c2 = new Api();
const parked = app.plus(1)(c2);
await collect();
assert.equal(envs(), dropped);
c2.resolvers[0](2);
assert.equal(await parked, 3);

// A suspended call keeps its arguments until it settles.
const e = new Api();
const live = instance.exports["test:live"];
const before = live();
const echoed = app.echo(e, "A");
assert.equal(live(), before + 1);
e.resolvers[0](1);
assert.equal(await echoed, 66);
assert.equal(live(), before);

// And its receiver: a collected `Task` would drop the handle `run` still
// uses. This runs optimized (`node` holds `--no-liftoff`), where a
// suspended frame doesn't keep the values it no longer reads.
const drops = instance.exports["test:drops"];
const t = new Api();
const ran = new Task(t).run();
await collect();
assert.equal(drops(), 0);
t.resolvers[0](5);
assert.equal(await ran, 5);
await collect();
assert.equal(drops(), 1);

assert.deepEqual(scratchUse(), empty);
console.log("ok");
