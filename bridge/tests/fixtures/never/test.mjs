import { assert, lang } from "./harness.mjs";

class Proc {
  constructor() {
    this.next = undefined;
  }
}
for (const m of ["exit", "halt"]) {
  Proc.prototype[m] = function () {
    if (typeof this.next === "function") return this.next();
    return this.next;
  };
}
globalThis.Proc = Proc;

const { instantiate } = await import("./bridge.js");
const fs = await import("node:fs");
const compile = (f) => new WebAssembly.Module(fs.readFileSync(f));
const L = lang(instantiate(compile("lang.wasm"), compile("bridge.wasm")));
const errTag = L.ex["tag:err"];
const proc = new Proc();

const call = (key, ...wire) => {
  let caught;
  L.balanced(() => {
    try {
      L.call(key, proc, ...wire);
    } catch (e) {
      caught = e;
    }
  });
  return caught;
};

// A `never` call that throws reports the throw as usual.
proc.next = () => {
  throw new RangeError("bye");
};
let caught = call("Proc#exit(u8)", 3);
assert.ok(caught instanceof WebAssembly.Exception && caught.is(errTag));
assert.deepEqual([caught.getArg(errTag, 0), caught.getArg(errTag, 1)], [1, "bye"]);

// One that returns breaks its contract: a TypeError, read as `err`'s arm
// under `throws`, and thrown to the caller without.
proc.next = 42;
caught = call("Proc#exit(u8)", 3);
assert.ok(caught instanceof WebAssembly.Exception && caught.is(errTag));
assert.deepEqual(
  [caught.getArg(errTag, 0), caught.getArg(errTag, 1)],
  [1, "a `never` call returned"],
);
caught = call("Proc#halt()");
assert.ok(caught instanceof TypeError, `${caught}`);
assert.equal(caught.message, "a `never` call returned");
