import { assert, lang } from "./harness.mjs";

class Driver {
  run(step, x) {
    return step(g, x);
  }
}
globalThis.Driver = Driver;
// JS's `Next<f64>`: it passes itself on through the language's.
const g = (next, y) => next(g, y * 2);

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const envs = [];
let L;
const impl = {
  // The language's `Next<f64>`: it calls the JS one it's given, passing
  // the language closure 16, until the value passes 50.
  "tw:call:fn(Next<f64>,f64)->f64": (env, next, x) => {
    envs.push(env);
    return x > 50 ? x : L.call("call:fn(Next<f64>,f64)->f64", next, 16, x + 1);
  },
  "tw:drop:fn(Next<f64>,f64)->f64": () => {},
};
L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }));

// The back-edge crosses both ways: each `next` a JS `Next<f64>` gets is
// a language function, and each one the language gets is a JS function.
assert.equal(L.call("Driver#run(fn(Next<f64>,f64)->f64,f64)", new Driver(), 8, 2), 62);
assert.deepEqual(envs, [8, 16, 16, 16, 16]);

console.log("ok");
