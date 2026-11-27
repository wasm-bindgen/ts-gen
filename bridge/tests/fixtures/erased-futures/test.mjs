import { assert, lang } from "./harness.mjs";

class Box {}
class Kv {
  get() {
    return Promise.resolve(this.next);
  }
  boxed() {
    return Promise.resolve(this.next);
  }
}
globalThis.Box = Box;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const tick = () => new Promise((r) => setTimeout(r, 0));
const kv = new Kv();
const settle = async (key, v) => {
  kv.next = v;
  const f = L.call(key, kv, ...(key.includes("str") ? L.str("k") : []));
  await tick();
  return f;
};

let f = await settle("Kv#get(str)", { a: 1 });
assert.equal(L.call("future:state", f), 1);
assert.deepEqual(L.call("future:get<Any?>", f), [1, { a: 1 }]);
f = await settle("Kv#get(str)", null);
assert.equal(L.call("future:state", f), 1);
assert.deepEqual(L.call("future:get<Any?>", f), [1, null]);

const b = new Box();
f = await settle("Kv#boxed()", b);
assert.equal(L.call("future:state", f), 1);
assert.deepEqual(L.call("future:get<Box<Any>>", f), [1, b]);

console.log("ok");
