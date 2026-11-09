import { assert, lang } from "./harness.mjs";

class Api {
  fetch() {
    if (this.throwing) throw this.throwing;
    return this.next;
  }
  slow(url, signal) {
    this.signal = signal;
    return new Promise((_, reject) => signal.addEventListener("abort", () => reject(new Error("aborted"))));
  }
  number() {
    return this.next;
  }
  nothing() {
    return this.next;
  }
  forever() {
    return this.next;
  }
  strict() {
    throw new TypeError("now");
  }
  give(p) {
    this.given = p;
  }
  later(cb) {
    this.cb = cb;
  }
}
globalThis.Api = Api;

const { instantiate, app } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const tick = () => new Promise((r) => setTimeout(r, 0));
const later = (ms) => new Promise((r) => setTimeout(r, ms));

const wakes = [];
const ticks = [];
let tickResults = [];
let sleeps = [];
let runs = [];
let L;
const impl = {
  "tw:future/wake": (w) => wakes.push(w),
  "tw:loop/tick": (loop, max) => {
    ticks.push([loop, max]);
    return tickResults.shift() ?? 0;
  },
  "tw:loop/next_sleep": () => sleeps.shift() ?? -(2n ** 63n),
  "tw:drop:fn(u32)->future<u32>": () => {},
  "tw:call:fn(u32)->future<u32>": (env, x) => {
    const [f, r] = L.call("future:new");
    L.call("future:resolve<u32>", r, x + env);
    return f;
  },
  "app.run(u32)": (x) => {
    const [f, r] = L.call("future:new");
    runs.push([x, r]);
    return f;
  },
  "app.wait(future<str>)": (f) => {
    runs.push(f);
    return L.call("future:poll", f, 3);
  },
};
L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }));
const api = new Api();
const poll = (f, w = 1) => L.call("future:poll", f, w);
const err = L.ex["tag:err"];

// A JS promise as a future: poll, wake, take.
let resolve;
api.next = new Promise((r) => (resolve = r));
let f = L.call("Api#fetch(str)", api, ...L.str("u"));
assert.equal(poll(f, 7), 0);
assert.equal(poll(f, 8), 0, "polling again replaces the waker");
resolve("body");
await tick();
assert.deepEqual(wakes, [8]);
assert.equal(poll(f), 1);
assert.equal(L.takeStr(...L.call("future:take<str>", f)), "body");
L.throwsType(() => L.call("future:take<str>", f), /isn't resolved/);
L.call("future:drop", f);

// Rejections map through `E`.
api.next = Promise.reject(new DOMException("gone", "AbortError"));
f = L.call("Api#fetch(str)", api, ...L.str("u"));
await tick();
assert.equal(poll(f), 2);
const [arm, msg] = L.call("future:error<FetchError>", f);
assert.equal(msg, "gone");
assert.ok(arm > 0, "not the fallback arm");
L.call("future:drop", f);

// Without `throws`, a synchronous throw rejects the future.
api.throwing = new RangeError("sync");
f = L.call("Api#fetch(str)", api, ...L.str("u"));
api.throwing = null;
await tick();
assert.equal(poll(f), 2);
assert.equal(L.call("future:error<FetchError>", f)[1], "sync");
// With `throws`, it goes to the tag.
assert.throws(
  () => L.call("Api#strict()", api),
  (e) => e instanceof WebAssembly.Exception && e.is(err),
);

// A resolution that fails validation rejects with its TypeError.
api.next = Promise.resolve("x");
f = L.call("Api#number()", api);
await tick();
assert.equal(poll(f), 2);
assert.match(L.call("future:error<err>", f)[1], /u32/);
api.next = Promise.resolve(7);
f = L.call("Api#number()", api);
await tick();
assert.equal(poll(f), 1);
assert.equal(L.call("future:take<u32>", f), 7);
api.next = 5;
f = L.call("Api#nothing()", api);
await tick();
assert.equal(poll(f), 1);
L.call("future:take<unit>", f);

// A `future<never>` that resolves rejects instead.
api.next = Promise.resolve(1);
f = L.call("Api#forever()", api);
await tick();
assert.equal(poll(f), 2);
assert.match(L.call("future:error<err>", f)[1], /never/);

// Dropping cancels once and ignores the later settlement.
f = L.call("Api#slow(str)", api, ...L.str("u"));
assert.equal(poll(f, 9), 0);
L.call("future:drop", f);
assert.ok(api.signal.aborted);
await tick();
assert.ok(!wakes.includes(9));
L.throwsType(() => poll(f), /dropped/);

// A future the language holds reaches JS as its promise.
const p = Promise.resolve(3);
api.next = p;
f = L.call("Api#number()", api);
L.call("Api#give(future<u32>)", api, f);
assert.equal(api.given, p, "the same promise");

// Language-created futures: JS sees a promise, settled by resolve/reject.
const r1 = app.run(4);
assert.ok(r1 instanceof Promise);
L.call("future:resolve<u32>", runs[0][1], 44);
assert.equal(await r1, 44);
const r2 = app.run(5);
L.call("future:reject<err>", runs[1][1], 1, "bad");
await assert.rejects(r2, (e) => e instanceof TypeError && e.message === "bad");
// Argument failures fold into the rejection, never a synchronous throw.
const r3 = app.run("x");
await assert.rejects(r3, TypeError);
await assert.rejects(app.run(), TypeError);
// JS promises crossing into an export.
let res2;
assert.equal(app.wait(new Promise((r) => (res2 = r))), 0);
res2("s");
await tick();
assert.equal(poll(runs[runs.length - 1]), 1);

// A callback returning a future rejects it on a synchronous failure.
L.call("Api#later(fn(u32)->future<u32>)", api, 100);
assert.equal(await api.cb(1), 101);
await assert.rejects(api.cb("x"), TypeError);

// Loops: one coalesced macrotask per loop, more work reposts, then the
// next sleep decides.
L.call("loop:work_available", 1);
L.call("loop:work_available", 1);
tickResults = [1, 0];
sleeps = [0n, 20_000_000n];
await tick();
await tick();
await tick();
assert.deepEqual(ticks, [[1, 64], [1, 64], [1, 64]]);
await later(5);
assert.equal(ticks.length, 3, "sleeping");
await later(40);
assert.equal(ticks.length, 4, "the timer ticked");
await later(10);
assert.equal(ticks.length, 4, "idle once nothing is scheduled");
// A negative sleep other than the absent niche is the language's bug.
const caught = new Promise((r) => process.once("uncaughtException", r));
L.call("loop:work_available", 3);
sleeps = [-5n];
assert.match((await caught).message, /outside u53/);
const before = ticks.length;
// Dropping clears the pending macrotask.
L.call("loop:work_available", 2);
L.call("loop:drop", 2);
await tick();
assert.equal(ticks.length, before);
console.log("done");
