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
  joined(url, init) {
    this.init = init;
    return new Promise((_, reject) => init.signal.addEventListener("abort", () => reject(new Error("aborted"))));
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
    return L.call("future:state", f);
  },
};
L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }));
const api = new Api();
const state = (f) => L.call("future:state", f);
// Futures as the language's `arr<Any>`: `[base, len]` in its table.
const refs = (fs) => {
  const base = L.ex["tw:table/alloc"](fs.length);
  fs.forEach((f, i) => L.ex["tw:table"].set(base + i, f));
  return [base, fs.length];
};
const states = (fs) => [...L.take(...L.call("future:states", ...refs(fs)))];
const listen = (f, w) => L.call("future:add_listener", f, w);
const listenAll = (fs, w) => L.call("future:add_listener_all", ...refs(fs), w);
const unlistenAll = (fs, w) => L.call("future:remove_listener_all", ...refs(fs), w);
const err = L.ex["tag:err"];
const fetchError = L.ex["tag:FetchError"];
// A read's failure, thrown to the future's `E` with its message.
const thrown = (tag, re) => (e) => e instanceof WebAssembly.Exception && e.is(tag) && re.test(e.getArg(tag, 1));

// A JS promise as a future: its state, its wake, and its value.
let resolve;
api.next = new Promise((r) => (resolve = r));
let f = L.call("Api#fetch(str)", api, ...L.str("u"));
assert.equal(state(f), 0);
assert.deepEqual(L.call("future:get<str,FetchError>", f), [0, 0, 0], "no value yet");
listen(f, 7);
listen(f, 7);
listen(f, 8);
listen(f, 5);
L.call("future:remove_listener", f, 5);
// Removing a listener that isn't there does nothing.
L.call("future:remove_listener", f, 4);
resolve("body");
await tick();
assert.deepEqual(wakes, [7, 8], "a listener is woken once, and a removed one not at all");
assert.equal(state(f), 1);
assert.deepEqual(states([f]), [1]);
// A settled future wakes nothing.
listen(f, 6);
// `get` is the value's option, lowered afresh at each read.
for (let i = 0; i < 2; i++) {
  const [present, ...s] = L.call("future:get<str,FetchError>", f);
  assert.equal(present, 1);
  assert.equal(L.takeStr(...s), "body");
}
L.throwsType(() => L.call("future:get<str,FetchError>", {}), /a future/);
L.throwsType(() => listen({}, 1), /a future/);
L.throwsType(() => L.call("future:remove_listener", {}, 1), /a future/);

// Rejections map through `E`, read as often as asked.
api.next = Promise.reject(new DOMException("gone", "AbortError"));
f = L.call("Api#fetch(str)", api, ...L.str("u"));
await tick();
assert.equal(state(f), 2);
const [arm, msg] = L.call("future:error<FetchError>", f);
assert.equal(msg, "gone");
assert.ok(arm > 0, "not the fallback arm");
assert.deepEqual(L.call("future:error<FetchError>", f), [arm, msg]);
assert.deepEqual(L.call("future:get<str,FetchError>", f), [0, 0, 0], "a rejection is no value");

// Without `throws`, a synchronous throw rejects the future.
api.throwing = new RangeError("sync");
f = L.call("Api#fetch(str)", api, ...L.str("u"));
api.throwing = null;
await tick();
assert.equal(state(f), 2);
assert.equal(L.call("future:error<FetchError>", f)[1], "sync");
// With `throws`, it goes to the tag.
assert.throws(
  () => L.call("Api#strict()", api),
  (e) => e instanceof WebAssembly.Exception && e.is(err),
);

// A resolution is validated where it's read, as an import's result is:
// the future fulfils, and reading it throws to its `E`.
api.next = Promise.resolve("x");
f = L.call("Api#number()", api);
await tick();
assert.equal(state(f), 1);
L.balanced(() => assert.throws(() => L.call("future:get<u32>", f), thrown(err, /u32/)));
api.next = Promise.resolve(5);
f = L.call("Api#fetch(str)", api, ...L.str("u"));
await tick();
assert.equal(state(f), 1);
assert.throws(() => L.call("future:get<str,FetchError>", f), thrown(fetchError, /str/));
// A value that is itself a none is still a value.
api.next = Promise.resolve(null);
f = L.call("Api#number()", api);
await tick();
assert.throws(() => L.call("future:get<u32>", f), thrown(err, /u32/));
api.next = Promise.resolve(7);
f = L.call("Api#number()", api);
await tick();
assert.deepEqual(L.call("future:get<u32>", f), [1, 7]);
// A `unit` future's option is its flag alone.
api.next = new Promise((r) => (resolve = r));
f = L.call("Api#nothing()", api);
assert.equal(L.call("future:get<unit>", f), 0);
resolve(5);
await tick();
assert.equal(L.call("future:get<unit>", f), 1);

// A `future<never>` that resolves rejects instead.
api.next = Promise.resolve(1);
f = L.call("Api#forever()", api);
await tick();
assert.equal(state(f), 2);
assert.match(L.call("future:error<err>", f)[1], /never/);

// Cancelling is a request: the cancel function runs, and the future
// settles as its promise does.
f = L.call("Api#slow(str)", api, ...L.str("u"));
assert.equal(state(f), 0);
listen(f, 9);
L.call("future:cancel", f);
assert.ok(api.signal.aborted);
assert.equal(state(f), 0);
await tick();
assert.ok(wakes.includes(9));
assert.equal(state(f), 2);
assert.match(L.call("future:error<err>", f)[1], /aborted/);
// Cancelling a settled future does nothing.
L.call("future:cancel", f);
assert.equal(state(f), 2);

// A listener is added to, and removed from, several futures at once
// through the language's own array: it's woken as each of them settles.
{
  const rs = [];
  const fs = [0, 1, 2].map(() => {
    api.next = new Promise((r) => rs.push(r));
    return L.call("Api#number()", api);
  });
  assert.deepEqual(states(fs), [0, 0, 0]);
  wakes.length = 0;
  listenAll(fs, 20);
  // A future holds a listener once, however it was added.
  listenAll([fs[1]], 20);
  listen(fs[1], 20);
  listen(fs[1], 21);
  rs[1](1);
  await tick();
  assert.deepEqual(wakes, [20, 21], "one wake a listener for a future");
  assert.deepEqual(states(fs), [0, 1, 0], "a state says nothing of the value");
  // The rest still hold it, until it's removed from them.
  rs[2]("x");
  await tick();
  assert.deepEqual(wakes, [20, 21, 20]);
  unlistenAll(fs, 20);
  rs[0](0);
  await tick();
  assert.deepEqual(wakes, [20, 21, 20]);
  assert.deepEqual(states(fs), [1, 1, 1]);
  // Settled futures wake nothing.
  listenAll(fs, 22);
  await tick();
  assert.deepEqual(wakes, [20, 21, 20]);
  L.throwsType(() => states([fs[0], {}]), /a future/);
  // An element that isn't a future fails the whole call.
  api.next = new Promise((r) => rs.push(r));
  const g = L.call("Api#number()", api);
  L.throwsType(() => listenAll([g, {}], 23), /a future/);
  L.throwsType(() => unlistenAll([g, {}], 23), /a future/);
  rs[3](0);
  await tick();
  assert.deepEqual(wakes, [20, 21, 20]);
}

// A cancel signal joins the caller's: the caller's own aborts the call,
// and its init is passed as a copy.
{
  const mine = new AbortController();
  const init = { signal: mine.signal };
  f = L.call("Api#joined(str,Init?)", api, ...L.str("u"), init);
  assert.equal(state(f), 0);
  assert.notEqual(api.init, init);
  assert.equal(init.signal, mine.signal);
  mine.abort();
  await tick();
  assert.equal(state(f), 2);
  assert.match(L.call("future:error<err>", f)[1], /aborted/);
}
// Cancelling aborts the call, and not the caller's signal.
{
  const mine = new AbortController();
  f = L.call("Api#joined(str,Init?)", api, ...L.str("u"), { signal: mine.signal });
  assert.equal(state(f), 0);
  L.call("future:cancel", f);
  assert.ok(api.init.signal.aborted);
  assert.ok(!mine.signal.aborted);
}
// With no init, or one without a signal (`null` is none too, as `fetch`
// reads it), the signal is the bridge's alone.
for (const init of [null, {}, { signal: null }, { signal: undefined }]) {
  api.init = undefined;
  f = L.call("Api#joined(str,Init?)", api, ...L.str("u"), init);
  assert.equal(state(f), 0);
  assert.ok(!api.init.signal.aborted);
  L.call("future:cancel", f);
  assert.ok(api.init.signal.aborted);
}
// An overload without the init cancels through one the bridge makes.
api.init = undefined;
f = L.call("Api#short(str)", api, ...L.str("u"));
assert.equal(state(f), 0);
assert.deepEqual(Object.keys(api.init), ["signal"]);
assert.ok(!api.init.signal.aborted);
L.call("future:cancel", f);
assert.ok(api.init.signal.aborted);

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
assert.equal(state(runs[runs.length - 1]), 1);

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
