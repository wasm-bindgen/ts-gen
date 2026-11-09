import { assert, lang } from "./harness.mjs";

class Timer {
  setTimeout(h) {
    this.handler = h;
  }
  onRest(cb) {
    this.cb = cb;
  }
}
globalThis.Timer = Timer;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const calls = [];
let L;
const f64s = (p, n) => Array.from({ length: n }, (_, i) => L.dv().getFloat64(p + i * 8, true));
const impl = {
  "tw:call:fn(...arr<f64>)": (env, p, n) => calls.push([env, f64s(p, n)]),
  "tw:call:fn(str,...arr<f64>)": (env, sp, sl, p, n) => calls.push([env, L.readStr(sp, sl), f64s(p, n)]),
  "tw:drop:fn(...arr<f64>)": () => {},
  "tw:drop:fn(str,...arr<f64>)": () => {},
};
L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }));

// JS calls a language callback: the remaining arguments gather into the
// rest parameter.
const t = new Timer();
L.call("Timer#setTimeout(fn(...arr<f64>))", t, 8);
L.balanced(() => {
  t.handler();
  t.handler(1.5);
  t.handler(1.5, 2);
});
assert.deepEqual(calls.splice(0), [
  [8, []],
  [8, [1.5]],
  [8, [1.5, 2]],
]);
L.throwsType(() => t.handler(1, "x"), /f64/);
L.call("Timer#onRest(fn(str,...arr<f64>))", t, 16);
L.balanced(() => {
  t.cb("a");
  t.cb("b", 3);
  t.cb("c", 3, 4);
});
assert.deepEqual(calls.splice(0), [
  [16, "a", []],
  [16, "b", [3]],
  [16, "c", [3, 4]],
]);
L.throwsType(() => t.cb(), /str/);

// The language calls a JS function: the sequence spreads.
const seen = [];
const f = (...a) => seen.push(a);
for (const xs of [[], [1.5], [1.5, 2]]) {
  L.balanced(() => {
    L.call("call:fn(...arr<f64>)", f, L.bytes(new Float64Array(xs), 8), xs.length);
    L.call("call:fn(str,...arr<f64>)", f, ...L.str("s"), L.bytes(new Float64Array(xs), 8), xs.length);
  });
}
assert.deepEqual(seen, [[], ["s"], [1.5], ["s", 1.5], [1.5, 2], ["s", 1.5, 2]]);

console.log("ok");
