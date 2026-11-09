import { assert, lang } from "./harness.mjs";

class ReadableStream {}
class Request {}
class Probe {
  constructor() {
    this.seen = [];
    this.next = undefined;
  }
}
for (const m of ["method", "optMethod", "mode", "flags", "optFlags", "small", "body", "optBody", "value", "strict", "maybe", "arg", "unit", "time", "gapped"]) {
  Probe.prototype[m] = function (v) {
    this.seen.push(v);
    return this.next;
  };
}
Object.assign(globalThis, { ReadableStream, Request, Probe });

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const p = new Probe();
const call = (key, next, ...wire) => {
  p.next = next;
  return L.call(key, p, ...wire);
};
const seen = () => p.seen.pop();

// Index enums: implicit indices from 1; literals both ways.
assert.equal(call("Probe#method(Method)", "GET", 2), 1);
assert.equal(seen(), "POST");
assert.equal(call("Probe#method(Method)", "POST", 1), 2);
assert.equal(seen(), "GET");
L.throwsType(() => call("Probe#method(Method)", "PUT", 1), /Method/);
L.throwsType(() => call("Probe#method(Method)", 1, 1), /Method/);
assert.throws(() => call("Probe#method(Method)", "GET", 3));
assert.throws(() => call("Probe#method(Method)", "GET", 0));
// Nullable: 0 is absent.
assert.equal(call("Probe#optMethod(Method?)", null, 0), 0);
assert.equal(seen(), undefined);
assert.equal(call("Probe#optMethod(Method?)", "POST", 1), 2);
assert.equal(seen(), "GET");

// A unit fallback catches unknown literals; outgoing it has no JS value.
assert.equal(call("Probe#mode(Mode)", "no-cors", 1), -1);
assert.equal(call("Probe#mode(Mode)", "same-origin", 1), 2);
L.throwsType(() => call("Probe#mode(Mode)", "cors", -1), /Unknown/);

// Carrier enums: an open set of numbers.
assert.equal(call("Probe#flags(Flags)", 7, 3), 7);
assert.equal(seen(), 3);
assert.equal(call("Probe#flags(Flags)", 4294967295, 0), -1);
L.throwsType(() => call("Probe#flags(Flags)", -1, 0), /u32/);
L.throwsType(() => call("Probe#flags(Flags)", "1", 0));
assert.deepEqual(call("Probe#optFlags(Flags?)", null, 1, 5), [0, 0]);
assert.equal(seen(), 5);
assert.deepEqual(call("Probe#optFlags(Flags?)", 0, 0, 5), [1, 0]);
assert.equal(seen(), undefined);
assert.equal(call("Probe#small(Small)", -128, -1), -128);
assert.equal(seen(), -1);
L.throwsType(() => call("Probe#small(Small)", 128, 0), /i8/);

// Payload enums: (arm, i32, i32, externref) with first-fit slots.
L.balanced(() => {
  const [arm, ptr, len, x] = call("Probe#body(Body)", "hi", 1, 1, 0, null);
  assert.deepEqual([arm, len, x], [1, 2, null]);
  assert.equal(L.takeStr(ptr, len), "hi");
});
L.balanced(() => {
  const [arm, ptr, len] = call("Probe#body(Body)", new Uint8Array([5, 6]), 1, 1, 0, null);
  assert.equal(arm, 2);
  assert.deepEqual([...L.take(ptr, len)], [5, 6]);
});
{
  const s = new ReadableStream();
  assert.deepEqual(call("Probe#body(Body)", s, 1, 1, 0, null), [3, 0, 0, s]);
  call("Probe#body(Body)", s, 3, 0, 0, s);
  assert.equal(seen(), s);
}
L.balanced(() => {
  call("Probe#body(Body)", new ReadableStream(), 1, ...L.str("text"), null);
  assert.equal(seen(), "text");
});
L.throwsType(() => call("Probe#body(Body)", 5, 1, 1, 0, null), /Body/);
L.throwsType(() => call("Probe#body(Body)", new Request(), 1, 1, 0, null), /Body/);
assert.deepEqual(call("Probe#optBody(Body?)", null, 0, 0, 0, null), [0, 0, 0, null]);
assert.equal(seen(), undefined);

// Discrimination order, nested enums, multi-payload arms, and a payload fallback.
const value = (v) => {
  const w = call("Probe#value(Value)", v, 1, 0, 0, 0n, 0, 0, null);
  return w;
};
const req = new Request();
assert.deepEqual(value(1.5).slice(0, 2), [1, 1.5]);
assert.deepEqual(value(true), [2, 0, 1, 0n, 0, 0, null]);
assert.deepEqual(value(-5n), [3, 0, 0, -5n, 0, 0, null]);
assert.deepEqual(value("POST"), [5, 0, 2, 0n, 0, 0, null]);
L.balanced(() => {
  const w = value([7, "seven"]);
  assert.deepEqual(w.slice(0, 2), [4, 0]);
  assert.equal(w[2], 7);
  assert.equal(L.takeStr(w[4], w[5]), "seven");
});
{
  const w = value([req]);
  assert.equal(w[0], 6);
  assert.equal(w[4], 1);
  assert.equal(L.ex["tw:table"].get(w[2]), req);
  L.ex["tw:table/free"](w[2], 1);
}
const other = { any: true };
assert.deepEqual(value(other), [-1, 0, 0, 0n, 0, 0, other]);
assert.deepEqual(value("not a method"), [-1, 0, 0, 0n, 0, 0, "not a method"]);
// Outgoing: each arm reads its own slots.
call("Probe#value(Value)", 0, 1, -0.5, 0, 0n, 0, 0, null);
assert.equal(seen(), -0.5);
call("Probe#value(Value)", 0, 2, 0, 1, 0n, 0, 0, null);
assert.equal(seen(), true);
call("Probe#value(Value)", 0, 5, 0, 1, 0n, 0, 0, null);
assert.equal(seen(), "GET");
L.balanced(() => {
  const [ptr, len] = L.str("x");
  call("Probe#value(Value)", 0, 4, 0, 9, 0n, ptr, len, null);
  assert.deepEqual(seen(), [9, "x"]);
});
call("Probe#value(Value)", 0, -1, 0, 0, 0n, 0, 0, other);
assert.equal(seen(), other);

// No fallback: an unmatched value is a TypeError, and arm failures propagate.
L.throwsType(() => call("Probe#strict(Strict)", true, 1, 0, 0), /Strict/);
L.throwsType(() => call("Probe#strict(Strict)", 300, 1, 0, 0), /u8/);
L.balanced(() => {
  const [arm, ptr, len] = call("Probe#strict(Strict)", "n", 1, 0, 0);
  assert.equal(arm, 2);
  assert.equal(L.takeStr(ptr, len), "n");
});

// Stated indices: combined bases keep theirs, so widening is free.
assert.equal(call("Probe#time(TimeUnit)", "minute", 3), 4);
assert.equal(seen(), "hour");
assert.throws(() => call("Probe#time(TimeUnit)", "hour", 1));
assert.equal(call("Probe#unit(AnyUnit)", "minute", 2), 4);
assert.equal(seen(), "day");
assert.equal(call("Probe#unit(AnyUnit)", "week", 3), 5);
assert.equal(seen(), "hour");
assert.equal(call("Probe#unit(AnyUnit)", "year", 5), 1);
assert.equal(seen(), "week");
L.balanced(() => {
  const [arm, ptr, len] = call("Probe#gapped(Gapped)", "n", 5, 7, 0);
  assert.equal(seen(), 7);
  assert.equal(arm, 6);
  assert.equal(L.takeStr(ptr, len), "n");
});
assert.deepEqual(call("Probe#gapped(Gapped)", 9, 5, 7, 0).slice(0, 2), [5, 9]);
assert.throws(() => call("Probe#gapped(Gapped)", 9, 1, 7, 0));

// str??: undefined, null and a value are three states.
assert.deepEqual(call("Probe#maybe(str??)", undefined, 1, 0, 0).slice(0, 1), [1]);
assert.deepEqual(call("Probe#maybe(str??)", null, 1, 0, 0), [2, 0, -1]);
L.balanced(() => {
  const [arm, ptr, len] = call("Probe#maybe(str??)", "v", 1, 0, 0);
  assert.equal(arm, 2);
  assert.equal(L.takeStr(ptr, len), "v");
});
call("Probe#maybe(str??)", undefined, 1, 0, 0);
assert.equal(seen(), undefined);
call("Probe#maybe(str??)", undefined, 2, 0, -1);
assert.equal(seen(), null);

// Arm tiers: a thenable is a future before an object is an interface.
assert.equal(call("Probe#arg()", { url: "u" })[0], 3);
assert.equal(call("Probe#arg()", Promise.resolve(1))[0], 1);
assert.equal(call("Probe#arg()", { then() {} })[0], 1);
L.balanced(() => {
  const [arm, , ptr, len] = call("Probe#arg()", "x");
  assert.equal(arm, 2);
  assert.equal(L.takeStr(ptr, len), "x");
});
L.throwsType(() => call("Probe#arg()", 5), /Arg/);
L.throwsType(() => call("Probe#arg()", null), /Arg/);

console.log("ok");
