import { assert, lang, collect } from "./harness.mjs";

class Target {
  on(cb) {
    this.cb = cb;
  }
  fire(x) {
    if (this.boom) throw this.boom;
    return x * 2;
  }
}
globalThis.Target = Target;

const { instantiate, app } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const drops = () => L.ex["test:drops"]();
const t = new Target();
const start = L.live();

// Closures over environments in the language's memory.
app.listen(t, 10);
assert.equal(t.cb(5), 15);
app.listen(t, 20);
assert.equal(t.cb(1), 21);
assert.equal(L.live().allocations, start.allocations + 2);

// destroy revokes the wrapper; the language frees its environment.
const dead = t.cb;
app.forget();
assert.throws(() => dead(1), /destroyed/);
assert.equal(L.live().allocations, start.allocations + 1);

// The collected first wrapper drops its environment, once.
t.cb = null;
await collect();
assert.equal(drops(), 1);
assert.deepEqual(L.live(), start);

// An exception unwinds through the language's cleanup and the entry's.
assert.equal(L.balanced(() => app.guarded(t, "abc")), 6);
t.boom = new RangeError("boom");
L.balanced(() => assert.throws(() => app.guarded(t, "héllo"), (e) => e === t.boom));
t.boom = null;

// Termination: entries refuse, and finalizers skip their drops.
app.listen(t, 1);
app.terminate();
assert.throws(() => t.cb(1), /Module terminated/);
assert.throws(() => app.guarded(t, "x"), /Module terminated/);
t.cb = null;
await collect();
assert.equal(drops(), 1);
console.log("done");
