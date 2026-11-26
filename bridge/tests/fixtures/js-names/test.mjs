import { assert } from "./harness.mjs";

class Str {
  constructor(n) {
    this.n = n;
  }
  static make() {
    return new Str(7);
  }
}
class Bool {
  static of() {
    return new Bool();
  }
}
globalThis.Str = Str;
globalThis.Any = { f: () => 1, Vec: { g: () => 2, Bool } };

const mod = await import("./bridge.js");
const { instantiate, Arr, Tuple } = mod;
const { Unit } = await import("./host.mjs");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const impl = {
  "Arr_.new(u32)": (self, n) => ({ n }),
  "Arr_#get:n()": (self, h) => h.n,
  "Tuple_.twice(u32)": (n) => n * 2,
  "Tuple_.new_.thrice(u32)": (n) => n * 3,
};
const ex = instantiate(compile("lang.wasm"), compile("bridge.wasm"), { "test:impl": impl }).exports;
const call = (k, ...a) => ex["call:" + k](...a);

// An import's key keeps the IR names, and its JS path is the JS names.
assert.equal(call("Any_.f()"), 1);
assert.equal(call("Any_.Vec_.g()"), 2);
const s = call("Str_.make()");
assert.ok(s instanceof Str);
assert.equal(call("Str_#get:n()", s), 7);
assert.equal(call("Str_.new(u32)", 3).n, 3);
assert.ok(call("Any_.Vec_.Bool_.of()") instanceof Bool);

// A result is checked against the class at its JS path.
Str.make = () => ({});
assert.throws(() => call("Str_.make()"), TypeError);
Bool.of = () => ({});
assert.throws(() => call("Any_.Vec_.Bool_.of()"), TypeError);

// Past a module's export, the path is JS names too.
assert.equal(call("Unit_.h()"), 4);
assert.ok(call("Unit_.in_.make()") instanceof Unit.in);

// The exports carry their JS names, at every level.
assert.ok(!("Arr_" in mod) && !("Tuple_" in mod));
assert.equal(new Arr(5).n, 5);
assert.throws(() => Arr.prototype.n, /Arr/);
assert.equal(Tuple.twice(2), 4);
assert.equal(Tuple.new.thrice(2), 6);
assert.deepEqual(Object.keys(Tuple).sort(), ["new", "twice"]);

console.log("ok");
